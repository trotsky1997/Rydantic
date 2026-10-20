#![cfg(feature = "http")]
mod common;
use common::wire::*;
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    rydantic::Model,
    Agent, Api, Capabilities, Error, HttpProvider, ModelProvider, ModelRequest, OutputEvent,
    ResponsesProvider, RunContext, ToolDefinition, TypedTool,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

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
#[derive(Debug, Model)]
struct Person {
    name: String,
}

#[tokio::test]
async fn native_json_mapping_auth_schema_and_usage() {
    for api in protocols() {
        let (server, seen) = fixture(vec![json_reply(native(api, "你好😊", None))]).await;
        let provider = provider(api, &server.base);
        let mut request = ModelRequest::new("test-model", "hello");
        request.instructions = Some("system instruction".into());
        request.max_output_tokens = Some(99);
        request.text = Some(
            json!({"format":{"type":"json_schema","name":"answer","strict":false,"schema":{"type":"object","properties":{"value":{"type":"integer"}}}}}),
        );
        request.tools.push(ToolDefinition {
            kind: "function".into(),
            name: "add".into(),
            description: "Add".into(),
            parameters: json!({"type":"object"}),
            strict: false,
        });
        let response = provider
            .complete(request, RunContext::default())
            .await
            .unwrap();
        assert_eq!(response.text().unwrap(), "你好😊");
        assert_eq!(response.usage.total().unwrap(), 8);
        let seen = seen.lock().unwrap();
        let (path, headers, body) = &seen[0];
        assert!(body.get("input").is_none());
        assert_eq!(body["stream"], true);
        if api == Api::ChatCompletions {
            assert_eq!(path, "/v1/chat/completions");
            assert_eq!(headers["authorization"], "Bearer fixture-key");
            assert_eq!(body["messages"][0]["content"], "system instruction");
            assert_eq!(body["max_completion_tokens"], 99);
            assert_eq!(body["response_format"]["json_schema"]["name"], "answer");
            assert_eq!(body["tools"][0]["function"]["name"], "add");
        } else {
            assert_eq!(path, "/v1/messages");
            assert_eq!(headers["x-api-key"], "fixture-key");
            assert_eq!(headers["anthropic-version"], "2023-06-01");
            assert!(headers.get("authorization").is_none());
            assert_eq!(body["system"], "system instruction");
            assert_eq!(body["max_tokens"], 99);
            assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
            assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        }
    }
}

#[tokio::test]
async fn sse_normalizes_native_events_and_text_with_final_usage() {
    for api in protocols() {
        let (server, _) = fixture(vec![sse(api, &native(api, "你好😊", None))]).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(128);
        let result = provider(api, &server.base)
            .complete(
                ModelRequest::new("test-model", "hello"),
                RunContext::default().with_events(tx),
            )
            .await
            .unwrap();
        let mut text = String::new();
        let mut terminal = 0;
        let mut native_events = 0;
        while let Some(event) = rx.recv().await {
            match event["type"].as_str().unwrap() {
                "response.output_text.delta" => text.push_str(event["delta"].as_str().unwrap()),
                "provider.event" => native_events += 1,
                "response.completed" => terminal += 1,
                other => panic!("unexpected event {other}"),
            }
        }
        assert_eq!(text, result.text().unwrap());
        assert_eq!(text, "你好😊");
        assert_eq!(terminal, 1);
        assert!(native_events > 3);
        assert_eq!(result.usage.total().unwrap(), 8);
    }
}

