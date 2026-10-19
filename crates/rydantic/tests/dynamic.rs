#![cfg(feature = "dynamic")]
use rydantic::{json, DynamicModel, Model, PathSegment, SchemaMode};

#[test]
fn runtime_schema_checks_nested_values_and_local_references() {
    let model = DynamicModel::compile(json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "$defs":{"entry":{"type":"integer","minimum":1}},
        "type":"object", "properties":{"items":{"type":"array","items":{"$ref":"#/$defs/entry"}}},
        "required":["items"], "additionalProperties":false
    }))
    .unwrap();
    assert!(model.is_valid(&json!({"items":[1,2]})));
    let errors = model.validate(&json!({"items":[0,"bad"]})).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors.errors()[0].loc,
        [PathSegment::Field("items".into()), PathSegment::Index(0)]
    );
    assert_eq!(errors.errors()[1].pointer(), "/items/1");
}

#[test]
fn invalid_schemas_fail_compilation() {
    assert!(DynamicModel::compile(json!({"type":"nonsense"})).is_err());
    assert!(DynamicModel::compile(json!({"type":"string","minLength":-1})).is_err());
}

#[test]
fn generated_portable_schemas_can_be_reused_dynamically() {
    #[derive(Model)]
    struct Task {
        #[model(min_length = 1)]
        title: String,
        #[model(ge = 0, le = 5)]
        retries: u8,
    }
    let dynamic = DynamicModel::compile(Task::json_schema(SchemaMode::Input)).unwrap();
    for value in [
        json!({"title":"index","retries":3}),
        json!({"title":"","retries":6}),
    ] {
        assert_eq!(dynamic.is_valid(&value), Task::parse_value(value).is_ok());
    }
}

#[test]
fn numeric_object_keys_stay_fields_in_error_paths() {
    let dynamic =
        DynamicModel::compile(json!({"type":"object","additionalProperties":{"type":"boolean"}}))
            .unwrap();
    let errors = dynamic.validate(&json!({"0":42})).unwrap_err();
    assert_eq!(errors.errors()[0].loc, [PathSegment::Field("0".into())]);
}

#[test]
fn local_execution_rules_require_an_explicit_structural_opt_in() {
    #[derive(Model)]
    struct Local {
        #[model(trim, min_length = 3)]
        name: String,
    }
    let schema = Local::json_schema(SchemaMode::Input);
    assert!(DynamicModel::compile(schema.clone()).is_err());
    let structural = DynamicModel::compile_structural(schema).unwrap();
    assert!(structural.is_valid(&json!({"name":"   "})));
    assert!(Local::parse_value(json!({"name":"   "})).is_err());
}
