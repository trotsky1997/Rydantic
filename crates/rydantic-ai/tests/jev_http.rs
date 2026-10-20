#![cfg(feature = "http")]
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rydantic_ai::{
    jev::{JevClient, Question, SystemOneRequest},
    Error, RunContext,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
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
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { base, task }
}
fn request() -> SystemOneRequest {
    SystemOneRequest::new("jev-1.14.0", "Payment failed. Please help now.")
        .question("urgent", Question::noul("Urgent?"))
        .unwrap()
}
fn response() -> Value {
    json!({"model":"jev-1.14.0","answers":{"urgent":{"type":"noul","noul":0.96}},"usage":{"input_tokens":10,"output_tokens":4}})
}
type Seen = Arc<Mutex<Vec<(String, Value)>>>;
async fn upstream(
    State(seen): State<Seen>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    seen.lock()
        .unwrap()
        .push((headers["authorization"].to_str().unwrap().to_owned(), body));
    Json(response())
}

#[tokio::test]
async fn opaque_bearer_headers_are_preserved_and_credentials_are_redacted() {
    let seen: Seen = Arc::default();
    let server=serve(Router::new().route("/v1/systemone",post(upstream)).route("/v1/models",get(||async{Json(json!({"models":[{"name":"jev-1.14.0","description":"Jev","release_date":"2026-10-01"}]}))})).with_state(seen.clone())).await;
    let key = "synthetic-jev-secret";
    let direct = JevClient::new(&server.base, key).unwrap();
    let opaque =
        JevClient::new(&format!("{}/", server.base), "synthetic-token?route=test").unwrap();
    for client in [&direct, &opaque] {
        assert!(!format!("{client:?}").contains(key));
        assert!(!format!("{client:?}").contains("synthetic-token"));
        assert!(!format!("{client:?}").contains(&server.base));
        assert_eq!(
            client
                .system_one(&request(), RunContext::default())
                .await
                .unwrap()
                .noul("urgent")
                .unwrap()
                .noul,
            0.96
        );
    }
    {
        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "Bearer synthetic-jev-secret");
        assert_eq!(calls[1].0, "Bearer synthetic-token?route=test");
        assert_eq!(calls[0].1, serde_json::to_value(request()).unwrap());
    }
    assert_eq!(
        direct.models(RunContext::default()).await.unwrap().models[0].name,
        "jev-1.14.0"
    );
}

#[tokio::test]
async fn jev_results_can_be_returned_by_a_validated_agent_tool() {
    use rydantic::Model;
    use rydantic_ai::{Tool, TypedTool};
    #[derive(Model)]
    struct Input {
        ticket: String,
    }
    #[derive(Model)]
    struct Decision {
        #[model(ge = 0.0, le = 1.0)]
        urgent: f64,
    }
    let seen: Seen = Arc::default();
    let server = serve(
        Router::new()
            .route("/v1/systemone", post(upstream))
            .with_state(seen.clone()),
    )
    .await;
    let client = Arc::new(JevClient::new(&server.base, "synthetic-key").unwrap());
    let tool = TypedTool::<JevClient, Input, Decision, _>::new(
        "judge_ticket",
        "Evaluate urgency",
        |input, client, context| async move {
            let request = SystemOneRequest::new("jev-1.14.0", input.ticket)
                .question("urgent", Question::noul("Urgent?"))?;
            client
                .system_one(&request, context.clone())
                .await?
                .parse::<Decision>(&context)
                .await
        },
    )
    .unwrap();
    let result = tool
        .call(
            json!({"ticket":"Please help now"}).to_string(),
            client,
            RunContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"urgent":0.96})
    );
    assert_eq!(seen.lock().unwrap()[0].1["state"], "Please help now");
}

