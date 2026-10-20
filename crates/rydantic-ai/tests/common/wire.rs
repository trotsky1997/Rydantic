use axum::{
    extract::{OriginalUri, State},
    http::HeaderMap,
    routing::post,
    Json, Router,
};
use rydantic_ai::{Api, HttpProvider};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct Reply {
    pub content_type: &'static str,
    pub body: String,
}
pub type Captured = Arc<Mutex<Vec<(String, HeaderMap, Value)>>>;
#[derive(Clone)]
struct Fixture {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    seen: Captured,
}
pub struct Server {
    pub base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub async fn serve(app: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { base, task }
}
pub async fn fixture(replies: Vec<Reply>) -> (Server, Captured) {
    async fn handler(
        State(state): State<Fixture>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> impl axum::response::IntoResponse {
        state
            .seen
            .lock()
            .unwrap()
            .push((uri.path().into(), headers, body));
        let reply = state
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected request");
        // Exercise HTTP-body fragmentation inside UTF-8 and SSE frame boundaries.
        let bytes: Vec<_> = reply
            .body
            .into_bytes()
            .into_iter()
            .map(|b| Ok::<_, std::io::Error>(vec![b]))
            .collect();
        (
            [("content-type", reply.content_type)],
            axum::body::Body::from_stream(futures_util::stream::iter(bytes)),
        )
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let state = Fixture {
        replies: Arc::new(Mutex::new(replies.into())),
        seen: seen.clone(),
    };
    (
        serve(
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .route("/v1/messages", post(handler))
                .route("/v1/responses", post(handler))
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
                .with_state(state),
        )
        .await,
        seen,
    )
}
pub fn protocols() -> [Api; 2] {
    [Api::ChatCompletions, Api::AnthropicMessages]
}
pub fn native(api: Api, text: &str, tool: Option<(&str, Value)>) -> Value {
    let usage = json!({"input_tokens":3,"output_tokens":5});
    let has_tool = tool.is_some();
    match api {
        Api::ChatCompletions => {
            let mut message = json!({"role":"assistant","content":text});
            if let Some((id, input)) = tool {
                message["reasoning_content"] = json!("retained reasoning");
                message["tool_calls"] = json!([{"id":id,"type":"function","function":{"name":"add","arguments":input.to_string()}}]);
            }
            json!({"id":"chat-1","object":"chat.completion","model":"test-model","choices":[{"index":0,"message":message,"finish_reason":if has_tool {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}})
        }
        Api::AnthropicMessages => {
            let mut content = vec![];
            if !text.is_empty() {
                content.push(json!({"type":"text","text":text}));
            }
            if let Some((id, input)) = tool {
                content.push(json!({"type":"thinking","thinking":"retained reasoning","signature":"opaque-signature"}));
                content.push(json!({"type":"redacted_thinking","data":"opaque-redacted"}));
                content.push(json!({"type":"tool_use","id":id,"name":"add","input":input}));
            }
            json!({"id":"msg-1","type":"message","role":"assistant","model":"test-model","content":content,"stop_reason":if has_tool {"tool_use"} else {"end_turn"},"stop_sequence":null,"usage":usage})
        }
        Api::Responses => {
            let mut output = vec![];
            if has_tool {
                output.push(
                    json!({"type":"reasoning","id":"rs_1","encrypted_content":"opaque-reasoning"}),
                );
            }
            output.push(json!({"id":"message_1","type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}));
            if let Some((id, input)) = tool {
                output.push(json!({"id":format!("fc_{id}"),"type":"function_call","call_id":id,"name":"add","arguments":input.to_string()}));
            }
            json!({"id":"resp-1","object":"response","status":"completed","model":"test-model","output":output,"usage":usage})
        }
    }
}
pub fn data(wire: &mut String, value: Value) {
    wire.push_str(&format!("data: {value}\r\n\r\n"));
}
pub fn sse(api: Api, raw: &Value) -> Reply {
    let mut body = ": keepalive\r\n\r\n".to_owned();
    match api {
        Api::ChatCompletions => {
            let choice = &raw["choices"][0];
            let message = &choice["message"];
            let mut chunk = |delta: Value, reason: Value| {
                data(
                    &mut body,
                    json!({"id":raw["id"],"object":"chat.completion.chunk","model":raw["model"],"choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":null}),
                )
            };
            chunk(json!({"role":"assistant"}), Value::Null);
            if let Some(reasoning) = message.get("reasoning_content") {
                chunk(json!({"reasoning_content":reasoning}), Value::Null);
            }
            for ch in message["content"].as_str().unwrap_or_default().chars() {
                chunk(json!({"content":ch.to_string()}), Value::Null);
            }
            if let Some(tools) = message["tool_calls"].as_array() {
                for (index, tool) in tools.iter().enumerate() {
                    chunk(
                        json!({"tool_calls":[{"index":index,"id":tool["id"],"type":"function","function":{"name":"a","arguments":""}}]}),
                        Value::Null,
                    );
                    chunk(
                        json!({"tool_calls":[{"index":index,"function":{"name":"dd","arguments":tool["function"]["arguments"]}}]}),
                        Value::Null,
                    );
                }
            }
            chunk(json!({}), choice["finish_reason"].clone());
            data(
                &mut body,
                json!({"id":raw["id"],"model":raw["model"],"choices":[],"usage":raw["usage"]}),
            );
            body.push_str("data: [DONE]\r\n\r\n");
        }
        Api::AnthropicMessages => {
            let mut start = raw.clone();
            start["content"] = json!([]);
            start["stop_reason"] = Value::Null;
            start["usage"]["output_tokens"] = json!(0);
            data(&mut body, json!({"type":"message_start","message":start}));
            for (index, block) in raw["content"].as_array().unwrap().iter().enumerate() {
                let mut start = block.clone();
                match block["type"].as_str().unwrap() {
                    "text" => {
                        start["text"] = json!("");
                    }
                    "thinking" => {
                        start["thinking"] = json!("");
                        start["signature"] = json!("");
                    }
                    "tool_use" => {
                        start["input"] = json!({});
                    }
                    _ => (),
                }
                data(
                    &mut body,
                    json!({"type":"content_block_start","index":index,"content_block":start}),
                );
                match block["type"].as_str().unwrap() {
                    "text" => {
                        for ch in block["text"].as_str().unwrap().chars() {
                            data(
                                &mut body,
                                json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":ch.to_string()}}),
                            );
                        }
                    }
                    "thinking" => {
                        data(
                            &mut body,
                            json!({"type":"content_block_delta","index":index,"delta":{"type":"thinking_delta","thinking":block["thinking"]}}),
                        );
                        data(
                            &mut body,
                            json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":block["signature"]}}),
                        );
                    }
                    "tool_use" => {
                        let args = block["input"].to_string();
                        let (a, b) = args.split_at(1);
                        for fragment in [a, b] {
                            data(
                                &mut body,
                                json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":fragment}}),
                            );
                        }
                    }
                    _ => (),
                }
                data(
                    &mut body,
                    json!({"type":"content_block_stop","index":index}),
                );
            }
            data(
                &mut body,
                json!({"type":"message_delta","delta":{"stop_reason":raw["stop_reason"],"stop_sequence":null},"usage":raw["usage"]}),
            );
            data(&mut body, json!({"type":"message_stop"}));
        }
        Api::Responses => {
            data(
                &mut body,
                json!({"type":"response.created","response":{"id":raw["id"],"model":raw["model"],"status":"in_progress","output":[]}}),
            );
            for (index, item) in raw["output"].as_array().unwrap().iter().enumerate() {
                let mut start = item.clone();
                if item["type"] == "function_call" {
                    start["arguments"] = json!("");
                }
                data(
                    &mut body,
                    json!({"type":"response.output_item.added","output_index":index,"item":start}),
                );
                if item["type"] == "message" {
                    for part in item["content"].as_array().unwrap() {
                        for text in part["text"].as_str().unwrap().chars() {
                            data(
                                &mut body,
                                json!({"type":"response.output_text.delta","output_index":index,"content_index":0,"item_id":item["id"],"delta":text.to_string()}),
                            );
                        }
                    }
                } else if item["type"] == "function_call" {
                    let args = item["arguments"].as_str().unwrap();
                    let (a, b) = args.split_at(1);
                    for delta in [a, b] {
                        data(
                            &mut body,
                            json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":item["id"],"delta":delta}),
                        );
                    }
                }
                data(
                    &mut body,
                    json!({"type":"response.output_item.done","output_index":index,"item":item}),
                );
            }
            data(
                &mut body,
                json!({"type":"response.completed","response":raw}),
            );
        }
    }
    Reply {
        content_type: "text/event-stream",
        body,
    }
}
pub fn json_reply(raw: Value) -> Reply {
    Reply {
        content_type: "application/json",
        body: raw.to_string(),
    }
}
pub fn provider(api: Api, base: &str) -> Arc<HttpProvider> {
    Arc::new(HttpProvider::new(api, base, Some("fixture-key".into())).unwrap())
}
