#![cfg(feature = "http")]
mod common;
use common::{wire::*, *};
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    rydantic::Model,
    Agent, Api, HttpProvider, ModelProvider, ModelRequest, OutputEvent, RunContext, TypedTool,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Notify;

const KEY: &str = "gateway-protocol-fixture-key";
fn all() -> [Api; 3] {
    [Api::Responses, Api::ChatCompletions, Api::AnthropicMessages]
}
fn path(api: Api) -> &'static str {
    match api {
        Api::Responses => "responses",
        Api::ChatCompletions => "chat/completions",
        Api::AnthropicMessages => "messages",
    }
}
fn setup(upstream: Arc<dyn ModelProvider>, limit: u64) -> Gateway {
    let budget = Budget {
        max_requests: limit,
        reservation_tokens: 10,
        ..Budget::default()
    };
    Gateway::new(
        BTreeMap::from([(
            "public-model".into(),
            vec![Route {
                provider: upstream,
                model: "test-model".into(),
                price: None,
            }],
        )]),
        vec![
            ClientPolicy {
                id: "app".into(),
                key: KEY.into(),
                models: HashSet::from(["public-model".into()]),
                budget: budget.clone(),
            },
            ClientPolicy {
                id: "other".into(),
                key: "other-client-fixture-key".into(),
                models: HashSet::from(["public-model".into()]),
                budget: budget.clone(),
            },
        ],
        budget,
    )
    .unwrap()
}
fn request(api: Api, stream: bool) -> Value {
    match api {
        Api::Responses => {
            json!({"model":"public-model","input":[{"role":"user","content":"hello"}],"stream":stream})
        }
        Api::ChatCompletions => {
            json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"stream":stream,"stream_options":{"include_usage":true}})
        }
        Api::AnthropicMessages => {
            json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":256,"stream":stream})
        }
    }
}
async fn post(base: &str, api: Api, body: &Value, key: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    let call = client.post(format!("{base}/{}", path(api))).json(body);
    if api == Api::AnthropicMessages {
        call.header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .unwrap()
    } else {
        call.bearer_auth(key).send().await.unwrap()
    }
}
fn text(api: Api, response: &Value) -> String {
    match api {
        Api::Responses => response["output"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|i| i["content"].as_array().into_iter().flatten())
            .filter_map(|p| p["text"].as_str())
            .collect(),
        Api::ChatCompletions => response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .into(),
        Api::AnthropicMessages => response["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect(),
    }
}
#[tokio::test]
async fn all_nine_json_routes_translate_shape_auth_aliases_and_usage() {
    for source in all() {
        for front in all() {
            let (upstream, seen) = fixture(vec![json_reply(native(source, "你好😊", None))]).await;
            let gateway = setup(provider(source, &upstream.base), 10);
            let server = serve(gateway::router(gateway.clone())).await;
            let response = post(&server.base, front, &request(front, false), KEY).await;
            assert!(response.status().is_success(), "{front:?} -> {source:?}");
            let raw: Value = response.json().await.unwrap();
            assert_eq!(raw["model"], "public-model");
            assert_eq!(text(front, &raw), "你好😊");
            assert!(!raw.to_string().contains("_rydantic_native"));
            assert!(!raw.to_string().contains("provider_response"));
            assert!(!raw.to_string().contains("test-model"));
            match front {
                Api::Responses => {
                    assert_eq!(raw["object"], "response");
                    assert_eq!(raw["usage"]["total_tokens"], 8);
                }
                Api::ChatCompletions => {
                    assert_eq!(raw["object"], "chat.completion");
                    assert_eq!(raw["choices"][0]["finish_reason"], "stop");
                    assert_eq!(raw["usage"]["total_tokens"], 8);
                }
                Api::AnthropicMessages => {
                    assert_eq!(raw["type"], "message");
                    assert_eq!(raw["stop_reason"], "end_turn");
                    assert_eq!(raw["usage"]["input_tokens"], 3);
                }
            }
            let seen = seen.lock().unwrap();
            assert_eq!(seen[0].0, format!("/v1/{}", path(source)));
            assert_eq!(seen[0].2["model"], "test-model");
            if source == Api::AnthropicMessages {
                assert_eq!(seen[0].1["x-api-key"], "fixture-key");
            } else {
                assert_eq!(seen[0].1["authorization"], "Bearer fixture-key");
            }
            assert_eq!(gateway.snapshot("app").unwrap().tokens, 8);
        }
    }
}
#[tokio::test]
async fn chat_instruction_roles_and_order_survive_supported_routes() {
    let messages = json!([
        {"role":"system","content":"system first"},
        {"role":"developer","content":"developer first"},
        {"role":"user","content":"question"},
        {"role":"assistant","content":"earlier answer"},
        {"role":"developer","content":"developer later"},
        {"role":"user","content":"next question"}
    ]);
    for source in [Api::Responses, Api::ChatCompletions] {
        let (upstream, seen) = fixture(vec![sse(source, &native(source, "ok", None))]).await;
        let server = serve(gateway::router(setup(provider(source, &upstream.base), 10))).await;
        let response = post(
            &server.base,
            Api::ChatCompletions,
            &json!({"model":"public-model","messages":messages}),
            KEY,
        )
        .await;
        assert_eq!(response.status(), 200);
        let captured = seen.lock().unwrap();
        let history = if source == Api::Responses {
            &captured[0].2["input"]
        } else {
            &captured[0].2["messages"]
        };
        let roles: Vec<_> = history
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(
            roles,
            [
                "system",
                "developer",
                "user",
                "assistant",
                "developer",
                "user"
            ]
        );
        assert_eq!(history[4]["content"], "developer later");
    }
    let (upstream, seen) = fixture(vec![sse(
        Api::AnthropicMessages,
        &native(Api::AnthropicMessages, "ok", None),
    )])
    .await;
    let server = serve(gateway::router(setup(
        provider(Api::AnthropicMessages, &upstream.base),
        10,
    )))
    .await;
    let response = post(
        &server.base,
        Api::ChatCompletions,
        &json!({"model":"public-model","messages":messages}),
        KEY,
    )
    .await;
    assert_eq!(response.status(), 400);
    assert!(seen.lock().unwrap().is_empty());
    let leading = messages.as_array().unwrap()[..3].to_vec();
    let response = post(
        &server.base,
        Api::ChatCompletions,
        &json!({"model":"public-model","messages":leading}),
        KEY,
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        seen.lock().unwrap()[0].2["system"],
        "system first\ndeveloper first"
    );
}

#[tokio::test]
async fn all_nine_sse_routes_work_with_native_protocol_clients() {
    for source in all() {
        for front in all() {
            let (upstream, _) = fixture(vec![sse(source, &native(source, "你好😊", None))]).await;
            let gateway = setup(provider(source, &upstream.base), 10);
            let server = serve(gateway::router(gateway.clone())).await;
            let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::channel(256);
            let result = client
                .complete(
                    ModelRequest::new("public-model", "hello"),
                    RunContext::default().with_events(tx),
                )
                .await
                .unwrap_or_else(|e| panic!("{front:?} -> {source:?}: {e}"));
            assert_eq!(result.text().unwrap(), "你好😊");
            assert_eq!(result.usage.total().unwrap(), 8);
            let mut deltas = String::new();
            let mut complete = 0;
            while let Some(event) = rx.recv().await {
                if event["type"] == "response.output_text.delta" {
                    deltas.push_str(event["delta"].as_str().unwrap());
                }
                if event["type"] == "response.completed" {
                    complete += 1;
                }
            }
            assert_eq!(deltas, "你好😊");
            assert_eq!(complete, 1);
            assert_eq!(gateway.snapshot("app").unwrap().tokens, 8);
        }
    }
}

#[derive(Debug, Model)]
struct Answer {
    #[model(ge = 0)]
    value: i64,
}
#[derive(Model)]
struct Add {
    a: i64,
    b: i64,
}
#[tokio::test]
async fn all_nine_tool_continuations_restore_native_history_without_client_extensions() {
    for source in all() {
        for front in all() {
            let (upstream, seen) = fixture(vec![
                sse(
                    source,
                    &native(source, "working", Some(("call-1", json!({"a":19,"b":23})))),
                ),
                sse(source, &native(source, r#"{"value":42}"#, None)),
            ])
            .await;
            let gateway = setup(provider(source, &upstream.base), 10);
            let server = serve(gateway::router(gateway.clone())).await;
            let client =
                Arc::new(HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap());
            let tool =
                TypedTool::<(), Add, Answer, _>::new("add", "Add", |args: Add, _, _| async move {
                    Ok(Answer {
                        value: args.a + args.b,
                    })
                })
                .unwrap();
            let result = Agent::<(), Answer>::new(client, "public-model")
                .tool(tool)
                .unwrap()
                .run("compute", Arc::new(()), RunContext::default())
                .await
                .unwrap_or_else(|e| panic!("{front:?} -> {source:?}: {e}"));
            assert_eq!(result.output.value, 42);
            assert_eq!(result.tool_calls, 1);
            assert_eq!(result.model_calls, 2);
            assert_eq!(result.usage.total().unwrap(), 16);
            let seen = seen.lock().unwrap();
            let next = &seen[1].2;
            match source {
                Api::Responses => assert!(next["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["encrypted_content"] == "opaque-reasoning")),
                Api::ChatCompletions => {
                    let assistant = next["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|v| v["role"] == "assistant")
                        .unwrap();
                    assert_eq!(assistant["reasoning_content"], "retained reasoning");
                    assert_eq!(assistant["tool_calls"].as_array().unwrap().len(), 1);
                }
                Api::AnthropicMessages => {
                    let assistant = next["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|v| v["role"] == "assistant")
                        .unwrap();
                    assert!(assistant["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|b| b["signature"] == "opaque-signature"));
                    assert!(assistant["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|b| b["data"] == "opaque-redacted"));
                }
            }
        }
    }
}

#[tokio::test]
async fn tool_only_responses_preserve_original_item_ids_for_every_frontend() {
    for front in all() {
        let mut first = native(Api::Responses, "", Some(("call-1", json!({"a":19,"b":23}))));
        first["output"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item["type"] == "function_call");
        let (upstream, seen) = fixture(vec![
            sse(Api::Responses, &first),
            sse(
                Api::Responses,
                &native(Api::Responses, r#"{"value":42}"#, None),
            ),
        ])
        .await;
        let server = serve(gateway::router(setup(
            provider(Api::Responses, &upstream.base),
            10,
        )))
        .await;
        let client = Arc::new(HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap());
        let tool =
            TypedTool::<(), Add, Answer, _>::new("add", "Add", |args: Add, _, _| async move {
                Ok(Answer {
                    value: args.a + args.b,
                })
            })
            .unwrap();
        let result = Agent::<(), Answer>::new(client, "public-model")
            .tool(tool)
            .unwrap()
            .run("compute", Arc::new(()), RunContext::default())
            .await
            .unwrap();
        assert_eq!(result.output.value, 42);
        let seen = seen.lock().unwrap();
        let replayed = seen[1].2["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call")
            .unwrap();
        assert_eq!(replayed, &first["output"][0], "{front:?}");
    }
}

#[tokio::test]
async fn all_frontends_share_output_validation_partial_and_iterable_modes() {
    #[derive(Debug, Model)]
    struct Person {
        name: String,
    }
    for front in all() {
        let source = Api::AnthropicMessages;
        let (upstream, _) = fixture(vec![
            sse(source, &native(source, r#"{"value":-1}"#, None)),
            sse(source, &native(source, r#"{"value":42}"#, None)),
            sse(
                source,
                &native(
                    source,
                    r#"{"items":[{"name":"Ada"},{"name":"你好😊"}]}"#,
                    None,
                ),
            ),
        ])
        .await;
        let server = serve(gateway::router(setup(provider(source, &upstream.base), 10))).await;
        let client = Arc::new(HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap());
        let mut partial = Agent::<(), Answer>::new(client.clone(), "public-model")
            .stream_partial("extract", Arc::new(()), RunContext::default())
            .unwrap();
        let mut accepted = false;
        let mut reset = false;
        while let Some(event) = partial.recv().await {
            match event.unwrap() {
                OutputEvent::Complete(result) => {
                    assert_eq!(result.output.value, 42);
                    accepted = true;
                }
                OutputEvent::Reset { .. } => reset = true,
                _ => (),
            }
        }
        assert!(accepted && reset);
        let result = Agent::<(), Person>::new(client, "public-model")
            .stream_iterable("extract", Arc::new(()), RunContext::default())
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(
            result
                .output
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>(),
            ["Ada", "你好😊"]
        );
    }
}

#[tokio::test]
async fn authentication_errors_and_unsupported_input_are_native_and_never_dispatch() {
    for front in all() {
        let scripted = Arc::new(Scripted::new(vec![]));
        let gateway = setup(scripted.clone(), 10);
        let server = serve(gateway::router(gateway.clone())).await;
        let denied = post(&server.base, front, &request(front, false), "invalid-key").await;
        assert_eq!(denied.status(), 401);
        let error: Value = denied.json().await.unwrap();
        assert_eq!(error["error"]["type"], "authentication_error");
        if front == Api::AnthropicMessages {
            assert_eq!(error["type"], "error");
        }
        let mut body = request(front, false);
        body["unsupported_private_setting"] = json!("private-value-never-expose");
        let error = post(&server.base, front, &body, KEY).await;
        assert_eq!(error.status(), 400);
        assert!(!error
            .text()
            .await
            .unwrap()
            .contains("private-value-never-expose"));
        let mut body = request(front, false);
        body["model"] = json!("unauthorized-alias");
        assert_eq!(post(&server.base, front, &body, KEY).await.status(), 404);
        let malformed = reqwest::Client::new()
            .post(format!("{}/{}", server.base, path(front)))
            .bearer_auth(KEY)
            .header("content-type", "application/json")
            .body("{private-malformed-data")
            .send()
            .await
            .unwrap();
        assert_eq!(malformed.status(), 400);
        assert!(!malformed
            .text()
            .await
            .unwrap()
            .contains("private-malformed-data"));
        assert!(scripted.requests.lock().unwrap().is_empty());
        assert_eq!(gateway.snapshot("app").unwrap().requests, 0);
    }
}

#[tokio::test]
async fn sampling_tool_choice_and_schema_translate_in_all_nine_directions() {
    let schema =
        json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]});
    for source in all() {
        for front in all() {
            let (upstream, seen) =
                fixture(vec![json_reply(native(source, r#"{"value":42}"#, None))]).await;
            let server = serve(gateway::router(setup(provider(source, &upstream.base), 10))).await;
            let mut body = request(front, false);
            body["temperature"] = json!(0.5);
            body["top_p"] = json!(0.8);
            match front {
                Api::Responses => {
                    body["tools"] = json!([{"type":"function","name":"add","description":"Add","parameters":schema,"strict":false}]);
                    body["tool_choice"] = json!({"type":"function","name":"add"});
                    body["parallel_tool_calls"] = json!(false);
                    body["text"] = json!({"format":{"type":"json_schema","name":"result","schema":schema,"strict":false}});
                    body["max_output_tokens"] = json!(123);
                }
                Api::ChatCompletions => {
                    body["tools"] = json!([{"type":"function","function":{"name":"add","description":"Add","parameters":schema}}]);
                    body["tool_choice"] = json!({"type":"function","function":{"name":"add"}});
                    body["parallel_tool_calls"] = json!(false);
                    body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"result","schema":schema}});
                    body["max_completion_tokens"] = json!(123);
                }
                Api::AnthropicMessages => {
                    body["tools"] =
                        json!([{"name":"add","description":"Add","input_schema":schema}]);
                    body["tool_choice"] =
                        json!({"type":"tool","name":"add","disable_parallel_tool_use":true});
                    body["output_config"] =
                        json!({"format":{"type":"json_schema","schema":schema}});
                    body["max_tokens"] = json!(123);
                }
            }
            let response = post(&server.base, front, &body, KEY).await;
            assert_eq!(response.status(), 200, "{front:?}->{source:?}");
            let seen = seen.lock().unwrap();
            let sent = &seen[0].2;
            assert_eq!(sent["temperature"], 0.5);
            assert_eq!(sent["top_p"], 0.8);
            match source {
                Api::Responses => {
                    assert_eq!(sent["tool_choice"]["name"], "add");
                    assert_eq!(sent["parallel_tool_calls"], false);
                    assert_eq!(sent["max_output_tokens"], 123);
                    assert_eq!(sent["text"]["format"]["schema"], schema);
                }
                Api::ChatCompletions => {
                    assert_eq!(sent["tool_choice"]["function"]["name"], "add");
                    assert_eq!(sent["parallel_tool_calls"], false);
                    assert_eq!(sent["max_completion_tokens"], 123);
                    assert_eq!(sent["response_format"]["json_schema"]["schema"], schema);
                }
                Api::AnthropicMessages => {
                    assert_eq!(sent["tool_choice"]["name"], "add");
                    assert_eq!(sent["tool_choice"]["disable_parallel_tool_use"], true);
                    assert_eq!(sent["max_tokens"], 123);
                    assert_eq!(sent["output_config"]["format"]["schema"], schema);
                }
            }
        }
    }
}

#[tokio::test]
async fn upstream_stream_failure_never_becomes_a_success_marker() {
    for source in all() {
        for front in all() {
            let mut reply = sse(source, &native(source, "partial", None));
            let last = match source {
                Api::Responses => "data: {\"response\"",
                Api::ChatCompletions => "data: [DONE]",
                Api::AnthropicMessages => "data: {\"type\":\"message_stop\"}",
            };
            let at = reply.body.rfind(last).unwrap();
            reply.body.truncate(at);
            let (upstream, _) = fixture(vec![reply]).await;
            let gateway = setup(provider(source, &upstream.base), 1);
            let server = serve(gateway::router(gateway.clone())).await;
            let response = post(&server.base, front, &request(front, true), KEY).await;
            assert_eq!(response.status(), 200);
            let wire = response.text().await.unwrap();
            assert!(wire.contains("upstream_error"));
            assert!(!wire.contains("data: [DONE]"));
            assert!(!wire.contains("\"type\":\"message_stop\""));
            assert!(!wire.contains("\"type\":\"response.completed\""));
            assert_eq!(gateway.snapshot("app").unwrap().tokens, 10);
        }
    }
}

#[tokio::test]
async fn terminal_markers_wait_for_successful_budget_settlement() {
    for source in all() {
        for front in all() {
            let first = native(source, "first", None);
            let mut overflow = native(source, "uncommitted", None);
            match source {
                Api::ChatCompletions => {
                    overflow["usage"] = json!({"prompt_tokens":u64::MAX,"completion_tokens":0,"total_tokens":u64::MAX})
                }
                _ => overflow["usage"] = json!({"input_tokens":u64::MAX,"output_tokens":0}),
            }
            let (upstream, _) = fixture(vec![json_reply(first), sse(source, &overflow)]).await;
            let gateway = setup(provider(source, &upstream.base), 10);
            let server = serve(gateway::router(gateway.clone())).await;
            assert_eq!(
                post(&server.base, front, &request(front, false), KEY)
                    .await
                    .status(),
                200
            );
            let wire = post(&server.base, front, &request(front, true), KEY)
                .await
                .text()
                .await
                .unwrap();
            assert!(
                wire.contains("budget_exhausted"),
                "{front:?}->{source:?}: {wire}"
            );
            assert!(!wire.contains("data: [DONE]"));
            assert!(!wire.contains("\"type\":\"message_stop\""));
            assert!(!wire.contains("\"type\":\"response.completed\""));
            assert_eq!(gateway.snapshot("app").unwrap().tokens, 18);
        }
    }
}

#[tokio::test]
async fn chat_usage_option_and_messages_authentication_follow_the_frontend_protocol() {
    let scripted = Arc::new(Scripted::new(vec![
        Ok(response("first")),
        Ok(response("second")),
    ]));
    let server = serve(gateway::router(setup(scripted, 10))).await;
    let mut body = request(Api::ChatCompletions, true);
    body.as_object_mut().unwrap().remove("stream_options");
    let wire = post(&server.base, Api::ChatCompletions, &body, KEY)
        .await
        .text()
        .await
        .unwrap();
    assert!(wire.contains("data: [DONE]"));
    assert!(!wire.contains("\"usage\""));
    let denied = reqwest::Client::new()
        .post(format!("{}/messages", server.base))
        .header("x-api-key", KEY)
        .bearer_auth("conflicting-fixture-key")
        .json(&request(Api::AnthropicMessages, false))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    let okay = reqwest::Client::new()
        .post(format!("{}/messages", server.base))
        .bearer_auth(KEY)
        .json(&request(Api::AnthropicMessages, false))
        .send()
        .await
        .unwrap();
    assert_eq!(okay.status(), 200);
}

struct Staged {
    release: Arc<Notify>,
}
impl ModelProvider for Staged {
    fn complete(
        &self,
        _: ModelRequest,
        context: RunContext,
    ) -> rydantic_ai::BoxFuture<'_, rydantic_ai::Result<rydantic_ai::ModelResponse>> {
        Box::pin(async move {
            context
                .emit(json!({"type":"response.output_text.delta","delta":"working"}))
                .await?;
            context.emit(json!({"type":"response.output_item.added","output_index":1,"item":{"id":"fc-stream","type":"function_call","call_id":"call-stream","name":"add","arguments":""}})).await?;
            context.emit(json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc-stream","delta":r#"{"a":19"#})).await?;
            self.release.notified().await;
            Ok(response_items(vec![
                json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"working"}]}),
                json!({"type":"function_call","call_id":"call-stream","name":"add","arguments":r#"{"a":19,"b":23}"#}),
            ]))
        })
    }
}
#[tokio::test]
async fn text_and_tool_fragments_arrive_before_the_upstream_finishes() {
    for front in all() {
        let release = Arc::new(Notify::new());
        let server = serve(gateway::router(setup(
            Arc::new(Staged {
                release: release.clone(),
            }),
            10,
        )))
        .await;
        let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(128);
        let task = tokio::spawn(async move {
            client
                .complete(
                    ModelRequest::new("public-model", "compute"),
                    RunContext::default().with_events(tx),
                )
                .await
        });
        let mut saw_text = false;
        let mut saw_tool = false;
        tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(event) = rx.recv().await {
                if event["type"] == "response.output_text.delta" {
                    saw_text = true;
                }
                let raw = if event["type"] == "provider.event" {
                    &event["event"]
                } else {
                    &event
                };
                if raw["type"] == "response.function_call_arguments.delta"
                    || raw["delta"]["type"] == "input_json_delta"
                    || raw["choices"][0]["delta"]["tool_calls"]
                        .as_array()
                        .is_some_and(|v| {
                            v.iter().any(|c| {
                                c["function"]["arguments"]
                                    .as_str()
                                    .is_some_and(|s| !s.is_empty())
                            })
                        })
                {
                    saw_tool = true;
                }
                if saw_text && saw_tool {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(saw_text && saw_tool && !task.is_finished());
        release.notify_one();
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.text().unwrap(), "working");
        assert_eq!(
            response.tool_calls().unwrap()[0].arguments,
            r#"{"a":19,"b":23}"#
        );
    }
}

#[tokio::test]
async fn cached_tokens_translate_without_double_counting() {
    for front in all() {
        let mut raw = native(Api::AnthropicMessages, "hello", None);
        raw["usage"] = json!({"input_tokens":3,"cache_creation_input_tokens":4,"cache_read_input_tokens":7,"output_tokens":5});
        let (upstream, _) = fixture(vec![json_reply(raw)]).await;
        let gateway = setup(provider(Api::AnthropicMessages, &upstream.base), 10);
        let server = serve(gateway::router(gateway.clone())).await;
        let result: Value = post(&server.base, front, &request(front, false), KEY)
            .await
            .json()
            .await
            .unwrap();
        match front {
            Api::Responses => {
                assert_eq!(result["usage"]["total_tokens"], 19);
                assert_eq!(
                    result["usage"]["input_tokens_details"]["cache_write_tokens"],
                    4
                );
                assert_eq!(result["usage"]["input_tokens_details"]["cached_tokens"], 7);
            }
            Api::ChatCompletions => {
                assert_eq!(result["usage"]["prompt_tokens"], 14);
                assert_eq!(result["usage"]["total_tokens"], 19);
            }
            Api::AnthropicMessages => {
                assert_eq!(result["usage"]["input_tokens"], 3);
                assert_eq!(result["usage"]["cache_read_input_tokens"], 7);
                assert_eq!(result["usage"]["cache_creation_input_tokens"], 4);
            }
        }
        assert_eq!(gateway.snapshot("app").unwrap().tokens, 19);
    }
}

#[tokio::test]
async fn replay_is_bound_to_client_model_and_conversation_prefix() {
    let source = Api::AnthropicMessages;
    let (upstream, seen) = fixture(vec![
        json_reply(native(
            source,
            "working",
            Some(("call-1", json!({"a":1,"b":2}))),
        )),
        json_reply(native(source, "done", None)),
        json_reply(native(source, "done", None)),
        json_reply(native(source, "done", None)),
    ])
    .await;
    let server = serve(gateway::router(setup(provider(source, &upstream.base), 10))).await;
    let mut first = request(Api::ChatCompletions, false);
    let response: Value = post(&server.base, Api::ChatCompletions, &first, KEY)
        .await
        .json()
        .await
        .unwrap();
    first["messages"]
        .as_array_mut()
        .unwrap()
        .push(response["choices"][0]["message"].clone());
    first["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"tool","tool_call_id":"call-1","content":"3"}));
    post(
        &server.base,
        Api::ChatCompletions,
        &first,
        "other-client-fixture-key",
    )
    .await
    .error_for_status()
    .unwrap();
    let mut changed = first.clone();
    changed["messages"][0]["content"] = json!("different conversation");
    post(&server.base, Api::ChatCompletions, &changed, KEY)
        .await
        .error_for_status()
        .unwrap();
    post(&server.base, Api::ChatCompletions, &first, KEY)
        .await
        .error_for_status()
        .unwrap();
    let seen = seen.lock().unwrap();
    for index in [1, 2] {
        assert!(!seen[index].2.to_string().contains("opaque-signature"));
    }
    assert!(seen[3].2.to_string().contains("opaque-signature"));
}
