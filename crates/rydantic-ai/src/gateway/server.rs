use super::{Gateway, GatewayClient};
use crate::{Error, ModelProvider, ModelRequest, RunContext};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub fn router(gateway: Gateway) -> Router {
    Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/models", get(models))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(gateway)
}
fn authenticate(gateway: &Gateway, headers: &HeaderMap) -> crate::Result<GatewayClient> {
    let key = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(Error::Unauthorized)?;
    gateway.client(key)
}
fn failure(error: Error) -> Response {
    let (status, code) = match error {
        Error::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        Error::UnknownRoute => (StatusCode::NOT_FOUND, "unknown_model"),
        Error::Budget => (StatusCode::TOO_MANY_REQUESTS, "budget_exhausted"),
        Error::Configuration(_) | Error::Validation(_) => {
            (StatusCode::BAD_REQUEST, "invalid_request")
        }
        Error::Timeout => (StatusCode::GATEWAY_TIMEOUT, "timeout"),
        _ => (StatusCode::BAD_GATEWAY, "upstream_error"),
    };
    (status, Json(json!({"error":{"code":code,"message":code}}))).into_response()
}
async fn models(State(gateway): State<Gateway>, headers: HeaderMap) -> Response {
    match authenticate(&gateway, &headers) {
        Ok(client) => Json(json!({"object":"list","data":client.models().into_iter().map(|id| json!({"id":id,"object":"model","owned_by":"rydantic-ai"})).collect::<Vec<_>>()})).into_response(),
        Err(error) => failure(error),
    }
}
async fn responses(
    State(gateway): State<Gateway>,
    headers: HeaderMap,
    Json(request): Json<ModelRequest>,
) -> Response {
    let client = match authenticate(&gateway, &headers) {
        Ok(client) => client,
        Err(error) => return failure(error),
    };
    if let Err(error) = request.validate() {
        return failure(error);
    }
    if !client.models().contains(&request.model) {
        return failure(Error::UnknownRoute);
    }
    if !request.stream {
        return match client.complete(request, RunContext::default()).await {
            Ok(response) => Json(response.raw).into_response(),
            Err(error) => failure(error),
        };
    }
    let context = RunContext::default();
    let guard = context.cancellation.clone().drop_guard();
    let (out_tx, out_rx) = mpsc::channel::<Value>(32);
    tokio::spawn(async move {
        let (in_tx, mut in_rx) = mpsc::channel::<Value>(32);
        let forward_tx = out_tx.clone();
        let forward_cancel = context.cancellation.clone();
        let forward = tokio::spawn(async move {
            while let Some(event) = in_rx.recv().await {
                if matches!(
                    event["type"].as_str(),
                    Some("response.completed" | "response.done")
                ) {
                    continue;
                }
                if forward_tx.send(event).await.is_err() {
                    forward_cancel.cancel();
                    break;
                }
            }
        });
        let result = client.complete(request, context.with_events(in_tx)).await;
        let _ = forward.await;
        let event = match result {
            Ok(response) => json!({"type":"response.completed","response":response.raw}),
            Err(error) => {
                json!({"type":"error","error":{"code":match error { Error::Budget => "budget_exhausted", Error::Cancelled => "cancelled", Error::Timeout => "timeout", _ => "upstream_error" }}})
            }
        };
        let _ = out_tx.send(event).await;
    });
    let stream = ReceiverStream::new(out_rx).map(move |value| {
        let _keep_guard_alive = &guard;
        Ok::<_, Infallible>(
            Event::default()
                .event(value["type"].as_str().unwrap_or("message"))
                .data(value.to_string()),
        )
    });
    Sse::new(stream).into_response()
}
