use rydantic::{DynamicModel, Model, Value};
use serde_json::json;
#[allow(dead_code)]
mod generated {
    include!("fixtures/generated.txt");
}
fn schema() -> Value {
    serde_json::from_str(include_str!("fixtures/schema.json")).unwrap()
}
fn valid() -> Value {
    json!({"type":"猫😀", "a-b":true,"a_b":["x"],"nullable":null,"mode":"a-b","count":1,"nested":{"flag":false},"quote\"\n\\":null})
}
#[test]
fn checked_in_generated_source_is_current_and_compiles() {
    assert_eq!(
        rydantic_codegen::generate(&schema()).unwrap(),
        include_str!("fixtures/generated.txt")
    );
}
#[test]
fn source_schema_and_generated_validation_agree() {
    let validator = DynamicModel::compile(schema()).unwrap();
    let check = |value: Value| {
        let result = generated::RootModel::parse_value(value.clone());
        assert_eq!(result.is_ok(), validator.is_valid(&value), "{value}");
        if let Ok(model) = result {
            let output = model.dump().unwrap();
            assert!(validator.is_valid(&output));
            let mut normalized = value;
            normalized["count"] = output["count"].clone();
            assert_eq!(output, normalized);
        }
    };
    check(valid());
    for key in valid().as_object().unwrap().keys() {
        let mut missing = valid();
        missing.as_object_mut().unwrap().remove(key);
        check(missing);
        for bad in [
            json!(null),
            json!(false),
            json!(0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            let mut value = valid();
            value[key] = bad;
            check(value);
        }
    }
    for (key, values) in [
        (
            "type",
            vec![json!(""), json!("a"), json!("😀猫x"), json!("abcd")],
        ),
        (
            "a_b",
            vec![
                json!([]),
                json!(["a"]),
                json!(["a", "b"]),
                json!(["a", "b", "c"]),
                json!([""]),
                json!([null]),
            ],
        ),
        ("nullable", vec![json!(null), json!("a"), json!("ab")]),
        (
            "mode",
            vec![json!("type"), json!("a_b"), json!("\"\n\\"), json!("bad")],
        ),
        (
            "count",
            vec![
                json!(-11),
                json!(-10),
                json!(10),
                json!(11),
                json!(1.0),
                json!(1.5),
                json!("1"),
            ],
        ),
        (
            "nested",
            vec![
                json!({}),
                json!({"flag":true,"extra":null}),
                json!({"flag":null}),
            ],
        ),
    ] {
        for item in values {
            let mut value = valid();
            value[key] = item;
            check(value);
        }
    }
    let mut extra = valid();
    extra["extra"] = json!(1);
    check(extra);
    for value in [json!(null), json!([]), json!("object"), json!(1)] {
        check(value);
    }
}
