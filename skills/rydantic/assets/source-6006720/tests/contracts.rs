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
