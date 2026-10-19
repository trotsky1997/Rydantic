//! Axum extractors that preserve structured model errors and raw JSON input.
//!
//! `Json<T>` validates synchronous models on ingress and response serialization.
//! `AsyncJson<T>` awaits asynchronous rules; call its `response().await` method
//! when returning a model with asynchronous validators.
//!
//! ```
//! use rydantic::{Model, axum::{Json, Router, routing::post}};
//! #[derive(Model)]
//! struct User { #[model(trim, min_length = 2)] name: String }
//! async fn echo(Json(user): Json<User>) -> Json<User> { Json(user) }
//! let _: Router = Router::new().route("/users", post(echo));
//! ```
use crate::{
    AsyncCheck, Context, Input, JsonType, RawValue, Stage, ValidationError, ValidationErrors,
};
pub use ::axum as framework;
use ::axum::{
    extract::{rejection::JsonRejection, FromRequest, Request},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
};
pub use ::axum::{routing, Router};

/// JSON request/response adapter for synchronous models, collections and scalars.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

/// JSON request adapter that awaits asynchronous model validators.
#[derive(Debug, Clone, Copy, Default)]
pub struct AsyncJson<T>(pub T);

/// A stable JSON error response: `{ "detail": [ValidationError, ...] }`.
#[derive(Debug)]
pub struct Rejection {
    status: StatusCode,
    errors: ValidationErrors,
}
impl Rejection {
    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn errors(&self) -> &ValidationErrors {
        &self.errors
    }
    pub fn into_errors(self) -> ValidationErrors {
        self.errors
    }
    fn validation(errors: ValidationErrors) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            errors,
        }
    }
    fn serialization(errors: ValidationErrors) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            errors,
        }
    }
}
impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.errors)
    }
}
impl std::error::Error for Rejection {}
impl From<ValidationErrors> for Rejection {
    fn from(errors: ValidationErrors) -> Self {
        Self::validation(errors)
    }
}
impl From<JsonRejection> for Rejection {
    fn from(error: JsonRejection) -> Self {
        let status = error.status();
        let (code, message) = match status {
            StatusCode::UNSUPPORTED_MEDIA_TYPE => (
                "content_type",
                "expected application/json or application/*+json",
            ),
            StatusCode::PAYLOAD_TOO_LARGE => (
                "body_too_large",
                "request body exceeds the configured limit",
            ),
            _ => (
                "json_syntax",
                "request body must contain one valid JSON document",
            ),
        };
        Self {
            status,
            errors: ValidationError::new(code, message)
                .with_stage(Stage::Parse)
                .into(),
        }
    }
}
impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        (
            self.status,
            ::axum::Json(crate::json!({"detail":self.errors.to_json()})),
        )
            .into_response()
    }
}

fn decode<T: JsonType>(raw: &RawValue) -> Result<T, ValidationErrors> {
    let mut ctx = Context::new();
    let value = T::decode_input(Input::Borrowed(raw), &mut ctx, false);
    ctx.finish(value)
}
fn encode<T: JsonType>(value: &T) -> Result<String, ValidationErrors> {
    let mut ctx = Context::new();
    let value = value.encode(&mut ctx);
    let value = ctx.finish(Some(value))?;
    serde_json::to_string(&value).map_err(|error| {
        ValidationError::new("serialization", error.to_string())
            .with_stage(Stage::Serialize)
            .into()
    })
}
fn response(json: String) -> Response {
    ([(CONTENT_TYPE, "application/json")], json).into_response()
}

impl<S, T> FromRequest<S> for Json<T>
where
    S: Send + Sync,
    T: JsonType + Send,
{
    type Rejection = Rejection;
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let ::axum::Json(raw) = ::axum::Json::<Box<RawValue>>::from_request(request, state)
            .await
            .map_err(Rejection::from)?;
        crate::asynchronous::sync_guard(T::async_required(&mut std::collections::HashSet::new()))
            .map_err(Rejection::validation)?;
        decode(&raw).map(Self).map_err(Rejection::validation)
    }
}
impl<T: JsonType> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        let result = (|| {
            crate::asynchronous::sync_guard(T::async_required(
                &mut std::collections::HashSet::new(),
            ))?;
            let mut ctx = Context::new();
            self.0.validate_into(&mut ctx);
            ctx.result()?;
            encode(&self.0)
        })();
        match result {
            Ok(json) => response(json),
            Err(errors) => Rejection::serialization(errors).into_response(),
        }
    }
}
impl<S, T> FromRequest<S> for AsyncJson<T>
where
    S: Send + Sync,
    T: JsonType + AsyncCheck + Send,
{
    type Rejection = Rejection;
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let ::axum::Json(raw) = ::axum::Json::<Box<RawValue>>::from_request(request, state)
            .await
            .map_err(Rejection::from)?;
        let value: T = decode(&raw).map_err(Rejection::validation)?;
        let mut ctx = Context::new();
        value.check_async(&mut ctx).await;
        ctx.result().map_err(Rejection::validation)?;
        Ok(Self(value))
    }
}
impl<T: JsonType + AsyncCheck> AsyncJson<T> {
    /// Await output validation before producing a response, including nested rules.
    pub async fn response(self) -> Result<Response, Rejection> {
        let mut ctx = Context::new();
        self.0.validate_into(&mut ctx);
        ctx.result().map_err(Rejection::serialization)?;
        let mut ctx = Context::new();
        self.0.check_async(&mut ctx).await;
        ctx.result().map_err(Rejection::serialization)?;
        encode(&self.0)
            .map(response)
            .map_err(Rejection::serialization)
    }
}
macro_rules! access {
    ($name:ident) => {
        impl<T> std::ops::Deref for $name<T> {
            type Target = T;
            fn deref(&self) -> &T {
                &self.0
            }
        }
        impl<T> std::ops::DerefMut for $name<T> {
            fn deref_mut(&mut self) -> &mut T {
                &mut self.0
            }
        }
    };
}
access!(Json);
access!(AsyncJson);
