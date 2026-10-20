#![allow(dead_code)]
use rydantic_ai::{
    BoxFuture, Capabilities, ModelProvider, ModelRequest, ModelResponse, Result, RunContext,
};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};

pub fn response(text: &str) -> ModelResponse {
    response_items(vec![
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}),
    ])
}
pub fn response_items(output: Vec<Value>) -> ModelResponse {
    ModelResponse::from_value(json!({"id":"test","object":"response","status":"completed","model":"mock","output":output,"usage":{"input_tokens":2,"output_tokens":3}})).unwrap()
}
pub fn call(id: &str, args: &str) -> Value {
    json!({"type":"function_call","call_id":id,"name":"add","arguments":args})
}
pub struct Scripted {
    pub requests: Mutex<Vec<ModelRequest>>,
    pub replies: Mutex<VecDeque<Result<ModelResponse>>>,
}
impl Scripted {
    pub fn new(replies: Vec<Result<ModelResponse>>) -> Self {
        Self {
            requests: Mutex::new(vec![]),
            replies: Mutex::new(replies.into()),
        }
    }
}
impl ModelProvider for Scripted {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            native_json_schema: false,
            output_token_limit: false,
        }
    }
    fn complete(
        &self,
        request: ModelRequest,
        _context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected model call")
        })
    }
}

#[cfg(feature = "http")]
pub mod wire;
