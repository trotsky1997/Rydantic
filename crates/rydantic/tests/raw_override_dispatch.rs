//! Custom raw-entry policies must not be bypassed by optimized JSON staging.
use rydantic::{
    json, AsyncCheck, Context, Input, JsonType, Model, RawValue, SchemaContext, ValidationError,
    ValidationErrors, Value,
};
use std::{
    any::Any,
    cell::RefCell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

thread_local! { static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
fn record(event: String) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}
fn events() -> Vec<String> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}
#[derive(Debug, PartialEq)]
struct PolicyModel(&'static str);
impl JsonType for PolicyModel {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        Some(Self("unexpected decode"))
    }
    fn decode_input(_: Input<'_>, _: &mut Context, _: bool) -> Option<Self> {
        record("unexpected decode".into());
        Some(Self("unexpected decode"))
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::Null
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}
impl AsyncCheck for PolicyModel {}
fn policy(raw: &RawValue, route: &'static str) -> Result<PolicyModel, ValidationErrors> {
    record(format!("{route}:{}", raw.get()));
    if raw.get() == "null" {
        Err(
            ValidationError::new("raw_policy", "custom raw policy rejected input")
                .at("policy/~")
                .with_param("route", route)
                .into(),
        )
    } else {
        Ok(PolicyModel(route))
    }
}
fn context_policy<C: Any + Send + Sync>(
    raw: &RawValue,
    context: Arc<C>,
    route: &'static str,
) -> Result<PolicyModel, ValidationErrors> {
    let ctx = Context::with_data(context);
    ctx.require_data::<AtomicUsize>()?
        .fetch_add(1, Ordering::SeqCst);
    policy(raw, route)
}
impl Model for PolicyModel {
    fn parse_raw(raw: &RawValue) -> Result<Self, ValidationErrors> {
        policy(raw, "sync")
    }
    fn parse_raw_with_context<C: Any + Send + Sync>(
        raw: &RawValue,
        context: Arc<C>,
    ) -> Result<Self, ValidationErrors> {
        context_policy(raw, context, "context")
    }
    async fn parse_raw_async(raw: &RawValue) -> Result<Self, ValidationErrors> {
        policy(raw, "async")
    }
    async fn parse_raw_async_with_context<C: Any + Send + Sync>(
        raw: &RawValue,
        context: Arc<C>,
    ) -> Result<Self, ValidationErrors> {
        context_policy(raw, context, "async_context")
    }
}
fn expected_error(route: &str) -> Value {
    json!([{"loc":["policy/~"],"code":"raw_policy","message":"custom raw policy rejected input",
        "stage":"validate","params":{"route":route}}])
}
const VALID: &str = r#"{ "n":1e+02, "x":1, "x":2 }"#;
const INVALID: [&str; 4] = ["", "not json", "null garbage", "[1,]"];
fn syntax_error(text: &str) -> Value {
    let error = serde_json::from_str::<&RawValue>(text).unwrap_err();
    json!([{"loc":[],"code":"json_invalid","message":error.to_string(),"stage":"parse",
        "params":{"line":error.line(),"column":error.column()}}])
}
#[test]
fn json_preserves_sync_raw_policy_and_slice_control() {
    events();
    assert_eq!(PolicyModel::parse_json(VALID).unwrap(), PolicyModel("sync"));
    assert_eq!(
        PolicyModel::parse_slice(VALID.as_bytes()).unwrap(),
        PolicyModel("sync")
    );
    assert_eq!(
        PolicyModel::parse_json("null").unwrap_err().to_json(),
        expected_error("sync")
    );
    assert_eq!(
        PolicyModel::parse_slice(b"null").unwrap_err().to_json(),
        expected_error("sync")
    );
    assert_eq!(
        events(),
        [
            format!("sync:{VALID}"),
            format!("sync:{VALID}"),
            "sync:null".into(),
            "sync:null".into()
        ]
    );
}
#[test]
fn json_preserves_context_raw_policy_and_shared_context_identity() {
    events();
    let context = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        PolicyModel::parse_json_with_context(VALID, context.clone()).unwrap(),
        PolicyModel("context")
    );
    assert_eq!(
        PolicyModel::parse_slice_with_context(VALID.as_bytes(), context.clone()).unwrap(),
        PolicyModel("context")
    );
    assert_eq!(
        PolicyModel::parse_json_with_context("null", context.clone())
            .unwrap_err()
            .to_json(),
        expected_error("context")
    );
    assert_eq!(
        PolicyModel::parse_slice_with_context(b"null", context.clone())
            .unwrap_err()
            .to_json(),
        expected_error("context")
    );
    assert_eq!(context.load(Ordering::SeqCst), 4);
    assert_eq!(
        events(),
        [
            format!("context:{VALID}"),
            format!("context:{VALID}"),
            "context:null".into(),
            "context:null".into()
        ]
    );
}
#[tokio::test]
async fn json_preserves_both_async_raw_policies_without_duplicate_callbacks() {
    events();
    let context = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        PolicyModel::parse_json_async(VALID).await.unwrap(),
        PolicyModel("async")
    );
    assert_eq!(
        PolicyModel::parse_json_async("null")
            .await
            .unwrap_err()
            .to_json(),
        expected_error("async")
    );
    assert_eq!(
        PolicyModel::parse_json_async_with_context(VALID, context.clone())
            .await
            .unwrap(),
        PolicyModel("async_context")
    );
    assert_eq!(
        PolicyModel::parse_json_async_with_context("null", context.clone())
            .await
            .unwrap_err()
            .to_json(),
        expected_error("async_context")
    );
    assert_eq!(context.load(Ordering::SeqCst), 2);
    assert_eq!(
        events(),
        [
            format!("async:{VALID}"),
            "async:null".into(),
            format!("async_context:{VALID}"),
            "async_context:null".into()
        ]
    );
}
#[tokio::test]
async fn malformed_json_never_dispatches_any_raw_override() {
    events();
    let context = Arc::new(AtomicUsize::new(0));
    for text in INVALID {
        let expected = syntax_error(text);
        assert_eq!(
            PolicyModel::parse_json(text).unwrap_err().to_json(),
            expected
        );
        assert_eq!(
            PolicyModel::parse_json_with_context(text, context.clone())
                .unwrap_err()
                .to_json(),
            expected
        );
        assert_eq!(
            PolicyModel::parse_json_async(text)
                .await
                .unwrap_err()
                .to_json(),
            expected
        );
        assert_eq!(
            PolicyModel::parse_json_async_with_context(text, context.clone())
                .await
                .unwrap_err()
                .to_json(),
            expected
        );
        assert_eq!(
            PolicyModel::parse_slice(text.as_bytes())
                .unwrap_err()
                .to_json(),
            expected
        );
        assert_eq!(
            PolicyModel::parse_slice_with_context(text.as_bytes(), context.clone())
                .unwrap_err()
                .to_json(),
            expected
        );
    }
    assert!(events().is_empty());
    assert_eq!(context.load(Ordering::SeqCst), 0);
}
