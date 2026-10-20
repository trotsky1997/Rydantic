use super::{bounded_append, field, history, sse::Frame, Api};
use crate::{Error, ModelRequest, ModelResponse, Result, Usage};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(super) fn request(request: &ModelRequest) -> Result<Value> {
    if request.reasoning.is_some() {
        return Err(Error::Configuration(
            "use Responses for reasoning request settings".into(),
        ));
    }
    let (system, messages) = history::messages(request)?;
    let mut body = json!({"model":request.model,"messages":messages,"max_tokens":request.max_output_tokens.unwrap_or(2048),"stream":true});
    if let Some(value) = request.temperature {
        if value > 1.0 {
            return Err(Error::Configuration(
                "Messages temperature must be at most 1".into(),
            ));
        }
        body["temperature"] = json!(value);
    }
    if let Some(value) = request.top_p {
        body["top_p"] = json!(value);
    }
    if let Some(choice) = &request.tool_choice {
        body["tool_choice"] = match choice.as_str() {
            Some("auto") => json!({"type":"auto"}),
            Some("none") => json!({"type":"none"}),
            Some("required") => json!({"type":"any"}),
            _ => json!({"type":"tool","name":choice["name"]}),
        };
    }
    if let Some(parallel) = request
        .parallel_tool_calls
        .filter(|_| !request.tools.is_empty())
    {
        if body.get("tool_choice").is_none() {
            body["tool_choice"] = json!({"type":"auto"});
        }
        if body["tool_choice"]["type"] != "none" {
            body["tool_choice"]["disable_parallel_tool_use"] = json!(!parallel);
        }
    }
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let Some(format) = request.text.as_ref().and_then(|v| v.get("format")) {
        match format["type"].as_str() {
            Some("text") => (),
            Some("json_schema") => {
                body["output_config"] =
                    json!({"format":{"type":"json_schema","schema":format["schema"]}})
            }
            _ => {
                return Err(Error::Configuration(
                    "unsupported structured output format".into(),
                ))
            }
        }
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools.iter().map(|tool| {
            let mut value = json!({"name":tool.name,"description":tool.description,"input_schema":tool.parameters});
            if tool.strict { value["strict"] = json!(true); } value
        }).collect::<Vec<_>>());
    }
    Ok(body)
}
fn usage(raw: &Value) -> Result<Usage> {
    let mut input_tokens = raw["input_tokens"]
        .as_u64()
        .ok_or_else(|| Error::Protocol("missing input token usage".into()))?;
    for name in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
        if let Some(value) = raw.get(name).filter(|v| !v.is_null()) {
            input_tokens = input_tokens
                .checked_add(
                    value
                        .as_u64()
                        .ok_or_else(|| Error::Protocol("invalid cached token usage".into()))?,
                )
                .ok_or_else(|| Error::Limit("token counter overflow".into()))?;
        }
    }
    let usage = Usage {
        input_tokens,
        output_tokens: raw["output_tokens"]
            .as_u64()
            .ok_or_else(|| Error::Protocol("missing output token usage".into()))?,
    };
    usage.total()?;
    Ok(usage)
}
pub(super) fn response(raw: Value) -> Result<ModelResponse> {
    field(&raw, "id")?;
    if raw["role"] != "assistant" || raw["type"] != "message" {
        return Err(Error::Protocol("invalid assistant message".into()));
    }
    if raw["stop_reason"] == "refusal" {
        return Err(Error::Refusal);
    }
    let mut text = String::new();
    let mut calls = Vec::new();
    let blocks = raw["content"]
        .as_array()
        .ok_or_else(|| Error::Protocol("missing content blocks".into()))?;
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => text.push_str(
                block["text"]
                    .as_str()
                    .ok_or_else(|| Error::Protocol("invalid text content".into()))?,
            ),
            Some("tool_use") => {
                if !block["input"].is_object() {
                    return Err(Error::Protocol("tool input must be an object".into()));
                }
                let arguments = serde_json::to_string(&block["input"])
                    .map_err(|_| Error::Protocol("invalid tool input".into()))?;
                calls.push(json!({"type":"function_call","call_id":field(block,"id")?,"name":field(block,"name")?,"arguments":arguments}));
            }
            Some("refusal") => return Err(Error::Refusal),
            // Opaque thinking, signatures and other native content stay in replay metadata.
            Some(_) => (),
            None => return Err(Error::Protocol("content block has no type".into())),
        }
    }
    match raw["stop_reason"].as_str() {
        Some("end_turn" | "stop_sequence") if calls.is_empty() => (),
        Some("tool_use") if !calls.is_empty() => (),
        _ => {
            return Err(Error::Protocol(
                "incomplete or inconsistent Messages response".into(),
            ))
        }
    }
    let usage = usage(&raw["usage"])?;
    history::normalized(
        Api::AnthropicMessages,
        raw.clone(),
        json!({"role":"assistant","content":blocks}),
        text,
        calls,
        usage,
    )
}

