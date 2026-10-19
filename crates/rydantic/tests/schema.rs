use rydantic::{json, Model, SchemaMode, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Model)]
struct Portable {
    #[model(min_length = 2, max_length = 4)]
    name: String,
    #[model(ge = 18, le = 120)]
    age: u8,
    #[model(min_length = 1, max_length = 2)]
    tags: Vec<String>,
    #[model(min_length = 1)]
    metadata: BTreeMap<String, bool>,
    #[model(min_length = 1)]
    note: Option<String>,
}

#[test]
fn independent_validator_agrees_across_a_boundary_matrix() {
    let report = Portable::schema_report(SchemaMode::Input);
    assert!(report.is_portable());
    let validator = jsonschema::validator_for(&report.schema).unwrap();
    let names = [
        json!("a"),
        json!("ab"),
        json!("张三"),
        json!("😀😀"),
        json!("abcde"),
        json!(null),
        json!(3),
    ];
    let ages = [
        json!(-1),
        json!(17),
        json!(18),
        json!(18.0),
        json!(18.5),
        json!(120),
        json!(121),
        json!(256),
        json!("25"),
        json!(null),
    ];
    let tags = [
        json!([]),
        json!(["a"]),
        json!(["a", "b"]),
        json!(["a", "b", "c"]),
        json!([1]),
    ];
    let notes = [json!(null), json!(""), json!("x"), json!(3)];
    for name in &names {
        for age in &ages {
            for tags in &tags {
                for note in &notes {
                    let value = json!({"name":name,"age":age,"tags":tags,"metadata":{"active":true},"note":note});
                    assert_eq!(
                        Portable::parse_value(value.clone()).is_ok(),
                        validator.is_valid(&value),
                        "schema/runtime disagreement: {value}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_successful_dump_satisfies_output_schema() {
    let schema = Portable::json_schema(SchemaMode::Output);
    let validator = jsonschema::validator_for(&schema).unwrap();
    for age in 18..=120 {
        let value = Portable::parse_value(
            json!({"name":"张三", "age":age, "tags":["x"], "metadata":{"enabled":false}}),
        )
        .unwrap();
        assert!(validator.is_valid(&value.dump().unwrap()));
    }
}

#[derive(Model)]
struct Views {
    #[model(validation_alias = "inName", serialization_alias = "outName")]
    name: String,
    #[model(exclude)]
    password: String,
    #[model(default = 3)]
    retries: u8,
    note: Option<String>,
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
fn input_and_output_views_match_presence_alias_and_exclusion_rules() {
    let input = Views::json_schema(SchemaMode::Input);
    let output = Views::json_schema(SchemaMode::Output);
    let i = definition(&input);
    let o = definition(&output);
    assert!(i["properties"].get("inName").is_some());
    assert!(o["properties"].get("outName").is_some());
    assert!(i["properties"].get("password").is_some());
    assert!(o["properties"].get("password").is_none());
    assert_eq!(i["required"], json!(["inName", "password"]));
    assert_eq!(o["required"], json!(["outName", "retries", "note"]));
    assert_eq!(i["properties"]["retries"]["default"], 3);
    let parsed = Views::parse_value(json!({"inName":"Alice", "password":"secret"})).unwrap();
    assert!(jsonschema::is_valid(&output, &parsed.dump().unwrap()));
}

#[derive(Model)]
struct Transform {
    #[model(trim, min_length = 3, max_length = 5)]
    text: String,
    #[model(coerce, ge = 18)]
    age: u8,
}

#[test]
fn transformed_input_is_a_documented_superset_and_output_is_constrained() {
    let input = Transform::schema_report(SchemaMode::Input);
    assert!(!input.is_portable());
    assert!(input.local_rules.iter().any(|rule| rule.rule == "trim"));
    assert!(input.local_rules.iter().any(|rule| rule.rule == "coerce"));
    let output = Transform::schema_report(SchemaMode::Output);
    assert!(output.is_portable());
    let raw = json!({"text":"     abc    ","age":"25"});
    let model = Transform::parse_value(raw.clone()).unwrap();
    assert!(jsonschema::is_valid(&input.schema, &raw));
    assert!(jsonschema::is_valid(&output.schema, &model.dump().unwrap()));
    let invalid = json!({"text":"    ","age":"1"});
    assert!(jsonschema::is_valid(&input.schema, &invalid));
    assert!(Transform::parse_value(invalid).is_err());
}

static FACTORY_CALLS: AtomicUsize = AtomicUsize::new(0);
fn factory() -> String {
    FACTORY_CALLS.fetch_add(1, Ordering::SeqCst);
    "x".into()
}
#[derive(Model)]
struct Factory {
    #[model(default_factory = factory)]
    field: String,
}

#[test]
fn schema_generation_never_executes_factories() {
    let _ = Factory::json_schema(SchemaMode::Input);
    let _ = Factory::json_schema(SchemaMode::Output);
    assert_eq!(FACTORY_CALLS.load(Ordering::SeqCst), 0);
    Factory::parse_value(json!({})).unwrap();
    assert_eq!(FACTORY_CALLS.load(Ordering::SeqCst), 1);
}

#[derive(Debug, Model)]
#[model(tag = "action")]
enum Action {
    #[model(rename = "search")]
    Search {
        #[model(min_length = 1)]
        query: String,
    },
    #[model(rename = "ping")]
    Ping,
}

#[test]
fn tagged_union_schema_matches_variants_and_extra_field_policy() {
    let schema = Action::json_schema(SchemaMode::Input);
    let validator = jsonschema::validator_for(&schema).unwrap();
    for input in [
        json!({"action":"search","query":"rust"}),
        json!({"action":"search","query":""}),
        json!({"action":"search"}),
        json!({"action":"ping"}),
        json!({"action":"ping","query":"x"}),
        json!({"action":"unknown"}),
    ] {
        assert_eq!(
            validator.is_valid(&input),
            Action::parse_value(input.clone()).is_ok(),
            "{input}"
        );
    }
}

#[derive(Model)]
struct Wide {
    #[model(ge = -100, le = 1000)]
    small: u8,
}

#[test]
fn weaker_user_bounds_do_not_override_integer_representation_bounds() {
    let schema = Wide::json_schema(SchemaMode::Input);
    let property = &definition(&schema)["properties"]["small"];
    assert_eq!(property["minimum"], 0);
    assert_eq!(property["maximum"], 255);
    assert!(!jsonschema::is_valid(&schema, &json!({"small":256})));
}

#[test]
fn recursive_type_names_from_different_modules_do_not_collide() {
    mod a {
        #[derive(rydantic::Model)]
        pub struct Item {
            pub value: String,
        }
    }
    mod b {
        #[derive(rydantic::Model)]
        pub struct Item {
            pub value: u32,
        }
    }
    #[derive(Model)]
    struct Root {
        first: a::Item,
        second: b::Item,
    }
    let schema = Root::json_schema(SchemaMode::Input);
    assert_eq!(schema["$defs"].as_object().unwrap().len(), 3);
    assert!(jsonschema::is_valid(
        &schema,
        &json!({"first":{"value":"x"},"second":{"value":2}})
    ));
}

#[derive(Model)]
struct ExactInteger {
    #[model(ge = 9007199254740993)]
    id: u64,
}

#[test]
fn schema_keeps_large_integer_constraints_exact() {
    let schema = ExactInteger::json_schema(SchemaMode::Input);
    assert_eq!(
        definition(&schema)["properties"]["id"]["minimum"].as_u64(),
        Some(9007199254740993)
    );
    for id in [9007199254740992u64, 9007199254740993, u64::MAX] {
        let value = json!({"id":id});
        assert_eq!(
            jsonschema::is_valid(&schema, &value),
            ExactInteger::parse_value(value).is_ok()
        );
    }
}
