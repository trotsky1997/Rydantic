use rydantic::{
    json, AsyncCheck, Context, JsonType, Model, SchemaContext, SchemaMode, Stage, ValidationError,
    ValidationErrors, Value,
};
use std::collections::{BTreeMap, HashMap};

// A custom codec with deliberately different input and output representations.
// Zero is invalid; 99 is valid in memory but cannot be exported.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WireId(u32);
impl AsyncCheck for WireId {}
impl JsonType for WireId {
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let text = String::decode(value, ctx, coerce)?;
        match text.strip_prefix("id:").and_then(|s| s.parse().ok()) {
            Some(value) if value != 0 => Some(Self(value)),
            _ => {
                ctx.push(ValidationError::new(
                    "invalid_id",
                    "expected a positive id:N",
                ));
                None
            }
        }
    }
    fn encode(&self, ctx: &mut Context) -> Value {
        if self.0 == 99 {
            ctx.push(
                ValidationError::new("reserved_id", "this identifier cannot be exported")
                    .with_stage(Stage::Serialize),
            );
        }
        json!(self.0)
    }
    fn encode_input(&self, _: &mut Context) -> Value {
        json!(format!("id:{}", self.0))
    }
    fn validate_into(&self, ctx: &mut Context) {
        if self.0 == 0 {
            ctx.push(ValidationError::new(
                "invalid_id",
                "expected a positive id:N",
            ));
        }
    }
    fn schema(ctx: &mut SchemaContext, _: bool) -> Value {
        match ctx.mode {
            SchemaMode::Input => json!({"type":"string", "pattern":"^id:[1-9][0-9]*$"}),
            SchemaMode::Output => json!({"type":"integer", "minimum":1}),
        }
    }
}

#[derive(Debug, PartialEq, Model)]
struct Collections {
    list: Vec<WireId>,
    array: [WireId; 2],
    ordered: BTreeMap<String, WireId>,
    hashed: HashMap<String, WireId>,
    boxed: Box<Option<WireId>>,
    optional: Option<Box<WireId>>,
}

fn collections(id: u32) -> Collections {
    Collections {
        list: vec![WireId(id), WireId(id)],
        array: [WireId(id), WireId(id)],
        ordered: [("a/b~c".into(), WireId(id)), ("z".into(), WireId(id))].into(),
        hashed: [("a/b~c".into(), WireId(id)), ("z".into(), WireId(id))].into(),
        boxed: Box::new(Some(WireId(id))),
        optional: Some(Box::new(WireId(id))),
    }
}

fn shape(value: Value) -> Value {
    json!({
        "list":[value, value], "array":[value, value],
        "ordered":{"a/b~c":value, "z":value},
        "hashed":{"a/b~c":value, "z":value},
        "boxed":value, "optional":value
    })
}

