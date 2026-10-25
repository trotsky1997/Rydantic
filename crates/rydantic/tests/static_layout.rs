use rydantic::{json, Model};

#[derive(Debug, Model)]
struct Static {
    #[cfg(any())]
    #[model(validation_alias = "value")]
    disabled: bool,
    #[model(validation_alias = "value", serialization_alias = "result", ge = 0)]
    number: i64,
    #[model(exclude, serialization_alias = "result", default)]
    hidden: bool,
}

#[test]
fn static_layout_retains_aliases_constraints_extra_rejection_and_exclusion() {
    let parsed = Static::parse_json(r#"{"value":42.0}"#).unwrap();
    assert_eq!(parsed.number, 42);
    assert_eq!(parsed.dump().unwrap(), json!({"result":42}));
    assert_eq!(
        Static::parse_json(r#"{"value":-1}"#).unwrap_err().errors()[0].pointer(),
        "/value"
    );
    assert!(Static::parse_json(r#"{"value":42,"unknown":true}"#)
        .unwrap_err()
        .errors()
        .iter()
        .any(|e| e.code == "extra_forbidden"));
}
