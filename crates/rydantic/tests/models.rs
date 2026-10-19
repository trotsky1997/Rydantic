use rydantic::{
    json, Model, PathSegment, SchemaMode, Stage, Validated, ValidationError, ValidationErrors,
    Value,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, PartialEq, Model)]
struct Address {
    #[model(min_length = 2)]
    city: String,
    #[model(pattern = "^[0-9]{5}$")]
    zip: String,
}

#[derive(Debug, PartialEq, Model)]
struct User {
    #[model(trim, min_length = 3, max_length = 20)]
    name: String,
    #[model(coerce, ge = 18, le = 120)]
    age: u8,
    #[model(trim, lowercase, email)]
    email: String,
    #[model(default)]
    addresses: Vec<Address>,
    note: Option<String>,
}

fn valid_user() -> Value {
    json!({"name":" Alice ", "age":"25", "email":" ALICE@EXAMPLE.COM "})
}

#[test]
fn parses_normalizes_and_serializes() {
    let user = User::parse_value(valid_user()).unwrap();
    assert_eq!(user.name, "Alice");
    assert_eq!(user.age, 25);
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.note, None);
    assert!(user.addresses.is_empty());
    assert_eq!(
        user.dump().unwrap(),
        json!({"name":"Alice", "age":25, "email":"alice@example.com", "addresses":[], "note":null})
    );
}

#[test]
fn accumulates_type_constraint_and_extra_errors() {
    let errors =
        User::parse_value(json!({"name":" ", "age":17, "email":"invalid", "surprise":true}))
            .unwrap_err();
    let codes: Vec<_> = errors.errors().iter().map(|e| e.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "too_short",
            "greater_than_equal",
            "email",
            "extra_forbidden"
        ]
    );
    assert!(errors.to_json().is_array());
}

#[test]
fn nested_collection_errors_keep_indices_and_field_names() {
    let mut value = valid_user();
    value["addresses"] = json!([{"city":"X", "zip":"bad"}, {"city":1, "zip":"12345"}]);
    let errors = User::parse_value(value).unwrap_err();
    assert_eq!(errors.errors().len(), 3);
    assert_eq!(
        errors.errors()[0].loc,
        vec!["addresses".into(), 0usize.into(), "city".into()]
    );
    assert_eq!(errors.errors()[2].pointer(), "/addresses/1/city");
}

#[derive(Debug, Model)]
struct Strict {
    count: u8,
    active: bool,
    label: String,
}

#[test]
fn strict_types_require_explicit_coercion() {
    let errors =
        Strict::parse_value(json!({"count":"25", "active":"true", "label":42})).unwrap_err();
    assert_eq!(errors.len(), 3);
    assert!(errors.errors().iter().all(|e| e.stage == Stage::Parse));
}

#[test]
fn mathematical_integer_json_numbers_are_accepted_without_truncation() {
    assert_eq!(
        Strict::parse_json(r#"{"count":25.0,"active":true,"label":"ok"}"#)
            .unwrap()
            .count,
        25
    );
    assert!(Strict::parse_value(json!({"count":25.5,"active":true,"label":"ok"})).is_err());
    assert!(Strict::parse_value(json!({"count":256,"active":true,"label":"ok"})).is_err());
}

#[derive(Debug, Model)]
struct Presence {
    ordinary: Option<String>,
    #[model(required)]
    nullable: Option<String>,
    #[model(default = 3, ge = 1)]
    attempts: u8,
    #[model(default = "guest")]
    role: String,
}

#[test]
fn missing_and_null_have_explicit_semantics() {
    let errors = Presence::parse_value(json!({})).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].pointer(), "/nullable");
    let model = Presence::parse_value(json!({"nullable":null})).unwrap();
    assert_eq!(model.ordinary, None);
    assert_eq!(model.attempts, 3);
    assert_eq!(model.role, "guest");
    assert!(Presence::parse_value(json!({"nullable":null,"attempts":null})).is_err());
}

