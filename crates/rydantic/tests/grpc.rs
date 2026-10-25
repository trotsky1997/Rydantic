#![cfg(all(feature = "grpc", feature = "derive"))]

#[allow(dead_code)]
#[path = "../examples/grpc_capnp.rs"]
mod example;

use bytes::{Buf, BufMut, Bytes};
use example::{models, Greeter};
use rydantic::grpc::{CapnpCodec, Limits};
use rydantic::CapnpModel;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::{Code, Request, Status};

// Raw peer exercises malformed protocol data that the typed encoder forbids.
struct RawCodec;
struct Raw;
impl Codec for RawCodec {
    type Encode = Bytes;
    type Decode = Bytes;
    type Encoder = Raw;
    type Decoder = Raw;
    fn encoder(&mut self) -> Raw {
        Raw
    }
    fn decoder(&mut self) -> Raw {
        Raw
    }
}
impl Encoder for Raw {
    type Item = Bytes;
    type Error = Status;
    fn encode(&mut self, item: Bytes, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        dst.put(item);
        Ok(())
    }
}
impl Decoder for Raw {
    type Item = Bytes;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Bytes>, Status> {
        Ok(Some(src.copy_to_bytes(src.remaining())))
    }
}

async fn start(
    limits: Limits,
    invalid_output: bool,
) -> (
    tonic::transport::Channel,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(Greeter {
                limits,
                invalid_output,
            })
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            ),
    );
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    (channel, stop, server)
}
async fn raw(channel: tonic::transport::Channel, data: Vec<u8>) -> Result<Bytes, Status> {
    let mut client = tonic::client::Grpc::new(channel);
    client.ready().await.unwrap();
    client
        .unary(
            Request::new(Bytes::from(data)),
            tonic::codegen::http::uri::PathAndQuery::from_static("/rydantic.Greeter/Hello"),
            RawCodec,
        )
        .await
        .map(tonic::Response::into_inner)
}
fn unchecked(name: &str) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    let mut context = rydantic::Context::new();
    models::Hello { name: name.into(), context: models::Context { language: "en".into() } }.write_capnp(message.init_root::<<<models::Hello as CapnpModel>::Wire as capnp::traits::Owned>::Builder<'_>>(), &mut context);
    context.result().unwrap();
    capnp::serialize::write_message_to_words(&message)
}

#[tokio::test]
async fn unary_roundtrip_and_untrusted_requests() {
    let limits = Limits::new(1024, 1024, 16).unwrap();
    let (channel, stop, server) = start(limits, false).await;
    assert_eq!(
        example::greet(channel.clone(), limits, " Alice ".into())
            .await
            .unwrap()
            .message,
        "Hello, Alice!"
    );
    // Outgoing validation preserves whitespace; the receiving parse normalizes.
    let prepared = rydantic::grpc::request(
        models::Hello {
            name: " Alice ".into(),
            context: models::Context {
                language: "en".into(),
            },
        },
        limits,
    )
    .unwrap();
    assert_eq!(prepared.get_ref().name, " Alice ");
    assert_eq!(
        example::greet(channel.clone(), limits, "   ".into())
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let response = raw(channel.clone(), unchecked(" Bob ")).await.unwrap();
    assert_eq!(
        models::Greeting::parse_capnp(&response).unwrap().message,
        "Hello, Bob!"
    );
    for data in [vec![], vec![255; 8], unchecked(""), {
        let mut b = unchecked("Alice");
        b.extend_from_slice(&[0; 8]);
        b
    }] {
        let error = raw(channel.clone(), data).await.unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "invalid Cap'n Proto message");
    }
    let error = example::greet(channel.clone(), limits, String::new())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(!error.message().contains("name"));
    let error = example::greet(channel.clone(), limits, "secret".repeat(1000))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(!error.message().contains("secret"));
    // The transport refuses an oversized frame before calling the codec.
    let error = raw(channel, vec![0; 1025]).await.unwrap_err();
    assert_eq!(error.code(), Code::OutOfRange);
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn server_output_failure_is_redacted_internal() {
    let limits = Limits::default();
    let (channel, stop, server) = start(limits, true).await;
    let error = example::greet(channel, limits, "Alice".into())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Internal);
    assert!(error.message().contains("invalid Cap'n Proto model"));
    assert!(!error.message().contains("min_length"));
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn traversal_and_nesting_budgets_are_enforced() {
    for limits in [
        Limits::new(1024, 1, 64).unwrap(),
        Limits::new(1024, 1024, 1).unwrap(),
    ] {
        let (channel, stop, server) = start(limits, false).await;
        let error = raw(channel, unchecked("Alice")).await.unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "invalid Cap'n Proto message");
        let _ = stop.send(());
        server.await.unwrap().unwrap();
    }
}

#[test]
fn limits_reject_invalid_configuration() {
    assert!(Limits::new(0, 1, 1).is_err());
    assert!(Limits::new(1, 0, 1).is_err());
    assert!(Limits::new(1, 1, 0).is_err());
    assert!(Limits::new(1, 1, -1).is_err());
    // This constructor deliberately has no default role to prevent accidental
    // request/response error inversion in manually wired tonic services.
    let _ = CapnpCodec::<models::Hello, models::Greeting>::client(Limits::default());
}

#[derive(Clone)]
struct BadResponse(Bytes);
impl tonic::server::NamedService for BadResponse {
    const NAME: &'static str = "rydantic.Greeter";
}
impl<B> tonic::codegen::Service<tonic::codegen::http::Request<B>> for BadResponse
where
    B: tonic::codegen::Body + Send + 'static,
    B::Error: Into<tonic::codegen::StdError> + Send + 'static,
{
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = std::convert::Infallible;
    type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;
    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: tonic::codegen::http::Request<B>) -> Self::Future {
        let bytes = self.0.clone();
        Box::pin(async move {
            struct Reply(Bytes);
            impl tonic::server::UnaryService<Bytes> for Reply {
                type Response = Bytes;
                type Future = tonic::codegen::BoxFuture<tonic::Response<Bytes>, Status>;
                fn call(&mut self, _: Request<Bytes>) -> Self::Future {
                    let bytes = self.0.clone();
                    Box::pin(async move { Ok(tonic::Response::new(bytes)) })
                }
            }
            Ok(tonic::server::Grpc::new(RawCodec)
                .unary(Reply(bytes), request)
                .await)
        })
    }
}

#[tokio::test]
async fn client_rejects_malformed_or_invalid_responses() {
    let mut empty = capnp::message::Builder::new_default();
    let mut context = rydantic::Context::new();
    models::Greeting { message: String::new() }.write_capnp(empty.init_root::<<<models::Greeting as CapnpModel>::Wire as capnp::traits::Owned>::Builder<'_>>(), &mut context);
    context.result().unwrap();
    let invalid_model = capnp::serialize::write_message_to_words(&empty);
    let mut trailing = models::Greeting {
        message: "Hello".into(),
    }
    .dump_capnp()
    .unwrap();
    trailing.extend_from_slice(&[0; 8]);
    for payload in [vec![], vec![255; 8], invalid_model, trailing] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(BadResponse(Bytes::from(payload)))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async {
                        let _ = stopped.await;
                    },
                ),
        );
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let error = example::greet(channel, Limits::default(), "Alice".into())
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Internal);
        assert_eq!(error.message(), "invalid Cap'n Proto message");
        let _ = stop.send(());
        server.await.unwrap().unwrap();
    }
}
