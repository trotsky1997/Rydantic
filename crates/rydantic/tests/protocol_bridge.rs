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
use rydantic::Model;
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
