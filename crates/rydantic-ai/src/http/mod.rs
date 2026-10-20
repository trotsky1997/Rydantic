//! HTTP protocol adapters with shared cancellation, bounds and credential handling.
mod chat;
mod history;
mod messages;
mod sse;
pub(crate) use history::{chat as chat_history, messages as messages_history};

pub(crate) fn chat_message(message: Value) -> Result<Vec<Value>> {
    let tools = message["tool_calls"]
        .as_array()
        .is_some_and(|v| !v.is_empty());
    chat::response(json!({"id":"history","choices":[{"index":0,"message":message,"finish_reason":if tools {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":0,"completion_tokens":0}}))
        .map(|response| response.output().to_vec())
        .map_err(|_| Error::Configuration("invalid assistant history".into()))
}
pub(crate) fn messages_message(content: Value) -> Result<Vec<Value>> {
    let tools = content
        .as_array()
        .is_some_and(|v| v.iter().any(|b| b["type"] == "tool_use"));
    messages::response(json!({"id":"history","type":"message","role":"assistant","content":content,"stop_reason":if tools {"tool_use"} else {"end_turn"},"usage":{"input_tokens":0,"output_tokens":0}}))
        .map(|response| response.output().to_vec())
        .map_err(|_| Error::Configuration("invalid assistant history".into()))
}
use crate::{
    BoxFuture, Capabilities, Error, ModelProvider, ModelRequest, ModelResponse, Result, RunContext,
};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sse::{Frame, SseDecoder};
use std::{fmt, str::FromStr};

/// The upstream wire protocol. BASEURL includes its API prefix, such as `/v1`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Api {
    #[default]
    Responses,
    ChatCompletions,
    AnthropicMessages,
}
impl FromStr for Api {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "responses" => Ok(Self::Responses),
            "chat_completions" => Ok(Self::ChatCompletions),
            "anthropic_messages" => Ok(Self::AnthropicMessages),
            _ => Err(Error::Configuration(
                "API must be responses, chat_completions or anthropic_messages".into(),
            )),
        }
    }
}
impl Api {
    fn path(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::ChatCompletions => "chat/completions",
            Self::AnthropicMessages => "messages",
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::ChatCompletions => "chat_completions",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }
}

/// A configurable HTTP provider for any supported upstream API.
///
/// No automatic request retries or redirects are performed. Credentials and URL
/// values are redacted from Debug and transport errors. API errors omit bodies.
#[derive(Clone)]
pub struct HttpProvider {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    api: Api,
    capabilities: Capabilities,
    preserve_native_history: bool,
}
impl fmt::Debug for HttpProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpProvider")
            .field("api", &self.api)
            .field("BASEURL", &"[redacted]")
            .field("APIKEY", &"[redacted]")
            .finish()
    }
}
impl HttpProvider {
    pub fn new(api: Api, base_url: &str, api_key: Option<String>) -> Result<Self> {
        let base = reqwest::Url::parse(base_url)
            .map_err(|_| Error::Configuration("invalid BASEURL".into()))?;
        let local = base.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !(base.scheme() == "https" || (base.scheme() == "http" && local))
        {
            return Err(Error::Configuration(
                "BASEURL must use HTTPS or loopback HTTP, without credentials, query or fragment"
                    .into(),
            ));
        }
        let endpoint = reqwest::Url::parse(&format!(
            "{}/{}",
            base.as_str().trim_end_matches('/'),
            api.path()
        ))
        .map_err(|_| Error::Configuration("invalid BASEURL".into()))?;
        let mut headers = HeaderMap::new();
        if api == Api::AnthropicMessages {
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        if let Some(key) = api_key {
            if key.is_empty() || key.bytes().any(|b| b.is_ascii_control()) {
                return Err(Error::Configuration("invalid APIKEY".into()));
            }
            let (name, value) = if api == Api::AnthropicMessages {
                ("x-api-key", key)
            } else {
                (AUTHORIZATION.as_str(), format!("Bearer {key}"))
            };
            let mut value = HeaderValue::from_str(&value)
                .map_err(|_| Error::Configuration("invalid APIKEY".into()))?;
            value.set_sensitive(true);
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .expect("static header name"),
                value,
            );
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|_| Error::Transport("HTTP client initialization failed".into()))?;
        Ok(Self {
            client,
            endpoint,
            api,
            preserve_native_history: false,
            capabilities: Capabilities {
                native_json_schema: api != Api::AnthropicMessages,
                output_token_limit: true,
            },
        })
    }
    /// Connect to a Rydantic gateway, retaining native message replay metadata
    /// across its Responses HTTP interface. Direct upstreams use `new`.
    pub fn gateway(base_url: &str, api_key: Option<String>) -> Result<Self> {
        let mut provider = Self::new(Api::Responses, base_url, api_key)?;
        provider.preserve_native_history = true;
        // Agent runtime validation covers routes with differing schema support.
        provider.capabilities.native_json_schema = false;
        Ok(provider)
    }
    /// Read BASEURL and an optional APIKEY. MODEL is selected by the Agent.
    pub fn from_env(api: Api) -> Result<Self> {
        let base = std::env::var("BASEURL")
            .map_err(|_| Error::Configuration("set BASEURL locally".into()))?;
        let key = match std::env::var("APIKEY") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(Error::Configuration("invalid APIKEY".into())),
        };
        Self::new(api, &base, key)
    }
    /// Override endpoint capabilities explicitly; validation remains active.
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    async fn execute(
        &self,
        mut request: ModelRequest,
        context: RunContext,
    ) -> Result<ModelResponse> {
        request.validate()?;
        if self.api != Api::Responses && request.tools.iter().any(|tool| tool.kind != "function") {
            return Err(Error::Configuration(
                "this adapter accepts function tools".into(),
            ));
        }
        if request.max_output_tokens.is_some() && !self.capabilities.output_token_limit {
            return Err(Error::Configuration(
                "this endpoint has no output-token request limit".into(),
            ));
        }
        request.stream = true;
        let body = match self.api {
            Api::Responses => history::responses_request(&request, self.preserve_native_history)?,
            Api::ChatCompletions => chat::request(&request)?,
            Api::AnthropicMessages => messages::request(&request)?,
        };
        let body = serde_json::to_vec(&body)
            .map_err(|_| Error::Configuration("invalid provider request".into()))?;
        if body.len() > 1024 * 1024 {
            return Err(Error::Limit("provider request exceeds 1 MiB".into()));
        }
        let call = self
            .client
            .post(self.endpoint.clone())
            .header("content-type", "application/json")
            .body(body);
        let response = context
            .wait(async {
                call.send()
                    .await
                    .map_err(|_| Error::Transport("provider connection failed".into()))
            })
            .await?;
        if !response.status().is_success() {
            return Err(Error::Provider {
                status: response.status().as_u16(),
            });
        }
        let streaming = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let mut chunks = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut state = State::new(self.api);
        let mut bytes = Vec::new();
        let mut received = 0usize;
        while let Some(chunk) = context.wait(async { Ok(chunks.next().await) }).await? {
            let chunk =
                chunk.map_err(|_| Error::Transport("provider response transfer failed".into()))?;
            received = received.saturating_add(chunk.len());
            if received > 16 * 1024 * 1024 {
                return Err(Error::Limit("response exceeds 16 MiB".into()));
            }
            if !streaming {
                bytes.extend_from_slice(&chunk);
                continue;
            }
            for frame in decoder.feed(&chunk)? {
                state.observe(frame, &context).await?;
            }
            if state.terminal() {
                return state.finish(&context).await;
            }
        }
        if !streaming {
            let raw: Value = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Protocol("invalid provider JSON".into()))?;
            let response = match self.api {
                Api::Responses => ModelResponse::from_value(raw)?,
                Api::ChatCompletions => chat::response(raw)?,
                Api::AnthropicMessages => messages::response(raw)?,
            };
            context
                .emit(json!({"type":"response.completed","response":response.raw}))
                .await?;
            return Ok(response);
        }
        for frame in decoder.finish()? {
            state.observe(frame, &context).await?;
        }
        state.finish(&context).await
    }
}
impl ModelProvider for HttpProvider {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn complete(
        &self,
        request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(self.execute(request, context))
    }
}

