use rydantic::{
    json, AsyncCheck, AsyncValidated, Context, JsonType, Model, PathSegment, SchemaContext, Stage,
    Validated, ValidationError, ValidationErrors, Value,
};
use std::io::{self, Read};

#[test]
fn error_paths_escape_json_pointer_tokens_without_losing_segment_types() {
    let error = ValidationError::custom("invalid")
        .at("")
        .at("a/b~c")
        .at("0".to_owned())
        .at(0usize);
    assert_eq!(error.pointer(), "//a~1b~0c/0/0");
    assert_eq!(
        error.loc,
        vec![
            PathSegment::Field(String::new()),
            PathSegment::Field("a/b~c".into()),
            PathSegment::Field("0".into()),
            PathSegment::Index(0),
        ]
    );
    let encoded = serde_json::to_value(&error).unwrap();
    assert_eq!(encoded["loc"], json!(["", "a/b~c", "0", 0]));
    assert_eq!(
        serde_json::from_value::<ValidationError>(encoded).unwrap(),
        error
    );
}

#[test]
fn error_serde_defaults_omitted_params_and_preserves_explicit_metadata() {
    let error = ValidationError::new("type_error", "expected integer");
    let encoded = serde_json::to_value(&error).unwrap();
    assert!(encoded.get("params").is_none());
    assert_eq!(encoded["stage"], "validate");
    assert_eq!(
        serde_json::from_value::<ValidationError>(encoded).unwrap(),
        error
    );

    for stage in [Stage::Parse, Stage::Validate, Stage::Serialize] {
        let errors = ValidationErrors::new(vec![error
            .clone()
            .with_stage(stage)
            .with_param("expected", "old")
            .with_param("expected", "integer")
            .with_param("details", json!({"minimum": 0}))]);
        let encoded = errors.to_json();
        assert!(encoded.is_array());
        assert_eq!(encoded[0]["params"]["expected"], "integer");
        assert_eq!(
            serde_json::from_value::<ValidationErrors>(encoded).unwrap(),
            errors
        );
    }
}

#[test]
fn empty_and_multiple_error_collections_have_consistent_accessors_and_display() {
    let empty = ValidationErrors::new(vec![]);
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.errors().is_empty());
    assert_eq!(empty.to_json(), json!([]));
    assert_eq!(empty.to_string(), "0 validation error(s)\n");

    let errors = ValidationErrors::new(vec![
        ValidationError::custom("root"),
        ValidationError::new("missing", "required").at("value"),
    ]);
    assert!(!errors.is_empty());
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors.to_string(),
        "2 validation error(s)\n$: root [custom]\n$/value: required [missing]"
    );
}

#[test]
fn context_scopes_append_relative_locations_and_restore_parent_paths() {
    let mut ctx = Context::new();
    let returned = ctx.at("outer", |ctx| {
        ctx.at(2usize, |ctx| {
            ctx.push(ValidationError::custom("nested").at("leaf"));
        });
        ctx.push(ValidationError::custom("sibling"));
        42
    });
    assert_eq!(returned, 42);
    ctx.push(ValidationError::custom("root"));
    assert_eq!(ctx.error_count(), 3);
    let errors = ctx.result().unwrap_err();
    assert_eq!(
        errors
            .errors()
            .iter()
            .map(ValidationError::pointer)
            .collect::<Vec<_>>(),
        ["/outer/2/leaf", "/outer", ""]
    );
}

#[test]
fn child_contexts_inherit_paths_not_errors_and_absorb_without_reprefixing() {
    let mut ctx = Context::new();
    ctx.at("outer", |ctx| {
        ctx.push(ValidationError::custom("parent"));
        let mut inherited = ctx.child(None);
        let mut indexed = ctx.child(Some(3usize.into()));
        assert_eq!(inherited.error_count(), 0);
        assert_eq!(indexed.error_count(), 0);
        inherited.push(ValidationError::custom("inherited").at("field"));
        indexed.push(ValidationError::custom("indexed").at("field"));
        assert_eq!(ctx.error_count(), 1);
        ctx.absorb(inherited);
        ctx.absorb(indexed);
        assert_eq!(ctx.error_count(), 3);
    });
    let errors = ctx.result().unwrap_err();
    assert_eq!(
        errors
            .errors()
            .iter()
            .map(ValidationError::pointer)
            .collect::<Vec<_>>(),
        ["/outer", "/outer/field", "/outer/3/field"]
    );
}

#[test]
fn extending_and_prepending_errors_preserves_order_stage_and_params() {
    let errors = ValidationErrors::new(vec![
        ValidationError::new("first", "first failure")
            .at("leaf")
            .with_stage(Stage::Serialize)
            .with_param("limit", 2),
        ValidationError::new("second", "second failure").at(1usize),
    ]);
    let prefixed = Context::prepend_errors(errors.clone(), "branch");
    let mut ctx = Context::new();
    ctx.at("outer", |ctx| ctx.extend(prefixed));
    let actual = ctx.result().unwrap_err();
    let expected = Context::prepend_errors(Context::prepend_errors(errors, "branch"), "outer");
    assert_eq!(actual, expected);
    assert_eq!(actual.errors()[0].pointer(), "/outer/branch/leaf");
    assert_eq!(actual.errors()[0].stage, Stage::Serialize);
    assert_eq!(actual.errors()[0].params["limit"], 2);
    assert_eq!(actual.errors()[1].pointer(), "/outer/branch/1");
    assert!(Context::prepend_errors(ValidationErrors::new(vec![]), "unused").is_empty());
}

