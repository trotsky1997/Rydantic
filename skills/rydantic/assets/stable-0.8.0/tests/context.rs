use rydantic::{Context, Model, ValidationError, ValidationErrors};
use std::sync::Arc;

struct Policy {
    maximum: u32,
}

fn allowed(value: &u32, ctx: &Context) -> Result<(), ValidationError> {
    let policy = ctx.require_data::<Policy>()?;
    if *value > policy.maximum {
        Err(ValidationError::new("limit", "request limit exceeded"))
    } else {
        Ok(())
    }
}

#[derive(Debug, Model)]
struct Request {
    #[model(validate_with_context = allowed)]
    amount: u32,
}

fn policy(maximum: u32) -> Arc<Policy> {
    Arc::new(Policy { maximum })
}

fn error_code(errors: ValidationErrors, code: &str) {
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, code);
    assert_eq!(errors.errors()[0].pointer(), "/amount");
}

#[test]
fn supplied_policy_accepts_boundary_and_rejects_neighbor() {
    let request = Request::parse_json_with_context(r#"{"amount":10}"#, policy(10)).unwrap();
    assert_eq!(request.amount, 10);
    error_code(
        Request::parse_json_with_context(r#"{"amount":11}"#, policy(10)).unwrap_err(),
        "limit",
    );
}

#[test]
fn missing_and_wrong_context_are_structured_errors() {
    error_code(
        Request::parse_json(r#"{"amount":1}"#).unwrap_err(),
        "context_missing",
    );
    error_code(
        Request::parse_json_with_context(r#"{"amount":1}"#, Arc::new(10u32)).unwrap_err(),
        "context_type_mismatch",
    );
}

#[test]
fn revalidation_uses_resupplied_policy_without_retaining_context() {
    let original = policy(10);
    let request = Request::parse_json_with_context(r#"{"amount":10}"#, original.clone()).unwrap();
    assert_eq!(Arc::strong_count(&original), 1);
    error_code(request.validate().unwrap_err(), "context_missing");
    request.validate_with_context(original.clone()).unwrap();
    error_code(
        request.validate_with_context(policy(9)).unwrap_err(),
        "limit",
    );
    assert_eq!(Arc::strong_count(&original), 1);
    error_code(request.dump_json().unwrap_err(), "context_missing");
    error_code(
        request.dump_json_with_context(policy(9)).unwrap_err(),
        "limit",
    );
    let dumped = request.dump_json_with_context(original.clone()).unwrap();
    assert_eq!(dumped, r#"{"amount":10}"#);
    assert_eq!(Arc::strong_count(&original), 1);
}

fn add_policy(value: rydantic::Value, ctx: &Context) -> Result<rydantic::Value, ValidationError> {
    let n = value
        .as_u64()
        .ok_or_else(|| ValidationError::new("type", "integer required"))?;
    Ok(rydantic::json!(
        n + u64::from(ctx.require_data::<Policy>()?.maximum)
    ))
}
#[derive(Debug, Model)]
struct Defaulted {
    #[model(default = 1, before_with_context = [add_policy, add_policy])]
    amount: u32,
}
#[test]
fn context_callback_list_transforms_defaults_in_order() {
    let value = Defaulted::parse_value_with_context(rydantic::json!({}), policy(2)).unwrap();
    assert_eq!(value.amount, 5);
    // Revalidation is not a second normalization pass.
    value.validate_with_context(policy(2)).unwrap();
    assert_eq!(value.amount, 5);
}

async fn async_allowed(value: &u32, ctx: &Context) -> Result<(), ValidationError> {
    allowed(value, ctx)
}
#[derive(Debug, Model)]
struct AsyncRequest {
    #[model(async_validate_with_context = async_allowed)]
    amount: u32,
}
fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut ctx = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut ctx) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("test future must be immediately ready"),
    }
}
#[test]
fn async_context_requires_async_entrypoint_and_pipeline_checks() {
    let errors = AsyncRequest::parse_value_with_context(rydantic::json!({"amount":1}), policy(2))
        .unwrap_err();
    assert_eq!(errors.errors()[0].code, "async_validation_required");
    let pipeline = rydantic::pipeline::AsyncPipeline::plain(|n: u32, limit: &u32| {
        Box::pin(async move {
            AsyncRequest::parse_value_async_with_context(
                rydantic::json!({"amount":n}),
                policy(*limit),
            )
            .await
        })
    })
    .then(|request, _| Box::pin(async move { Ok(request.amount) }));
    assert_eq!(ready(pipeline.run(2, &2)).unwrap(), 2);
    error_code(ready(pipeline.run(3, &2)).unwrap_err(), "limit");
}
