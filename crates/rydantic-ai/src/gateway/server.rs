use super::{
    egress::View,
    ingress,
    replay::Replay,
    stream::{Frame, Output},
    Gateway, GatewayClient,
};
use crate::{Api, Error, ModelProvider, ModelRequest, RunContext};
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone)]
struct Server {
    gateway: Gateway,
    replay: Arc<Mutex<Replay>>,
}
pub fn router(gateway: Gateway) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/responses", post(responses))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/messages", post(messages))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(Server {
            gateway,
            replay: Arc::new(Mutex::new(Replay::default())),
        })
}
fn authenticate(server: &Server, headers: &HeaderMap, api: Api) -> crate::Result<GatewayClient> {
    let bearer = match headers.get("authorization") {
        Some(value) => Some(
            value
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or(Error::Unauthorized)?,
        ),
        None => None,
    };
    let key = if api == Api::AnthropicMessages {
        let native = headers
            .get("x-api-key")
            .map(|v| v.to_str().map_err(|_| Error::Unauthorized))
            .transpose()?;
        if native.zip(bearer).is_some_and(|(a, b)| a != b) {
            return Err(Error::Unauthorized);
        }
        native.or(bearer)
    } else {
        bearer
    }
    .ok_or(Error::Unauthorized)?;
    server.gateway.client(key)
}
fn error_value(api: Api, code: &str, status: StatusCode) -> Value {
    let kind = match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::NOT_FOUND => "not_found_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::BAD_REQUEST
        | StatusCode::PAYLOAD_TOO_LARGE
        | StatusCode::UNSUPPORTED_MEDIA_TYPE => "invalid_request_error",
        _ => {
            if api == Api::AnthropicMessages {
                "api_error"
            } else {
                "server_error"
            }
        }
    };
    if api == Api::AnthropicMessages {
        json!({"type":"error","error":{"type":kind,"message":code}})
    } else {
        json!({"error":{"code":code,"type":kind,"message":code,"param":null}})
    }
}
fn status(error: &Error) -> (StatusCode, &'static str) {
    match error {
        Error::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        Error::UnknownRoute => (StatusCode::NOT_FOUND, "unknown_model"),
        Error::Budget => (StatusCode::TOO_MANY_REQUESTS, "budget_exhausted"),
        Error::Configuration(_) | Error::Validation(_) => {
            (StatusCode::BAD_REQUEST, "invalid_request")
        }
        Error::Timeout => (StatusCode::GATEWAY_TIMEOUT, "timeout"),
        Error::Cancelled => (StatusCode::BAD_GATEWAY, "cancelled"),
        _ => (StatusCode::BAD_GATEWAY, "upstream_error"),
    }
}
fn failure(api: Api, error: Error) -> Response {
    let (status, code) = status(&error);
    (status, Json(error_value(api, code, status))).into_response()
}
fn stream_error(api: Api, error: Error) -> Frame {
    let (status, code) = status(&error);
    let mut value = error_value(api, code, status);
    if api == Api::Responses {
        value = json!({"type":"error","code":code,"message":code,"param":null});
    }
    Frame::Json {
        event: if api == Api::ChatCompletions {
            None
        } else {
            Some("error".into())
        },
        value,
    }
}
async fn models(State(server): State<Server>, headers: HeaderMap) -> Response {
    match authenticate(&server,&headers,Api::Responses) {
        Ok(client)=>Json(json!({"object":"list","data":client.models().into_iter().map(|id|json!({"id":id,"object":"model","owned_by":"rydantic-ai"})).collect::<Vec<_>>()})).into_response(),
        Err(error)=>failure(Api::Responses,error),
    }
}
async fn responses(
    State(server): State<Server>,
    headers: HeaderMap,
    body: std::result::Result<Json<Value>, JsonRejection>,
) -> Response {
    handle(server, headers, body, Api::Responses).await
}
async fn chat(
    State(server): State<Server>,
    headers: HeaderMap,
    body: std::result::Result<Json<Value>, JsonRejection>,
) -> Response {
    handle(server, headers, body, Api::ChatCompletions).await
}
async fn messages(
    State(server): State<Server>,
    headers: HeaderMap,
    body: std::result::Result<Json<Value>, JsonRejection>,
) -> Response {
    handle(server, headers, body, Api::AnthropicMessages).await
}
async fn handle(
    server: Server,
    headers: HeaderMap,
    body: std::result::Result<Json<Value>, JsonRejection>,
    api: Api,
) -> Response {
    let client = match authenticate(&server, &headers, api) {
        Ok(client) => client,
        Err(error) => return failure(api, error),
    };
    if api == Api::AnthropicMessages
        && headers
            .get("anthropic-version")
            .is_some_and(|v| v != "2023-06-01")
    {
        return failure(api, Error::Configuration("unsupported API version".into()));
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => {
            let status = error.status();
            return (status, Json(error_value(api, "invalid_request", status))).into_response();
        }
    };
    let mut decoded = match ingress::parse(api, body) {
        Ok(decoded) => decoded,
        Err(error) => return failure(api, error),
    };
    if !client.models().contains(&decoded.request.model) {
        return failure(api, Error::UnknownRoute);
    }
    match server.replay.lock() {
        Ok(mut replay) => replay.restore(&client.id, &mut decoded.request),
        Err(_) => return failure(api, Error::Protocol("replay storage unavailable".into())),
    };
    let view = View::new(api, decoded.request.model.clone()).with_request(&decoded.request);
    if !decoded.request.stream {
        let context = RunContext::default();
        let _guard = context.cancellation.clone().drop_guard();
        let response = match client.complete(decoded.request.clone(), context).await {
            Ok(response) => response,
            Err(error) => return failure(api, error),
        };
        let public = match view.response(&response) {
            Ok(public) => public,
            Err(error) => return failure(api, error),
        };
        if let Ok(mut replay) = server.replay.lock() {
            replay.record(&client.id, &decoded.request, &response);
        }
        return Json(public).into_response();
    }
    streaming(
        server,
        client,
        decoded.request,
        view,
        decoded.include_usage,
        RunContext::default(),
    )
}
async fn send(
    context: &RunContext,
    sender: &mpsc::Sender<Frame>,
    frames: Vec<Frame>,
) -> crate::Result<()> {
    for frame in frames {
        context
            .wait(async { sender.send(frame).await.map_err(|_| Error::Cancelled) })
            .await?;
    }
    Ok(())
}
fn streaming(
    server: Server,
    client: GatewayClient,
    request: ModelRequest,
    view: View,
    include_usage: bool,
    context: RunContext,
) -> Response {
    let api = view.api;
    let guard = context.cancellation.clone().drop_guard();
    let (sender, receiver) = mpsc::channel::<Frame>(32);
    tokio::spawn(async move {
        let (events, mut native) = mpsc::channel(32);
        let mut output = Output::new(view, include_usage);
        let mut call = client.complete(request.clone(), context.clone().with_events(events));
        let mut receiving = true;
        let result = loop {
            tokio::select! {
                biased;
                result=&mut call=>break result,
                event=native.recv(), if receiving=>{
                    if let Some(event)=event {
                        match output.observe(event) {
                            Ok(frames)=>if let Err(error)=send(&context,&sender,frames).await {break Err(error);},
                            Err(error)=>break Err(error),
                        }
                    } else { receiving=false; }
                }
            }
        };
        drop(call);
        let result = async {
            let response = result?;
            while let Ok(event) = native.try_recv() {
                send(&context, &sender, output.observe(event)?).await?;
            }
            let frames = output.finish(&response)?;
            if let Ok(mut replay) = server.replay.lock() {
                replay.record(&client.id, &request, &response);
            }
            send(&context, &sender, frames).await
        }
        .await;
        if let Err(error) = result {
            let frame = stream_error(api, error);
            if context.check().is_ok() {
                let _ = context
                    .wait(async { sender.send(frame).await.map_err(|_| Error::Cancelled) })
                    .await;
            } else {
                let _ = sender.try_send(frame);
            }
        }
        context.cancellation.cancel();
    });
    let stream = ReceiverStream::new(receiver).map(move |frame| {
        let _keep_guard = &guard;
        let event = match frame {
            Frame::Done => Event::default().data("[DONE]"),
            Frame::Json { event, value } => {
                let mut frame = Event::default().data(value.to_string());
                if let Some(event) = event {
                    frame = frame.event(event);
                }
                frame
            }
        };
        Ok::<_, Infallible>(event)
    });
    Sse::new(stream).into_response()
}