#[test]
fn context_finish_prioritizes_recorded_errors_over_decoded_values() {
    assert_eq!(Context::new().finish(Some(42)).unwrap(), 42);
    assert!(Context::new().result().is_ok());
    for value in [None, Some(42)] {
        let mut ctx = Context::new();
        let error = ValidationError::custom("rejected");
        ctx.push(error.clone());
        assert_eq!(
            ctx.finish(value).unwrap_err(),
            ValidationErrors::from(error)
        );
    }
    let errors = Context::new().finish::<()>(None).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, "internal");
    assert_eq!(errors.errors()[0].stage, Stage::Validate);
    assert_eq!(errors.errors()[0].pointer(), "");
}

// Deliberately violates the extension-point contract to exercise the public
// entry points' defensive guard against decoders returning no value or error.
#[derive(Debug)]
struct SilentDecoder;
impl JsonType for SilentDecoder {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        None
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::Null
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}
impl Model for SilentDecoder {}
impl AsyncCheck for SilentDecoder {}

#[test]
fn model_entry_points_report_silent_decoder_failures_instead_of_succeeding() {
    let raw = serde_json::value::RawValue::from_string("null".into()).unwrap();
    let errors = [
        SilentDecoder::parse_value(Value::Null).unwrap_err(),
        SilentDecoder::parse_json("null").unwrap_err(),
        SilentDecoder::parse_slice(b"null").unwrap_err(),
        SilentDecoder::parse_reader(&b"null"[..]).unwrap_err(),
        SilentDecoder::parse_raw(&raw).unwrap_err(),
    ];
    for errors in errors {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "internal");
    }
    let error = serde_json::from_str::<Validated<SilentDecoder>>("null").unwrap_err();
    assert!(error.to_string().contains("internal"));
}

#[tokio::test]
async fn asynchronous_entry_points_also_guard_silent_decoder_failures() {
    let raw = serde_json::value::RawValue::from_string("null".into()).unwrap();
    let errors = [
        SilentDecoder::parse_value_async(Value::Null)
            .await
            .unwrap_err(),
        SilentDecoder::parse_json_async("null").await.unwrap_err(),
        SilentDecoder::parse_raw_async(&raw).await.unwrap_err(),
    ];
    for errors in errors {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "internal");
    }
}

#[derive(Debug)]
struct RejectingEncoder {
    invalid: bool,
}
impl JsonType for RejectingEncoder {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        Some(Self { invalid: false })
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        assert!(
            !self.invalid,
            "invalid models must be rejected before encoding"
        );
        ctx.push(
            ValidationError::new("encode_failure", "cannot encode")
                .with_stage(Stage::Serialize)
                .at("payload"),
        );
        json!({"not_a_success": true})
    }
    fn validate_into(&self, ctx: &mut Context) {
        if self.invalid {
            ctx.push(ValidationError::new("invalid_state", "cannot validate"));
        }
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({})
    }
}
impl Model for RejectingEncoder {}
impl AsyncCheck for RejectingEncoder {}

#[test]
fn dumps_and_validated_serde_propagate_custom_encoder_failures() {
    let model = RejectingEncoder { invalid: false };
    assert!(model.validate().is_ok());
    for errors in [model.dump().unwrap_err(), model.dump_json().unwrap_err()] {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "encode_failure");
        assert_eq!(errors.errors()[0].stage, Stage::Serialize);
        assert_eq!(errors.errors()[0].pointer(), "/payload");
    }
    let checked = Validated::new(model).unwrap();
    assert!(!checked.get().invalid);
    let error = serde_json::to_value(&checked).unwrap_err();
    assert!(error.to_string().contains("encode_failure"));
    assert!(error.to_string().contains("/payload"));
}

#[test]
fn invalid_models_are_rejected_before_the_encoder_runs() {
    let model = RejectingEncoder { invalid: true };
    for errors in [model.dump().unwrap_err(), model.dump_json().unwrap_err()] {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "invalid_state");
        assert_eq!(errors.errors()[0].stage, Stage::Validate);
    }
    assert!(Validated::new(model).is_err());
}

#[tokio::test]
async fn async_dump_and_async_validated_serde_propagate_encoder_failures() {
    let model = RejectingEncoder { invalid: false };
    for errors in [
        model.dump_async().await.unwrap_err(),
        model.dump_json_async().await.unwrap_err(),
    ] {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "encode_failure");
        assert_eq!(errors.errors()[0].stage, Stage::Serialize);
    }
    let checked = AsyncValidated::new(model).await.unwrap();
    assert!(!checked.get().invalid);
    let error = serde_json::to_value(&checked).unwrap_err();
    assert!(error.to_string().contains("encode_failure"));
    assert!(!checked.into_inner().invalid);

    let invalid = RejectingEncoder { invalid: true };
    let errors = invalid.dump_async().await.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, "invalid_state");
    assert!(AsyncValidated::new(invalid).await.is_err());
}

struct FailingReader;
impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("reader unavailable"))
    }
}

#[test]
fn parse_reader_reports_io_failures_as_parse_errors_before_decoding() {
    let errors = SilentDecoder::parse_reader(FailingReader).unwrap_err();
    assert_eq!(errors.len(), 1);
    let error = &errors.errors()[0];
    assert_eq!(error.code, "json_invalid");
    assert_eq!(error.stage, Stage::Parse);
    assert_eq!(error.pointer(), "");
    assert!(error.message.contains("reader unavailable"));
    assert!(error.params["line"].is_number());
    assert!(error.params["column"].is_number());
}

#[test]
fn slice_parsing_rejects_invalid_utf8_and_trailing_values_before_decoding() {
    for input in [&b"\"\xff\""[..], &b"null null"[..]] {
        let errors = SilentDecoder::parse_slice(input).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.errors()[0].code, "json_invalid");
        assert_eq!(errors.errors()[0].stage, Stage::Parse);
    }
}