macro_rules! provider {
    ($name:ident, $api:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug)]
        pub struct $name(HttpProvider);
        impl $name {
            pub fn new(base_url: &str, api_key: Option<String>) -> Result<Self> {
                Ok(Self(HttpProvider::new(Api::$api, base_url, api_key)?))
            }
            pub fn from_env() -> Result<Self> {
                Ok(Self(HttpProvider::from_env(Api::$api)?))
            }
            pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
                self.0 = self.0.with_capabilities(capabilities);
                self
            }
        }
        impl ModelProvider for $name {
            fn capabilities(&self) -> Capabilities {
                self.0.capabilities()
            }
            fn complete(
                &self,
                request: ModelRequest,
                context: RunContext,
            ) -> BoxFuture<'_, Result<ModelResponse>> {
                self.0.complete(request, context)
            }
        }
    };
}
provider!(
    ResponsesProvider,
    Responses,
    "Standard Responses API provider, using BASEURL and optional bearer APIKEY."
);
provider!(
    ChatCompletionsProvider,
    ChatCompletions,
    "Chat Completions API provider with native tools, structured output and streamed usage."
);
provider!(
    AnthropicMessagesProvider,
    AnthropicMessages,
    "Anthropic Messages API provider with native tool use, thinking-block replay and streaming."
);
impl ResponsesProvider {
    /// Connect to a Rydantic gateway and retain same-protocol native history.
    pub fn gateway(base_url: &str, api_key: Option<String>) -> Result<Self> {
        Ok(Self(HttpProvider::gateway(base_url, api_key)?))
    }
}

