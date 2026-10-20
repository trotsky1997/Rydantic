use crate::{BoxFuture, Error, Result, RunContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn total(self) -> Result<u64> {
        self.input_tokens
            .checked_add(self.output_tokens)
            .ok_or_else(|| Error::Limit("token counter overflow".into()))
    }
    pub fn add(&mut self, other: Self) -> Result<()> {
        let input = self.input_tokens.checked_add(other.input_tokens);
        let output = self.output_tokens.checked_add(other.output_tokens);
        *self = Self {
            input_tokens: input.ok_or_else(|| Error::Limit("token counter overflow".into()))?,
            output_tokens: output.ok_or_else(|| Error::Limit("token counter overflow".into()))?,
        };
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub native_json_schema: bool,
    pub output_token_limit: bool,
}
impl Default for Capabilities {
    fn default() -> Self {
        Self {
            native_json_schema: true,
            output_token_limit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub strict: bool,
}

/// Supported Responses request fields. Unknown fields fail deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub model: String,
    pub input: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub store: bool,
}

impl ModelRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: vec![json!({"role":"user", "content":prompt.into()})],
            instructions: None,
            tools: vec![],
            text: None,
            max_output_tokens: None,
            reasoning: None,
            stream: false,
            store: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty() || self.model.len() > 256 || self.input.is_empty() {
            return Err(Error::Configuration("model and input are required".into()));
        }
        if self.store {
            return Err(Error::Configuration("stored responses are disabled".into()));
        }
        if self.max_output_tokens == Some(0) {
            return Err(Error::Configuration(
                "max_output_tokens must be positive".into(),
            ));
        }
        if serde_json::to_vec(self)
            .map_err(|e| Error::Protocol(e.to_string()))?
            .len()
            > 1024 * 1024
        {
            return Err(Error::Limit("request exceeds 1 MiB".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Retains native output items, including opaque reasoning needed by later turns.
#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub raw: Value,
    pub usage: Usage,
}

impl ModelResponse {
    pub fn from_value(raw: Value) -> Result<Self> {
        if raw["status"] != "completed" {
            return Err(Error::Protocol("response did not complete".into()));
        }
        if !raw["output"].is_array() {
            return Err(Error::Protocol("response has no output array".into()));
        }
        let usage = Usage {
            input_tokens: raw["usage"]["input_tokens"]
                .as_u64()
                .ok_or_else(|| Error::Protocol("missing input token usage".into()))?,
            output_tokens: raw["usage"]["output_tokens"]
                .as_u64()
                .ok_or_else(|| Error::Protocol("missing output token usage".into()))?,
        };
        usage.total()?;
        Ok(Self { raw, usage })
    }
    pub fn output(&self) -> &[Value] {
        self.raw["output"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
    pub fn text(&self) -> Result<String> {
        let mut text = String::new();
        for item in self.output() {
            if let Some(parts) = item["content"].as_array() {
                for part in parts {
                    if part["type"] == "refusal" {
                        return Err(Error::Refusal);
                    }
                    if part["type"] == "output_text" {
                        text.push_str(
                            part["text"]
                                .as_str()
                                .ok_or_else(|| Error::Protocol("non-text output_text".into()))?,
                        );
                    }
                }
            }
        }
        Ok(text)
    }
    pub fn tool_calls(&self) -> Result<Vec<ToolCall>> {
        self.output()
            .iter()
            .filter(|item| item["type"] == "function_call")
            .map(|item| {
                let field = |name: &str| {
                    item[name]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| Error::Protocol(format!("function call missing {name}")))
                };
                let call = ToolCall {
                    call_id: field("call_id")?,
                    name: field("name")?,
                    arguments: field("arguments")?,
                };
                if call.arguments.len() > 1024 * 1024 {
                    return Err(Error::Limit("tool arguments exceed 1 MiB".into()));
                }
                Ok(call)
            })
            .collect()
    }
}

pub trait ModelProvider: Send + Sync {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn complete(
        &self,
        request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>>;
}
