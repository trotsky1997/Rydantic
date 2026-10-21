use rydantic::{json, Context, Model, ValidationError, ValidationErrors, Value};
use std::sync::Arc;
struct Policy(i64);
fn transform(v: Value, context: &Context) -> Result<Value, ValidationError> {
    Ok(json!(
        v.as_i64().unwrap() + context.require_data::<Policy>()?.0
    ))
}
fn check(v: &i64, context: &Context) -> Result<(), ValidationError> {
    if *v >= context.require_data::<Policy>()?.0 {
        Ok(())
    } else {
        Err(ValidationError::new("policy", "too small"))
    }
}
fn check_model(_: &Item, context: &Context) -> Result<(), ValidationErrors> {
    context
        .require_data::<Policy>()
        .map(|_| ())
        .map_err(|e| ValidationErrors(vec![e]))
}
#[derive(Debug, Model)]
#[model(validate_with_context = [check_model, check_model])]
struct Item {
    #[model(before_with_context = [transform, transform], validate_with_context = [check, check], default = 1)]
    value: i64,
}
#[test]
fn context_hooks_transform_defaults_and_require_matching_data() {
    let value = Item::parse_value_with_context(json!({}), Arc::new(Policy(2))).unwrap();
    assert_eq!(value.value, 5);
    assert_eq!(
        Item::parse_value(json!({})).unwrap_err().errors()[0].code,
        "context_missing"
    );
    assert_eq!(
        Item::parse_value_with_context(json!({}), Arc::new(()))
            .unwrap_err()
            .errors()[0]
            .code,
        "context_type_mismatch"
    );
    value.validate_with_context(Arc::new(Policy(2))).unwrap();
}
#[derive(Debug, Model)]
struct Newtype(#[model(before_with_context = [transform], validate_with_context = [check])] i64);
#[derive(Debug, Model)]
#[model(untagged)]
enum Choice {
    Number(#[model(before_with_context = [transform], validate_with_context = [check])] i64),
}
#[test]
fn context_hooks_work_on_newtypes_and_untagged_variants() {
    assert_eq!(
        Newtype::parse_value_with_context(json!(1), Arc::new(Policy(2)))
            .unwrap()
            .0,
        3
    );
    let Choice::Number(v) =
        Choice::parse_value_with_context(json!(1), Arc::new(Policy(2))).unwrap();
    assert_eq!(v, 3);
}
async fn async_check(v: &i64, context: &Context) -> Result<(), ValidationError> {
    check(v, context)
}
async fn async_model(_: &AsyncItem, context: &Context) -> Result<(), ValidationErrors> {
    context
        .require_data::<Policy>()
        .map(|_| ())
        .map_err(|e| ValidationErrors(vec![e]))
}
#[derive(Debug, Model)]
#[model(async_validate_with_context = [async_model, async_model])]
struct AsyncItem {
    #[model(async_validate_with_context = [async_check, async_check])]
    value: i64,
}
fn drive<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        _ => panic!("unexpected pending"),
    }
}
#[test]
fn async_context_lists_have_sync_guard_and_accumulate_errors() {
    assert!(AsyncItem::parse_value_with_context(json!({"value": 3}), Arc::new(Policy(2))).is_err());
    drive(AsyncItem::parse_value_async_with_context(
        json!({"value": 3}),
        Arc::new(Policy(2)),
    ))
    .unwrap();
    let errors = drive(AsyncItem::parse_value_async_with_context(
        json!({"value": 1}),
        Arc::new(Policy(2)),
    ))
    .unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors.errors()[0].loc, errors.errors()[1].loc);
}

#[derive(Debug, Model)]
struct NestedChoice {
    choices: Vec<Choice>,
}
#[test]
fn nested_union_trials_preserve_context_and_relative_branch_paths() {
    let raw = r#"{"choices":[1]}"#;
    let parsed = NestedChoice::parse_json_with_context(raw, Arc::new(Policy(2))).unwrap();
    assert!(matches!(parsed.choices[0], Choice::Number(3)));
    let parsed = drive(NestedChoice::parse_json_async_with_context(
        raw,
        Arc::new(Policy(3)),
    ))
    .unwrap();
    assert!(matches!(parsed.choices[0], Choice::Number(4)));
    let errors =
        NestedChoice::parse_value_with_context(json!({"choices":[-4]}), Arc::new(Policy(2)))
            .unwrap_err();
    assert_eq!(errors.0[0].pointer(), "/choices/0");
    assert_eq!(errors.0[0].code, "union_no_match");
    let branch = &errors.0[0].params["branches"][0]["errors"][0];
    assert_eq!(branch["code"], "policy");
    assert_eq!(branch["loc"], json!([]));
}
