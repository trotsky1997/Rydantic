//! Strict request decoding into the shared text/function-tool model.
use crate::{http, Api, Error, ModelRequest, Result, ToolDefinition};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) struct Decoded {
    pub request: ModelRequest,
    pub include_usage: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamOptions {
    #[serde(default)]
    include_usage: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Function {
    name: String,
    #[serde(default)]
    description: String,
    parameters: Value,
    strict: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatTool {
    #[serde(rename = "type")]
    kind: String,
    function: Function,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chat {
    model: String,
    messages: Vec<Value>,
    #[serde(default)]
    stream: bool,
    stream_options: Option<StreamOptions>,
    #[serde(default)]
    tools: Vec<ChatTool>,
    tool_choice: Option<Value>,
    parallel_tool_calls: Option<bool>,
    response_format: Option<Value>,
    max_completion_tokens: Option<u32>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    n: Option<u32>,
    store: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesTool {
    name: String,
    #[serde(default)]
    description: String,
    input_schema: Value,
    strict: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputConfig {
    format: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Messages {
    model: String,
    messages: Vec<Value>,
    max_tokens: u32,
    #[serde(default)]
    stream: bool,
    system: Option<Value>,
    #[serde(default)]
    tools: Vec<MessagesTool>,
    tool_choice: Option<Value>,
    output_config: Option<OutputConfig>,
    temperature: Option<f64>,
    top_p: Option<f64>,
}
fn bad() -> Error {
    Error::Configuration("unsupported or malformed request".into())
}
fn decode<T: for<'a> Deserialize<'a>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| bad())
}
pub(super) fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    if value
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(bad)
}
pub(super) fn text(value: &Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.into());
    }
    let mut output = String::new();
    for block in value.as_array().ok_or_else(bad)? {
        fields(block, &["type", "text"])?;
        if !matches!(
            block["type"].as_str(),
            Some("text" | "input_text" | "output_text")
        ) {
            return Err(bad());
        }
        output.push_str(block["text"].as_str().ok_or_else(bad)?);
    }
    Ok(output)
}
fn instruction(request: &mut ModelRequest, value: String) {
    if value.is_empty() {
        return;
    }
    let current = request.instructions.get_or_insert_default();
    if !current.is_empty() {
        current.push('\n');
    }
    current.push_str(&value);
}
fn format(value: &Value, nested: bool) -> Result<Value> {
    let kind = string(value, "type")?;
    match kind {
        "text" | "json_object" if nested => {
            fields(value, &["type"])?;
            Ok(value.clone())
        }
        "json_schema" => {
            if nested {
                fields(value, &["type", "json_schema"])?;
                let schema = &value["json_schema"];
                fields(schema, &["name", "schema", "strict", "description"])?;
                let name = string(schema, "name")?;
                if !schema["schema"].is_object() {
                    return Err(bad());
                }
                let strict = match schema.get("strict") {
                    None | Some(Value::Null) => false,
                    Some(v) => v.as_bool().ok_or_else(bad)?,
                };
                Ok(
                    json!({"type":"json_schema","name":name,"schema":schema["schema"],"strict":strict}),
                )
            } else {
                fields(value, &["type", "schema"])?;
                if !value["schema"].is_object() {
                    return Err(bad());
                }
                Ok(
                    json!({"type":"json_schema","name":"result","schema":value["schema"],"strict":false}),
                )
            }
        }
        _ => Err(bad()),
    }
}
fn function_choice(value: Value, chat: bool) -> Result<Value> {
    if value.is_string() {
        return Ok(value);
    }
    if chat {
        fields(&value, &["type", "function"])?;
        fields(&value["function"], &["name"])?;
    }
    if value["type"] != "function" {
        return Err(bad());
    }
    let name = if chat {
        string(&value["function"], "name")?
    } else {
        string(&value, "name")?
    };
    Ok(json!({"type":"function","name":name}))
}
fn validate_tools(request: &ModelRequest) -> Result<()> {
    let mut names = HashSet::new();
    for tool in &request.tools {
        if tool.kind != "function"
            || tool.name.is_empty()
            || tool.name.len() > 64
            || !tool
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || !names.insert(tool.name.as_str())
            || !tool.parameters.is_object()
        {
            return Err(bad());
        }
    }
    if let Some(choice) = &request.tool_choice {
        if let Some(name) = choice["name"].as_str() {
            if !names.contains(name) {
                return Err(bad());
            }
        }
    }
    Ok(())
}
pub(super) fn parse(api: Api, value: Value) -> Result<Decoded> {
    let decoded = match api {
        Api::Responses => Decoded {
            request: responses(value)?,
            include_usage: false,
        },
        Api::ChatCompletions => chat(decode(value)?)?,
        Api::AnthropicMessages => messages(decode(value)?)?,
    };
    decoded.request.validate()?;
    if decoded.request.input.len() > 4096 {
        return Err(bad());
    }
    validate_tools(&decoded.request)?;
    Ok(decoded)
}
fn responses(mut value: Value) -> Result<ModelRequest> {
    if let Some(text) = value["input"].as_str() {
        let text = text.to_owned();
        value["input"] = json!([{"role":"user","content":text}]);
    }
    decode(value)
}
fn chat(wire: Chat) -> Result<Decoded> {
    if wire.n.is_some_and(|n| n != 1)
        || wire.store == Some(true)
        || (wire.max_tokens.is_some() && wire.max_completion_tokens.is_some())
    {
        return Err(bad());
    }
    let mut request = ModelRequest::new(wire.model, "");
    request.input.clear();
    request.stream = wire.stream;
    request.max_output_tokens = wire.max_completion_tokens.or(wire.max_tokens);
    request.temperature = wire.temperature;
    request.top_p = wire.top_p;
    request.parallel_tool_calls = wire.parallel_tool_calls;
    request.tool_choice = wire
        .tool_choice
        .map(|v| function_choice(v, true))
        .transpose()?;
    if let Some(schema) = wire.response_format {
        request.text = Some(json!({"format":format(&schema,true)?}));
    }
    request.tools = wire
        .tools
        .into_iter()
        .map(|tool| {
            if tool.kind != "function" {
                return Err(bad());
            }
            Ok(ToolDefinition {
                kind: tool.kind,
                name: tool.function.name,
                description: tool.function.description,
                parameters: tool.function.parameters,
                strict: tool.function.strict.unwrap_or(false),
            })
        })
        .collect::<Result<_>>()?;
    for message in wire.messages {
        fields(
            &message,
            &[
                "role",
                "content",
                "tool_calls",
                "tool_call_id",
                "refusal",
                "reasoning_content",
            ],
        )?;
        match string(&message,"role")? {
            "system" | "developer" => instruction(&mut request,text(&message["content"])?),
            "user"=>request.input.push(json!({"role":"user","content":text(&message["content"])?})),
            "tool"=>request.input.push(json!({"type":"function_call_output","call_id":string(&message,"tool_call_id")?,"output":text(&message["content"])?})),
            "assistant"=> {
                let mut message=message;
                if !message["content"].is_null() {message["content"]=json!(text(&message["content"])?);}
                if let Some(calls)=message["tool_calls"].as_array() {
                    for call in calls {fields(call,&["id","type","function"])?; fields(&call["function"],&["name","arguments"])?;}
                }
                request.input.extend(http::chat_message(message)?);
            }
            _=>return Err(bad()),
        }
    }
    Ok(Decoded {
        request,
        include_usage: wire.stream_options.is_some_and(|v| v.include_usage),
    })
}
fn messages(wire: Messages) -> Result<Decoded> {
    let mut request = ModelRequest::new(wire.model, "");
    request.input.clear();
    request.stream = wire.stream;
    request.max_output_tokens = Some(wire.max_tokens);
    request.temperature = wire.temperature;
    request.top_p = wire.top_p;
    if request.temperature.is_some_and(|v| v > 1.0) {
        return Err(bad());
    }
    if let Some(system) = wire.system {
        instruction(&mut request, text(&system)?);
    }
    if let Some(choice) = wire.tool_choice {
        fields(&choice, &["type", "name", "disable_parallel_tool_use"])?;
        request.parallel_tool_calls = choice
            .get("disable_parallel_tool_use")
            .map(|v| v.as_bool().map(|b| !b).ok_or_else(bad))
            .transpose()?;
        request.tool_choice = Some(match string(&choice, "type")? {
            "auto" => json!("auto"),
            "none" => json!("none"),
            "any" => json!("required"),
            "tool" => json!({"type":"function","name":string(&choice,"name")?}),
            _ => return Err(bad()),
        });
    }
    if let Some(config) = wire.output_config {
        request.text = Some(json!({"format":format(&config.format,false)?}));
    }
    request.tools = wire
        .tools
        .into_iter()
        .map(|t| ToolDefinition {
            kind: "function".into(),
            name: t.name,
            description: t.description,
            parameters: t.input_schema,
            strict: t.strict.unwrap_or(false),
        })
        .collect();
    for message in wire.messages {
        fields(&message, &["role", "content"])?;
        let role = string(&message, "role")?;
        let content = if let Some(text) = message["content"].as_str() {
            json!([{"type":"text","text":text}])
        } else {
            message["content"].clone()
        };
        let blocks = content.as_array().ok_or_else(bad)?;
        for block in blocks {
            match string(block, "type")? {
                "text" => {
                    fields(block, &["type", "text"])?;
                    block["text"].as_str().ok_or_else(bad)?;
                }
                "thinking" if role == "assistant" => {
                    fields(block, &["type", "thinking", "signature"])?;
                    string(block, "signature")?;
                    block["thinking"].as_str().ok_or_else(bad)?;
                }
                "redacted_thinking" if role == "assistant" => {
                    fields(block, &["type", "data"])?;
                    string(block, "data")?;
                }
                "tool_use" if role == "assistant" => {
                    fields(block, &["type", "id", "name", "input"])?;
                }
                "tool_result" if role == "user" => {
                    fields(block, &["type", "tool_use_id", "content", "is_error"])?;
                }
                _ => return Err(bad()),
            }
        }
        match role {
            "assistant" => request.input.extend(http::messages_message(content)?),
            "user" => {
                let mut user = String::new();
                let mut saw_text = false;
                for block in blocks {
                    if block["type"] == "text" {
                        saw_text = true;
                        user.push_str(block["text"].as_str().ok_or_else(bad)?);
                    } else {
                        if saw_text {
                            return Err(bad());
                        }
                        let output = match block.get("content") {
                            None => String::new(),
                            Some(value) => text(value)?,
                        };
                        let error = block
                            .get("is_error")
                            .map(|v| v.as_bool().ok_or_else(bad))
                            .transpose()?
                            .unwrap_or(false);
                        let encoded = if error {
                            json!({"error":output}).to_string()
                        } else {
                            output
                        };
                        request.input.push(json!({"type":"function_call_output","call_id":string(block,"tool_use_id")?,"output":encoded,"_rydantic_tool_result":block}));
                    }
                }
                if saw_text {
                    request.input.push(json!({"role":"user","content":user}));
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(Decoded {
        request,
        include_usage: false,
    })
}
