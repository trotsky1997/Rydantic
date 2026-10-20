use super::{ModelList, SystemOneRequest, SystemOneResponse};
use crate::{Error, Result, RunContext};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Url,
};
use serde::de::DeserializeOwned;
use std::{fmt, time::Duration};

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Async Jev client. Requests are sent once; redirects and automatic retries are disabled.
#[derive(Clone)]
pub struct JevClient {
    client: reqwest::Client,
    base: Url,
    authorization: HeaderValue,
}
impl fmt::Debug for JevClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JevClient")
            .field("base", &"[configured]")
            .field("authorization", &"[redacted]")
            .finish()
    }
}
impl JevClient {
    /// Read both endpoint and opaque bearer credential from the local environment.
    pub fn from_env() -> Result<Self> {
        let base = std::env::var("TYPESAFE_BASEURL")
            .map_err(|_| Error::Configuration("set TYPESAFE_BASEURL locally".into()))?;
        let key = std::env::var("TYPESAFE_APIKEY")
            .map_err(|_| Error::Configuration("set TYPESAFE_APIKEY locally".into()))?;
        Self::new(&base, &key)
    }
    /// Configure an explicit `/v1` API base and an opaque bearer credential.
    /// Prefer HTTPS for network endpoints. Redirects never forward credentials.
    pub fn new(base_url: &str, api_key: &str) -> Result<Self> {
        let base = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
            .map_err(|_| Error::Configuration("invalid Jev API base".into()))?;
        if base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !matches!(base.scheme(), "https" | "http")
        {
            return Err(Error::Configuration(
                "Jev base requires HTTP or HTTPS without URL credentials, query or fragment".into(),
            ));
        }
        if api_key.is_empty() || !api_key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::Configuration("invalid Jev API key".into()));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|_| Error::Configuration("invalid Jev API key".into()))?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| Error::Transport("Jev HTTP client initialization failed".into()))?;
        Ok(Self {
            client,
            base,
            authorization,
        })
    }
    pub async fn system_one(
        &self,
        request: &SystemOneRequest,
        context: RunContext,
    ) -> Result<SystemOneResponse> {
        context.check()?;
        request.validate()?;
        let body = serde_json::to_vec(request)
            .map_err(|_| Error::Configuration("cannot encode Jev request".into()))?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(Error::Limit("Jev request exceeds 1 MiB".into()));
        }
        let response: SystemOneResponse = self
            .execute(
                reqwest::Method::POST,
                "systemone",
                Some(body),
                context.clone(),
            )
            .await?;
        context.check()?;
        response.validate_for(request)?;
        context.check()?;
        Ok(response)
    }
    /// Model discovery uses the TypeSafe `/v1/models` response contract.
    pub async fn models(&self, context: RunContext) -> Result<ModelList> {
        self.execute(reqwest::Method::GET, "models", None, context)
            .await
    }
    async fn execute<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        context: RunContext,
    ) -> Result<T> {
        context
            .wait(async {
                let url = self
                    .base
                    .join(path)
                    .map_err(|_| Error::Configuration("invalid Jev endpoint".into()))?;
                let mut call = self
                    .client
                    .request(method, url)
                    .header(AUTHORIZATION, self.authorization.clone())
                    .header("accept", "application/json");
                if let Some(body) = body {
                    call = call.header("content-type", "application/json").body(body);
                }
                let response = call
                    .send()
                    .await
                    .map_err(|_| Error::Transport("Jev HTTP request failed".into()))?;
                if !response.status().is_success() {
                    return Err(Error::Provider {
                        status: response.status().as_u16(),
                    });
                }
                if response
                    .content_length()
                    .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
                {
                    return Err(Error::Limit("Jev response exceeds 8 MiB".into()));
                }
                let mut bytes = Vec::new();
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk =
                        chunk.map_err(|_| Error::Transport("Jev response read failed".into()))?;
                    if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
                        return Err(Error::Limit("Jev response exceeds 8 MiB".into()));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let decoded = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Protocol("invalid Jev JSON response".into()))?;
                context.check()?;
                Ok(decoded)
            })
            .await
    }
}
