use crate::{
    BoxFuture, Capabilities, Error, ModelProvider, ModelRequest, ModelResponse, Result, RunContext,
};
use futures_util::StreamExt;
use serde_json::Value;
use std::fmt;

/// Responses-over-HTTP client. Redirects are disabled to keep bearer credentials local to the endpoint.
#[derive(Clone)]
pub struct ResponsesProvider {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    bearer: Option<String>,
    capabilities: Capabilities,
}

impl fmt::Debug for ResponsesProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponsesProvider")
            .field("endpoint", &self.endpoint)
            .field("bearer", &self.bearer.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl ResponsesProvider {
    pub fn new(base_url: &str, bearer: Option<String>) -> Result<Self> {
        let endpoint =
            reqwest::Url::parse(&format!("{}/responses", base_url.trim_end_matches('/')))
                .map_err(|_| Error::Configuration("invalid provider URL".into()))?;
        let local = endpoint.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !(endpoint.scheme() == "https" || (endpoint.scheme() == "http" && local))
        {
            return Err(Error::Configuration("provider URL must use HTTPS or loopback HTTP, without credentials, query or fragment".into()));
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            bearer,
            capabilities: Capabilities::default(),
        })
    }

    /// pi's subscription transport uses runtime-validated JSON instructions and has no output-token request limit.
    pub fn pi_codex(base_url: &str) -> Result<Self> {
        let mut provider = Self::new(base_url, None)?;
        provider.capabilities = Capabilities {
            native_json_schema: false,
            output_token_limit: false,
        };
        Ok(provider)
    }

    /// Authenticates a local bridge-compatible endpoint such as the Rust gateway.
    pub fn with_bearer(mut self, bearer: impl Into<String>) -> Self {
        self.bearer = Some(bearer.into());
        self
    }
}

impl ModelProvider for ResponsesProvider {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn complete(
        &self,
        mut request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            request.validate()?;
            if request.max_output_tokens.is_some() && !self.capabilities.output_token_limit {
                return Err(Error::Configuration(
                    "this provider has no output-token request limit".into(),
                ));
            }
            request.stream = true;
            let mut call = self.client.post(self.endpoint.clone()).json(&request);
            if let Some(bearer) = &self.bearer {
                call = call.bearer_auth(bearer);
            }
            let response = context
                .wait(async {
                    call.send()
                        .await
                        .map_err(|e| Error::Transport(e.without_url().to_string()))
                })
                .await?;
            if !response.status().is_success() {
                return Err(Error::Provider {
                    status: response.status().as_u16(),
                });
            }
            let sse = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.starts_with("text/event-stream"));
            let mut chunks = response.bytes_stream();
            let mut decoder = SseDecoder::default();
            let mut json_body = Vec::new();
            let mut received = 0usize;
            let mut completed = None;
            while let Some(chunk) = context.wait(async { Ok(chunks.next().await) }).await? {
                let chunk = chunk.map_err(|e| Error::Transport(e.without_url().to_string()))?;
                received = received.saturating_add(chunk.len());
                if received > 16 * 1024 * 1024 {
                    return Err(Error::Limit("response exceeds 16 MiB".into()));
                }
                if !sse {
                    json_body.extend_from_slice(&chunk);
                    continue;
                }
                for event in decoder.feed(&chunk)? {
                    observe_event(event, &context, &mut completed).await?;
                }
            }
            if !sse {
                let value = serde_json::from_slice(&json_body)
                    .map_err(|_| Error::Protocol("invalid response JSON".into()))?;
                return ModelResponse::from_value(value);
            }
            for event in decoder.finish()? {
                observe_event(event, &context, &mut completed).await?;
            }
            completed
                .ok_or_else(|| Error::Protocol("stream ended before response.completed".into()))
        })
    }
}

async fn observe_event(
    event: Value,
    context: &RunContext,
    completed: &mut Option<ModelResponse>,
) -> Result<()> {
    let kind = event["type"].as_str().unwrap_or("");
    if matches!(kind, "error" | "response.failed" | "response.incomplete") {
        return Err(Error::Protocol(format!("upstream event {kind}")));
    }
    if matches!(kind, "response.completed" | "response.done") {
        *completed = Some(ModelResponse::from_value(event["response"].clone())?);
    }
    context.emit(event).await
}

/// Incremental SSE decoder: handles split UTF-8, CRLF, comments and multiline data.
#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<u8>,
}
impl SseDecoder {
    fn line(&mut self, line: &[u8], output: &mut Vec<Value>) -> Result<()> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            if !self.data.is_empty() {
                if self.data != b"[DONE]" {
                    output.push(
                        serde_json::from_slice(&self.data)
                            .map_err(|_| Error::Protocol("invalid SSE JSON".into()))?,
                    );
                }
                self.data.clear();
            }
        } else if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(data);
            if self.data.len() > 1024 * 1024 {
                return Err(Error::Limit("SSE event exceeds 1 MiB".into()));
            }
        }
        Ok(())
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Value>> {
        self.buffer.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some(index) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            self.line(&line[..line.len() - 1], &mut output)?;
        }
        if self.buffer.len() > 1024 * 1024 {
            return Err(Error::Limit("SSE line exceeds 1 MiB".into()));
        }
        Ok(output)
    }
    fn finish(mut self) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        let last = std::mem::take(&mut self.buffer);
        if !last.is_empty() {
            self.line(&last, &mut result)?;
        }
        self.line(b"", &mut result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_handles_every_byte_boundary() {
        let bytes = ":ping\r\nevent: x\r\ndata: {\"type\":\"文字\",\r\ndata: \"delta\":\"😊\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
        for split in 0..bytes.len() {
            let mut decoder = SseDecoder::default();
            let mut out = decoder.feed(&bytes[..split]).unwrap();
            out.extend(decoder.feed(&bytes[split..]).unwrap());
            out.extend(decoder.finish().unwrap());
            assert_eq!(out, vec![serde_json::json!({"type":"文字","delta":"😊"})]);
        }
    }
}