#[derive(Debug, Model)]
struct InvalidDefault {
    #[model(default = 0, ge = 1)]
    count: u8,
}

#[test]
fn defaults_are_validated() {
    let errors = InvalidDefault::parse_value(json!({})).unwrap_err();
    assert_eq!(errors.errors()[0].code, "greater_than_equal");
}

#[derive(Debug, Model)]
#[model(extra = "ignore")]
struct Compatible {
    value: u8,
}

#[test]
fn ignored_unknown_fields_are_dropped() {
    let value = Compatible::parse_value(json!({"value":3, "future":42})).unwrap();
    assert_eq!(value.dump().unwrap(), json!({"value":3}));
}

#[derive(Debug, Model)]
struct Unicode {
    #[model(min_length = 3, max_length = 3)]
    name: String,
}

#[test]
fn string_length_counts_unicode_scalars() {
    assert!(Unicode::parse_value(json!({"name":"李"})).is_err());
    assert!(Unicode::parse_value(json!({"name":"张三丰"})).is_ok());
    assert!(Unicode::parse_value(json!({"name":"😀😀😀"})).is_ok());
    assert!(Unicode::parse_value(json!({"name":"e\u{301}ab"})).is_err());
}

#[derive(Debug, PartialEq, Model)]
struct Aliased {
    #[model(validation_alias = "incoming", serialization_alias = "outgoing")]
    value: u8,
    #[model(exclude, min_length = 3)]
    secret: String,
}

fn aliased_default() -> Aliased {
    Aliased {
        value: 7,
        secret: "token".into(),
    }
}

#[derive(Debug, Model)]
struct WithFactory {
    #[model(default_factory = aliased_default)]
    child: Aliased,
    #[model(default_factory = Vec::new)]
    children: Vec<Aliased>,
}

#[test]
fn input_output_aliases_and_exclusions_work() {
    let value = Aliased::parse_value(json!({"incoming":5, "secret":"token"})).unwrap();
    assert_eq!(value.dump().unwrap(), json!({"outgoing":5}));
    let mut value = value;
    value.secret.clear();
    assert!(value.dump().is_err());
}

#[test]
fn typed_factories_preserve_input_aliases_and_excluded_fields() {
    let value = WithFactory::parse_value(json!({})).unwrap();
    assert_eq!(value.child, aliased_default());
    assert!(value.children.is_empty());
}

fn children_default() -> Vec<Option<Box<Aliased>>> {
    vec![Some(Box::new(aliased_default()))]
}

#[test]
fn typed_defaults_preserve_input_views_through_containers() {
    #[derive(Model)]
    struct Defaults {
        #[model(default_factory = children_default)]
        children: Vec<Option<Box<Aliased>>>,
    }
    let value = Defaults::parse_value(json!({})).unwrap();
    assert_eq!(value.children[0].as_ref().unwrap().value, 7);
    assert_eq!(value.children[0].as_ref().unwrap().secret, "token");
}

fn even(value: &u32) -> Result<(), ValidationError> {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(ValidationError::new("even", "value must be even"))
    }
}
fn before_count(value: Value) -> Result<Value, ValidationError> {
    if value == "many" {
        Ok(json!(4))
    } else {
        Ok(value)
    }
}
fn check_pair(value: &Pair) -> Result<(), ValidationErrors> {
    if value.end >= value.start {
        Ok(())
    } else {
        Err(ValidationError::custom("end must be >= start")
            .at("end")
            .into())
    }
}

#[derive(Debug, Model)]
#[model(validate = check_pair)]
struct Pair {
    start: u32,
    #[model(before = before_count, validate = even)]
    end: u32,
}

