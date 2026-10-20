use rydantic::{json, AsyncCheck, Context, Model, Stage, ValidationError, ValidationErrors, Value};
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll, Waker},
};
// Manual polling keeps each trace on its test thread, even under parallel test execution.
thread_local! { static TRACE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn log(s: impl Into<String>) {
    TRACE.with(|trace| trace.borrow_mut().push(s.into()));
}
fn trace() -> Vec<String> {
    TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}
fn reset() {
    trace();
}
fn drive<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut ctx = TaskContext::from_waker(waker);
    let mut f = std::pin::pin!(future);
    for _ in 0..100 {
        if let Poll::Ready(out) = f.as_mut().poll(&mut ctx) {
            return out;
        }
    }
    panic!("test future did not complete within 100 polls")
}
fn inspect_input(v: Value) -> Result<Value, ValidationError> {
    log(format!("before:{v}"));
    Ok(v)
}
fn field(v: &String) -> Result<(), ValidationError> {
    log(format!("field:{v}"));
    Ok(())
}
fn model(v: &Ordered) -> Result<(), ValidationErrors> {
    log(format!("model:{}", v.name));
    Ok(())
}
#[derive(Debug, Model)]
#[model(validate=model)]
struct Ordered {
    #[model(before=inspect_input, trim, lowercase, min_length=2, validate=field, default=" DEF ")]
    name: String,
}
#[test]
fn parsing_and_defaults_run_fixed_transform_pipeline() {
    reset();
    let m = Ordered::parse_json(r#"{"name":" HI "}"#).unwrap();
    assert_eq!(m.name, "hi");
    assert_eq!(trace(), ["before:\" HI \"", "field:hi", "model:hi"]);
    let m = Ordered::parse_value(json!({})).unwrap();
    assert_eq!(m.name, "def");
    assert_eq!(trace(), ["before:\" DEF \"", "field:def", "model:def"]);
}
#[test]
fn validate_and_dump_do_not_transform_or_rerun_before() {
    reset();
    let m = Ordered {
        name: " HI ".into(),
    };
    m.validate().unwrap();
    assert_eq!(trace(), ["field: HI ", "model: HI "]);
    assert_eq!(m.dump().unwrap()["name"], " HI ");
    assert_eq!(trace(), ["field: HI ", "model: HI "]);
}
fn fail_custom(_: &String) -> Result<(), ValidationError> {
    log("custom");
    Err(ValidationError::new("custom_check", "custom failure"))
}
#[derive(Debug, Model)]
struct Gate {
    #[model(min_length=3,validate=fail_custom)]
    value: String,
}
#[test]
fn parsing_and_revalidation_keep_their_existing_field_callback_gates() {
    // This is an observational contract, not a request to unify these entry points.
    for errors in [
        Gate::parse_json(r#"{"value":"x"}"#).unwrap_err(),
        Gate::parse_value(json!({"value":"x"})).unwrap_err(),
        drive(Gate::parse_json_async(r#"{"value":"x"}"#)).unwrap_err(),
        drive(Gate::parse_value_async(json!({"value":"x"}))).unwrap_err(),
    ] {
        assert_eq!(
            errors
                .errors()
                .iter()
                .map(|e| e.code.as_str())
                .collect::<Vec<_>>(),
            ["too_short"]
        );
        assert!(trace().is_empty());
    }
    let typed = Gate { value: "x".into() };
    // Execute separately so each callback trace belongs to exactly one entry point.
    for check in 0..4 {
        let errors = match check {
            0 => typed.validate().unwrap_err(),
            1 => typed.dump().unwrap_err(),
            2 => drive(typed.validate_async()).unwrap_err(),
            _ => drive(typed.dump_async()).unwrap_err(),
        };
        assert_eq!(
            errors
                .errors()
                .iter()
                .map(|e| e.code.as_str())
                .collect::<Vec<_>>(),
            ["too_short", "custom_check"]
        );
        assert_eq!(trace(), ["custom"]);
        assert!(errors
            .errors()
            .iter()
            .all(|e| e.pointer() == "/value" && e.stage == Stage::Validate));
    }
}
fn reject_raw(_: Value) -> Result<Value, ValidationError> {
    log("reject_before");
    Err(ValidationError::custom("reject"))
}
fn sibling(_: &String) -> Result<(), ValidationError> {
    log("sibling");
    Err(ValidationError::custom("sibling"))
}
fn parent(_: &Failure) -> Result<(), ValidationErrors> {
    log("parent");
    Ok(())
}
#[derive(Debug, Model)]
#[model(validate=parent)]
struct Failure {
    #[model(before=reject_raw,validate=field)]
    first: String,
    #[model(validate=sibling)]
    second: String,
}
#[test]
fn before_failure_collects_sibling_errors_and_skips_parent() {
    reset();
    let e = Failure::parse_value(json!({"first":"a","second":"b"})).unwrap_err();
    assert_eq!(trace(), ["reject_before", "sibling"]);
    assert_eq!(e.len(), 2);
    assert_eq!(e.errors()[0].pointer(), "/first");
    assert_eq!(e.errors()[0].stage, Stage::Parse);
    assert_eq!(e.errors()[1].pointer(), "/second");
}
#[derive(Debug, Model)]
struct Constraints {
    #[model(min_length=3,pattern="^[A-Z]+$",validate=field)]
    value: String,
}
#[test]
fn multiple_builtin_constraints_accumulate_but_skip_custom() {
    reset();
    let e = Constraints::parse_value(json!({"value":"x"})).unwrap_err();
    assert_eq!(e.len(), 2);
    assert!(trace().is_empty());
}
fn one(v: &String) -> Result<(), ValidationError> {
    log("one");
    if v == "bad" {
        Err(ValidationError::custom("one"))
    } else {
        Ok(())
    }
}
fn two(_: &String) -> Result<(), ValidationError> {
    log("two");
    Ok(())
}
fn composed(v: &String) -> Result<(), ValidationError> {
    one(v)?;
    two(v)
}
#[derive(Debug, Model)]
struct Composition {
    #[model(validate=composed)]
    value: String,
}
#[test]
fn composition_inside_one_callback_obeys_rust_control_flow() {
    reset();
    Composition::parse_value(json!({"value":"ok"})).unwrap();
    assert_eq!(trace(), ["one", "two"]);
    Composition::parse_value(json!({"value":"bad"})).unwrap_err();
    assert_eq!(trace(), ["one"]);
}
fn many(_: &Inner) -> Result<(), ValidationErrors> {
    Err(ValidationErrors::new(vec![
        ValidationError::custom("a").at("x"),
        ValidationError::custom("b").at("y"),
    ]))
}
#[derive(Debug, Model)]
#[model(validate=many)]
struct Inner {
    n: u32,
}
#[derive(Debug, Model)]
struct Outer {
    inner: Inner,
    sibling: bool,
}
#[test]
fn model_multiple_errors_keep_nested_relative_paths() {
    let e = Outer::parse_value(json!({"inner":{"n":1},"sibling":"wrong"})).unwrap_err();
    assert_eq!(
        e.errors().iter().map(|e| e.pointer()).collect::<Vec<_>>(),
        ["/inner/x", "/inner/y", "/sibling"]
    );
}
fn raw_wrong(_: Value) -> Result<Value, ValidationError> {
    Ok(json!("not a number"))
}
#[derive(Debug, Model)]
struct StillTyped {
    #[model(before=raw_wrong)]
    value: u32,
}
#[test]
fn before_cannot_bypass_typed_decoding() {
    let errors = StillTyped::parse_value(json!({"value":4})).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].pointer(), "/value");
    assert_eq!(errors.errors()[0].stage, Stage::Parse);
    assert_eq!(errors.errors()[0].code, "type_error");
}
async fn async_field(v: &String) -> Result<(), ValidationError> {
    log(format!("async:{v}"));
    if v == "bad" {
        Err(ValidationError::custom("bad"))
    } else {
        Ok(())
    }
}
async fn async_model(_: &AsyncModel) -> Result<(), ValidationErrors> {
    log("async:model");
    Ok(())
}
#[derive(Debug, Model)]
#[model(async_validate=async_model)]
struct AsyncModel {
    #[model(min_length=2,async_validate=async_field)]
    a: String,
    #[model(async_validate=async_field)]
    b: String,
}
#[test]
fn async_runs_sequentially_and_gates_model_on_sibling_success() {
    reset();
    drive(AsyncModel::parse_value_async(
        json!({"a":"first","b":"second"}),
    ))
    .unwrap();
    assert_eq!(trace(), ["async:first", "async:second", "async:model"]);
    let e = drive(AsyncModel::parse_json_async(r#"{"a":"bad","b":"bad"}"#)).unwrap_err();
    assert_eq!(e.len(), 2);
    assert_eq!(trace(), ["async:bad", "async:bad"]);
    assert_eq!(
        e.errors().iter().map(|e| e.pointer()).collect::<Vec<_>>(),
        ["/a", "/b"]
    );
}
#[test]
fn sync_errors_skip_all_async_and_sync_entrypoint_rejects_async_model() {
    reset();
    assert!(drive(AsyncModel::parse_value_async(json!({"a":"x","b":"ok"}))).is_err());
    assert!(trace().is_empty());
    assert_eq!(
        AsyncModel::parse_value(json!({"a":"first","b":"second"}))
            .unwrap_err()
            .errors()[0]
            .code,
        "async_validation_required"
    );
}
async fn pending(_: &String) -> Result<(), ValidationError> {
    log("pending");
    std::future::pending().await
}
#[derive(Model)]
struct Wait {
    #[model(async_validate=async_field)]
    first: String,
    #[model(async_validate=pending)]
    second: String,
    #[model(async_validate=async_field)]
    third: String,
}
#[test]
fn cancelling_after_completed_sibling_preserves_parent_context_and_skips_later_work() {
    reset();
    let m = Wait {
        first: "bad".into(),
        second: "wait".into(),
        third: "never".into(),
    };
    let mut ctx = Context::new();
    {
        let mut f = m.check_async(&mut ctx);
        let w = Waker::noop();
        assert!(Pin::as_mut(&mut f)
            .poll(&mut TaskContext::from_waker(w))
            .is_pending());
    }
    ctx.push(ValidationError::custom("root"));
    let errors = ctx.result().unwrap_err();
    assert_eq!(trace(), ["async:bad", "pending"]);
    assert_eq!(
        errors
            .errors()
            .iter()
            .map(|e| e.pointer())
            .collect::<Vec<_>>(),
        ["/first", ""]
    );
}
async fn inner_done(_: &NestedAsync) -> Result<(), ValidationErrors> {
    log("inner:model");
    Ok(())
}
#[derive(Debug, Model)]
#[model(async_validate=inner_done)]
struct NestedAsync {
    #[model(async_validate=async_field)]
    value: String,
}
async fn parent_field(_: &NestedAsync) -> Result<(), ValidationError> {
    log("outer:field");
    Ok(())
}
async fn outer_done(_: &AsyncOuter) -> Result<(), ValidationErrors> {
    log("outer:model");
    Ok(())
}
#[derive(Debug, Model)]
#[model(async_validate=outer_done)]
struct AsyncOuter {
    #[model(async_validate=parent_field)]
    nested: NestedAsync,
}
#[test]
fn nested_async_callbacks_run_inner_before_outer_and_bubble_paths() {
    reset();
    drive(AsyncOuter::parse_value_async(
        json!({"nested":{"value":"ok"}}),
    ))
    .unwrap();
    assert_eq!(
        trace(),
        ["async:ok", "inner:model", "outer:field", "outer:model"]
    );
    let e = drive(AsyncOuter::parse_value_async(
        json!({"nested":{"value":"bad"}}),
    ))
    .unwrap_err();
    assert_eq!(trace(), ["async:bad"]);
    assert_eq!(e.errors()[0].pointer(), "/nested/value");
}
async fn reject_variant(_: &String) -> Result<(), ValidationError> {
    log("first:async");
    Err(ValidationError::custom("first rejected"))
}
async fn accept_variant(_: &String) -> Result<(), ValidationError> {
    log("second:async");
    Ok(())
}
#[derive(Debug, Model)]
#[model(untagged)]
enum AsyncUnion {
    First(#[model(async_validate=reject_variant)] String),
    Second(#[model(async_validate=accept_variant)] String),
}
#[test]
fn untagged_union_does_not_retry_branches_after_async_failure() {
    reset();
    let e = drive(AsyncUnion::parse_value_async(json!("same shape"))).unwrap_err();
    assert_eq!(trace(), ["first:async"]);
    assert_eq!(e.errors()[0].message, "first rejected");
}
#[derive(Model)]
struct NestedWait {
    list: Vec<Wait>,
}
#[test]
fn cancellation_inside_nested_collection_does_not_leak_path_or_unabsorbed_child_errors() {
    reset();
    let m = NestedWait {
        list: vec![Wait {
            first: "bad".into(),
            second: "wait".into(),
            third: "never".into(),
        }],
    };
    let mut ctx = Context::new();
    ctx.push(ValidationError::custom("earlier"));
    {
        let mut f = m.check_async(&mut ctx);
        let w = Waker::noop();
        assert!(f
            .as_mut()
            .poll(&mut TaskContext::from_waker(w))
            .is_pending());
    }
    ctx.push(ValidationError::custom("after"));
    let e = ctx.result().unwrap_err();
    assert_eq!(e.len(), 2);
    assert_eq!(
        e.errors().iter().map(|e| e.pointer()).collect::<Vec<_>>(),
        ["", ""]
    );
    assert_eq!(trace(), ["async:bad", "pending"]);
}

#[derive(Debug, Model)]
struct InvalidDefault {
    #[model(default = "x", min_length = 3, validate = fail_custom)]
    value: String,
}
#[test]
fn invalid_defaults_use_the_construction_callback_gate() {
    for errors in [
        InvalidDefault::parse_json("{}").unwrap_err(),
        InvalidDefault::parse_value(json!({})).unwrap_err(),
    ] {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "too_short");
        assert_eq!(errors.errors()[0].pointer(), "/value");
        assert!(trace().is_empty());
    }
}

fn check_nested(_: &Gate) -> Result<(), ValidationError> {
    log("outer:sync");
    Ok(())
}
#[derive(Debug, Model)]
struct NestedGate {
    #[model(validate = check_nested)]
    inner: Gate,
    #[model(validate = sibling)]
    other: String,
}
#[test]
fn nested_failure_skips_enclosing_field_check_but_not_siblings_during_revalidation() {
    let errors = NestedGate {
        inner: Gate { value: "x".into() },
        other: "bad".into(),
    }
    .validate()
    .unwrap_err();
    assert_eq!(trace(), ["custom", "sibling"]);
    assert_eq!(
        errors
            .errors()
            .iter()
            .map(|e| e.pointer())
            .collect::<Vec<_>>(),
        ["/inner/value", "/inner/value", "/other"]
    );
}
