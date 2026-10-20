#![cfg(feature = "http")]
mod common;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use common::*;
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    Error, ModelProvider, ModelRequest, ResponsesProvider, RunContext,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

struct Server {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(app: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { base, task }
}
type Seen = Arc<Mutex<Vec<(String, Value)>>>;
async fn upstream(
    State(seen): State<Seen>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    seen.lock()
        .unwrap()
        .push((headers["authorization"].to_str().unwrap().into(), body));
    let raw = response("你好").raw;
    let stream = format!("event: response.output_text.delta\r\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}}\r\n\r\nevent: response.completed\r\ndata: {}\r\n\r\n", json!({"type":"response.completed","response":raw}));
    ([("content-type", "text/event-stream")], stream)
}

#[tokio::test]
async fn responses_http_preserves_native_events_and_auth_is_redacted() {
    let seen: Seen = Arc::new(Mutex::new(vec![]));
    let server = serve(
        Router::new()
            .route("/v1/responses", post(upstream))
            .with_state(seen.clone()),
    )
    .await;
    let provider = ResponsesProvider::new(
        &format!("{}/v1", server.base),
        Some("secret-token-never-log".into()),
    )
    .unwrap();
    assert!(!format!("{provider:?}").contains("secret-token-never-log"));
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let result = provider
        .complete(
            ModelRequest::new("mock", "hello"),
            RunContext::default().with_events(tx),
        )
        .await
        .unwrap();
    assert_eq!(result.text().unwrap(), "你好");
    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["type"], "response.completed");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, "Bearer secret-token-never-log");
    assert_eq!(seen[0].1["stream"], true);
    assert_eq!(seen[0].1["store"], false);
}

#[tokio::test]
async fn structured_iterable_decodes_sse_split_at_every_byte() {
    #[derive(Debug, rydantic_ai::rydantic::Model)]
    struct Person {
        name: String,
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let recorded = seen.clone();
    let server = serve(Router::new().route(
        "/v1/responses",
        post(move |Json(body): Json<Value>| {
            let recorded = recorded.clone();
            async move {
                recorded.lock().unwrap().push(body);
                let text = r#"{"items":[{"name":"你好😊"},{"name":"Ada"}]}"#;
                let mut wire = String::new();
                for delta in [r#"{"items":[{"name":"你好😊"},"#, r#"{"name":"Ada"}]}"#] {
                    wire.push_str(&format!(
                        "data: {}\r\n\r\n",
                        json!({"type":"response.output_text.delta","delta":delta})
                    ));
                }
                wire.push_str(&format!(
                    "data: {}\r\n\r\n",
                    json!({"type":"response.completed","response":response(text).raw})
                ));
                let bytes: Vec<_> = wire
                    .into_bytes()
                    .into_iter()
                    .map(|byte| Ok::<_, std::io::Error>(vec![byte]))
                    .collect();
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from_stream(futures_util::stream::iter(bytes)),
                )
            }
        }),
    ))
    .await;
    let provider = Arc::new(ResponsesProvider::new(&format!("{}/v1", server.base), None).unwrap());
    let result = rydantic_ai::Agent::<(), Person>::new(provider, "mock")
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
    let requests = seen.lock().unwrap();
    assert_eq!(requests[0]["stream"], true);
    assert_eq!(
        requests[0]["text"]["format"]["schema"]["properties"]["items"]["type"],
        "array"
    );
}

#[tokio::test]
async fn structured_partial_never_commits_a_premature_sse_response() {
    #[derive(Debug, rydantic_ai::rydantic::Model)]
    struct Answer {
        value: u8,
    }
    let server = serve(Router::new().route(
        "/v1/responses",
        post(|| async {
            (
                [("content-type", "text/event-stream")],
                format!(
                    "data: {}\n\n",
                    json!({"type":"response.output_text.delta","delta":r#"{"value":1}"#})
                ),
            )
        }),
    ))
    .await;
    let provider = Arc::new(ResponsesProvider::new(&format!("{}/v1", server.base), None).unwrap());
    let mut stream = rydantic_ai::Agent::<(), Answer>::new(provider, "mock")
        .stream_partial("extract", Arc::new(()), RunContext::default())
        .unwrap();
    let mut error = false;
    while let Some(event) = stream.recv().await {
        match event {
            Ok(rydantic_ai::OutputEvent::Complete(result)) => {
                panic!("unexpected accepted result: {}", result.output.value)
            }
            Err(Error::Protocol(_)) => error = true,
            other => {
                assert!(other.is_ok());
            }
        }
    }
    assert!(error);
}

#[tokio::test]
async fn premature_stream_and_non_success_http_are_errors() {
    let server = serve(Router::new().route(
        "/v1/responses",
        post(|| async {
            (
                [("content-type", "text/event-stream")],
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            )
        }),
    ))
    .await;
    let provider = ResponsesProvider::new(&format!("{}/v1", server.base), None).unwrap();
    assert!(matches!(
        provider
            .complete(ModelRequest::new("m", "x"), RunContext::default())
            .await,
        Err(Error::Protocol(_))
    ));
    let server = serve(Router::new().route(
        "/v1/responses",
        post(|| async { (StatusCode::UNAUTHORIZED, "private upstream response body") }),
    ))
    .await;
    let provider = ResponsesProvider::new(&format!("{}/v1", server.base), None).unwrap();
    assert!(matches!(
        provider
            .complete(ModelRequest::new("m", "x"), RunContext::default())
            .await,
        Err(Error::Provider { status: 401 })
    ));
}

#[tokio::test]
async fn gateway_http_enforces_keys_routes_budget_and_sse_completion() {
    let key = "local-gateway-key-12345";
    let scripted = Arc::new(Scripted::new(vec![
        Ok(response("first")),
        Ok(response("second")),
    ]));
    let limits = Budget {
        max_requests: 2,
        reservation_tokens: 10,
        ..Budget::default()
    };
    let gateway = Gateway::new(
        BTreeMap::from([(
            "chat".into(),
            vec![Route {
                provider: scripted,
                model: "private-upstream".into(),
                price: None,
            }],
        )]),
        vec![ClientPolicy {
            id: "app".into(),
            key: key.into(),
            models: HashSet::from(["chat".into()]),
            budget: limits.clone(),
        }],
        limits,
    )
    .unwrap();
    let server = serve(gateway::router(gateway)).await;
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{}/v1/models", server.base))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let models: Value = http
        .get(format!("{}/v1/models", server.base))
        .bearer_auth(key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], "chat");
    assert!(!models.to_string().contains("private-upstream"));
    let provider =
        ResponsesProvider::gateway(&format!("{}/v1", server.base), Some(key.into())).unwrap();
    for expected in ["first", "second"] {
        assert_eq!(
            provider
                .complete(ModelRequest::new("chat", "x"), RunContext::default())
                .await
                .unwrap()
                .text()
                .unwrap(),
            expected
        );
    }
    let denied = http
        .post(format!("{}/v1/responses", server.base))
        .bearer_auth(key)
        .json(&ModelRequest::new("chat", "x"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 429);
    let denied: Value = denied.json().await.unwrap();
    assert_eq!(denied["error"]["code"], "budget_exhausted");
}

#[test]
fn provider_configuration_rejects_credential_urls_and_public_plaintext() {
    for base in [
        "https://user:password@example.com/v1",
        "https://example.com/v1?token=x",
        "http://example.com/v1",
    ] {
        assert!(ResponsesProvider::new(base, None).is_err());
    }
}