#[test]
fn before_field_and_model_validators_are_composed() {
    assert_eq!(
        Pair::parse_value(json!({"start":2,"end":"many"}))
            .unwrap()
            .end,
        4
    );
    assert_eq!(
        Pair::parse_value(json!({"start":2,"end":3}))
            .unwrap_err()
            .errors()[0]
            .code,
        "even"
    );
    assert_eq!(
        Pair::parse_value(json!({"start":8,"end":4}))
            .unwrap_err()
            .errors()[0]
            .pointer(),
        "/end"
    );
}

#[test]
fn cross_field_errors_are_prefixed_when_nested() {
    #[derive(Debug, Model)]
    struct Container {
        pair: Pair,
    }
    let errors = Container::parse_value(json!({"pair":{"start":8,"end":4}})).unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/pair/end");
}

#[derive(Debug, PartialEq, Model)]
#[model(tag = "kind")]
enum Tool {
    #[model(rename = "search")]
    Search {
        #[model(min_length = 1)]
        query: String,
        #[model(default = 10, ge = 1, le = 100)]
        limit: u8,
    },
    #[model(rename = "ping")]
    Ping,
}

#[test]
fn tagged_enums_parse_default_validate_and_round_trip() {
    let tool = Tool::parse_value(json!({"kind":"search","query":"rust"})).unwrap();
    assert_eq!(
        tool,
        Tool::Search {
            query: "rust".into(),
            limit: 10
        }
    );
    assert_eq!(
        tool.dump().unwrap(),
        json!({"kind":"search","query":"rust","limit":10})
    );
    assert_eq!(
        Tool::parse_value(json!({"kind":"ping"})).unwrap(),
        Tool::Ping
    );
    assert!(Tool::parse_value(json!({"kind":"ping","query":"x"})).is_err());
    assert_eq!(
        Tool::parse_value(json!({"kind":"search","query":""}))
            .unwrap_err()
            .errors()[0]
            .pointer(),
        "/query"
    );
}

#[test]
fn tagged_enum_errors_identify_the_discriminator() {
    for (value, code) in [
        (json!({}), "union_tag_missing"),
        (json!({"kind":3}), "union_tag_type"),
        (json!({"kind":"unknown"}), "union_tag_invalid"),
    ] {
        let errors = Tool::parse_value(value).unwrap_err();
        assert_eq!(errors.errors()[0].code, code);
        assert_eq!(errors.errors()[0].pointer(), "/kind");
    }
}

#[derive(Debug, PartialEq, Model)]
enum Status {
    #[model(rename = "open")]
    Open,
    #[model(rename = "closed")]
    Closed,
}

#[test]
fn unit_enums_use_string_literals() {
    assert_eq!(Status::parse_json("\"open\"").unwrap(), Status::Open);
    assert_eq!(Status::Closed.dump().unwrap(), json!("closed"));
    assert!(Status::parse_json("\"unknown\"").is_err());
}

#[derive(Debug, PartialEq, Model)]
struct Node {
    value: u32,
    next: Option<Box<Node>>,
}

#[test]
fn recursive_models_parse_and_generate_finite_schemas() {
    let node = Node::parse_value(json!({"value":1,"next":{"value":2}})).unwrap();
    assert_eq!(node.next.as_ref().unwrap().value, 2);
    let schema = Node::json_schema(SchemaMode::Output);
    assert_eq!(schema["$defs"].as_object().unwrap().len(), 1);
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&node.dump().unwrap()));
}

#[derive(Debug, Model)]
struct Page<T> {
    items: Vec<T>,
    #[model(default = 1)]
    page: u32,
}

