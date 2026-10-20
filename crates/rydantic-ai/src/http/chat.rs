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
    let mut body = json!({"model":request.model,"messages":history::chat(request)?,"stream":true,"stream_options":{"include_usage":true},"store":false});
    if let Some(limit) = request.max_output_tokens {
        body["max_completion_tokens"] = json!(limit);
    }
    if let Some(format) = request.text.as_ref().and_then(|v| v.get("format")) {
        if format["type"] != "json_schema" {
            return Err(Error::Configuration(
                "unsupported structured output format".into(),
            ));
        }
        body["response_format"] = json!({"type":"json_schema","json_schema":{"name":format["name"],"schema":format["schema"],"strict":format["strict"]}});
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools.iter().map(|tool| json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters,"strict":tool.strict}})).collect::<Vec<_>>());
    }
    Ok(body)
}
fn usage(value: &Value) -> Result<Usage> {
    let usage = Usage {
        input_tokens: value["prompt_tokens"]
            .as_u64()
            .ok_or_else(|| Error::Protocol("missing prompt token usage".into()))?,
        output_tokens: value["completion_tokens"]
            .as_u64()
            .ok_or_else(|| Error::Protocol("missing completion token usage".into()))?,
    };
    let total = usage.total()?;
    if let Some(reported) = value.get("total_tokens") {
        if reported.as_u64() != Some(total) {
            return Err(Error::Protocol("inconsistent token usage".into()));
        }
    }
    Ok(usage)
}
pub(super) fn response(raw: Value) -> Result<ModelResponse> {
    field(&raw, "id")?;
    let choices = raw["choices"]
        .as_array()
        .filter(|c| c.len() == 1)
        .ok_or_else(|| Error::Protocol("expected one completion choice".into()))?;
    if choices[0]["index"].as_u64() != Some(0) {
        return Err(Error::Protocol("invalid choice index".into()));
    }
    let choice = &choices[0];
    let message = &choice["message"];
    if message["role"] != "assistant" {
        return Err(Error::Protocol("invalid assistant message".into()));
    }
    if message["refusal"].as_str().is_some_and(|s| !s.is_empty())
        || choice["finish_reason"] == "content_filter"
    {
        return Err(Error::Refusal);
    }
    let content = match &message["content"] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => return Err(Error::Protocol("non-text chat completion".into())),
    };
    let mut calls = Vec::new();
    if let Some(tools) = message.get("tool_calls").filter(|v| !v.is_null()) {
        for tool in tools
            .as_array()
            .ok_or_else(|| Error::Protocol("invalid tool calls".into()))?
        {
            if tool["type"] != "function" {
                return Err(Error::Protocol("unsupported tool type".into()));
            }
            calls.push(json!({"type":"function_call","call_id":field(tool,"id")?,"name":field(&tool["function"],"name")?,"arguments":field(&tool["function"],"arguments")?}));
        }
    }
    match choice["finish_reason"].as_str() {
        Some("stop") if calls.is_empty() => (),
        Some("tool_calls") if !calls.is_empty() => (),
        _ => {
            return Err(Error::Protocol(
                "incomplete or inconsistent chat completion".into(),
            ))
        }
    }
    let usage = usage(&raw["usage"])?;
    history::normalized(
        Api::ChatCompletions,
        raw.clone(),
        message.clone(),
        content,
        calls,
        usage,
    )
}

