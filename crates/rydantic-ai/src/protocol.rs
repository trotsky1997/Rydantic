use crate::{content, BoxFuture, Error, Prompt, Result, RunContext};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub store: bool,
}

impl ModelRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<Prompt>) -> Self {
        Self {
            model: model.into(),
            input: vec![json!({"role":"user", "content":prompt.into().responses()})],
            instructions: None,
            tools: vec![],
            text: None,
            max_output_tokens: None,
            reasoning: None,
            tool_choice: None,
            parallel_tool_calls: None,
            temperature: None,
            top_p: None,
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
        if self
            .temperature
            .is_some_and(|v| !v.is_finite() || !(0.0..=2.0).contains(&v))
            || self
                .top_p
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(Error::Configuration("invalid sampling settings".into()));
        }
        if let Some(choice) = &self.tool_choice {
            let valid = matches!(choice.as_str(), Some("auto" | "none" | "required"))
                || (choice["type"] == "function"
                    && choice["name"].as_str().is_some_and(|v| !v.is_empty())
                    && choice.as_object().is_some_and(|v| v.len() == 2));
            if !valid {
                return Err(Error::Configuration("unsupported tool_choice".into()));
            }
            if choice != "none" && self.tools.is_empty() {
                return Err(Error::Configuration("tool_choice requires tools".into()));
            }
        }
        content::validate_input(&self.input)?;
        if serde_json::to_vec(self)
            .map_err(|e| Error::Protocol(e.to_string()))?
            .len()
            > content::MAX_REQUEST_BYTES
        {
            return Err(Error::Limit("request exceeds 16 MiB".into()));
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