fn assert_member_errors(errors: ValidationErrors, prefix: &str, code: &str, stage: Stage) {
    let mut actual: Vec<_> = errors
        .errors()
        .iter()
        .map(|error| {
            assert_eq!(error.code, code);
            assert_eq!(error.stage, stage);
            error.pointer()
        })
        .collect();
    let mut expected: Vec<_> = [
        "/list/0",
        "/list/1",
        "/array/0",
        "/array/1",
        "/ordered/a~1b~0c",
        "/ordered/z",
        "/hashed/a~1b~0c",
        "/hashed/z",
        "/boxed",
        "/optional",
    ]
    .map(|path| format!("{prefix}{path}"))
    .into();
    // HashMap validation and serialization must not promise an iteration order.
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

fn defaults() -> Collections {
    collections(7)
}
fn invalid_defaults() -> Collections {
    collections(0)
}

#[derive(Debug, Model)]
struct Defaults {
    #[model(default_factory = defaults)]
    data: Collections,
}
#[derive(Debug, Model)]
struct InvalidDefaults {
    #[model(default_factory = invalid_defaults)]
    data: Collections,
}

#[test]
fn typed_defaults_recurse_without_substituting_output_for_input() {
    for parsed in [Defaults::parse_json("{}"), Defaults::parse_value(json!({}))] {
        let parsed = parsed.unwrap();
        assert_eq!(parsed.data, collections(7));
        assert_eq!(parsed.dump().unwrap(), json!({"data":shape(json!(7))}));
    }
}

#[test]
fn invalid_typed_defaults_collect_all_member_errors() {
    for result in [
        InvalidDefaults::parse_json("{}"),
        InvalidDefaults::parse_value(json!({})),
    ] {
        assert_member_errors(result.unwrap_err(), "/data", "invalid_id", Stage::Validate);
    }
}

fn inspect_default_input(value: Value) -> Result<Value, ValidationError> {
    assert_eq!(value, shape(json!("id:7")));
    Ok(value)
}
#[derive(Debug, Model)]
struct DefaultsWithBefore {
    #[model(default_factory = defaults, before = inspect_default_input)]
    data: Collections,
}

#[test]
fn default_before_callbacks_receive_each_members_input_representation() {
    for result in [
        DefaultsWithBefore::parse_json("{}"),
        DefaultsWithBefore::parse_value(json!({})),
    ] {
        assert_eq!(result.unwrap().data, collections(7));
    }
}

#[test]
fn collection_input_and_output_keep_distinct_custom_representations() {
    let input = shape(json!("id:7"));
    let parsed = Collections::parse_value(input.clone()).unwrap();
    assert_eq!(Collections::parse_json(&input.to_string()).unwrap(), parsed);
    assert_eq!(parsed.dump().unwrap(), shape(json!(7)));
    let input_schema =
        jsonschema::validator_for(&Collections::json_schema(SchemaMode::Input)).unwrap();
    let output_schema =
        jsonschema::validator_for(&Collections::json_schema(SchemaMode::Output)).unwrap();
    assert!(input_schema.is_valid(&input));
    assert!(!input_schema.is_valid(&parsed.dump().unwrap()));
    assert!(output_schema.is_valid(&parsed.dump().unwrap()));
    assert!(!output_schema.is_valid(&input));
}

#[test]
fn rejected_members_do_not_hide_sibling_errors_in_either_parse_path() {
    let input = shape(json!("id:0"));
    for result in [
        Collections::parse_json(&input.to_string()),
        Collections::parse_value(input),
    ] {
        assert_member_errors(result.unwrap_err(), "", "invalid_id", Stage::Validate);
    }
}

#[test]
fn validation_and_dump_reject_invalid_members_created_in_rust() {
    let value = collections(0);
    assert_member_errors(
        value.validate().unwrap_err(),
        "",
        "invalid_id",
        Stage::Validate,
    );
    assert_member_errors(value.dump().unwrap_err(), "", "invalid_id", Stage::Validate);
}

#[test]
fn custom_encoder_errors_keep_all_collection_member_locations() {
    let value = collections(99);
    value.validate().unwrap();
    assert_member_errors(
        value.dump().unwrap_err(),
        "",
        "reserved_id",
        Stage::Serialize,
    );
}

#[test]
fn fixed_arrays_enforce_exact_length_including_zero() {
    #[derive(Debug, Model)]
    struct Fixed {
        empty: [WireId; 0],
        pair: [WireId; 2],
    }
    let valid = json!({"empty":[], "pair":["id:1", "id:2"]});
    for result in [
        Fixed::parse_json(&valid.to_string()),
        Fixed::parse_value(valid),
    ] {
        assert_eq!(
            result.unwrap().dump().unwrap(),
            json!({"empty":[], "pair":[1, 2]})
        );
    }
    for (empty, pair) in [
        (json!([]), json!(["id:1"])),
        (json!([]), json!(["id:1", "id:2", "id:3"])),
        (json!(["id:1"]), json!(["id:1", "id:2"])),
    ] {
        let (path, expected, actual) = if empty.as_array().unwrap().is_empty() {
            ("/pair", 2, pair.as_array().unwrap().len())
        } else {
            ("/empty", 0, 1)
        };
        let input = json!({"empty":empty, "pair":pair});
        for result in [
            Fixed::parse_json(&input.to_string()),
            Fixed::parse_value(input),
        ] {
            let errors = result.unwrap_err();
            assert_eq!(errors.len(), 1);
            let error = &errors.errors()[0];
            assert_eq!(error.code, "array_length");
            assert_eq!(error.pointer(), path);
            assert_eq!(error.stage, Stage::Validate);
            assert_eq!(error.params["expected"], json!(expected));
            assert_eq!(error.params["actual"], json!(actual));
        }
    }
}

#[test]
fn nullable_boxes_preserve_missing_null_normalization_and_constraints() {
    #[derive(Debug, PartialEq, Model)]
    struct Nullable {
        #[model(trim, lowercase, min_length = 2)]
        boxed: Box<Option<String>>,
        #[model(trim, lowercase, min_length = 2)]
        // Deliberately exercise Box forwarding through an optional string.
        #[allow(clippy::box_collection)]
        optional: Option<Box<String>>,
    }
    for input in [json!({}), json!({"boxed":null, "optional":null})] {
        for result in [
            Nullable::parse_json(&input.to_string()),
            Nullable::parse_value(input),
        ] {
            let value = result.unwrap();
            assert_eq!(value.boxed, Box::new(None));
            assert_eq!(value.optional, None);
            assert_eq!(
                value.dump().unwrap(),
                json!({"boxed":null, "optional":null})
            );
        }
    }
    let input = json!({"boxed":" AB ", "optional":" CD "});
    for result in [
        Nullable::parse_json(&input.to_string()),
        Nullable::parse_value(input),
    ] {
        let mut value = result.unwrap();
        assert_eq!(
            value.dump().unwrap(),
            json!({"boxed":"ab", "optional":"cd"})
        );
        *value.boxed = Some("x".into());
        value.optional = Some(Box::new("y".into()));
        let errors = value.validate().unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors.errors()[0].pointer(), "/boxed");
        assert_eq!(errors.errors()[1].pointer(), "/optional");
        assert!(errors
            .errors()
            .iter()
            .all(|error| error.code == "too_short"));
    }
}

#[test]
fn wrong_collection_shapes_report_field_errors_without_member_errors() {
    let input =
        json!({"list":{}, "array":{}, "ordered":[], "hashed":[], "boxed":null, "optional":null});
    for result in [
        Collections::parse_json(&input.to_string()),
        Collections::parse_value(input),
    ] {
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 4);
        assert_eq!(
            errors
                .errors()
                .iter()
                .map(|error| error.pointer())
                .collect::<Vec<_>>(),
            ["/list", "/array", "/ordered", "/hashed"]
        );
        for error in errors.errors() {
            assert_eq!(error.code, "type_error");
            assert_eq!(error.stage, Stage::Parse);
        }
    }
}
