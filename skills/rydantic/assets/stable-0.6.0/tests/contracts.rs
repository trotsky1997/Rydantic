use rydantic::{json, DynamicModel, Model, SchemaMode, Stage, ValidationError, Value};

#[derive(Debug, Model)]
struct Account {
    #[model(trim, min_length = 2, max_length = 6)]
    name: String,
    #[model(coerce, ge = 18, le = 120)]
    age: u8,
    #[model(email)]
    email: String,
    #[model(default = 3, ge = 1)]
    retries: u8,
    note: Option<String>,
    #[model(required)]
    nullable: Option<String>,
}

fn valid(age: Value) -> Value {
    json!({"name":" Ada ","age":age,"email":"ada@example.com","nullable":null})
}

#[test]
fn typed_normalizes_defaults_and_validates_boundaries() {
    for age in [json!(18), json!(120), json!("25")] {
        let account = Account::parse_value(valid(age)).unwrap();
        assert_eq!(account.name, "Ada");
        assert_eq!(account.retries, 3);
        assert_eq!(account.note, None);
        assert_eq!(account.nullable, None);
        // Explicit schema-only output assertion; typed email rules remain local.
        assert!(
            DynamicModel::compile_structural(Account::json_schema(SchemaMode::Output))
                .unwrap()
                .is_valid(&account.dump().unwrap())
        );
    }
    for age in [json!(17), json!(121), json!(256), json!(18.5), json!(null)] {
        assert!(Account::parse_value(valid(age)).is_err());
    }
    for name in ["", " ", "a", "abcdefg"] {
        let mut input = valid(json!(20));
        input["name"] = json!(name);
        assert!(Account::parse_value(input).is_err());
    }
    for name in ["ab", "abcdef", "张三"] {
        let mut input = valid(json!(20));
        input["name"] = json!(name);
        assert!(Account::parse_value(input).is_ok());
    }
}

#[test]
fn missing_null_extra_and_email_are_distinct() {
    let mut input = valid(json!(20));
    input.as_object_mut().unwrap().remove("nullable");
    assert!(Account::parse_value(input).is_err());
    for (field, value) in [
        ("retries", json!(null)),
        ("retries", json!(0)),
        ("email", json!("invalid")),
        ("extra", json!(true)),
    ] {
        let mut input = valid(json!(20));
        input[field] = value;
        assert!(Account::parse_value(input).is_err());
    }
}

#[derive(Debug, Model)]
struct Views {
    #[model(validation_alias = "inName", serialization_alias = "outName")]
    name: String,
    #[model(exclude)]
    password: String,
}

fn definition(schema: &Value) -> &Value {
    schema["$defs"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
}

#[test]
fn input_output_contracts_have_different_names_and_visibility() {
    let input = Views::json_schema(SchemaMode::Input);
    let output = Views::json_schema(SchemaMode::Output);
    assert!(definition(&input)["properties"].get("inName").is_some());
    assert!(definition(&input)["properties"].get("outName").is_none());
    assert!(definition(&output)["properties"].get("outName").is_some());
    assert!(definition(&output)["properties"].get("password").is_none());
    let model = Views::parse_value(json!({"inName":"Ada","password":"fixture-only"})).unwrap();
    assert_eq!(model.dump().unwrap(), json!({"outName":"Ada"}));
    assert!(Views::parse_value(json!({"outName":"Ada","password":"fixture-only"})).is_err());
}

#[derive(Debug, Model)]
struct Transform {
    #[model(trim, min_length = 3)]
    text: String,
    #[model(coerce, ge = 18)]
    age: u8,
}

#[test]
fn structural_only_is_not_typed_validation() {
    let report = Transform::schema_report(SchemaMode::Input);
    assert!(!report.is_portable());
    assert!(report.local_rules.iter().any(|r| r.rule == "trim"));
    assert!(report.local_rules.iter().any(|r| r.rule == "coerce"));
    assert!(DynamicModel::compile(report.schema.clone()).is_err());
    let structural = DynamicModel::compile_structural(report.schema).unwrap();
    let invalid = json!({"text":"    ","age":"1"});
    assert!(structural.is_valid(&invalid));
    assert!(Transform::parse_value(invalid).is_err());
    let model = Transform::parse_value(json!({"text":" abc ","age":"18"})).unwrap();
    assert_eq!(model.dump().unwrap(), json!({"text":"abc","age":18}));
}

#[test]
fn dynamic_is_assertion_only_and_format_is_not_an_email_check() {
    let dynamic = DynamicModel::compile(json!({"type":"object","properties":{
        "count":{"type":"integer","minimum":1,"default":3},
        "email":{"type":"string","format":"email"}},"additionalProperties":false}))
    .unwrap();
    let input = json!({"email":"not-an-email"});
    assert!(dynamic.validate(&input).is_ok());
    assert!(input.get("count").is_none());
    assert!(dynamic.validate(&json!({"count":0})).is_err());
    assert!(dynamic.validate(&json!({"count":"3"})).is_err());
}

#[test]
fn diagnostics_preserve_code_stage_and_escaped_location() {
    let errors = Account::parse_value(valid(json!(17))).unwrap_err();
    let age_error = errors
        .errors()
        .iter()
        .find(|e| e.pointer() == "/age")
        .unwrap();
    assert_eq!(age_error.code, "greater_than_equal");
    assert_eq!(age_error.stage, Stage::Validate);
    assert!(errors.to_json().is_array());
    let nested = ValidationError::new("custom", "fixture")
        .at("a/b~c")
        .at(0usize);
    assert_eq!(nested.pointer(), "/a~1b~0c/0");
}
