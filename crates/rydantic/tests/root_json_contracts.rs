use rydantic::{
    json, AsyncCheck, Context, Input, JsonType, Model, RawValue, SchemaContext, ValidationError,
    ValidationErrors, Value,
};
use std::{cell::RefCell, fmt::Debug, sync::Arc};

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn record(event: String) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

fn take_events() -> Vec<String> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

fn errors<T: Debug>(result: Result<T, ValidationErrors>) -> Value {
    result.unwrap_err().to_json()
}

fn syntax_errors(input: &str) -> Value {
    let error = serde_json::from_str::<&RawValue>(input).unwrap_err();
    json!([{
        "loc": [], "code": "json_invalid", "message": error.to_string(),
        "params": {"line": error.line(), "column": error.column()}, "stage": "parse"
    }])
}

// Custom raw decoders may consume a valid JSON value without constructing a DOM.
// The public parsing entry point, not each custom decoder, guarantees JSON syntax.
#[derive(Debug)]
struct AnyJson;

impl JsonType for AnyJson {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        Some(Self)
    }

    fn decode_input(_: Input<'_>, _: &mut Context, _: bool) -> Option<Self> {
        record("decode".into());
        Some(Self)
    }

    fn encode(&self, _: &mut Context) -> Value {
        Value::Null
    }

    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}

impl Model for AnyJson {}
impl AsyncCheck for AnyJson {}

const INVALID_JSON: [&str; 4] = ["", "not json", "null garbage", "[1,]"];

#[test]
fn custom_decoders_only_receive_complete_json_in_string_and_context_entry_points() {
    take_events();
    for input in INVALID_JSON {
        let expected = syntax_errors(input);
        assert_eq!(errors(AnyJson::parse_json(input)), expected);
        assert_eq!(
            errors(AnyJson::parse_json_with_context(input, Arc::new(()))),
            expected
        );
        assert_eq!(errors(AnyJson::parse_slice(input.as_bytes())), expected);
        assert!(take_events().is_empty(), "decoder ran for {input:?}");
    }
    AnyJson::parse_json("null").unwrap();
    AnyJson::parse_json_with_context("[]", Arc::new(())).unwrap();
    assert_eq!(take_events(), ["decode", "decode"]);
}

#[tokio::test]
async fn custom_decoders_only_receive_complete_json_in_async_entry_points() {
    take_events();
    for input in INVALID_JSON {
        let expected = syntax_errors(input);
        assert_eq!(errors(AnyJson::parse_json_async(input).await), expected);
        assert_eq!(
            errors(AnyJson::parse_json_async_with_context(input, Arc::new(())).await),
            expected
        );
        assert!(take_events().is_empty(), "decoder ran for {input:?}");
    }
    AnyJson::parse_json_async("null").await.unwrap();
    AnyJson::parse_json_async_with_context("[]", Arc::new(()))
        .await
        .unwrap();
    assert_eq!(take_events(), ["decode", "decode"]);
}

fn before(value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    record(format!("before:{value}:{}", ctx.error_count()));
    Ok(value)
}

#[derive(Debug, Model)]
struct FixedCallback {
    #[model(before_with_context = before)]
    value: i64,
}

#[derive(Debug, Model)]
struct Item {
    #[model(before_with_context = before, ge = 0)]
    value: i64,
}

#[derive(Debug, Model)]
struct Items(Vec<Item>);

const INVALID_ARRAYS: [&str; 3] = [
    r#"[{"value":1},"#,
    r#"[{"value":-1},"#,
    r#"[{"value":1}] false"#,
];

