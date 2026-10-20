//! Canonical text/tool history and same-protocol native message replay.
use super::{field, Api};
use crate::{Error, ModelRequest, ModelResponse, Result, Usage};
use serde_json::{json, Value};
use std::collections::HashSet;

const NATIVE: &str = "_rydantic_native";

pub(super) fn responses_request(request: &ModelRequest, preserve_native: bool) -> Result<Value> {
    let mut body = serde_json::to_value(request)
        .map_err(|_| Error::Configuration("invalid request".into()))?;
    if !preserve_native {
        for item in body["input"].as_array_mut().expect("serialized input") {
            if let Some(object) = item.as_object_mut() {
                object.remove(NATIVE);
                object.remove("_rydantic_tool_result");
            }
        }
    }
    Ok(body)
}

pub(super) fn normalized(
    api: Api,
    raw: Value,
    native_message: Value,
    text: String,
    calls: Vec<Value>,
    usage: Usage,
) -> Result<ModelResponse> {
    usage.total()?;
    let mut message = json!({"type":"message","role":"assistant","content":[]});
    if !text.is_empty() {
        message["content"] = json!([{"type":"output_text","text":text}]);
    }
    message[NATIVE] = json!({"api":api.name(),"message":native_message});
    let mut output = vec![message];
    output.extend(calls);
    ModelResponse::from_value(
        json!({"id":raw["id"],"object":"response","status":"completed","output":output,
        "usage":{"input_tokens":usage.input_tokens,"output_tokens":usage.output_tokens},"provider_response":{"api":api.name(),"response":raw}}),
    )
}

fn native(item: &Value, api: Api) -> Option<&Value> {
    (item[NATIVE]["api"] == api.name())
        .then_some(&item[NATIVE]["message"])
        .filter(|v| v.is_object())
}
fn text(value: &Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.into());
    }
    let parts = value.as_array().ok_or_else(|| {
        Error::Configuration("message content must be text or text blocks".into())
    })?;
    let mut text = String::new();
    for part in parts {
        if !matches!(
            part["type"].as_str(),
            Some("text" | "input_text" | "output_text")
        ) {
            return Err(Error::Configuration(
                "this adapter accepts text content and client tools".into(),
            ));
        }
        text.push_str(
            part["text"]
                .as_str()
                .ok_or_else(|| Error::Configuration("invalid text block".into()))?,
        );
    }
    Ok(text)
}
fn configuration() -> Error {
    Error::Configuration("unsupported or malformed conversation item".into())
}

pub(crate) fn chat(request: &ModelRequest) -> Result<Vec<Value>> {
    let mut messages: Vec<Value> = Vec::new();
    let mut covered = HashSet::new();
    if let Some(instructions) = &request.instructions {
        messages.push(json!({"role":"system","content":instructions}));
    }
    for item in &request.input {
        if let Some(message) = native(item, Api::ChatCompletions) {
            // Keep request-compatible fields, including reasoning used by compatible endpoints.
            let mut replay = json!({});
            for key in [
                "role",
                "content",
                "tool_calls",
                "refusal",
                "reasoning_content",
                "audio",
            ] {
                if let Some(value) = message.get(key) {
                    replay[key] = value.clone();
                }
            }
            if let Some(calls) = message["tool_calls"].as_array() {
                for call in calls {
                    covered.insert(field(call, "id")?.to_owned());
                }
            }
            messages.push(replay);
            continue;
        }
        match item["type"].as_str() {
            Some("function_call") => {
                let id = field(item, "call_id")?;
                if covered.contains(id) { continue; }
                let call = json!({"id":id,"type":"function","function":{"name":field(item,"name")?,"arguments":field(item,"arguments")?}});
                if messages.last().is_none_or(|last| last["role"] != "assistant") { messages.push(json!({"role":"assistant","content":null,"tool_calls":[]})); }
                let last = messages.last_mut().expect("assistant");
                if last.get("tool_calls").is_none() { last["tool_calls"] = json!([]); }
                last["tool_calls"].as_array_mut().ok_or_else(configuration)?.push(call);
            }
            Some("function_call_output") => messages.push(json!({"role":"tool","tool_call_id":field(item,"call_id")?,"content":item["output"].as_str().ok_or_else(configuration)?})),
            Some("reasoning") => (),
            Some("message") | None => {
                let role = item["role"].as_str().ok_or_else(configuration)?;
                if !matches!(role, "user" | "assistant" | "system" | "developer") { return Err(configuration()); }
                messages.push(json!({"role":role,"content":text(&item["content"])?}));
            }
            _ => return Err(configuration()),
        }
    }
    Ok(messages)
}

