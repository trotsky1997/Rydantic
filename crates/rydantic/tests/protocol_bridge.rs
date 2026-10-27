#![cfg(all(
    feature = "grpc",
    feature = "axum",
    feature = "openapi",
    feature = "derive"
))]

#[allow(dead_code)]
#[path = "../examples/protocol_bridge.rs"]
mod bridge;

use bridge::grpc_capnp::{self, models};
use rydantic::axum::framework::{
    self,
    http::{Request, StatusCode},
};
use rydantic::grpc::Limits;
use rydantic::{CapnpModel, Model};
use tokio_stream::wrappers::TcpListenerStream;
use tower::ServiceExt;

#[tokio::test]
async fn http_json_uses_capnp_grpc_and_publishes_the_same_models() {
    let limits = Limits::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(grpc_capnp::Greeter {
                limits,
                invalid_output: false,
            })
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let app = bridge::app(channel, limits).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/greet")
                .header("content-type", "application/json")
                .body(framework::body::Body::from(
                    r#"{"name":" Alice ","context":{"language":"en"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = framework::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    let greeting = models::Greeting::parse_slice(&body).unwrap();
    assert_eq!(greeting.message, "Hello, Alice!");

    let rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/greet")
                .header("content-type", "application/json")
                .body(framework::body::Body::from(
                    r#"{"name":" ","context":{"language":"en"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(framework::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = framework::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    let document: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        document["paths"]["/greet"]["post"]["x-rydantic-grpc-method"],
        grpc_capnp::HELLO_RPC_PATH
    );
    assert!(document["components"]["schemas"]
        .get("HelloRequest")
        .is_some());
    assert!(document["components"]["schemas"]
        .get("GreetingResponse")
        .is_some());

    let _ = stop.send(());
    server.await.unwrap().unwrap();
}

// Count real RPC dispatches and deliberately return a remote InvalidArgument.
// A gateway must not confuse this with its own preflight rejection.
#[derive(Clone)]
struct RejectingBackend(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl tonic::server::NamedService for RejectingBackend {
    const NAME: &'static str = "rydantic.Greeter";
}

impl<B> tonic::codegen::Service<Request<B>> for RejectingBackend {
    type Response = framework::http::Response<tonic::body::Body>;
    type Error = std::convert::Infallible;
    type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request<B>) -> Self::Future {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Ok(tonic::Status::invalid_argument("private backend details").into_http())
        })
    }
}

#[tokio::test]
async fn local_wire_limit_is_413_without_dispatch_but_backend_errors_remain_502() {
    use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc};
    use std::time::Duration;

    let calls = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(RejectingBackend(calls.clone()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let request =
        models::Hello::parse_json(r#"{"name":"Alice","context":{"language":"en"}}"#).unwrap();
    let message = request.to_capnp_message().unwrap();
    let exact_size = capnp::serialize::compute_serialized_size_in_words(&message) * 8;
    for (limits, expected_status, expected_calls) in [
        (
            Limits::new(8, 1024, 64).unwrap(),
            StatusCode::PAYLOAD_TOO_LARGE,
            0,
        ),
        (
            Limits::new(exact_size, 1024, 64).unwrap(),
            StatusCode::BAD_GATEWAY,
            1,
        ),
        (Limits::default(), StatusCode::BAD_GATEWAY, 2),
    ] {
        let app = bridge::app(channel.clone(), limits).unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/greet")
                    .header("content-type", "application/json")
                    .body(framework::body::Body::from(
                        r#"{"name":"Alice","context":{"language":"en"}}"#,
                    ))
                    .unwrap(),
            ),
        )
        .await
        .expect("gateway request must finish")
        .unwrap();
        assert_eq!(response.status(), expected_status);
        assert!(framework::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
    let document = bridge::openapi_document().unwrap();
    assert!(
        document["paths"]["/greet"]["post"]["responses"]["413"]["description"]
            .as_str()
            .unwrap()
            .contains("encoded gRPC message limit")
    );
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}