#[cfg(test)]
mod tests {
    use super::super::{Budget, ClientPolicy, Route};
    use super::*;
    use crate::{BoxFuture, ModelResponse, Result};
    use std::{
        collections::{BTreeMap, HashSet},
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    use tokio::sync::Notify;
    struct Pending {
        started: Arc<Notify>,
        dropped: Arc<AtomicBool>,
        closed: bool,
    }
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl ModelProvider for Pending {
        fn complete(
            &self,
            _: ModelRequest,
            mut context: RunContext,
        ) -> BoxFuture<'_, Result<ModelResponse>> {
            Box::pin(async move {
                let _guard = Guard(self.dropped.clone());
                self.started.notify_one();
                if self.closed {
                    drop(context.events.take());
                } else {
                    for _ in 0..128 {
                        context
                            .emit(json!({"type":"response.output_text.delta","delta":"x"}))
                            .await?;
                    }
                }
                std::future::pending().await
            })
        }
    }
    fn setup(closed: bool) -> (Server, Arc<Notify>, Arc<AtomicBool>) {
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let gateway = Gateway::new(
            BTreeMap::from([(
                "model".into(),
                vec![Route {
                    provider: Arc::new(Pending {
                        started: started.clone(),
                        dropped: dropped.clone(),
                        closed,
                    }),
                    model: "upstream".into(),
                    price: None,
                }],
            )]),
            vec![ClientPolicy {
                id: "app".into(),
                key: "deadline-fixture-key".into(),
                models: HashSet::from(["model".into()]),
                budget: Budget::default(),
            }],
            Budget::default(),
        )
        .unwrap();
        (
            Server {
                gateway,
                replay: Arc::new(Mutex::new(Replay::default())),
            },
            started,
            dropped,
        )
    }
    #[tokio::test]
    async fn stalled_consumers_and_closed_event_sinks_obey_the_absolute_deadline() {
        for api in [Api::Responses, Api::ChatCompletions, Api::AnthropicMessages] {
            for closed in [false, true] {
                let (server, started, dropped) = setup(closed);
                let client = server.gateway.client("deadline-fixture-key").unwrap();
                let response = streaming(
                    server.clone(),
                    client,
                    ModelRequest::new("model", "x"),
                    View::new(api, "model".into()),
                    true,
                    RunContext::with_timeout(Duration::from_millis(120)),
                );
                started.notified().await;
                tokio::time::sleep(Duration::from_millis(180)).await;
                let body = tokio::time::timeout(
                    Duration::from_secs(2),
                    axum::body::to_bytes(response.into_body(), 1024 * 1024),
                )
                .await
                .unwrap()
                .unwrap();
                let text = String::from_utf8(body.to_vec()).unwrap();
                assert!(
                    !text.contains("response.completed")
                        && !text.contains("message_stop")
                        && !text.contains("[DONE]")
                );
                assert!(dropped.load(Ordering::SeqCst));
                assert_eq!(server.gateway.snapshot("app").unwrap().requests, 1);
                assert_eq!(server.gateway.snapshot("app").unwrap().reserved_tokens, 0);
            }
        }
    }
    #[tokio::test]
    async fn dropping_each_frontend_stream_cancels_the_pending_provider() {
        for api in [Api::Responses, Api::ChatCompletions, Api::AnthropicMessages] {
            let (server, started, dropped) = setup(false);
            let client = server.gateway.client("deadline-fixture-key").unwrap();
            let response = streaming(
                server,
                client,
                ModelRequest::new("model", "x"),
                View::new(api, "model".into()),
                true,
                RunContext::default(),
            );
            started.notified().await;
            drop(response);
            tokio::time::timeout(Duration::from_secs(2), async {
                while !dropped.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
    }
}
