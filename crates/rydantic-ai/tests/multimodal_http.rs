#![cfg(feature = "http")]
mod common;
use common::{wire::*, *};
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    rydantic::Model,
    Agent, Api, Error, HttpProvider, Image, ImageDetail, ModelProvider, ModelRequest, Prompt,
    RunContext, TypedTool,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

const KEY: &str = "image-gateway-fixture-key";
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const OTHER_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYPgPAAEDAQAIicLsAAAAAElFTkSuQmCC";
const URL: &str = "https://example.com/photo.png?signature=opaque-reference";
fn apis() -> [Api; 3] {
    [Api::Responses, Api::ChatCompletions, Api::AnthropicMessages]
}
fn gateway(provider: Arc<dyn ModelProvider>) -> Gateway {
    let budget = Budget {
        reservation_tokens: 10,
        ..Budget::default()
    };
    Gateway::new(
        BTreeMap::from([(
            "vision".into(),
            vec![Route {
                provider,
                model: "upstream".into(),
                price: None,
            }],
        )]),
        vec![ClientPolicy {
            id: "app".into(),
            key: KEY.into(),
            models: HashSet::from(["vision".into()]),
            budget: budget.clone(),
        }],
        budget,
    )
    .unwrap()
}
fn prompt() -> Prompt {
    Prompt::new("before")
        .image(Image::url(URL).unwrap())
        .text("between")
        .image(Image::base64("image/png", PNG).unwrap())
        .text("after")
}
fn content(api: Api) -> Value {
    match api {
        Api::Responses => json!([
            {"type":"input_text","text":"before"}, {"type":"input_image","image_url":URL,"detail":"auto"},
            {"type":"input_text","text":"between"}, {"type":"input_image","image_url":format!("data:image/png;base64,{PNG}"),"detail":"auto"},
            {"type":"input_text","text":"after"}]),
        Api::ChatCompletions => json!([
            {"type":"text","text":"before"}, {"type":"image_url","image_url":{"url":URL,"detail":"auto"}},
            {"type":"text","text":"between"}, {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PNG}"),"detail":"auto"}},
            {"type":"text","text":"after"}]),
        Api::AnthropicMessages => json!([
            {"type":"text","text":"before"}, {"type":"image","source":{"type":"url","url":URL}},
            {"type":"text","text":"between"}, {"type":"image","source":{"type":"base64","media_type":"image/png","data":PNG}},
            {"type":"text","text":"after"}]),
    }
}
fn request(api: Api, parts: Value) -> Value {
    if api == Api::Responses {
        json!({"model":"vision","input":[{"role":"user","content":parts}]})
    } else {
        let mut body = json!({"model":"vision","messages":[{"role":"user","content":parts}]});
        if api == Api::AnthropicMessages {
            body["max_tokens"] = json!(256);
        }
        body
    }
}
async fn post(base: &str, api: Api, body: Value) -> reqwest::Response {
    let path = match api {
        Api::Responses => "responses",
        Api::ChatCompletions => "chat/completions",
        Api::AnthropicMessages => "messages",
    };
    let call = reqwest::Client::new()
        .post(format!("{base}/{path}"))
        .json(&body);
    if api == Api::AnthropicMessages {
        call.header("x-api-key", KEY)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .unwrap()
    } else {
        call.bearer_auth(KEY).send().await.unwrap()
    }
}
fn user(api: Api, body: &Value) -> &Value {
    let history = if api == Api::Responses {
        &body["input"]
    } else {
        &body["messages"]
    };
    &history
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["role"] == "user")
        .unwrap()["content"]
}