#[test]
fn incomplete_or_trailing_array_input_never_runs_callbacks_or_constraint_checks() {
    take_events();
    for input in INVALID_ARRAYS {
        let expected = syntax_errors(input);
        assert_eq!(errors(Items::parse_json(input)), expected);
        assert_eq!(
            errors(Items::parse_json_with_context(input, Arc::new(()))),
            expected
        );
        assert_eq!(errors(Items::parse_slice(input.as_bytes())), expected);
        assert!(take_events().is_empty(), "callback ran for {input:?}");
    }
    Items::parse_json(r#"[{"value":1}]"#).unwrap();
    assert_eq!(take_events(), ["before:1:0"]);
}

#[tokio::test]
async fn incomplete_or_trailing_arrays_remain_syntax_only_failures_in_async_parsing() {
    take_events();
    for input in INVALID_ARRAYS {
        let expected = syntax_errors(input);
        assert_eq!(errors(Items::parse_json_async(input).await), expected);
        assert_eq!(
            errors(Items::parse_json_async_with_context(input, Arc::new(())).await),
            expected
        );
        assert!(take_events().is_empty(), "callback ran for {input:?}");
    }
    Items::parse_json_async(r#"[{"value":1}]"#).await.unwrap();
    assert_eq!(take_events(), ["before:1:0"]);
}

#[derive(Debug, Model)]
struct Object {
    value: i64,
}

#[derive(Debug, Model)]
struct Unit;

#[tokio::test]
async fn fixed_slot_json_stages_complete_syntax_before_callbacks() {
    for input in [r#"{"value":1,"tail":[}"#, r#"{"value":1} false"#] {
        let expected = syntax_errors(input);
        assert_eq!(errors(FixedCallback::parse_json(input)), expected);
        assert_eq!(
            errors(FixedCallback::parse_json_with_context(input, Arc::new(()))),
            expected
        );
        assert_eq!(
            errors(FixedCallback::parse_json_async(input).await),
            expected
        );
        assert_eq!(
            errors(FixedCallback::parse_json_async_with_context(input, Arc::new(())).await),
            expected
        );
        assert!(take_events().is_empty(), "callback ran for {input:?}");
    }

    FixedCallback::parse_json(r#"{"value":1}"#).unwrap();
    assert_eq!(take_events(), ["before:1:0"]);
    FixedCallback::parse_json_async_with_context(r#"{"value":1}"#, Arc::new(()))
        .await
        .unwrap();
    assert_eq!(take_events(), ["before:1:0"]);
}

#[test]
fn malformed_wrong_shape_roots_keep_full_json_errors_before_type_checks() {
    for input in INVALID_JSON {
        let expected = syntax_errors(input);
        assert_eq!(errors(Object::parse_json(input)), expected);
        assert_eq!(errors(Object::parse_slice(input.as_bytes())), expected);
        assert_eq!(errors(Unit::parse_json(input)), expected);
        assert_eq!(errors(Unit::parse_slice(input.as_bytes())), expected);
    }
    assert_eq!(
        errors(Object::parse_json("[]")),
        json!([{"loc":[],"code":"type_error","message":"expected object","stage":"parse"}])
    );
    assert_eq!(
        errors(Unit::parse_json("{}")),
        json!([{"loc":[],"code":"type_error","message":"expected null","stage":"parse"}])
    );
}

#[test]
fn decoded_invalid_object_keys_retain_the_existing_raw_object_error_mapping() {
    // RawValue accepts these escaped keys; decoding the object key then fails.
    // Preserve the established object-boundary error rather than reclassifying it.
    for input in [r#"{"value":1,"\uD800":0}"#, r#"{"value":1,"\uDC00":0}"#] {
        let raw = serde_json::from_str::<&RawValue>(input).unwrap();
        let expected = json!([
            {"loc":[],"code":"type_error","message":"expected object","stage":"parse"}
        ]);
        assert_eq!(errors(Object::parse_json(input)), expected);
        assert_eq!(errors(Object::parse_slice(input.as_bytes())), expected);
        assert_eq!(errors(Object::parse_raw(raw)), expected);
    }
}

#[derive(Debug, Model)]
struct Opaque(Box<RawValue>);

#[test]
fn opaque_newtype_roots_keep_raw_whitespace_and_duplicate_members() {
    for input in [r#"{ "n":1e+02, "x":1, "x":2 }"#, "[ 1, 2 ]"] {
        assert_eq!(Opaque::parse_json(input).unwrap().0.get(), input);
        assert_eq!(
            Opaque::parse_json_with_context(input, Arc::new(()))
                .unwrap()
                .0
                .get(),
            input
        );
        assert_eq!(
            Opaque::parse_slice(input.as_bytes()).unwrap().0.get(),
            input
        );
    }
}

#[tokio::test]
async fn opaque_newtype_roots_keep_raw_text_in_async_entry_points() {
    let input = r#"{ "n":1e+02, "x":1, "x":2 }"#;
    assert_eq!(
        Opaque::parse_json_async(input).await.unwrap().0.get(),
        input
    );
    assert_eq!(
        Opaque::parse_json_async_with_context(input, Arc::new(()))
            .await
            .unwrap()
            .0
            .get(),
        input
    );
}
