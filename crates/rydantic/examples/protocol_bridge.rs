#[path = "grpc_capnp.rs"]
#[allow(dead_code)]
pub mod grpc_capnp;

use rydantic::axum::{
    framework::{self, extract::State, http::StatusCode, routing::get, routing::post, Router},
    Json,
};
use rydantic::grpc::Limits;
use rydantic::schema::openapi::OpenApiComponents;
use rydantic::{json, CapnpModel, SchemaMode};
use serde_json::Value;
use std::error::Error;

#[derive(Clone)]
struct GatewayState {
    channel: tonic::transport::Channel,
    limits: Limits,
    document: Value,
}

async fn greet_http(
    State(state): State<GatewayState>,
    Json(request): Json<grpc_capnp::models::Hello>,
) -> Result<Json<grpc_capnp::models::Greeting>, StatusCode> {
    // Reject local wire-encoding failures before contacting the backend. Do not
    // classify a backend's InvalidArgument status as an HTTP client error.
    let message = request
        .to_capnp_message()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let size = capnp::serialize::compute_serialized_size_in_words(&message).checked_mul(8);
    if size.is_none_or(|size| size > state.limits.message_bytes()) {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    drop(message);

    grpc_capnp::greet_model(state.channel, state.limits, request)
        .await
        .map(Json)
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

async fn openapi_http(State(state): State<GatewayState>) -> framework::response::Json<Value> {
    framework::response::Json(state.document)
}

pub fn openapi_document() -> Result<Value, Box<dyn Error>> {
    let mut components = OpenApiComponents::new();
    let request =
        components.insert::<grpc_capnp::models::Hello>("HelloRequest", SchemaMode::Input)?;
    let response = components
        .insert::<grpc_capnp::models::Greeting>("GreetingResponse", SchemaMode::Output)?;
    let mut local_rules = request.local_rules;
    local_rules.extend(response.local_rules);

    Ok(json!({
        "openapi": "3.1.1",
        "info": {"title": "Rydantic protocol bridge", "version": "1.0.0"},
        "paths": {
            "/greet": {"post": {
                "operationId": "greet",
                "x-rydantic-grpc-method": grpc_capnp::HELLO_RPC_PATH,
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": request.reference}}
                },
                "responses": {
                    "200": {
                        "description": "Validated greeting",
                        "content": {"application/json": {"schema": response.reference}}
                    },
                    "400": {"description": "Malformed JSON"},
                    "413": {"description": "Request exceeds the HTTP body or encoded gRPC message limit"},
                    "415": {"description": "Unsupported content type"},
                    "422": {"description": "Request model validation or local Cap'n Proto encoding failed"},
                    "502": {"description": "gRPC backend failure"}
                }
            }}
        },
        "components": {"schemas": components.into_schemas()},
        "x-rydantic-local-rules": local_rules
    }))
}

pub fn app(channel: tonic::transport::Channel, limits: Limits) -> Result<Router, Box<dyn Error>> {
    let document = openapi_document()?;
    Ok(Router::new()
        .route("/greet", post(greet_http))
        .route("/openapi.json", get(openapi_http))
        .with_state(GatewayState {
            channel,
            limits,
            document,
        }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let limits = Limits::default();
    let grpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let grpc_address = grpc_listener.local_addr()?;
    let grpc_server = tonic::transport::Server::builder()
        .add_service(grpc_capnp::Greeter {
            limits,
            invalid_output: false,
        })
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(
            grpc_listener,
        ));
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{grpc_address}"))?
        .connect()
        .await?;

    let app = app(channel, limits)?;
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let http_address = http_listener.local_addr()?;
    println!("POST http://{http_address}/greet");
    println!("GET  http://{http_address}/openapi.json");
    println!("HTTP JSON requests are validated and forwarded over Cap'n Proto gRPC.");

    let http_server = framework::serve(http_listener, app);
    let (grpc_result, http_result) = tokio::join!(grpc_server, http_server);
    grpc_result?;
    http_result?;
    Ok(())
}
