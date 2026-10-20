use rydantic_codegen::generate;
use serde_json::{json, Value};
#[test]
fn unsupported_semantics_fail_closed_at_the_keyword() {
    for (schema, path) in [
        (json!({"type":"number"}), "/type"),
        (json!({"type":"integer"}), "/minimum"),
        (json!({"type":"string","format":"email"}), "/format"),
        (json!({"type":"string","pattern":"x"}), "/pattern"),
        (json!({"type":"string","default":"x"}), "/default"),
        (
            json!({"type":"string","x-rust-type":"std::process::exit(1)"}),
            "/x-rust-type",
        ),
        (
            json!({"type":"array","items":{"type":"string"},"uniqueItems":true}),
            "/uniqueItems",
        ),
        (
            json!({"type":"object","properties":{"x":{"type":"boolean"}},"required":[],"additionalProperties":false}),
            "/required",
        ),
        (
            json!({"type":"object","properties":{},"required":[]}),
            "/additionalProperties",
        ),
        (json!({"type":["string","boolean"]}), "/type"),
        (json!({"type":"string","enum":["a",null]}), "/enum/1"),
        (json!({"type":"string","minLength":-1}), "/minLength"),
        (
            json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"string"}),
            "/$schema",
        ),
        (json!({"$ref":"https://example.invalid/schema"}), "/$ref"),
        (json!({"$ref":"file:///etc/passwd"}), "/$ref"),
        (json!({"$ref":"#/$defs/nope"}), "/$ref"),
        (
            json!({"$defs":{"x":{"$ref":"#/$defs/x"}},"$ref":"#/$defs/x"}),
            "/$defs/x/$ref",
        ),
        (
            json!({"$defs":{"unused":{"type":"string","pattern":"x"}},"type":"boolean"}),
            "/$defs/unused/pattern",
        ),
        (
            json!({"$defs":{"x":{"type":"boolean"}},"$ref":"#/$defs/x","type":"string"}),
            "/type",
        ),
    ] {
        assert_eq!(generate(&schema).unwrap_err().pointer, path, "{schema}");
    }
}
#[test]
fn names_literals_and_escaped_reference_keys_are_safe_and_deterministic() {
    let schema = json!({"$defs":{"a/b~c":{"type":"string"}},"type":"object","additionalProperties":false,"required":["self","Self","type","\";fn main(){}//"],"properties":{"self":{"$ref":"#/$defs/a~1b~0c"},"Self":{"type":"boolean"},"type":{"type":"string"},"\";fn main(){}//":{"type":"null"}}});
    let a = generate(&schema).unwrap();
    let b = generate(&schema).unwrap();
    assert_eq!(a, b);
    assert!(a.contains("pub field_0:"));
    assert!(!a.contains("pub self:"));
    assert!(a.contains("rename = \"\\\";fn main(){}//\""));
}
#[test]
fn bounds_and_invalid_shapes_are_rejected_without_panics() {
    for schema in [
        json!(true),
        json!({}),
        json!({"type":"string","minLength":4,"maxLength":2}),
        json!({"type":"integer","minimum":4,"maximum":2}),
        json!({"type":["null",1]}),
        json!({"type":"object","properties":{},"required":["x"],"additionalProperties":false}),
        json!({"type":"string","title":4}),
        json!({"type":"string","enum":[]}),
    ] {
        assert!(generate(&schema).is_err());
    }
    let mut deep = json!({"type":"boolean"});
    for _ in 0..40 {
        deep = json!({"type":"array","items":deep});
    }
    assert!(generate(&deep).unwrap_err().message.contains("limit"));
    let big = json!({"type":"string","description":"a".repeat(65_536)});
    assert!(generate(&big).is_err());
    let mut wide = serde_json::Map::new();
    for i in 0..300 {
        wide.insert(format!("p{i}"), json!({"type":"boolean"}));
    }
    let required = wide.keys().cloned().map(Value::String).collect::<Vec<_>>();
    assert!(generate(
        &json!({"type":"object","properties":wide,"required":required,"additionalProperties":false})
    )
    .is_err());
}

#[test]
fn object_input_order_does_not_change_generated_names() {
    let a: Value=serde_json::from_str(r#"{"type":"object","additionalProperties":false,"properties":{"z":{"type":"boolean"},"a":{"type":"string"}},"required":["z","a"]}"#).unwrap();
    let b: Value=serde_json::from_str(r#"{"required":["a","z"],"properties":{"a":{"type":"string"},"z":{"type":"boolean"}},"additionalProperties":false,"type":"object"}"#).unwrap();
    assert_eq!(generate(&a).unwrap(), generate(&b).unwrap());
}

#[test]
fn schema_numeric_notation_and_parser_reserved_names_fail_closed() {
    for input in [
        r#"{"type":"string","minLength":1.0}"#,
        r#"{"type":"array","items":{"type":"boolean"},"minItems":1e0}"#,
        r#"{"type":"integer","minimum":-1.0,"maximum":1}"#,
    ] {
        let schema: Value = serde_json::from_str(input).unwrap();
        assert!(generate(&schema).is_err());
    }
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let mut properties = serde_json::Map::new();
        properties.insert(key.into(), json!({"type":"string"}));
        let schema = json!({"type":"object","properties":properties,"required":[key],"additionalProperties":false});
        assert_eq!(
            generate(&schema).unwrap_err().pointer,
            format!("/properties/{key}")
        );
        let mut defs = serde_json::Map::new();
        defs.insert(key.into(), json!({"type":"string"}));
        assert!(generate(&json!({"type":"string","$defs":defs})).is_err());
    }
}