#[test]
fn generic_models_and_definitions_are_distinct() {
    let page = Page::<Status>::parse_value(json!({"items":["open"]})).unwrap();
    assert_eq!(page.items, [Status::Open]);
    assert_eq!(page.page, 1);
    #[derive(Model)]
    struct Both {
        strings: Page<String>,
        integers: Page<u64>,
    }
    assert_eq!(
        Both::json_schema(SchemaMode::Input)["$defs"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
}

#[derive(Debug, Model)]
struct Collections {
    #[model(min_length = 1, max_length = 3)]
    labels: BTreeMap<String, u32>,
    points: [u8; 2],
    flags: HashMap<String, bool>,
}

#[test]
fn maps_arrays_and_escaped_paths_work() {
    let errors =
        Collections::parse_value(json!({"labels":{"a/b~c":"x"},"points":[1,2,3],"flags":{"0":1}}))
            .unwrap_err();
    assert_eq!(errors.len(), 3);
    assert_eq!(errors.errors()[0].pointer(), "/labels/a~1b~0c");
    assert_eq!(
        errors.errors()[2].loc,
        [
            PathSegment::Field("flags".into()),
            PathSegment::Field("0".into())
        ]
    );
    assert!(Collections::parse_value(json!({"labels":{"x":1},"points":[1,2],"flags":{}})).is_ok());
}

#[derive(Debug, Model)]
struct BigNumber {
    #[model(ge = 9007199254740993, le = 18446744073709551615)]
    id: u64,
}

#[test]
fn integer_constraints_keep_all_64_bits() {
    assert!(BigNumber::parse_json(r#"{"id":9007199254740992}"#).is_err());
    assert_eq!(
        BigNumber::parse_json(r#"{"id":9007199254740993}"#)
            .unwrap()
            .id,
        9007199254740993
    );
    assert_eq!(
        BigNumber::parse_json(r#"{"id":18446744073709551615}"#)
            .unwrap()
            .id,
        u64::MAX
    );
    assert!(BigNumber::parse_json(r#"{"id":18446744073709551616}"#).is_err());
}

#[derive(Debug, Model)]
struct Float {
    #[model(gt = -0.5, lt = 2.5)]
    value: f64,
}

#[test]
fn floating_constraints_and_non_finite_values_are_checked() {
    assert!(Float::parse_value(json!({"value":-0.5})).is_err());
    assert!(Float::parse_value(json!({"value":0.0})).is_ok());
    assert!(Float::parse_value(json!({"value":2.5})).is_err());
    assert!(Float { value: f64::NAN }.dump().is_err());
    assert!(Float {
        value: f64::INFINITY
    }
    .validate()
    .is_err());
}

#[test]
fn mutated_values_and_serde_serialization_are_revalidated() {
    let mut user = User::parse_value(valid_user()).unwrap();
    user.age = 1;
    assert!(user.validate().is_err());
    assert!(user.dump_json().is_err());
    assert!(serde_json::to_string(&user).is_err());
}

#[test]
fn serde_deserialization_cannot_skip_validation() {
    let user: User = serde_json::from_value(valid_user()).unwrap();
    assert_eq!(user.name, "Alice");
    assert!(serde_json::from_str::<User>(r#"{"name":"A","age":1,"email":"bad"}"#).is_err());
    let output = serde_json::to_value(&user).unwrap();
    assert_eq!(output, user.dump().unwrap());
}

#[test]
fn validated_wrapper_supports_serde_and_owned_extraction() {
    let value = User::parse_validated_json(&valid_user().to_string()).unwrap();
    assert_eq!(value.age, 25);
    let encoded = serde_json::to_string(&value).unwrap();
    let decoded: Validated<User> = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.into_inner(), value.into_inner());
    assert!(Validated::new(Float { value: f64::NAN }).is_err());
}

#[test]
fn malformed_json_has_line_and_column() {
    let errors = User::parse_json("{\n  broken").unwrap_err();
    assert_eq!(errors.errors()[0].code, "json_invalid");
    assert_eq!(errors.errors()[0].params["line"], 2);
}

#[test]
fn normalization_follows_explicit_coercion() {
    #[derive(Model)]
    struct Coerced {
        #[model(coerce, uppercase)]
        value: String,
    }
    assert_eq!(
        Coerced::parse_value(json!({"value":true})).unwrap().value,
        "TRUE"
    );
}
