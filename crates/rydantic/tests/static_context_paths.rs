use rydantic::{
    json, BorrowModel, BorrowType, Context, JsonType, Model, Stage, ValidationError, Value,
};
use std::collections::BTreeMap;

fn reject_negative(value: &i64) -> Result<(), ValidationError> {
    if *value < 0 {
        Err(ValidationError::new("negative", "must not be negative")
            .at("relative/~雪")
            .with_param("actual", *value))
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
struct Leaf {
    #[model(validation_alias = "in/~雪", serialization_alias = "out/~雪", validate = reject_negative)]
    number: i64,
}
#[derive(Debug, Model)]
struct Tree {
    items: Vec<Leaf>,
    dynamic: BTreeMap<String, Leaf>,
}
fn negative(loc: Value, actual: i64) -> Value {
    json!({"loc":loc,"code":"negative","message":"must not be negative","stage":"validate","params":{"actual":actual}})
}
#[test]
fn nested_static_aliases_dynamic_keys_indices_and_error_order_are_exact() {
    let input = json!({"items":[{"in/~雪":-1},{"in/~雪":-2}],"dynamic":{"a/~雪":{"in/~雪":-3},"0":{"in/~雪":-4}}});
    let expected = json!([
        negative(json!(["items", 0, "in/~雪", "relative/~雪"]), -1),
        negative(json!(["items", 1, "in/~雪", "relative/~雪"]), -2),
        negative(json!(["dynamic", "0", "in/~雪", "relative/~雪"]), -4),
        negative(json!(["dynamic", "a/~雪", "in/~雪", "relative/~雪"]), -3)
    ]);
    let errors = Tree::parse_json(&input.to_string()).unwrap_err();
    assert_eq!(errors.to_json(), expected);
    assert_eq!(Tree::parse_value(input).unwrap_err().to_json(), expected);
    assert_eq!(
        errors.errors()[3].pointer(),
        "/dynamic/a~1~0雪/in~1~0雪/relative~1~0雪"
    );
    let model = Tree {
        items: vec![Leaf { number: -1 }, Leaf { number: -2 }],
        dynamic: BTreeMap::from([
            ("0".into(), Leaf { number: -4 }),
            ("a/~雪".into(), Leaf { number: -3 }),
        ]),
    };
    assert_eq!(model.validate().unwrap_err().to_json(), expected);
}

#[derive(Debug, Model)]
struct AliasedFloat {
    #[model(validation_alias = "incoming", serialization_alias = "outgoing")]
    value: f64,
}
#[test]
fn serialization_and_input_encoding_use_their_own_aliases() {
    let model = AliasedFloat { value: f64::NAN };
    for (input, alias) in [(true, "incoming"), (false, "outgoing")] {
        let mut ctx = Context::new();
        let value = if input {
            JsonType::encode_input(&model, &mut ctx)
        } else {
            JsonType::encode(&model, &mut ctx)
        };
        assert_eq!(value, json!({alias:null}));
        assert_eq!(
            ctx.result().unwrap_err().to_json(),
            json!([{
                "loc":[alias], "code":"finite_number", "message":"NaN and infinity cannot be represented in JSON", "stage":"serialize"
            }])
        );
    }
}

#[derive(Debug, Model)]
struct Pair(i64, String);
#[derive(Debug, Model)]
struct Flattened {
    #[model(flatten)]
    leaf: Leaf,
    tuple: Pair,
}
#[derive(Debug, Model)]
struct Wrapped(Leaf);
#[test]
fn flattened_and_newtype_models_add_no_artificial_field_segment() {
    let input = json!({"in/~雪":-7,"tuple":[1,"ok"]});
    assert_eq!(
        Flattened::parse_value(input).unwrap_err().to_json(),
        json!([negative(json!(["in/~雪", "relative/~雪"]), -7)])
    );
    assert_eq!(
        Wrapped::parse_value(json!({"in/~雪":-8}))
            .unwrap_err()
            .to_json(),
        json!([negative(json!(["in/~雪", "relative/~雪"]), -8)])
    );
}

#[derive(Debug, Model)]
struct Borrowed<'a> {
    #[model(validation_alias = "source", serialization_alias = "target")]
    text: &'a str,
}
#[test]
fn borrowed_models_keep_borrows_and_full_parse_errors() {
    let input = String::from(r#"{"source":"borrow me"}"#);
    let model = Borrowed::parse_json(&input).unwrap();
    let expected = input.find("borrow me").unwrap();
    assert_eq!(model.text.as_ptr(), input[expected..].as_ptr());
    let mut ctx = Context::new();
    assert_eq!(
        BorrowType::encode(&model, &mut ctx),
        json!({"target":"borrow me"})
    );
    ctx.result().unwrap();
    assert_eq!(
        Borrowed::parse_json("{}").unwrap_err().to_json(),
        json!([{
            "loc":["source"],"code":"missing","message":"field is required","stage":"parse"
        }])
    );
}

#[derive(Debug, Model)]
#[model(tag = "kind")]
enum Tagged {
    Item { value: bool },
}
#[test]
fn discriminator_errors_keep_full_paths_and_payloads() {
    let expected = ValidationError::new("union_tag_missing", "discriminator is required")
        .at("kind")
        .with_stage(Stage::Parse);
    assert_eq!(
        Tagged::parse_json("{}").unwrap_err().to_json(),
        json!([expected])
    );
}