fn append_message(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) -> Result<()> {
    if messages.last().is_some_and(|last| last["role"] == role) {
        messages.last_mut().expect("message")["content"]
            .as_array_mut()
            .ok_or_else(configuration)?
            .extend(blocks);
    } else {
        messages.push(json!({"role":role,"content":blocks}));
    }
    Ok(())
}
pub(crate) fn messages(request: &ModelRequest) -> Result<(String, Vec<Value>)> {
    let mut messages = Vec::new();
    let mut covered = HashSet::new();
    let mut system = request.instructions.clone().unwrap_or_default();
    for item in &request.input {
        if let Some(message) = native(item, Api::AnthropicMessages) {
            let blocks = message["content"].as_array().ok_or_else(configuration)?;
            for block in blocks {
                if block["type"] == "tool_use" {
                    covered.insert(field(block, "id")?.to_owned());
                }
            }
            append_message(&mut messages, "assistant", blocks.clone())?;
            continue;
        }
        match item["type"].as_str() {
            Some("function_call") => {
                let id = field(item, "call_id")?;
                if covered.contains(id) {
                    continue;
                }
                let input: Value =
                    serde_json::from_str(field(item, "arguments")?).map_err(|_| configuration())?;
                if !input.is_object() {
                    return Err(configuration());
                }
                append_message(
                    &mut messages,
                    "assistant",
                    vec![
                        json!({"type":"tool_use","id":id,"name":field(item,"name")?,"input":input}),
                    ],
                )?;
            }
            Some("function_call_output") => {
                if let Some(block) = item.get("_rydantic_tool_result") {
                    append_message(&mut messages, "user", vec![block.clone()])?;
                    continue;
                }
                // Tool results lead the user turn; later user text may follow them.
                if messages.last().is_some_and(|last| {
                    last["role"] == "user"
                        && last["content"]
                            .as_array()
                            .is_some_and(|blocks| blocks.iter().any(|b| b["type"] != "tool_result"))
                }) {
                    return Err(configuration());
                }
                append_message(
                    &mut messages,
                    "user",
                    vec![
                        json!({"type":"tool_result","tool_use_id":field(item,"call_id")?,"content":item["output"].as_str().ok_or_else(configuration)?}),
                    ],
                )?;
            }
            Some("reasoning") => (),
            Some("message") | None => {
                let role = item["role"].as_str().ok_or_else(configuration)?;
                let content = text(&item["content"])?;
                if role == "assistant" && content.is_empty() {
                    continue;
                }
                if matches!(role, "system" | "developer") {
                    if !messages.is_empty() {
                        return Err(Error::Configuration(
                            "Messages cannot represent interleaved system or developer messages"
                                .into(),
                        ));
                    }
                    if !system.is_empty() {
                        system.push('\n');
                    }
                    system.push_str(&content);
                } else if matches!(role, "user" | "assistant") {
                    append_message(
                        &mut messages,
                        role,
                        vec![json!({"type":"text","text":content})],
                    )?;
                } else {
                    return Err(configuration());
                }
            }
            _ => return Err(configuration()),
        }
    }
    Ok((system, messages))
}
