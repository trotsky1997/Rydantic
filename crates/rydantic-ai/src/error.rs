use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Configuration(String),
    #[error("model validation failed: {0}")]
    Validation(#[from] rydantic::ValidationErrors),
    #[error("tool arguments failed validation: {0}")]
    ToolArguments(rydantic::ValidationErrors),
    #[error("unknown tool")]
    UnknownTool,
    #[error("request cancelled")]
    Cancelled,
    #[error("request deadline exceeded")]
    Timeout,
    #[error("limit reached: {0}")]
    Limit(String),
    #[error("provider request failed (HTTP {status})")]
    Provider { status: u16 },
    #[error("provider transport failed: {0}")]
    Transport(String),
    #[error("invalid provider response: {0}")]
    Protocol(String),
    #[error("model refused the request")]
    Refusal,
    #[error("tool execution failed: {0}")]
    Tool(String),
    #[error("gateway authorization failed")]
    Unauthorized,
    #[error("gateway budget exhausted")]
    Budget,
    #[error("unknown model route")]
    UnknownRoute,
    #[error("graph error: {0}")]
    Graph(String),
}

impl Error {
    /// Only explicit transient HTTP failures are eligible for gateway failover.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Provider {
                status: 429 | 502 | 503 | 504
            }
        )
    }
}
