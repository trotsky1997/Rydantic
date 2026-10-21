#![cfg(feature = "http")]
mod common;
use common::{wire::*, *};
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    rydantic::Model,
    Agent, Api, Error, HttpProvider, Image, ModelProvider, ModelRequest, Pdf, PdfDetail, Prompt,
    RunContext, TypedTool,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

const KEY: &str = "pdf-gateway-fixture-key";
const PDF: &[u8] = include_bytes!("data/receipt.pdf");
const URL: &str = "https://example.com/receipt.pdf?signature=opaque-pdf-reference";
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
            "pdf-model".into(),
            vec![Route {
                provider,
                model: "upstream".into(),
                price: None,
            }],
        )]),
        vec![ClientPolicy {
            id: "app".into(),
            key: KEY.into(),
            models: HashSet::from(["pdf-model".into()]),
            budget: budget.clone(),
        }],
        budget,
    )
    .unwrap()
}
fn pdf() -> Pdf {
    Pdf::from_bytes("receipt.pdf", PDF).unwrap()
}
fn prompt() -> Prompt {
    Prompt::new("before")
        .pdf(pdf())
        .text("between")
        .image(Image::from_bytes("image/png", include_bytes!("data/vision-colors.png")).unwrap())
        .text("after")
        .pdf(pdf().filename("second.pdf").unwrap())
}
fn pdf_block(api: Api, name: &str, url: bool) -> Value {
    let canonical =
        ModelRequest::new("x", Prompt::default().pdf(pdf())).input[0]["content"][0].clone();
    let data = canonical["file_data"].as_str().unwrap();
    match api {
        Api::Responses => {
            if url {
                json!({"type":"input_file","filename":name,"file_url":URL,"detail":"auto"})
            } else {
                json!({"type":"input_file","filename":name,"file_data":data,"detail":"auto"})
            }
        }
        Api::ChatCompletions => json!({"type":"file","file":{"filename":name,"file_data":data}}),
        Api::AnthropicMessages => {
            json!({"type":"document","title":name,"source":if url { json!({"type":"url","url":URL}) } else { json!({"type":"base64","media_type":"application/pdf","data":data.trim_start_matches("data:application/pdf;base64,")}) }})
        }
    }
}
fn content(api: Api) -> Value {
    let canonical = ModelRequest::new("x", prompt());
    let image_url = canonical.input[0]["content"][3]["image_url"]
        .as_str()
        .unwrap();
    let image = match api {
        Api::Responses => json!({"type":"input_image","image_url":image_url,"detail":"auto"}),
        Api::ChatCompletions => {
            json!({"type":"image_url","image_url":{"url":image_url,"detail":"auto"}})
        }
        Api::AnthropicMessages => {
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":image_url.trim_start_matches("data:image/png;base64,")}})
        }
    };
    let text = if api == Api::Responses {
        "input_text"
    } else {
        "text"
    };
    json!([{ "type":text,"text":"before" },pdf_block(api,"receipt.pdf",false),{"type":text,"text":"between"},image,{"type":text,"text":"after"},pdf_block(api,"second.pdf",false)])
}
fn request(api: Api, content: Value) -> Value {
    match api {
        Api::Responses => json!({"model":"pdf-model","input":[{"role":"user","content":content}]}),
        Api::ChatCompletions => {
            json!({"model":"pdf-model","messages":[{"role":"user","content":content}]})
        }
        Api::AnthropicMessages => {
            json!({"model":"pdf-model","max_tokens":256,"messages":[{"role":"user","content":content}]})
        }
    }
}
async fn post(base: &str, api: Api, body: Value) -> reqwest::Response {
    let path = match api {
        Api::Responses => "responses",
        Api::ChatCompletions => "chat/completions",
        Api::AnthropicMessages => "messages",
    };
    let request = reqwest::Client::new()
        .post(format!("{base}/{path}"))
        .json(&body);
    if api == Api::AnthropicMessages {
        request
            .header("x-api-key", KEY)
            .header("anthropic-version", "2023-06-01")
    } else {
        request.bearer_auth(KEY)
    }
    .send()
    .await
    .unwrap()
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
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
}
fn assert_reply(api: Api, streaming: bool, body: &str) {
    let text: String = if streaming {
        body.lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter_map(|event| match api {
                Api::Responses => (event["type"] == "response.output_text.delta")
                    .then(|| event["delta"].as_str().map(str::to_owned))
                    .flatten(),
                Api::ChatCompletions => event["choices"][0]["delta"]["content"]
                    .as_str()
                    .map(str::to_owned),
                Api::AnthropicMessages => (event["type"] == "content_block_delta")
                    .then(|| event["delta"]["text"].as_str().map(str::to_owned))
                    .flatten(),
            })
            .collect()
    } else {
        let value: Value = serde_json::from_str(body).unwrap();
        match api {
            Api::Responses => value["output"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|i| i["content"].as_array().into_iter().flatten())
                .filter_map(|b| b["text"].as_str())
                .collect(),
            Api::ChatCompletions => value["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .into(),
            Api::AnthropicMessages => value["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect(),
        }
    };
    assert_eq!(text, "ok", "{body}");
}

#[tokio::test]
async fn all_nine_pdf_routes_preserve_bytes_filenames_images_and_order_in_json_and_sse() {
    for source in apis() {
        for front in apis() {
            let (upstream, seen) = fixture(vec![
                json_reply(native(source, "ok", None)),
                sse(source, &native(source, "ok", None)),
            ])
            .await;
            let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
            for streaming in [false, true] {
                let mut parts = content(front);
                if !streaming && front != Api::AnthropicMessages {
                    for i in [1, 5] {
                        let value = if front == Api::Responses {
                            &mut parts[i]["file_data"]
                        } else {
                            &mut parts[i]["file"]["file_data"]
                        };
                        *value = json!(value
                            .as_str()
                            .unwrap()
                            .trim_start_matches("data:application/pdf;base64,"));
                    }
                }
                let mut body = request(front, parts);
                body["stream"] = json!(streaming);
                let reply = post(&server.base, front, body).await;
                assert_eq!(reply.status(), 200, "{front:?} -> {source:?}");
                let body = reply.text().await.unwrap();
                assert_reply(front, streaming, &body);
            }
            let captured = seen.lock().unwrap();
            assert_eq!(captured.len(), 2);
            for (_, _, body) in captured.iter() {
                assert_eq!(
                    user(source, body),
                    &content(source),
                    "{front:?} -> {source:?}"
                );
            }
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
async fn all_nine_pdf_tool_continuations_keep_documents_and_native_assistant_history() {
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
            let result = Agent::<(), Answer>::new(client, "pdf-model")
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
            let second = &captured[1].2;
            match source {
                Api::Responses => assert!(second["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["encrypted_content"] == "opaque-reasoning")),
                Api::ChatCompletions => assert!(second["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["reasoning_content"] == "retained reasoning")),
                Api::AnthropicMessages => assert!(second["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["content"]
                        .as_array()
                        .is_some_and(|b| b.iter().any(|b| b["signature"] == "opaque-signature")))),
            }
        }
    }
}

#[tokio::test]
async fn pdf_urls_preserve_signed_references_between_responses_and_messages() {
    for source in [Api::Responses, Api::AnthropicMessages] {
        for front in [Api::Responses, Api::AnthropicMessages] {
            let (upstream, seen) = fixture(vec![
                json_reply(native(source, "ok", None)),
                sse(source, &native(source, "ok", None)),
            ])
            .await;
            let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
            for streaming in [false, true] {
                let mut body = request(front, json!([pdf_block(front, "document.pdf", true)]));
                body["stream"] = json!(streaming);
                let reply = post(&server.base, front, body).await;
                assert_eq!(reply.status(), 200);
                assert_reply(front, streaming, &reply.text().await.unwrap());
            }
            for (_, _, body) in seen.lock().unwrap().iter() {
                assert_eq!(
                    user(source, body),
                    &json!([pdf_block(source, "document.pdf", true)])
                );
            }
        }
    }
    let (upstream, seen) = fixture(vec![]).await;
    let request = ModelRequest::new("pdf-model", Prompt::default().pdf(Pdf::url(URL).unwrap()));
    assert!(matches!(
        provider(Api::ChatCompletions, &upstream.base)
            .complete(request, RunContext::default())
            .await,
        Err(Error::Configuration(_))
    ));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn messages_tool_results_can_precede_new_pdfs_in_the_same_user_turn() {
    for source in apis() {
        let (upstream, seen) = fixture(vec![
            sse(
                source,
                &native(source, "working", Some(("call-1", json!({"a":1,"b":2})))),
            ),
            sse(source, &native(source, "done", None)),
        ])
        .await;
        let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
        let client =
            HttpProvider::new(Api::AnthropicMessages, &server.base, Some(KEY.into())).unwrap();
        let mut request = ModelRequest::new("pdf-model", prompt());
        let first = client
            .complete(request.clone(), RunContext::default())
            .await
            .unwrap();
        request.input.extend_from_slice(first.output());
        request
            .input
            .push(json!({"type":"function_call_output","call_id":"call-1","output":"3"}));
        let followup = ModelRequest::new(
            "x",
            Prompt::new("check another document").pdf(pdf().filename("followup.pdf").unwrap()),
        );
        request.input.extend(followup.input);
        client
            .complete(request, RunContext::default())
            .await
            .unwrap();
        let captured = seen.lock().unwrap();
        let history = if source == Api::Responses {
            &captured[1].2["input"]
        } else {
            &captured[1].2["messages"]
        };
        let content = &history
            .as_array()
            .unwrap()
            .iter()
            .rfind(|i| i["role"] == "user")
            .unwrap()["content"];
        let blocks = content.as_array().unwrap();
        let offset = usize::from(source == Api::AnthropicMessages);
        if offset == 1 {
            assert_eq!(blocks[0]["type"], "tool_result");
            assert_eq!(blocks[0]["tool_use_id"], "call-1");
        }
        assert_eq!(blocks[offset]["text"], "check another document");
        assert_eq!(blocks[offset + 1], pdf_block(source, "followup.pdf", false));
    }
}

#[tokio::test]
async fn pdf_bytes_names_order_and_urls_scope_replay_identity() {
    for front in apis() {
        for changed in ["data", "name", "order", "url", "detail"] {
            if front != Api::Responses && matches!(changed, "url" | "detail") {
                continue;
            }
            let source = Api::Responses;
            let (upstream, seen) = fixture(vec![
                sse(
                    source,
                    &native(source, "working", Some(("call-1", json!({"a":1,"b":2})))),
                ),
                sse(source, &native(source, "done", None)),
                sse(source, &native(source, "done", None)),
            ])
            .await;
            let server = serve(gateway::router(gateway(provider(source, &upstream.base)))).await;
            let client = HttpProvider::new(front, &server.base, Some(KEY.into())).unwrap();
            let first = client
                .complete(
                    ModelRequest::new("pdf-model", prompt()),
                    RunContext::default(),
                )
                .await
                .unwrap();
            let mut next = ModelRequest::new("pdf-model", prompt());
            next.input.extend_from_slice(first.output());
            next.input
                .push(json!({"type":"function_call_output","call_id":"call-1","output":"3"}));
            let mut different = next.clone();
            let parts = different.input[0]["content"].as_array_mut().unwrap();
            match changed {
                "data" => {
                    let mut bytes = PDF.to_vec();
                    bytes.extend_from_slice(b"\n% a different PDF\n");
                    parts[1] = ModelRequest::new(
                        "x",
                        Prompt::default().pdf(Pdf::from_bytes("receipt.pdf", &bytes).unwrap()),
                    )
                    .input[0]["content"][0]
                        .clone();
                }
                "name" => parts[1]["filename"] = json!("changed.pdf"),
                "order" => parts.swap(1, 5),
                "url" => {
                    parts[1] = ModelRequest::new(
                        "x",
                        Prompt::default()
                            .pdf(Pdf::url(URL).unwrap().filename("receipt.pdf").unwrap()),
                    )
                    .input[0]["content"][0]
                        .clone()
                }
                _ => parts[1]["detail"] = json!("low"),
            }
            client
                .complete(different, RunContext::default())
                .await
                .unwrap();
            client.complete(next, RunContext::default()).await.unwrap();
            let captured = seen.lock().unwrap();
            let opaque = |i: usize| {
                captured[i].2["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["encrypted_content"] == "opaque-reasoning")
            };
            assert!(!opaque(1), "changed {changed} through {front:?}");
            assert!(opaque(2));
        }
    }
}

#[tokio::test]
async fn malformed_and_nonportable_pdf_blocks_fail_before_gateway_admission() {
    for front in apis() {
        let model = Arc::new(Scripted::new(vec![]));
        let gateway = gateway(model.clone());
        let server = serve(gateway::router(gateway.clone())).await;
        let valid = pdf_block(front, "receipt.pdf", false);
        let mut invalid = Vec::new();
        for marker in ["id", "mime", "data", "extra", "name"] {
            let mut block = valid.clone();
            match (front, marker) {
                (Api::Responses, "id") => block["file_id"] = json!("file-private"),
                (Api::Responses, "mime") => {
                    block["file_data"] = json!("data:text/plain;base64,JVBERi0xLjQK")
                }
                (Api::Responses, "data") => block["file_data"] = json!("not base64"),
                (Api::Responses, "extra") => block["file_url"] = json!(URL),
                (Api::Responses, _) => block["filename"] = json!("../private.pdf"),
                (Api::ChatCompletions, "id") => block["file"]["file_id"] = json!("file-private"),
                (Api::ChatCompletions, "mime") => {
                    block["file"]["file_data"] = json!("data:image/png;base64,JVBERi0xLjQK")
                }
                (Api::ChatCompletions, "data") => block["file"]["file_data"] = json!("not base64"),
                (Api::ChatCompletions, "extra") => block["file"]["file_url"] = json!(URL),
                (Api::ChatCompletions, _) => block["file"]["filename"] = json!("../private.pdf"),
                (Api::AnthropicMessages, "id") => {
                    block["source"] = json!({"type":"file","file_id":"file-private"})
                }
                (Api::AnthropicMessages, "mime") => {
                    block["source"]["media_type"] = json!("text/plain")
                }
                (Api::AnthropicMessages, "data") => block["source"]["data"] = json!("not base64"),
                (Api::AnthropicMessages, "extra") => block["citations"] = json!({"enabled":true}),
                (Api::AnthropicMessages, _) => block["title"] = json!("../private.pdf"),
            }
            invalid.push(block);
        }
        for block in invalid {
            let response = post(&server.base, front, request(front, json!([block]))).await;
            assert_eq!(response.status(), 400);
        }
        assert_eq!(gateway.snapshot("app").unwrap().requests, 0);
        assert!(model.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn responses_pdf_detail_is_preserved_and_other_protocols_reject_it() {
    for detail in [PdfDetail::Low, PdfDetail::High] {
        let (upstream, seen) = fixture(vec![json_reply(native(Api::Responses, "ok", None))]).await;
        provider(Api::Responses, &upstream.base)
            .complete(
                ModelRequest::new("pdf-model", Prompt::default().pdf(pdf().detail(detail))),
                RunContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            user(Api::Responses, &seen.lock().unwrap()[0].2)[0]["detail"],
            serde_json::to_value(detail).unwrap()
        );
        for api in [Api::ChatCompletions, Api::AnthropicMessages] {
            let (upstream, seen) = fixture(vec![]).await;
            assert!(matches!(
                provider(api, &upstream.base)
                    .complete(
                        ModelRequest::new("pdf-model", Prompt::default().pdf(pdf().detail(detail))),
                        RunContext::default()
                    )
                    .await,
                Err(Error::Configuration(_))
            ));
            assert!(seen.lock().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn large_pdf_data_above_replay_budget_keeps_private_tool_history() {
    let mut bytes = PDF.to_vec();
    // A valid incremental PDF update adds an unreferenced stream object. The
    // original page stays readable and both cross-reference tables stay valid.
    let previous_xref = std::str::from_utf8(PDF)
        .unwrap()
        .split("startxref\n")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    let object_offset = bytes.len();
    let payload_size = 6 * 1024 * 1024;
    bytes.extend_from_slice(format!("6 0 obj\n<< /Length {payload_size} >>\nstream\n").as_bytes());
    bytes.resize(bytes.len() + payload_size, b'x');
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n6 1\n{object_offset:010} 00000 n \ntrailer\n<< /Size 7 /Root 1 0 R /Prev {previous_xref} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    let prompt = Prompt::default().pdf(Pdf::from_bytes("large.pdf", &bytes).unwrap());
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
        let mut request = ModelRequest::new("pdf-model", &prompt);
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
            .any(|i| i["encrypted_content"] == "opaque-reasoning"));
    }
}