async fn tool_round_trip(api: Api, through_gateway: bool) {
    let (server, seen) = fixture(vec![
        sse(
            api,
            &native(api, "", Some(("invalid", json!({"a":"bad","b":2})))),
        ),
        sse(
            api,
            &native(api, "", Some(("valid", json!({"a":19,"b":23})))),
        ),
        sse(api, &native(api, r#"{"value":42}"#, None)),
    ])
    .await;
    let upstream: Arc<dyn ModelProvider> = provider(api, &server.base);
    let gateway_server;
    let model_provider: Arc<dyn ModelProvider> = if through_gateway {
        let gateway = Gateway::new(
            BTreeMap::from([(
                "alias".into(),
                vec![Route {
                    provider: upstream,
                    model: "test-model".into(),
                    price: None,
                }],
            )]),
            vec![ClientPolicy {
                id: "app".into(),
                key: "gateway-fixture-key".into(),
                models: HashSet::from(["alias".into()]),
                budget: Budget::default(),
            }],
            Budget::default(),
        )
        .unwrap();
        gateway_server = serve(gateway::router(gateway)).await;
        Arc::new(
            ResponsesProvider::gateway(&gateway_server.base, Some("gateway-fixture-key".into()))
                .unwrap(),
        )
    } else {
        upstream
    };
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", move |args: Add, _, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        async move {
            Ok(Answer {
                value: args.a + args.b,
            })
        }
    })
    .unwrap();
    let result = Agent::<(), Answer>::new(
        model_provider,
        if through_gateway {
            "alias"
        } else {
            "test-model"
        },
    )
    .tool(tool)
    .unwrap()
    .run("compute", Arc::new(()), RunContext::default())
    .await
    .unwrap();
    assert_eq!(result.output.value, 42);
    assert_eq!(result.model_calls, 3);
    assert_eq!(result.tool_calls, 2);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(result.usage.total().unwrap(), 24);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    let history = seen[1].2["messages"].as_array().unwrap();
    if api == Api::ChatCompletions {
        let assistant = history.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["reasoning_content"], "retained reasoning");
        assert_eq!(assistant["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(history.last().unwrap()["role"], "tool");
        assert_eq!(history.last().unwrap()["tool_call_id"], "invalid");
        assert!(history.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("invalid_tool_arguments"));
    } else {
        let assistant = history.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["content"][0]["signature"], "opaque-signature");
        assert_eq!(assistant["content"][1]["data"], "opaque-redacted");
        assert_eq!(assistant["content"].as_array().unwrap().len(), 3);
        assert_eq!(
            history.last().unwrap()["content"][0]["tool_use_id"],
            "invalid"
        );
        assert!(history.last().unwrap()["content"][0]["content"]
            .as_str()
            .unwrap()
            .contains("invalid_tool_arguments"));
    }
}
#[tokio::test]
async fn tool_validation_fragments_and_native_history_survive_continuation() {
    for api in protocols() {
        tool_round_trip(api, false).await;
    }
}
#[tokio::test]
async fn http_gateway_preserves_each_upstreams_native_tool_history() {
    for api in protocols() {
        tool_round_trip(api, true).await;
    }
}