enum State {
    Responses(Option<ModelResponse>),
    Chat(chat::Stream),
    Messages(messages::Stream),
}
impl State {
    fn new(api: Api) -> Self {
        match api {
            Api::Responses => Self::Responses(None),
            Api::ChatCompletions => Self::Chat(Default::default()),
            Api::AnthropicMessages => Self::Messages(Default::default()),
        }
    }
    fn terminal(&self) -> bool {
        match self {
            Self::Responses(r) => r.is_some(),
            Self::Chat(s) => s.done,
            Self::Messages(s) => s.done,
        }
    }
    async fn observe(&mut self, frame: Frame, context: &RunContext) -> Result<()> {
        match self {
            Self::Responses(completed) => {
                if let Frame::Data(event) = frame {
                    let kind = event["type"].as_str().unwrap_or("");
                    if matches!(kind, "error" | "response.failed" | "response.incomplete") {
                        return Err(Error::Protocol("provider response failed".into()));
                    }
                    if matches!(kind, "response.completed" | "response.done") {
                        let response = ModelResponse::from_value(event["response"].clone())?;
                        if completed
                            .as_ref()
                            .is_some_and(|old| old.raw != response.raw)
                        {
                            return Err(Error::Protocol("conflicting terminal responses".into()));
                        }
                        *completed = Some(response);
                    }
                    context.emit(event).await?;
                }
            }
            Self::Chat(stream) => {
                let event = if let Frame::Data(event) = &frame {
                    Some(event.clone())
                } else {
                    None
                };
                let deltas = stream.push(frame)?;
                if let Some(event) = event {
                    native(context, Api::ChatCompletions, &event).await?;
                }
                for text in deltas {
                    text_delta(context, &text).await?;
                }
            }
            Self::Messages(stream) => {
                let event = if let Frame::Data(event) = &frame {
                    Some(event.clone())
                } else {
                    None
                };
                let deltas = stream.push(frame)?;
                if let Some(event) = event {
                    native(context, Api::AnthropicMessages, &event).await?;
                }
                for text in deltas {
                    text_delta(context, &text).await?;
                }
            }
        }
        Ok(())
    }
    async fn finish(self, context: &RunContext) -> Result<ModelResponse> {
        let response = match self {
            Self::Responses(response) => {
                return response.ok_or_else(|| {
                    Error::Protocol("stream ended before response.completed".into())
                })
            }
            Self::Chat(stream) => stream.finish()?,
            Self::Messages(stream) => stream.finish()?,
        };
        context
            .emit(json!({"type":"response.completed","response":response.raw}))
            .await?;
        Ok(response)
    }
}
async fn native(context: &RunContext, api: Api, event: &Value) -> Result<()> {
    context
        .emit(json!({"type":"provider.event","api":api.name(),"event":event}))
        .await
}
async fn text_delta(context: &RunContext, text: &str) -> Result<()> {
    context.emit(json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"text","delta":text})).await
}

pub(super) fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::Protocol(format!("missing or invalid {name}")))
}
pub(super) fn bounded_append(buffer: &mut String, text: &str) -> Result<()> {
    if text.len() > (1024 * 1024usize).saturating_sub(buffer.len()) {
        return Err(Error::Limit("provider field exceeds 1 MiB".into()));
    }
    buffer.push_str(text);
    Ok(())
}
