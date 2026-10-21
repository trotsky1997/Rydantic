use rydantic::{json, Model, ValidationError, ValidationErrors, Value};
thread_local! { static TRACE: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn log(s: &'static str) {
    TRACE.with(|t| t.borrow_mut().push(s));
}
fn trace() -> Vec<&'static str> {
    TRACE.with(|t| std::mem::take(&mut *t.borrow_mut()))
}
fn raw(v: Value) -> Result<Value, ValidationError> {
    log("before1");
    Ok(json!(v.as_i64().unwrap() + 1))
}
fn value(v: Value) -> Result<Value, ValidationError> {
    log("before2");
    Ok(json!(v.as_i64().unwrap() * 2))
}
fn child(_: &i32) -> Result<(), ValidationError> {
    log("check1");
    Ok(())
}
fn start(_: &i32) -> Result<(), ValidationError> {
    log("check2");
    Ok(())
}
fn first(_: &Ordered) -> Result<(), ValidationErrors> {
    log("model1");
    Ok(())
}
fn second(_: &Ordered) -> Result<(), ValidationErrors> {
    log("model2");
    Ok(())
}
#[derive(Debug, Model)]
#[model(validate=[first,second])]
struct Ordered {
    #[model(before=[raw,value],validate=[child,start],default=2)]
    x: i32,
}
#[test]
fn ordered_transforms_checks_and_defaults() {
    for input in [json!({"x":2}), json!({})] {
        let m = Ordered::parse_value(input).unwrap();
        assert_eq!(m.x, 6);
        assert_eq!(
            trace(),
            ["before1", "before2", "check1", "check2", "model1", "model2"]
        );
        m.validate().unwrap();
        assert_eq!(trace(), ["check1", "check2", "model1", "model2"]);
    }
}
fn fail1(_: &i32) -> Result<(), ValidationError> {
    log("fail1");
    Err(ValidationError::new("one", "one"))
}
fn fail2(_: &i32) -> Result<(), ValidationError> {
    log("fail2");
    Err(ValidationError::new("two", "two"))
}
#[derive(Debug, Model)]
struct Fail {
    #[model(validate=[fail1,fail2])]
    a: i32,
    #[model(validate=[fail2,fail1])]
    b: i32,
}
#[test]
fn field_errors_accumulate_in_callback_then_field_order() {
    let e = Fail::parse_value(json!({"a":1,"b":2})).unwrap_err();
    assert_eq!(
        e.errors()
            .iter()
            .map(|e| e.code.as_str())
            .collect::<Vec<_>>(),
        ["one", "two", "two", "one"]
    );
    assert_eq!(trace(), ["fail1", "fail2", "fail2", "fail1"]);
    assert_eq!(e.errors()[0].loc, e.errors()[1].loc);
    assert_ne!(e.errors()[1].loc, e.errors()[2].loc);
}
fn stop(_: Value) -> Result<Value, ValidationError> {
    log("stop");
    Err(ValidationError::new("stop", "stop"))
}
#[derive(Debug, Model)]
struct Stops {
    #[model(before=[stop,raw])]
    a: i32,
    #[model(validate=[child,start])]
    b: i32,
}
#[test]
fn before_fails_fast_but_sibling_checks_continue() {
    assert!(Stops::parse_value(json!({"a":1,"b":2})).is_err());
    assert_eq!(trace(), ["stop", "check1", "check2"]);
}
async fn async1(_: &i32) -> Result<(), ValidationError> {
    log("async1");
    Err(ValidationError::new("async1", "one"))
}
async fn async2(_: &i32) -> Result<(), ValidationError> {
    log("async2");
    Err(ValidationError::new("async2", "two"))
}
#[derive(Debug, Model)]
struct Async {
    #[model(async_validate=[async1,async2])]
    a: i32,
}
fn drive<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut c = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut c) {
        std::task::Poll::Ready(v) => v,
        _ => panic!("unexpected pending"),
    }
}
#[test]
fn async_checks_accumulate_sequentially() {
    let e = drive(Async::parse_value_async(json!({"a":1}))).unwrap_err();
    assert_eq!(trace(), ["async1", "async2"]);
    assert_eq!(e.len(), 2);
}
fn model_fail1(_: &ModelFail) -> Result<(), ValidationErrors> {
    log("model1");
    Err(ValidationErrors(vec![ValidationError::new(
        "model1", "one",
    )]))
}
fn model_fail2(_: &ModelFail) -> Result<(), ValidationErrors> {
    log("model2");
    Err(ValidationErrors(vec![ValidationError::new(
        "model2", "two",
    )]))
}
#[derive(Debug, Model)]
#[model(validate=[model_fail1,model_fail2])]
struct ModelFail {
    a: i32,
}
#[test]
fn model_checks_accumulate_after_fields_succeed() {
    let e = ModelFail::parse_value(json!({"a":1})).unwrap_err();
    assert_eq!(e.len(), 2);
    assert_eq!(trace(), ["model1", "model2"]);
    assert!(ModelFail::parse_value(json!({"a":"bad"})).is_err());
    assert!(trace().is_empty());
}
async fn model_async1(_: &AsyncModel) -> Result<(), ValidationErrors> {
    log("model_async1");
    Err(ValidationErrors(vec![ValidationError::new(
        "model_async1",
        "one",
    )]))
}
async fn model_async2(_: &AsyncModel) -> Result<(), ValidationErrors> {
    log("model_async2");
    Err(ValidationErrors(vec![ValidationError::new(
        "model_async2",
        "two",
    )]))
}
#[derive(Debug, Model)]
#[model(async_validate=[model_async1,model_async2])]
struct AsyncModel {
    a: i32,
}
#[test]
fn model_async_checks_accumulate() {
    let e = drive(AsyncModel::parse_value_async(json!({"a":1}))).unwrap_err();
    assert_eq!(e.len(), 2);
    assert_eq!(trace(), ["model_async1", "model_async2"]);
}
#[derive(Debug, Model)]
struct Gate {
    #[model(ge=5,validate=[fail1,fail2])]
    a: i32,
}
#[test]
fn list_preserves_parse_vs_revalidate_gate() {
    assert_eq!(Gate::parse_value(json!({"a":1})).unwrap_err().len(), 1);
    assert!(trace().is_empty());
    assert_eq!(Gate { a: 1 }.validate().unwrap_err().len(), 3);
    assert_eq!(trace(), ["fail1", "fail2"]);
}
#[test]
fn input_schema_retains_all_before_annotations() {
    let schema = Ordered::json_schema(rydantic::SchemaMode::Input);
    let text = schema.to_string();
    assert!(text.contains("before:raw"));
    assert!(text.contains("before:value"));
}
