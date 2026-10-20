//! Public response shapes. Private replay metadata stays inside the gateway.
use crate::{http, Api, ModelRequest, ModelResponse, Result, ToolCall};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub(super) struct View {
    pub api: Api,
    pub id: String,
    pub model: String,
    pub created: u64,
    pub settings: Value,
}
impl View {
    pub fn new(api: Api, model: String) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let prefix = match api {
            Api::Responses => "resp",
            Api::ChatCompletions => "chatcmpl",
            Api::AnthropicMessages => "msg",
        };
        Self {
            api,
            id: format!(
                "{prefix}_gw_{:x}_{:x}",
                time.as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            model,
            created: time.as_secs(),
            settings: json!({}),
        }
    }
    pub fn with_request(mut self, request: &ModelRequest) -> Self {
        self.settings = json!({"instructions":request.instructions,"tools":request.tools,"tool_choice":request.tool_choice.clone().unwrap_or(json!("auto")),"parallel_tool_calls":request.parallel_tool_calls.unwrap_or(true),"text":request.text.clone().unwrap_or(json!({"format":{"type":"text"}})),"temperature":request.temperature.unwrap_or(1.0),"top_p":request.top_p.unwrap_or(1.0),"max_output_tokens":request.max_output_tokens,"reasoning":request.reasoning});
        self
    }
    pub fn apply_settings(&self, value: &mut Value) {
        if let Some(settings) = self.settings.as_object() {
            for (key, value_setting) in settings {
                value[key] = value_setting.clone();
            }
        }
    }
    pub fn item_id(&self, index: usize) -> String {
        format!("{}_item_{index}", self.id)
    }
    pub fn message_usage(&self, response: &ModelResponse) -> Value {
        if response.raw["provider_response"]["api"] == "anthropic_messages" {
            return response.raw["provider_response"]["response"]["usage"].clone();
        }
        json!({"input_tokens":response.usage.input_tokens,"output_tokens":response.usage.output_tokens,"cache_creation_input_tokens":0,"cache_read_input_tokens":0})
    }
    pub fn chat_usage(&self, response: &ModelResponse) -> Result<Value> {
        let mut usage = json!({"prompt_tokens":response.usage.input_tokens,"completion_tokens":response.usage.output_tokens,"total_tokens":response.usage.total()?});
        if response.raw["provider_response"]["api"] == "chat_completions" {
            for key in ["prompt_tokens_details", "completion_tokens_details"] {
                if let Some(value) = response.raw["provider_response"]["response"]["usage"].get(key)
                {
                    usage[key] = value.clone();
                }
            }
        }
        Ok(usage)
    }
    fn response_usage(&self, response: &ModelResponse) -> Result<Value> {
        let native = &response.raw["provider_response"];
        let api = native["api"].as_str().unwrap_or("responses");
        let usage = if api == "responses" {
            &response.raw["usage"]
        } else {
            &native["response"]["usage"]
        };
        let (read, write, reasoning) = match api {
            "anthropic_messages" => (
                usage["cache_read_input_tokens"].as_u64(),
                usage["cache_creation_input_tokens"].as_u64(),
                None,
            ),
            "chat_completions" => (
                usage["prompt_tokens_details"]["cached_tokens"].as_u64(),
                usage["prompt_tokens_details"]["cache_write_tokens"].as_u64(),
                usage["completion_tokens_details"]["reasoning_tokens"].as_u64(),
            ),
            _ => (
                usage["input_tokens_details"]["cached_tokens"].as_u64(),
                usage["input_tokens_details"]["cache_write_tokens"].as_u64(),
                usage["output_tokens_details"]["reasoning_tokens"].as_u64(),
            ),
        };
        Ok(
            json!({"input_tokens":response.usage.input_tokens,"input_tokens_details":{"cached_tokens":read.unwrap_or(0),"cache_write_tokens":write.unwrap_or(0)},"output_tokens":response.usage.output_tokens,"output_tokens_details":{"reasoning_tokens":reasoning.unwrap_or(0)},"total_tokens":response.usage.total()?}),
        )
    }
    pub fn response_item(&self, call: &ToolCall, index: usize, status: &str) -> Value {
        json!({"id":self.item_id(index+1),"type":"function_call","status":status,"call_id":call.call_id,"name":call.name,"arguments":call.arguments})
    }
    pub fn text_item(&self, text: &str, status: &str) -> Value {
        json!({"id":self.item_id(0),"type":"message","status":status,"role":"assistant","content":[{"type":"output_text","text":text,"annotations":[],"logprobs":[]}]})
    }
    pub fn response(&self, response: &ModelResponse) -> Result<Value> {
        response.usage.total()?;
        let text = response.text()?;
        let calls = response.tool_calls()?;
        let mut ids = HashSet::new();
        if calls.iter().any(|call| !ids.insert(&call.call_id)) {
            return Err(crate::Error::Protocol("duplicate tool call ID".into()));
        }
        match self.api {
            Api::Responses => {
                let mut output = vec![self.text_item(&text, "completed")];
                for (index, call) in calls.iter().enumerate() {
                    output.push(self.response_item(call, index, "completed"));
                }
                let mut public = json!({"id":self.id,"object":"response","created_at":self.created,"completed_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),"status":"completed","error":null,"incomplete_details":null,"model":self.model,"output":output,"store":false,"metadata":{},"parallel_tool_calls":true,"tools":[],"tool_choice":"auto","text":{"format":{"type":"text"}},"temperature":1.0,"top_p":1.0,"usage":{"input_tokens":response.usage.input_tokens,"input_tokens_details":{"cached_tokens":0},"output_tokens":response.usage.output_tokens,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":response.usage.total()?}});
                public["usage"] = self.response_usage(response)?;
                self.apply_settings(&mut public);
                Ok(public)
            }
            Api::ChatCompletions => {
                let mut request = ModelRequest::new(&self.model, "");
                request.input = response.output().to_vec();
                let messages = http::chat_history(&request)?;
                let mut message = json!({"role":"assistant","content":text,"refusal":null});
                for native in messages.iter().filter(|v| v["role"] == "assistant") {
                    for key in ["reasoning_content", "refusal"] {
                        if let Some(v) = native.get(key) {
                            message[key] = v.clone();
                        }
                    }
                }
                if !calls.is_empty() {
                    message["tool_calls"]=json!(calls.iter().map(|c|json!({"id":c.call_id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>());
                }
                Ok(
                    json!({"id":self.id,"object":"chat.completion","created":self.created,"model":self.model,"choices":[{"index":0,"message":message,"finish_reason":if calls.is_empty(){"stop"}else{"tool_calls"},"logprobs":null}],"usage":self.chat_usage(response)?}),
                )
            }
            Api::AnthropicMessages => {
                let mut request = ModelRequest::new(&self.model, "");
                request.input = response.output().to_vec();
                let (_, messages) = http::messages_history(&request)?;
                let content: Vec<Value> = messages
                    .into_iter()
                    .filter(|v| v["role"] == "assistant")
                    .flat_map(|v| v["content"].as_array().cloned().unwrap_or_default())
                    .collect();
                let native = &response.raw["provider_response"];
                let reason = if native["api"] == "anthropic_messages" {
                    native["response"]["stop_reason"].clone()
                } else {
                    json!(if calls.is_empty() {
                        "end_turn"
                    } else {
                        "tool_use"
                    })
                };
                let sequence = if native["api"] == "anthropic_messages" {
                    native["response"]["stop_sequence"].clone()
                } else {
                    Value::Null
                };
                Ok(
                    json!({"id":self.id,"type":"message","role":"assistant","model":self.model,"content":content,"stop_reason":reason,"stop_sequence":sequence,"usage":self.message_usage(response)}),
                )
            }
        }
    }
}