#[tokio::test]
async fn all_nine_image_routes_preserve_mixed_content_in_json_and_sse() {
    for source in apis() {
        for front in apis() {
            let raw = native(source, "ok", None);
            let (upstream, seen) = fixture(vec![json_reply(raw.clone()), sse(source, &raw)]).await;
            let gateway = gateway(provider(source, &upstream.base));
            let server = serve(gateway::router(gateway.clone())).await;
            let reply = post(&server.base, front, request(front, content(front))).await;
            assert_eq!(reply.status(), 200, "{front:?} -> {source:?}");
            let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
            assert_eq!(
                client
                    .complete(ModelRequest::new("vision", prompt()), RunContext::default())
                    .await
                    .unwrap()
                    .text()
                    .unwrap(),
                "ok"
            );
            for (_, _, body) in seen.lock().unwrap().iter() {
                assert_eq!(
                    user(source, body),
                    &content(source),
                    "{front:?} -> {source:?}"
                );
            }
            assert_eq!(gateway.snapshot("app").unwrap().tokens, 16);
        }
    }
}
#[derive(Debug, Model)]
struct Answer {
    value: i64,
}
#[derive(Model)]
struct Add {
    a: i64,
    b: i64,
}
#[tokio::test]
async fn all_nine_image_tool_continuations_keep_images_and_native_history() {
    for source in apis() {
        for front in apis() {
            let (upstream, seen) = fixture(vec![
                sse(
                    source,
                    &native(source, "working", Some(("call-1", json!({"a":19,"b":23})))),
                ),
                sse(source, &native(source, r#"{"value":42}"#, None)),
            ])
            .await;
            let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
            let client =
                Arc::new(HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap());
            let tool =
                TypedTool::<(), Add, Answer, _>::new("add", "Add", |args: Add, _, _| async move {
                    Ok(Answer {
                        value: args.a + args.b,
                    })
                })
                .unwrap();
            let result = Agent::<(), Answer>::new(client, "vision")
                .tool(tool)
                .unwrap()
                .run(prompt(), Arc::new(()), RunContext::default())
                .await
                .unwrap();
            assert_eq!(result.output.value, 42);
            assert_eq!(result.tool_calls, 1);
            let captured = seen.lock().unwrap();
            assert_eq!(captured.len(), 2);
            for (_, _, body) in captured.iter() {
                assert_eq!(user(source, body), &content(source));
            }
            if source == Api::Responses {
                assert!(captured[1].2["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["encrypted_content"] == "opaque-reasoning"));
            }
        }
    }
}
#[tokio::test]
async fn image_identity_and_order_isolate_replay_for_every_frontend() {
    for front in apis() {
        for changed in ["url", "data", "order"] {
            let source = Api::Responses;
            let (upstream, seen) = fixture(vec![
                sse(
                    source,
                    &native(source, "working", Some(("call-1", json!({"a":19,"b":23})))),
                ),
                sse(source, &native(source, "done", None)),
                sse(source, &native(source, "done", None)),
            ])
            .await;
            let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
            let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
            let first = client
                .complete(ModelRequest::new("vision", prompt()), RunContext::default())
                .await
                .unwrap();
            let mut next = ModelRequest::new("vision", prompt());
            next.input.extend_from_slice(first.output());
            next.input
                .push(json!({"type":"function_call_output","call_id":"call-1","output":"42"}));
            let mut different = next.clone();
            let parts = different.input[0]["content"].as_array_mut().unwrap();
            match changed {
                "url" => parts[1]["image_url"] = json!("https://example.com/other.png"),
                "data" => {
                    parts[3]["image_url"] = json!(format!("data:image/png;base64,{OTHER_PNG}"))
                }
                _ => parts.swap(1, 3),
            }
            client
                .complete(different, RunContext::default())
                .await
                .unwrap();
            client.complete(next, RunContext::default()).await.unwrap();
            let captured = seen.lock().unwrap();
            let opaque = |index: usize| {
                captured[index].2["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["encrypted_content"] == "opaque-reasoning")
            };
            assert!(!opaque(1), "changed {changed}, frontend {front:?}");
            assert!(opaque(2), "same images, frontend {front:?}");
        }
    }
}
#[tokio::test]
async fn large_images_above_the_replay_byte_budget_keep_tool_history() {
    let original = include_bytes!("data/vision-colors.png");
    let mut comment = b"Comment".to_vec();
    comment.push(0);
    comment.resize(6 * 1024 * 1024 + 3, b'x');
    let mut chunk = (comment.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&comment);
    let mut crc = !0u32;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb88320
            } else {
                crc >> 1
            };
        }
    }
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    let mut png = original[..33].to_vec();
    png.extend(chunk);
    png.extend_from_slice(&original[33..]);
    let image = Image::from_bytes("image/png", &png).unwrap();
    for front in apis() {
        let source = Api::Responses;
        let (upstream, seen) = fixture(vec![
            sse(
                source,
                &native(source, "working", Some(("call-1", json!({"a":1,"b":2})))),
            ),
            sse(source, &native(source, "done", None)),
        ])
        .await;
        let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
        let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
        let mut request = ModelRequest::new("vision", Prompt::new("inspect").image(image.clone()));
        let first = client
            .complete(request.clone(), RunContext::default())
            .await
            .unwrap();
        request.input.extend_from_slice(first.output());
        request
            .input
            .push(json!({"type":"function_call_output","call_id":"call-1","output":"3"}));
        client
            .complete(request, RunContext::default())
            .await
            .unwrap();
        let captured = seen.lock().unwrap();
        assert!(user(source, &captured[0].2).to_string().len() > 8 * 1024 * 1024);
        assert!(captured[1].2["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["encrypted_content"] == "opaque-reasoning"));
    }
}
#[tokio::test]
async fn invalid_images_fail_before_gateway_admission_and_oversized_bodies_are_native() {
    for front in apis() {
        let model = Arc::new(Scripted::new(vec![]));
        let gateway = gateway(model.clone());
        let server = serve(gateway::router(gateway.clone())).await;
        let bad = match front {
            Api::Responses => json!([{"type":"input_image","image_url":"file:///private.png"}]),
            Api::ChatCompletions => {
                json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,invalid!"}}])
            }
            Api::AnthropicMessages => {
                json!([{"type":"image","source":{"type":"base64","media_type":"image/svg+xml","data":"AQID"}}])
            }
        };
        let response = post(&server.base, front, request(front, bad)).await;
        assert_eq!(response.status(), 400);
        assert_eq!(gateway.snapshot("app").unwrap().requests, 0);
        let response = post(
            &server.base,
            front,
            request(front, json!("x".repeat(16 * 1024 * 1024))),
        )
        .await;
        assert_eq!(response.status(), 413);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["type"], "invalid_request_error");
        assert!(model.requests.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn openai_image_detail_is_preserved_and_messages_rejects_unrepresentable_detail() {
    for detail in [ImageDetail::Low, ImageDetail::High, ImageDetail::Original] {
        for source in [Api::Responses, Api::ChatCompletions] {
            let (upstream, seen) = fixture(vec![json_reply(native(source, "ok", None))]).await;
            let prompt = Prompt::new("inspect").image(Image::url(URL).unwrap().detail(detail));
            provider(source, &upstream.base)
                .complete(ModelRequest::new("vision", prompt), RunContext::default())
                .await
                .unwrap();
            let captured = seen.lock().unwrap();
            let block = &user(source, &captured[0].2)[1];
            let actual = if source == Api::Responses {
                &block["detail"]
            } else {
                &block["image_url"]["detail"]
            };
            assert_eq!(actual, &serde_json::to_value(detail).unwrap());
        }
    }
    let (upstream, seen) = fixture(vec![]).await;
    let prompt = Prompt::new("inspect").image(Image::url(URL).unwrap().detail(ImageDetail::High));
    assert!(matches!(
        provider(Api::AnthropicMessages, &upstream.base)
            .complete(ModelRequest::new("vision", prompt), RunContext::default())
            .await,
        Err(Error::Configuration(_))
    ));
    assert!(seen.lock().unwrap().is_empty());
}