#[derive(Default)]
struct Tool {
    id: String,
    name: String,
    arguments: String,
}
#[derive(Default)]
pub(super) struct Stream {
    pub(super) done: bool,
    id: Option<String>,
    model: Value,
    content: String,
    refusal: String,
    reasoning: String,
    tools: BTreeMap<u64, Tool>,
    finish_reason: Option<String>,
    usage: Option<Value>,
}
impl Stream {
    pub(super) fn push(&mut self, frame: Frame) -> Result<Vec<String>> {
        if self.done {
            return Err(Error::Protocol("data after completion marker".into()));
        }
        let event = match frame {
            Frame::Done => {
                self.done = true;
                return Ok(vec![]);
            }
            Frame::Data(v) => v,
        };
        if event.get("error").is_some() {
            return Err(Error::Protocol("provider stream failed".into()));
        }
        let id = field(&event, "id")?;
        if self.id.as_ref().is_some_and(|old| old != id) {
            return Err(Error::Protocol("completion ID changed".into()));
        }
        self.id.get_or_insert_with(|| id.to_owned());
        self.model = event["model"].clone();
        if let Some(raw_usage) = event.get("usage").filter(|v| !v.is_null()) {
            usage(raw_usage)?;
            if self.usage.as_ref().is_some_and(|old| old != raw_usage) {
                return Err(Error::Protocol("conflicting usage chunks".into()));
            }
            self.usage = Some(raw_usage.clone());
        }
        let choices = event["choices"]
            .as_array()
            .filter(|c| c.len() <= 1)
            .ok_or_else(|| Error::Protocol("invalid completion choices".into()))?;
        let Some(choice) = choices.first() else {
            return Ok(vec![]);
        };
        if choice["index"].as_u64() != Some(0) || self.finish_reason.is_some() {
            return Err(Error::Protocol("unexpected choice after finish".into()));
        }
        let delta = &choice["delta"];
        if let Some(role) = delta.get("role") {
            if role != "assistant" {
                return Err(Error::Protocol("invalid streamed role".into()));
            }
        }
        let mut output = Vec::new();
        for (key, buffer) in [
            ("content", &mut self.content),
            ("refusal", &mut self.refusal),
            ("reasoning_content", &mut self.reasoning),
        ] {
            if let Some(text) = delta.get(key).filter(|v| !v.is_null()) {
                let text = text
                    .as_str()
                    .ok_or_else(|| Error::Protocol("non-text completion delta".into()))?;
                bounded_append(buffer, text)?;
                if key == "content" && !text.is_empty() {
                    output.push(text.to_owned());
                }
            }
        }
        if let Some(tools) = delta.get("tool_calls").filter(|v| !v.is_null()) {
            for update in tools
                .as_array()
                .ok_or_else(|| Error::Protocol("invalid tool delta".into()))?
            {
                let index = update["index"]
                    .as_u64()
                    .filter(|i| *i < 4096)
                    .ok_or_else(|| Error::Protocol("invalid tool index".into()))?;
                let tool = self.tools.entry(index).or_default();
                if let Some(id) = update.get("id") {
                    let id = id
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| Error::Protocol("invalid tool ID".into()))?;
                    if !tool.id.is_empty() && tool.id != id {
                        return Err(Error::Protocol("tool ID changed".into()));
                    }
                    tool.id = id.into();
                }
                if let Some(kind) = update.get("type") {
                    if kind != "function" {
                        return Err(Error::Protocol("unsupported tool type".into()));
                    }
                }
                for (key, buffer) in [("name", &mut tool.name), ("arguments", &mut tool.arguments)]
                {
                    if let Some(value) = update["function"].get(key) {
                        bounded_append(
                            buffer,
                            value
                                .as_str()
                                .ok_or_else(|| Error::Protocol("invalid function delta".into()))?,
                        )?;
                    }
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").filter(|v| !v.is_null()) {
            self.finish_reason = Some(
                reason
                    .as_str()
                    .ok_or_else(|| Error::Protocol("invalid finish reason".into()))?
                    .into(),
            );
        }
        Ok(output)
    }
    pub(super) fn finish(self) -> Result<ModelResponse> {
        if !self.done {
            return Err(Error::Protocol("stream ended before [DONE]".into()));
        }
        let mut message = json!({"role":"assistant","content":self.content});
        if !self.refusal.is_empty() {
            message["refusal"] = json!(self.refusal);
        }
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        if !self.tools.is_empty() {
            message["tool_calls"] = json!(self.tools.values().map(|t| json!({"id":t.id,"type":"function","function":{"name":t.name,"arguments":t.arguments}})).collect::<Vec<_>>());
        }
        response(
            json!({"id":self.id,"object":"chat.completion","model":self.model,"choices":[{"index":0,"message":message,"finish_reason":self.finish_reason}],"usage":self.usage}),
        )
    }
}
