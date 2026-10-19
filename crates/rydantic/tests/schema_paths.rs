use rydantic::{json, BorrowModel, Model, SchemaMode, SchemaReport};

#[test]
fn local_rule_paths_use_json_pointer_escaping_for_instance_names() {
    let report = SchemaReport::new(json!({
        "properties": {
            "a b/雪~%": {"x-rydantic-local-rules": ["custom"]}
        }
    }));
    assert_eq!(report.local_rules[0].schema_path, "/properties/a b~1雪~0%");
    for rule in &report.local_rules {
        assert!(report.schema.pointer(&rule.schema_path).is_some());
    }
}

#[test]
fn generated_refs_are_uri_encoded_and_local_paths_remain_resolvable() {
    #[derive(Model)]
    struct View<'a> {
        #[model(rename = "a b/雪~%", trim)]
        value: &'a str,
    }
    let report = View::schema_report(SchemaMode::Input);
    assert!(!report.local_rules.is_empty());
    for rule in &report.local_rules {
        assert!(
            report.schema.pointer(&rule.schema_path).is_some(),
            "{}",
            rule.schema_path
        );
    }
    assert!(report
        .local_rules
        .iter()
        .any(|rule| rule.schema_path.ends_with("/properties/a b~1雪~0%")));
    let reference = report.schema["$ref"].as_str().unwrap();
    assert!(reference.contains("%3C"));
    assert!(reference.contains("%27"));
    let validator = jsonschema::validator_for(&report.schema).unwrap();
    assert!(validator.is_valid(&json!({"a b/雪~%":"hello"})));
}