struct Block {
    value: Value,
    arguments: String,
    has_arguments: bool,
    closed: bool,
}
#[derive(Default)]
pub(super) struct Stream {
    pub(super) done: bool,
    message: Option<Value>,
    blocks: BTreeMap<u64, Block>,
    usage: Value,
    reason: Option<String>,
    last_text: Option<u64>,
    saw_delta: bool,
    final_usage: bool,
}
impl Stream {
    fn index(event: &Value) -> Result<u64> {
        event["index"]
            .as_u64()
            .filter(|i| *i < 4096)
            .ok_or_else(|| Error::Protocol("invalid content index".into()))
    }
    fn text(&mut self, index: u64, text: String) -> Result<Vec<String>> {
        if self.last_text.is_some_and(|old| old > index) {
            return Err(Error::Protocol("out-of-order text content".into()));
        }
        self.last_text = Some(index);
        Ok(if text.is_empty() { vec![] } else { vec![text] })
    }
    pub(super) fn push(&mut self, frame: Frame) -> Result<Vec<String>> {
        if self.done {
            return Err(Error::Protocol("data after message_stop".into()));
        }
        let event = match frame {
            Frame::Data(value) => value,
            Frame::Done => return Err(Error::Protocol("Messages requires message_stop".into())),
        };
        let kind = field(&event, "type")?;
        if kind == "error" {
            return Err(Error::Protocol("provider stream failed".into()));
        }
        if kind == "ping" {
            return Ok(vec![]);
        }
        if kind == "message_start" {
            if self.message.is_some() {
                return Err(Error::Protocol("duplicate message_start".into()));
            }
            let message = &event["message"];
            field(message, "id")?;
            if message["type"] != "message"
                || message["role"] != "assistant"
                || message["content"].as_array().is_none_or(|c| !c.is_empty())
            {
                return Err(Error::Protocol("invalid message_start".into()));
            }
            usage(&message["usage"])?;
            self.usage = message["usage"].clone();
            self.message = Some(message.clone());
            return Ok(vec![]);
        }
        if self.message.is_none() {
            return Err(Error::Protocol("event before message_start".into()));
        }
        match kind {
            "content_block_start" => {
                if self.saw_delta {
                    return Err(Error::Protocol("content after message_delta".into()));
                }
                let index = Self::index(&event)?;
                if index != self.blocks.len() as u64 {
                    return Err(Error::Protocol(
                        "duplicate or unordered content index".into(),
                    ));
                }
                let value = event["content_block"].clone();
                let kind = field(&value, "type")?;
                let initial = if kind == "text" {
                    Some(
                        value["text"]
                            .as_str()
                            .ok_or_else(|| Error::Protocol("invalid initial text".into()))?
                            .to_owned(),
                    )
                } else {
                    None
                };
                self.blocks.insert(
                    index,
                    Block {
                        value,
                        arguments: String::new(),
                        has_arguments: false,
                        closed: false,
                    },
                );
                if let Some(text) = initial {
                    return self.text(index, text);
                }
            }
            "content_block_delta" => {
                if self.saw_delta {
                    return Err(Error::Protocol("content after message_delta".into()));
                }
                let index = Self::index(&event)?;
                let block = self
                    .blocks
                    .get_mut(&index)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| Error::Protocol("delta without an open content block".into()))?;
                let delta = &event["delta"];
                let (key, fragment) = match field(delta, "type")? {
                    "text_delta" => ("text", field_allow_empty(delta, "text")?),
                    "thinking_delta" => ("thinking", field_allow_empty(delta, "thinking")?),
                    "signature_delta" => ("signature", field_allow_empty(delta, "signature")?),
                    "input_json_delta" => {
                        if block.value["type"] != "tool_use" {
                            return Err(Error::Protocol(
                                "tool JSON delta on another block type".into(),
                            ));
                        }
                        bounded_append(
                            &mut block.arguments,
                            field_allow_empty(delta, "partial_json")?,
                        )?;
                        block.has_arguments = true;
                        return Ok(vec![]);
                    }
                    "citations_delta" => {
                        if block.value.get("citations").is_none() {
                            block.value["citations"] = json!([]);
                        }
                        block.value["citations"]
                            .as_array_mut()
                            .ok_or_else(|| Error::Protocol("invalid citations".into()))?
                            .push(delta["citation"].clone());
                        return Ok(vec![]);
                    }
                    _ => return Ok(vec![]),
                };
                if (key == "text" && block.value["type"] != "text")
                    || (matches!(key, "thinking" | "signature")
                        && block.value["type"] != "thinking")
                {
                    return Err(Error::Protocol("delta does not match its block".into()));
                }
                let mut value = block.value[key].as_str().unwrap_or_default().to_owned();
                bounded_append(&mut value, fragment)?;
                block.value[key] = json!(value);
                if key == "text" {
                    return self.text(index, fragment.to_owned());
                }
            }
            "content_block_stop" => {
                let index = Self::index(&event)?;
                let block = self
                    .blocks
                    .get_mut(&index)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| Error::Protocol("stop without an open block".into()))?;
                if block.has_arguments {
                    block.value["input"] = serde_json::from_str(&block.arguments)
                        .map_err(|_| Error::Protocol("invalid streamed tool JSON".into()))?;
                }
                block.closed = true;
            }
            "message_delta" => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(Error::Protocol(
                        "message_delta before content closed".into(),
                    ));
                }
                if let Some(reason) = event["delta"].get("stop_reason").filter(|v| !v.is_null()) {
                    let reason = reason
                        .as_str()
                        .ok_or_else(|| Error::Protocol("invalid stop reason".into()))?;
                    if self.reason.as_ref().is_some_and(|old| old != reason) {
                        return Err(Error::Protocol("stop reason changed".into()));
                    }
                    self.reason = Some(reason.into());
                }
                if let Some(sequence) = event["delta"].get("stop_sequence") {
                    if !sequence.is_null() && !sequence.is_string() {
                        return Err(Error::Protocol("invalid stop sequence".into()));
                    }
                    self.message.as_mut().expect("started message")["stop_sequence"] =
                        sequence.clone();
                }
                if let Some(delta_usage) = event.get("usage") {
                    let delta_usage = delta_usage
                        .as_object()
                        .ok_or_else(|| Error::Protocol("invalid message usage".into()))?;
                    if let Some(output) = delta_usage.get("output_tokens") {
                        if output.as_u64().is_none() {
                            return Err(Error::Protocol("invalid output token usage".into()));
                        }
                        self.final_usage = true;
                    }
                    for (key, value) in delta_usage {
                        if !value.is_null() {
                            self.usage[key] = value.clone();
                        }
                    }
                    usage(&self.usage)?;
                }
                self.saw_delta = true;
            }
            "message_stop" => {
                if !self.saw_delta
                    || !self.final_usage
                    || self.reason.is_none()
                    || self.blocks.values().any(|b| !b.closed)
                {
                    return Err(Error::Protocol("premature message_stop".into()));
                }
                self.done = true;
            }
            _ => (),
        }
        Ok(vec![])
    }
    pub(super) fn finish(self) -> Result<ModelResponse> {
        if !self.done {
            return Err(Error::Protocol("stream ended before message_stop".into()));
        }
        let mut message = self
            .message
            .ok_or_else(|| Error::Protocol("missing message_start".into()))?;
        message["content"] = json!(self
            .blocks
            .into_values()
            .map(|block| block.value)
            .collect::<Vec<_>>());
        message["stop_reason"] = json!(self.reason);
        message["usage"] = self.usage;
        response(message)
    }
}
fn field_allow_empty<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| Error::Protocol(format!("invalid {name}")))
}
