//! Incremental protocol projection. Success markers are sent after budget settlement.
use super::egress::View;
use crate::{Api, Error, ModelResponse, Result, ToolCall};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) enum Frame {
    Json { event: Option<String>, value: Value },
    Done,
}
struct Tool {
    key: String,
    id: String,
    name: String,
    args: String,
    ready: bool,
    opened: bool,
    closed: bool,
    sent: usize,
    block: usize,
}
pub(super) struct Output {
    view: View,
    include_usage: bool,
    decided: bool,
    pass: bool,
    started: bool,
    text: String,
    observed: String,
    tools: Vec<Tool>,
    text_block: Option<usize>,
    blocks: usize,
    finish_chat: Option<Value>,
    sequence: u64,
}
fn bad() -> Error {
    Error::Protocol("inconsistent gateway stream".into())
}
fn append(buffer: &mut String, text: &str) -> Result<()> {
    if text.len() > (1024 * 1024usize).saturating_sub(buffer.len()) {
        return Err(Error::Limit("gateway field exceeds 1 MiB".into()));
    }
    buffer.push_str(text);
    Ok(())
}
impl Output {
    pub fn new(view: View, include_usage: bool) -> Self {
        Self {
            view,
            include_usage,
            decided: false,
            pass: false,
            started: false,
            text: String::new(),
            observed: String::new(),
            tools: Vec::new(),
            text_block: None,
            blocks: 0,
            finish_chat: None,
            sequence: 0,
        }
    }
    fn frame(&mut self, mut value: Value) -> Frame {
        if self.view.api == Api::Responses {
            value["sequence_number"] = json!(self.sequence);
            self.sequence += 1;
        }
        let event = if self.view.api == Api::ChatCompletions {
            None
        } else {
            value["type"].as_str().map(str::to_owned)
        };
        Frame::Json { event, value }
    }
    fn chat_chunk(&self, delta: Value, reason: Value) -> Value {
        let mut value = json!({"id":self.view.id,"object":"chat.completion.chunk","created":self.view.created,"model":self.view.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason,"logprobs":null}]});
        if self.include_usage {
            value["usage"] = Value::Null;
        }
        value
    }
    fn start(&mut self, out: &mut Vec<Frame>) {
        if self.started {
            return;
        }
        self.started = true;
        match self.view.api {
            Api::ChatCompletions=>out.push(self.frame(self.chat_chunk(json!({"role":"assistant","content":""}),Value::Null))),
            Api::AnthropicMessages=>out.push(self.frame(json!({"type":"message_start","message":{"id":self.view.id,"type":"message","role":"assistant","model":self.view.model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}))),
            Api::Responses=> {
                let mut response=json!({"id":self.view.id,"object":"response","model":self.view.model,"created_at":self.view.created,"status":"in_progress","output":[],"usage":null,"parallel_tool_calls":true,"tool_choice":"auto","tools":[]});
                self.view.apply_settings(&mut response);
                out.push(self.frame(json!({"type":"response.created","response":response})));
                out.push(self.frame(json!({"type":"response.in_progress","response":response})));
                let mut item=self.view.text_item("","in_progress");item["content"]=json!([]);
                out.push(self.frame(json!({"type":"response.output_item.added","output_index":0,"item":item})));
                out.push(self.frame(json!({"type":"response.content_part.added","item_id":self.view.item_id(0),"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[],"logprobs":[]}})));
            }
        }
    }
    fn text(&mut self, text: &str, out: &mut Vec<Frame>) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.start(out);
        append(&mut self.text, text)?;
        let event = match self.view.api {
            Api::ChatCompletions => self.chat_chunk(json!({"content":text}), Value::Null),
            Api::Responses => {
                json!({"type":"response.output_text.delta","item_id":self.view.item_id(0),"output_index":0,"content_index":0,"delta":text,"logprobs":[]})
            }
            Api::AnthropicMessages => {
                if self.text_block.is_none() {
                    let index = self.blocks;
                    self.blocks += 1;
                    self.text_block = Some(index);
                    out.push(self.frame(json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}})));
                }
                json!({"type":"content_block_delta","index":self.text_block,"delta":{"type":"text_delta","text":text}})
            }
        };
        out.push(self.frame(event));
        Ok(())
    }
    fn close_text(&mut self, out: &mut Vec<Frame>) {
        if let Some(index) = self.text_block.take() {
            out.push(self.frame(json!({"type":"content_block_stop","index":index})));
        }
    }
    fn tool(&mut self, key: String) -> Result<usize> {
        if let Some(index) = self.tools.iter().position(|t| t.key == key) {
            return Ok(index);
        }
        if self.tools.len() >= 4096 {
            return Err(Error::Limit("too many streamed tools".into()));
        }
        let index = self.tools.len();
        self.tools.push(Tool {
            key,
            id: String::new(),
            name: String::new(),
            args: String::new(),
            ready: false,
            opened: false,
            closed: false,
            sent: 0,
            block: 0,
        });
        Ok(index)
    }
    fn flush_tool(&mut self, index: usize, out: &mut Vec<Frame>) -> Result<()> {
        if !self.tools[index].ready {
            return Ok(());
        }
        self.start(out);
        if !self.tools[index].opened {
            if self.tools[index].id.is_empty() || self.tools[index].name.is_empty() {
                return Err(bad());
            }
            self.close_text(out);
            let tool = &self.tools[index];
            let event=match self.view.api {
                Api::ChatCompletions=>self.chat_chunk(json!({"tool_calls":[{"index":index,"id":tool.id,"type":"function","function":{"name":tool.name,"arguments":""}}]}),Value::Null),
                Api::Responses=>json!({"type":"response.output_item.added","output_index":index+1,"item":self.view.response_item(&ToolCall {call_id:tool.id.clone(),name:tool.name.clone(),arguments:String::new()},index,"in_progress")}),
                Api::AnthropicMessages=>{
                    let event=json!({"type":"content_block_start","index":self.blocks,"content_block":{"type":"tool_use","id":tool.id,"name":tool.name,"input":{}}});
                    self.tools[index].block=self.blocks;self.blocks+=1;event
                }
            };
            self.tools[index].opened = true;
            out.push(self.frame(event));
        }
        let tool = &mut self.tools[index];
        let delta = tool.args[tool.sent..].to_owned();
        tool.sent = tool.args.len();
        if !delta.is_empty() {
            let event = match self.view.api {
                Api::ChatCompletions => self.chat_chunk(
                    json!({"tool_calls":[{"index":index,"function":{"arguments":delta}}]}),
                    Value::Null,
                ),
                Api::Responses => {
                    json!({"type":"response.function_call_arguments.delta","item_id":self.view.item_id(index+1),"output_index":index+1,"delta":delta})
                }
                Api::AnthropicMessages => {
                    json!({"type":"content_block_delta","index":self.tools[index].block,"delta":{"type":"input_json_delta","partial_json":delta}})
                }
            };
            out.push(self.frame(event));
        }
        Ok(())
    }
    fn native(&mut self, api: &str, event: &Value, out: &mut Vec<Frame>) -> Result<()> {
        match api {
            "chat_completions" => {
                if let Some(updates) = event["choices"][0]["delta"]["tool_calls"].as_array() {
                    for update in updates {
                        let native = update["index"].as_u64().ok_or_else(bad)?;
                        let key = format!("chat:{native}");
                        let index = self.tool(key)?;
                        let tool = &mut self.tools[index];
                        if let Some(id) = update["id"].as_str() {
                            if !tool.id.is_empty() && tool.id != id {
                                return Err(bad());
                            }
                            tool.id = id.into();
                        }
                        if let Some(name) = update["function"]["name"].as_str() {
                            append(&mut tool.name, name)?;
                        }
                        if let Some(args) = update["function"]["arguments"].as_str() {
                            append(&mut tool.args, args)?;
                        }
                    }
                }
                if !event["choices"][0]["finish_reason"].is_null() {
                    for index in 0..self.tools.len() {
                        self.tools[index].ready = true;
                        self.flush_tool(index, out)?;
                    }
                }
            }
            "anthropic_messages" => {
                let index = event["index"].as_u64().unwrap_or_default();
                let key = format!("messages:{index}");
                match event["type"].as_str() {
                    Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                        let index = self.tool(key)?;
                        let tool = &mut self.tools[index];
                        tool.id = event["content_block"]["id"]
                            .as_str()
                            .ok_or_else(bad)?
                            .into();
                        tool.name = event["content_block"]["name"]
                            .as_str()
                            .ok_or_else(bad)?
                            .into();
                        tool.ready = true;
                        if event["content_block"]["input"]
                            .as_object()
                            .is_some_and(|v| !v.is_empty())
                        {
                            tool.args = event["content_block"]["input"].to_string();
                        }
                        self.flush_tool(index, out)?;
                    }
                    Some("content_block_delta") if event["delta"]["type"] == "input_json_delta" => {
                        let index = self
                            .tools
                            .iter()
                            .position(|t| t.key == key)
                            .ok_or_else(bad)?;
                        append(
                            &mut self.tools[index].args,
                            event["delta"]["partial_json"].as_str().ok_or_else(bad)?,
                        )?;
                        self.flush_tool(index, out)?;
                    }
                    _ => (),
                }
            }
            "responses" => match event["type"].as_str() {
                Some("response.output_item.added") if event["item"]["type"] == "function_call" => {
                    let key = event["item"]["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("response:{}", event["output_index"]));
                    let index = self.tool(key)?;
                    let tool = &mut self.tools[index];
                    tool.id = event["item"]["call_id"].as_str().ok_or_else(bad)?.into();
                    tool.name = event["item"]["name"].as_str().ok_or_else(bad)?.into();
                    tool.args = event["item"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .into();
                    tool.ready = true;
                    self.flush_tool(index, out)?;
                }
                Some("response.function_call_arguments.delta") => {
                    let key = event["item_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("response:{}", event["output_index"]));
                    let index = self
                        .tools
                        .iter()
                        .position(|t| t.key == key)
                        .ok_or_else(bad)?;
                    append(
                        &mut self.tools[index].args,
                        event["delta"].as_str().ok_or_else(bad)?,
                    )?;
                    self.flush_tool(index, out)?;
                }
                _ => (),
            },
            _ => return Err(bad()),
        }
        Ok(())
    }
    pub fn observe(&mut self, event: Value) -> Result<Vec<Frame>> {
        let mut out = Vec::new();
        let kind = event["type"].as_str().unwrap_or_default();
        if kind == "response.completed" || kind == "response.done" {
            return Ok(out);
        }
        if kind == "response.output_text.delta" {
            let text = event["delta"].as_str().ok_or_else(bad)?;
            append(&mut self.observed, text)?;
            if !self.pass {
                self.decided = true;
                self.text(text, &mut out)?;
            }
            return Ok(out);
        }
        if kind == "provider.event" {
            let api = event["api"].as_str().ok_or_else(bad)?;
            let raw = &event["event"];
            if !self.decided {
                self.pass = api == self.view.api.name() && self.view.api != Api::Responses;
                self.decided = true;
            }
            if self.pass {
                let mut raw = raw.clone();
                self.started = true;
                if self.view.api == Api::ChatCompletions {
                    if raw["choices"].as_array().is_none_or(|v| v.is_empty()) {
                        return Ok(out);
                    }
                    if !raw["choices"][0]["finish_reason"].is_null() {
                        self.finish_chat = Some(raw);
                        return Ok(out);
                    }
                    raw["id"] = json!(self.view.id);
                    raw["model"] = json!(self.view.model);
                    raw["object"] = json!("chat.completion.chunk");
                    raw["created"] = json!(self.view.created);
                    if self.include_usage {
                        raw["usage"] = Value::Null;
                    } else if let Some(object) = raw.as_object_mut() {
                        object.remove("usage");
                    }
                } else {
                    match raw["type"].as_str() {
                        Some("message_delta" | "message_stop") => return Ok(out),
                        Some("message_start") => {
                            raw["message"]["id"] = json!(self.view.id);
                            raw["message"]["model"] = json!(self.view.model);
                        }
                        _ => (),
                    }
                }
                out.push(self.frame(raw));
                return Ok(out);
            }
            self.native(api, raw, &mut out)?;
        } else {
            self.decided = true;
            self.native("responses", &event, &mut out)?;
        }
        Ok(out)
    }
    pub fn finish(&mut self, response: &ModelResponse) -> Result<Vec<Frame>> {
        let mut out = Vec::new();
        let text = response.text()?;
        let calls = response.tool_calls()?;
        let mut ids = HashSet::new();
        if calls.iter().any(|c| !ids.insert(&c.call_id)) {
            return Err(bad());
        }
        let public = self.view.response(response)?;
        if !self.observed.is_empty() && self.observed != text {
            return Err(bad());
        }
        if self.pass {
            match self.view.api {
                Api::ChatCompletions => {
                    let mut last = self.finish_chat.take().ok_or_else(bad)?;
                    last["id"] = json!(self.view.id);
                    last["model"] = json!(self.view.model);
                    last["object"] = json!("chat.completion.chunk");
                    last["created"] = json!(self.view.created);
                    if self.include_usage {
                        last["usage"] = Value::Null;
                    } else if let Some(object) = last.as_object_mut() {
                        object.remove("usage");
                    }
                    out.push(self.frame(last));
                }
                Api::AnthropicMessages => (),
                Api::Responses => unreachable!(),
            }
        } else {
            if !text.starts_with(&self.text) {
                return Err(bad());
            }
            let remaining = text[self.text.len()..].to_owned();
            self.text(&remaining, &mut out)?;
            self.start(&mut out);
            for call in &calls {
                let index =
                    if let Some(index) = self.tools.iter().position(|t| t.id == call.call_id) {
                        index
                    } else {
                        self.tool(format!("final:{}", call.call_id))?
                    };
                let tool = &mut self.tools[index];
                if !call.arguments.starts_with(&tool.args)
                    || (!tool.name.is_empty() && !call.name.starts_with(&tool.name))
                {
                    return Err(bad());
                }
                if tool.opened && tool.name != call.name {
                    return Err(bad());
                }
                tool.id = call.call_id.clone();
                tool.name = call.name.clone();
                tool.args = call.arguments.clone();
                tool.ready = true;
                self.flush_tool(index, &mut out)?;
            }
            if self.tools.len() != calls.len() {
                return Err(bad());
            }
            self.close_text(&mut out);
            for index in 0..self.tools.len() {
                if self.tools[index].closed {
                    continue;
                }
                self.tools[index].closed = true;
                let tool = &self.tools[index];
                match self.view.api {
                    Api::AnthropicMessages => out
                        .push(self.frame(json!({"type":"content_block_stop","index":tool.block}))),
                    Api::Responses => {
                        let done = json!({"type":"response.function_call_arguments.done","item_id":self.view.item_id(index+1),"output_index":index+1,"arguments":tool.args,"name":tool.name});
                        let item = self.view.response_item(
                            &ToolCall {
                                call_id: tool.id.clone(),
                                name: tool.name.clone(),
                                arguments: tool.args.clone(),
                            },
                            index,
                            "completed",
                        );
                        out.push(self.frame(done));
                        out.push(self.frame(json!({"type":"response.output_item.done","output_index":index+1,"item":item})));
                    }
                    Api::ChatCompletions => (),
                }
            }
            match self.view.api {
                Api::ChatCompletions => out.push(self.frame(self.chat_chunk(
                    json!({}),
                    json!(if calls.is_empty() {
                        "stop"
                    } else {
                        "tool_calls"
                    }),
                ))),
                Api::Responses => {
                    out.push(self.frame(json!({"type":"response.output_text.done","item_id":self.view.item_id(0),"output_index":0,"content_index":0,"text":text,"logprobs":[]})));
                    out.push(self.frame(json!({"type":"response.content_part.done","item_id":self.view.item_id(0),"output_index":0,"content_index":0,"part":{"type":"output_text","text":text,"annotations":[],"logprobs":[]}})));
                    out.push(self.frame(json!({"type":"response.output_item.done","output_index":0,"item":self.view.text_item(&text,"completed")})));
                }
                Api::AnthropicMessages => (),
            }
        }
        match self.view.api {
            Api::ChatCompletions => {
                if self.include_usage {
                    let chunk = json!({"id":self.view.id,"object":"chat.completion.chunk","created":self.view.created,"model":self.view.model,"choices":[],"usage":self.view.chat_usage(response)?});
                    out.push(self.frame(chunk));
                }
                out.push(Frame::Done);
            }
            Api::AnthropicMessages => {
                out.push(self.frame(json!({"type":"message_delta","delta":{"stop_reason":public["stop_reason"],"stop_sequence":public["stop_sequence"]},"usage":self.view.message_usage(response)})));
                out.push(self.frame(json!({"type":"message_stop"})));
            }
            Api::Responses => {
                out.push(self.frame(json!({"type":"response.completed","response":public})))
            }
        }
        Ok(out)
    }
}
