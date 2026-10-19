#![cfg(feature = "axum")]
use axum::{
    body::{to_bytes, Body},
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    response::IntoResponse,
};
use rydantic::{
    axum::{routing::post, AsyncJson, Json, Router},
    Model, ValidationError,
};
use tower::ServiceExt;

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 2, serialization_alias = "fullName")]
    name: String,
    #[model(ge = 18)]
    age: u8,
    #[model(exclude, default = "hidden")]
    secret: String,
}
async fn echo(Json(user): Json<User>) -> Json<User> {
    Json(user)
}
async fn list(Json(users): Json<Vec<User>>) -> Json<Vec<User>> {
    Json(users)
}
fn request(body: &str, mime: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("POST").uri("/");
    if let Some(mime) = mime {
        builder = builder.header("content-type", mime);
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}
async fn json_body(response: axum::response::Response) -> rydantic::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn requests_normalize_and_responses_preserve_aliases_and_exclusions() {
    let app = Router::new().route("/", post(echo));
    let response = app
        .oneshot(request(
            r#"{"name":" Alice ","age":25}"#,
            Some("application/vnd.api+json; charset=utf-8"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        json_body(response).await,
        rydantic::json!({"fullName":"Alice","age":25})
    );
    let response = Router::new()
        .route("/", post(list))
        .oneshot(request(
            r#"[{"name":" Alice ","age":25}]"#,
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(json_body(response).await[0]["fullName"], "Alice");
}
#[tokio::test]
async fn syntax_content_type_size_and_model_errors_have_distinct_statuses() {
    let app = Router::new()
        .route("/", post(echo))
        .layer(DefaultBodyLimit::max(128));
    for (body, mime, status, code) in [
        ("{}", None, 415, "content_type"),
        ("{", Some("application/json"), 400, "json_syntax"),
        (
            r#"{"name":"A","age":17}"#,
            Some("application/json"),
            422,
            "too_short",
        ),
    ] {
        let response = app.clone().oneshot(request(body, mime)).await.unwrap();
        assert_eq!(response.status().as_u16(), status);
        let errors = json_body(response).await;
        assert_eq!(errors["detail"][0]["code"], code);
        if status == 422 {
            assert_eq!(errors["detail"][0]["loc"], rydantic::json!(["name"]));
            assert_eq!(errors["detail"].as_array().unwrap().len(), 2);
        }
    }
    let response = app
        .oneshot(request(
            &format!(r#"{{"name":"{}","age":25}}"#, "A".repeat(200)),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        json_body(response).await["detail"][0]["code"],
        "body_too_large"
    );
}
#[tokio::test]
async fn invalid_mutated_output_is_rejected_before_serialization() {
    let response = Json(User {
        name: "Alice".into(),
        age: 1,
        secret: "sensitive".into(),
    })
    .into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json_body(response).await.to_string();
    assert!(body.contains("greater_than_equal"));
    assert!(!body.contains("sensitive"));
}
async fn allowed(name: &str) -> Result<(), ValidationError> {
    tokio::task::yield_now().await;
    if name == "taken" {
        Err(ValidationError::new("taken", "name is taken"))
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
struct AsyncUser {
    #[model(trim,async_validate=allowed)]
    name: String,
}
#[tokio::test]
async fn async_extractors_and_responses_await_nested_rules() {
    async fn echo_async(
        AsyncJson(user): AsyncJson<AsyncUser>,
    ) -> Result<axum::response::Response, rydantic::axum::Rejection> {
        AsyncJson(user).response().await
    }
    let app = Router::new().route("/", post(echo_async));
    let response = app
        .clone()
        .oneshot(request(r#"{"name":" taken "}"#, Some("application/json")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["detail"][0]["code"], "taken");
    let response = app
        .oneshot(request(r#"{"name":" free "}"#, Some("application/json")))
        .await
        .unwrap();
    assert_eq!(json_body(response).await["name"], "free");
    let response = AsyncJson(vec![AsyncUser {
        name: "taken".into(),
    }])
    .response()
    .await
    .unwrap_err()
    .into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["detail"][0]["loc"],
        rydantic::json!([0, "name"])
    );
}
#[tokio::test]
async fn request_extraction_preserves_raw_subtrees() {
    #[derive(Model)]
    struct Raw {
        payload: Box<rydantic::RawValue>,
    }
    async fn raw(Json(value): Json<Raw>) -> StatusCode {
        assert_eq!(value.payload.get(), "{ \"n\": 1e+100 }");
        StatusCode::NO_CONTENT
    }
    let response = Router::new()
        .route("/", post(raw))
        .oneshot(request(
            r#"{"payload":{ "n": 1e+100 }}"#,
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
