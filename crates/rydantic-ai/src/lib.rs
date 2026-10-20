//! Typed agents, explicit graph transitions and a metered Responses gateway.
//!
//! Providers implement [`ModelProvider`]. Both agent outputs and tool arguments use
//! Rydantic's runtime validation, including async validators. HTTP and the pi Codex
//! bridge are optional; graphs and custom providers work without default features.

mod agent;
mod context;
mod error;
pub mod gateway;
pub mod graph;
#[cfg(feature = "http")]
mod http;
pub mod jev;
mod protocol;
pub mod schema;
pub mod structured;
mod tool;

pub use agent::{Agent, AgentLimits, RunResult};
pub use context::{BoxFuture, RunContext};
pub use error::{Error, Result};
#[cfg(feature = "http")]
pub use http::ResponsesProvider;
pub use protocol::{
    Capabilities, ModelProvider, ModelRequest, ModelResponse, ToolCall, ToolDefinition, Usage,
};
pub use rydantic;
pub use structured::{
    Iterable, IterableEvent, OutputEvent, Partial, PartialEvent, PartialSnapshot, ResetReason,
    StreamItem, StreamOptions,
};
pub use tokio_util::sync::CancellationToken;
pub use tool::{Tool, TypedTool};