#[tokio::test]
async fn partial_and_iterable_work_with_both_native_protocols() {
    for api in protocols() {
        let (server, seen) = fixture(vec![
            sse(api, &native(api, r#"{"value":-1}"#, None)),
            sse(api, &native(api, r#"{"value":42}"#, None)),
            sse(
                api,
                &native(api, r#"{"items":[{"name":"你好😊"},{"name":"Ada"}]}"#, None),
            ),
        ])
        .await;
        let model = provider(api, &server.base);
        let mut partial = Agent::<(), Answer>::new(model.clone(), "test-model")
            .stream_partial("compute", Arc::new(()), RunContext::default())
            .unwrap();
        let mut snapshots = 0;
        let mut resets = 0;
        let mut final_value = None;
        while let Some(event) = partial.recv().await {
            match event.unwrap() {
                OutputEvent::Delta { .. } => snapshots += 1,
                OutputEvent::Reset { .. } => resets += 1,
                OutputEvent::Complete(result) => {
                    assert_eq!(result.model_calls, 2);
                    final_value = Some(result.output.value);
                }
                _ => (),
            }
        }
        assert_eq!(final_value, Some(42));
        assert!(snapshots > 0);
        assert_eq!(resets, 1);
        let result = Agent::<(), Person>::new(model, "test-model")
            .stream_iterable("extract", Arc::new(()), RunContext::default())
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(
            result
                .output
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["你好😊", "Ada"]
        );
        let seen = seen.lock().unwrap();
        assert!(
            seen[1].2["messages"].as_array().unwrap().last().unwrap()["content"]
                .to_string()
                .contains("Validation errors")
        );
        if api == Api::AnthropicMessages {
            assert!(seen[0].2.get("output_config").is_none());
        }
    }
}

#[tokio::test]
async fn terminal_markers_and_final_usage_are_required() {
    for api in protocols() {
        let raw = native(api, r#"{"value":42}"#, None);
        let mut missing_terminal = sse(api, &raw);
        let marker = if api == Api::ChatCompletions {
            "data: [DONE]"
        } else {
            "data: {\"type\":\"message_stop\"}"
        };
        missing_terminal
            .body
            .truncate(missing_terminal.body.find(marker).unwrap());
        let mut missing_usage = raw.clone();
        missing_usage.as_object_mut().unwrap().remove("usage");
        let (server, _) = fixture(vec![missing_terminal, sse(api, &missing_usage)]).await;
        let model = provider(api, &server.base);
        for _ in 0..2 {
            assert!(
                matches!(
                    model
                        .complete(ModelRequest::new("m", "x"), RunContext::default())
                        .await,
                    Err(Error::Protocol(_))
                ),
                "{api:?}"
            );
        }
    }
}

#[tokio::test]
async fn truncation_refusal_and_native_errors_never_execute_tools() {
    for api in protocols() {
        for refusal in [false, true] {
            let mut raw = native(api, "", Some(("call", json!({"a":1,"b":2}))));
            if api == Api::ChatCompletions {
                raw["choices"][0]["finish_reason"] =
                    json!(if refusal { "content_filter" } else { "length" });
            } else {
                raw["stop_reason"] = json!(if refusal { "refusal" } else { "max_tokens" });
            }
            let (server, _) = fixture(vec![sse(api, &raw)]).await;
            let count = Arc::new(AtomicUsize::new(0));
            let calls = count.clone();
            let tool = TypedTool::<(), Add, Answer, _>::new("add", "Add", move |_: Add, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(Answer { value: 3 }) }
            })
            .unwrap();
            let result = Agent::<(), Answer>::new(provider(api, &server.base), "m")
                .tool(tool)
                .unwrap()
                .run("x", Arc::new(()), RunContext::default())
                .await;
            assert!(if refusal {
                matches!(result, Err(Error::Refusal))
            } else {
                matches!(result, Err(Error::Protocol(_)))
            });
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
        let mut body = String::new();
        data(
            &mut body,
            json!({"type":"error","error":{"message":"upstream-private-value"}}),
        );
        let (server, _) = fixture(vec![Reply {
            content_type: "text/event-stream",
            body,
        }])
        .await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let error = provider(api, &server.base)
            .complete(
                ModelRequest::new("m", "x"),
                RunContext::default().with_events(tx),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Protocol(_)));
        assert!(!error.to_string().contains("upstream-private-value"));
        assert!(rx.recv().await.is_none());
    }
}

#[tokio::test]
async fn messages_cache_usage_is_cumulative_and_overflow_checked() {
    let api = Api::AnthropicMessages;
    let mut raw = native(api, "ok", None);
    raw["usage"] = json!({"input_tokens":3,"cache_creation_input_tokens":4,"cache_read_input_tokens":7,"output_tokens":5});
    let mut overflow = raw.clone();
    overflow["usage"]["cache_read_input_tokens"] = json!(u64::MAX);
    let (server, _) = fixture(vec![sse(api, &raw), json_reply(overflow)]).await;
    let model = provider(api, &server.base);
    let result = model
        .complete(ModelRequest::new("m", "x"), RunContext::default())
        .await
        .unwrap();
    assert_eq!(result.usage.input_tokens, 14);
    assert_eq!(result.usage.output_tokens, 5);
    assert!(matches!(
        model
            .complete(ModelRequest::new("m", "x"), RunContext::default())
            .await,
        Err(Error::Limit(_))
    ));
}

#[tokio::test]
async fn unsupported_input_and_reasoning_fail_before_network_dispatch() {
    for api in protocols() {
        let (server, seen) = fixture(vec![]).await;
        let model = provider(api, &server.base);
        let mut request = ModelRequest::new("m", "x");
        request.reasoning = Some(json!({"effort":"high"}));
        assert!(matches!(
            model.complete(request, RunContext::default()).await,
            Err(Error::Configuration(_))
        ));
        let mut request = ModelRequest::new("m", "x");
        request.input[0]["content"] =
            json!([{"type":"input_image","image_url":"https://example.com/image.png"}]);
        assert!(matches!(
            model.complete(request, RunContext::default()).await,
            Err(Error::Configuration(_))
        ));
        assert!(seen.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn native_event_backpressure_obeys_deadlines_and_cancellation() {
    for api in protocols() {
        let reply = sse(api, &native(api, "ok", None));
        let (server, _) = fixture(vec![reply.clone(), reply]).await;
        let model = provider(api, &server.base);
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        assert!(matches!(
            model
                .complete(
                    ModelRequest::new("m", "x"),
                    RunContext::with_timeout(Duration::from_millis(80)).with_events(tx)
                )
                .await,
            Err(Error::Timeout)
        ));
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let context = RunContext::default().with_events(tx);
        let cancellation = context.cancellation.clone();
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            cancellation.cancel();
        });
        assert!(matches!(
            model.complete(ModelRequest::new("m", "x"), context).await,
            Err(Error::Cancelled)
        ));
        cancel.await.unwrap();
    }
}

#[test]
fn protocol_names_capabilities_and_credentials_are_explicit() {
    for (name, api) in [
        ("responses", Api::Responses),
        ("chat_completions", Api::ChatCompletions),
        ("anthropic_messages", Api::AnthropicMessages),
    ] {
        assert_eq!(name.parse::<Api>().unwrap(), api);
        assert_eq!(serde_json::from_value::<Api>(json!(name)).unwrap(), api);
        for base in [
            "https://example.com/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:1234/v1",
        ] {
            let model = HttpProvider::new(api, base, Some("private-fixture-key".into())).unwrap();
            assert!(!format!("{model:?}").contains("private-fixture-key"));
            assert!(!format!("{model:?}").contains(base));
            assert_eq!(
                model.capabilities().native_json_schema,
                api != Api::AnthropicMessages
            );
        }
        for key in ["", "key\nheader: injected", "key\rvalue"] {
            assert!(HttpProvider::new(api, "https://example.com/v1", Some(key.into())).is_err());
        }
        for base in [
            "https://user:secret@example.com/v1",
            "http://example.com/v1",
            "https://example.com/v1?key=secret",
        ] {
            assert!(HttpProvider::new(api, base, None).is_err());
        }
    }
    assert!("unknown".parse::<Api>().is_err());
    assert!(serde_json::from_value::<Capabilities>(
        json!({"native_json_schema":true,"output_token_limit":true,"unknown":true})
    )
    .is_err());
}

#[tokio::test]
async fn direct_responses_requests_strip_internal_native_replay_metadata() {
    let (chat, _) = fixture(vec![json_reply(native(
        Api::ChatCompletions,
        "hello",
        None,
    ))])
    .await;
    let response = provider(Api::ChatCompletions, &chat.base)
        .complete(ModelRequest::new("m", "x"), RunContext::default())
        .await
        .unwrap();
    let (server, seen) = fixture(vec![json_reply(json!({"id":"r","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1}}))]).await;
    let mut request = ModelRequest::new("m", "x");
    request.input.extend_from_slice(response.output());
    ResponsesProvider::new(&server.base, None)
        .unwrap()
        .complete(request, RunContext::default())
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].2["input"][1].get("_rydantic_native").is_none());
    assert_eq!(seen[0].2["input"][1]["content"][0]["text"], "hello");
}

#[tokio::test]
async fn malformed_stream_state_and_inconsistent_usage_fail_explicitly() {
    let chat = Api::ChatCompletions;
    let messages = Api::AnthropicMessages;
    let mut changed_id = sse(chat, &native(chat, "ok", None));
    let first_end = changed_id.body.find("\r\n\r\n").unwrap() + 4;
    // Keep the first chunk's ID and change all later chunks to a different ID.
    let data_end = changed_id.body[first_end..].find("\r\n\r\n").unwrap() + first_end + 4;
    changed_id.body = format!(
        "{}{}",
        &changed_id.body[..data_end],
        changed_id.body[data_end..].replace("chat-1", "changed")
    );
    let mut bad_total = native(chat, "ok", None);
    bad_total["usage"]["total_tokens"] = json!(1);
    let mut duplicate_block = sse(messages, &native(messages, "ok", None));
    let start =
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}});
    duplicate_block.body = duplicate_block.body.replace(
        &format!("data: {start}\r\n\r\n"),
        &format!("data: {start}\r\n\r\ndata: {start}\r\n\r\n"),
    );
    let mut missing_delta_usage = sse(messages, &native(messages, "ok", None));
    let old = json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":3,"output_tokens":5}});
    let new =
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}});
    missing_delta_usage.body = missing_delta_usage
        .body
        .replace(&old.to_string(), &new.to_string());
    let mut unclosed = sse(messages, &native(messages, "ok", None));
    unclosed.body = unclosed.body.replace(
        &format!(
            "data: {}\r\n\r\n",
            json!({"type":"content_block_stop","index":0})
        ),
        "",
    );
    for (api, reply) in [
        (chat, changed_id),
        (chat, json_reply(bad_total)),
        (messages, duplicate_block),
        (messages, missing_delta_usage),
        (messages, unclosed),
    ] {
        let (server, _) = fixture(vec![reply]).await;
        assert!(
            matches!(
                provider(api, &server.base)
                    .complete(ModelRequest::new("m", "x"), RunContext::default())
                    .await,
                Err(Error::Protocol(_))
            ),
            "{api:?}"
        );
    }
}
