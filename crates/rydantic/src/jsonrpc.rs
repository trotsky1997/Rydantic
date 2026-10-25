//! Opt-in, synchronous, transport-neutral JSON-RPC 2.0 server adapter.
//!
//! Requests and batches are processed sequentially. A `None` reply means send
//! **nothing**, not JSON null or an empty array. The application owns framing,
//! body/batch limits, authentication, deadlines and panic handling. This module
//! does not implement a client, network listener, async validation or context.
use crate::{Model, Value};
use std::collections::BTreeMap;

/// The parameter structure accepted by a method. No positional/name conversion
/// is performed. Omitted params become the corresponding empty structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Params {
    Named,
    Positional,
}

/// A public protocol error. Model errors are deliberately redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    code: i32,
    message: String,
}
impl Error {
    fn standard(code: i32, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    /// Application-defined error, outside the protocol-reserved range.
    /// The message is sent to the client; do not include secrets or raw inputs.
    pub fn application(code: i32, message: impl Into<String>) -> Option<Self> {
        if (-32768..=-32000).contains(&code) {
            return None;
        }
        Some(Self {
            code,
            message: message.into(),
        })
    }
    pub fn invalid_params() -> Self {
        Self::standard(-32602, "Invalid params")
    }
    pub fn internal() -> Self {
        Self::standard(-32603, "Internal error")
    }
    pub fn code(&self) -> i32 {
        self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

/// Registration refuses duplicate or JSON-RPC-reserved method names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    Duplicate,
    ReservedName,
}
impl std::fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Duplicate => "duplicate method",
            Self::ReservedName => "reserved rpc. method name",
        })
    }
}
impl std::error::Error for RegistrationError {}

type Handler = Box<dyn Fn(Value) -> Result<Value, Error> + Send + Sync>;
struct Method {
    params: Params,
    handler: Handler,
}

/// A registry of synchronous typed methods. No new dependencies are enabled.
#[derive(Default)]
pub struct Router {
    methods: BTreeMap<String, Method>,
}
impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a method with named struct or positional tuple-model parameters.
    /// Input parsing applies the model's normal parsing/normalization rules.
    /// Outputs are revalidated and serialized through `Model::dump`; they are
    /// not normalized. Async/context-dependent models are not supported here.
    /// Errors from the callback are explicitly public application/protocol errors.
    pub fn register<P, R, F>(
        &mut self,
        name: impl Into<String>,
        params: Params,
        handler: F,
    ) -> Result<(), RegistrationError>
    where
        P: Model,
        R: Model,
        F: Fn(P) -> Result<R, Error> + Send + Sync + 'static,
    {
        let name = name.into();
        if name.starts_with("rpc.") {
            return Err(RegistrationError::ReservedName);
        }
        if self.methods.contains_key(&name) {
            return Err(RegistrationError::Duplicate);
        }
        self.methods.insert(
            name,
            Method {
                params,
                handler: Box::new(move |value| {
                    let input = P::parse_value(value).map_err(|_| Error::invalid_params())?;
                    handler(input)?.dump().map_err(|_| Error::internal())
                }),
            },
        );
        Ok(())
    }

    /// Parse one complete JSON document, then handle it. Invalid JSON produces
    /// -32700; valid JSON with an invalid envelope produces -32600.
    /// As with serde_json::Value, duplicate object keys use the last value.
    pub fn handle_json(&self, input: &str) -> Option<Value> {
        match serde_json::from_str::<&crate::RawValue>(input).and_then(|raw| materialize(raw, 0)) {
            Ok(value) => self.handle_value(value),
            Err(_) => Some(response(
                Value::Null,
                Err(Error::standard(-32700, "Parse error")),
            )),
        }
    }

    /// Handle a single request or sequential batch. Nested batches are invalid.
    /// Unknown envelope members are ignored; method matching is case-sensitive.
    /// IDs are retained as JSON values without integer/floating-point coercion.
    pub fn handle_value(&self, input: Value) -> Option<Value> {
        match input {
            Value::Array(items) if !items.is_empty() => {
                let replies: Vec<_> = items.into_iter().filter_map(|v| self.single(v)).collect();
                if replies.is_empty() {
                    None
                } else {
                    Some(Value::Array(replies))
                }
            }
            input => self.single(input),
        }
    }

    fn single(&self, input: Value) -> Option<Value> {
        let invalid = || {
            Some(response(
                Value::Null,
                Err(Error::standard(-32600, "Invalid Request")),
            ))
        };
        let Value::Object(mut request) = input else {
            return invalid();
        };
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return invalid();
        }
        let Some(name) = request.get("method").and_then(Value::as_str) else {
            return invalid();
        };
        if request
            .get("id")
            .is_some_and(|v| !matches!(v, Value::Null | Value::String(_) | Value::Number(_)))
        {
            return invalid();
        }
        if request
            .get("params")
            .is_some_and(|v| !matches!(v, Value::Object(_) | Value::Array(_)))
        {
            return invalid();
        }
        let method = self.methods.get(name);
        let id = request.remove("id");
        let outcome = match method {
            None => Err(Error::standard(-32601, "Method not found")),
            Some(method) => {
                let params = request
                    .remove("params")
                    .unwrap_or_else(|| match method.params {
                        Params::Named => Value::Object(Default::default()),
                        Params::Positional => Value::Array(Vec::new()),
                    });
                match (&params, method.params) {
                    (Value::Object(_), Params::Named) | (Value::Array(_), Params::Positional) => {
                        (method.handler)(params)
                    }
                    _ => Err(Error::invalid_params()),
                }
            }
        };
        // Execute valid notifications too, including model validation, but never reply.
        id.map(|id| response(id, outcome))
    }
}

fn response(id: Value, outcome: Result<Value, Error>) -> Value {
    match outcome {
        Ok(result) => crate::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => {
            crate::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": error.code, "message": error.message } })
        }
    }
}

// Deserialize containers as raw children: Value's arbitrary_precision/raw_value
// visitor recognizes private object keys as internal type markers. Those keys
// are ordinary client data, never permission to change an envelope's JSON type.
fn materialize(raw: &crate::RawValue, depth: usize) -> Result<Value, serde_json::Error> {
    if depth >= 128 {
        return Err(<serde_json::Error as serde::de::Error>::custom(
            "JSON nesting limit exceeded",
        ));
    }
    match raw.get().as_bytes().first() {
        Some(b'{') => {
            let fields: BTreeMap<String, &crate::RawValue> = serde_json::from_str(raw.get())?;
            fields
                .into_iter()
                .map(|(key, child)| Ok((key, materialize(child, depth + 1)?)))
                .collect::<Result<serde_json::Map<String, Value>, _>>()
                .map(Value::Object)
        }
        Some(b'[') => {
            let items: Vec<&crate::RawValue> = serde_json::from_str(raw.get())?;
            items
                .into_iter()
                .map(|child| materialize(child, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        _ => serde_json::from_str(raw.get()),
    }
}
