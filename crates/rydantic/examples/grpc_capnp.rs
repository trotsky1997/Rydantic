//! A real unary gRPC exchange without a protobuf model or `.proto` file.
use rydantic::grpc::{CapnpCodec, Limits};
use tonic::codegen::{http, Body, BoxFuture, Service, StdError};
use tonic::{Request, Response, Status};

pub const HELLO_RPC_PATH: &str = "/rydantic.Greeter/Hello";

#[rydantic::capnp_models(namespace = "rydantic.examples.grpc")]
pub mod models {
    #[derive(rydantic::Model, Debug, PartialEq)]
    pub struct Hello {
        #[model(trim, min_length = 1)]
        pub name: String,
        pub context: Context,
    }
    #[derive(rydantic::Model, Debug, PartialEq)]
    pub struct Context {
        pub language: String,
    }
    #[derive(rydantic::Model, Debug, PartialEq)]
    pub struct Greeting {
        #[model(min_length = 1)]
        pub message: String,
    }
}

// This explicit routing is the equivalent of generated tonic service plumbing.
// The models above are the only model definitions; no protobuf DTO is involved.
#[derive(Clone)]
pub struct Greeter {
    pub limits: Limits,
    pub invalid_output: bool,
}
impl tonic::server::NamedService for Greeter {
    const NAME: &'static str = "rydantic.Greeter";
}
impl<B> Service<http::Request<B>> for Greeter
where
    B: Body + Send + 'static,
    B::Error: Into<StdError> + Send + 'static,
{
    type Response = http::Response<tonic::body::Body>;
    type Error = std::convert::Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;
    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        let limits = self.limits;
        let invalid_output = self.invalid_output;
        Box::pin(async move {
            if request.uri().path() != HELLO_RPC_PATH {
                return Ok(Status::unimplemented("unknown method").into_http());
            }
            struct Hello(bool);
            impl tonic::server::UnaryService<models::Hello> for Hello {
                type Response = models::Greeting;
                type Future = BoxFuture<Response<Self::Response>, Status>;
                fn call(&mut self, request: Request<models::Hello>) -> Self::Future {
                    let invalid = self.0;
                    Box::pin(async move {
                        Ok(Response::new(models::Greeting {
                            message: if invalid {
                                String::new()
                            } else {
                                format!("Hello, {}!", request.into_inner().name)
                            },
                        }))
                    })
                }
            }
            let codec = CapnpCodec::<models::Greeting, models::Hello>::server(limits);
            let mut grpc = tonic::server::Grpc::new(codec)
                .max_decoding_message_size(limits.message_bytes())
                .max_encoding_message_size(limits.message_bytes());
            Ok(grpc.unary(Hello(invalid_output), request).await)
        })
    }
}

pub async fn greet_model(
    channel: tonic::transport::Channel,
    limits: Limits,
    request: models::Hello,
) -> Result<models::Greeting, Status> {
    let mut client = tonic::client::Grpc::new(channel)
        .max_decoding_message_size(limits.message_bytes())
        .max_encoding_message_size(limits.message_bytes());
    client
        .ready()
        .await
        .map_err(|_| Status::unavailable("service unavailable"))?;
    let response = client
        .unary(
            rydantic::grpc::request(request, limits)?,
            http::uri::PathAndQuery::from_static(HELLO_RPC_PATH),
            CapnpCodec::<models::Hello, models::Greeting>::client(limits),
        )
        .await?;
    Ok(response.into_inner())
}

pub async fn greet(
    channel: tonic::transport::Channel,
    limits: Limits,
    name: String,
) -> Result<models::Greeting, Status> {
    greet_model(
        channel,
        limits,
        models::Hello {
            name,
            context: models::Context {
                language: "en".into(),
            },
        },
    )
    .await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(Greeter {
                limits,
                invalid_output: false,
            })
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            ),
    );
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))?
        .connect()
        .await?;
    let result = greet(channel, limits, " Alice ".into()).await;
    let _ = stop.send(());
    server.await??;
    assert_eq!(result?.message, "Hello, Alice!");
    println!("Cap’n Proto over gRPC: unary loopback passed");
    Ok(())
}