#[tokio::test]
async fn invalid_and_oversized_requests_do_not_reach_the_provider() {
    let seen: Seen = Arc::default();
    let server = serve(
        Router::new()
            .route("/v1/systemone", post(upstream))
            .with_state(seen.clone()),
    )
    .await;
    let client = JevClient::new(&server.base, "synthetic-key").unwrap();
    assert!(client
        .system_one(&SystemOneRequest::new("m", "x"), RunContext::default())
        .await
        .is_err());
    let huge = SystemOneRequest::new("m", "x".repeat(1024 * 1024))
        .question("urgent", Question::noul("x"))
        .unwrap();
    assert!(matches!(
        client.system_one(&huge, RunContext::default()).await,
        Err(Error::Limit(_))
    ));
    let context = RunContext::default();
    context.cancellation.cancel();
    assert!(matches!(
        client.system_one(&request(), context).await,
        Err(Error::Cancelled)
    ));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn redirects_errors_and_malformed_json_never_expose_upstream_bodies() {
    let sink_seen: Seen = Arc::default();
    let sink = serve(
        Router::new()
            .route("/v1/systemone", post(upstream))
            .with_state(sink_seen.clone()),
    )
    .await;
    let destination = format!("{}/systemone", sink.base);
    let redirect = serve(Router::new().route(
        "/v1/systemone",
        post(move || {
            let destination = destination.clone();
            async move {
                (
                    StatusCode::TEMPORARY_REDIRECT,
                    [("location", destination)],
                    "private response",
                )
            }
        }),
    ))
    .await;
    let client = JevClient::new(&redirect.base, "synthetic-key").unwrap();
    assert!(matches!(
        client.system_one(&request(), RunContext::default()).await,
        Err(Error::Provider { status: 307 })
    ));
    assert!(sink_seen.lock().unwrap().is_empty());
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::UNPROCESSABLE_ENTITY,
    ] {
        let server = serve(Router::new().route(
            "/v1/systemone",
            post(move || async move { (status, "private credential echo") }),
        ))
        .await;
        let result = JevClient::new(&server.base, "synthetic-key")
            .unwrap()
            .system_one(&request(), RunContext::default())
            .await
            .unwrap_err();
        assert!(matches!(result,Error::Provider{status:s} if s==status.as_u16()));
        assert!(!result.to_string().contains("private credential echo"));
    }
    let server = serve(Router::new().route(
        "/v1/systemone",
        post(|| async { "private credential echo" }),
    ))
    .await;
    let result = JevClient::new(&server.base, "synthetic-key")
        .unwrap()
        .system_one(&request(), RunContext::default())
        .await
        .unwrap_err();
    assert!(matches!(result, Error::Protocol(_)));
    assert!(!result.to_string().contains("private credential echo"));
}

#[tokio::test]
async fn deadlines_cancellation_and_chunked_body_limits_cover_the_entire_request() {
    let server = serve(Router::new().route(
        "/v1/systemone",
        post(|| async { std::future::pending::<Json<Value>>().await }),
    ))
    .await;
    let client = JevClient::new(&server.base, "synthetic-key").unwrap();
    assert!(matches!(
        client
            .system_one(
                &request(),
                RunContext::with_timeout(Duration::from_millis(20))
            )
            .await,
        Err(Error::Timeout)
    ));
    let context = RunContext::default();
    let cancellation = context.cancellation.clone();
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancellation.cancel();
    };
    let request = request();
    let (result, ()) = tokio::join!(client.system_one(&request, context), cancel);
    assert!(matches!(result, Err(Error::Cancelled)));
    let server = serve(Router::new().route(
        "/v1/systemone",
        post(|| async {
            let chunks = vec![
                Ok::<_, std::convert::Infallible>(vec![b'x'; 4 * 1024 * 1024]),
                Ok(vec![b'x'; 4 * 1024 * 1024]),
                Ok(vec![b'x']),
            ];
            axum::body::Body::from_stream(futures_util::stream::iter(chunks)).into_response()
        }),
    ))
    .await;
    assert!(matches!(
        JevClient::new(&server.base, "synthetic-key")
            .unwrap()
            .system_one(&request, RunContext::default())
            .await,
        Err(Error::Limit(_))
    ));
}

#[test]
fn invalid_urls_and_header_injection_fail_configuration() {
    for url in [
        "ftp://example.com/v1",
        "https://user:private@example.com/v1",
        "https://example.com/v1?key=private",
        "https://example.com/v1#private",
    ] {
        assert!(JevClient::new(url, "synthetic-key").is_err());
    }
    for key in ["", "key\r\nheader: injection", "key with spaces"] {
        assert!(JevClient::new("https://example.com/v1", key).is_err());
    }
    assert!(JevClient::new("http://[::1]:8080/v1", "synthetic-key").is_ok());
}
