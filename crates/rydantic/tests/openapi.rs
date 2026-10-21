#![cfg(all(feature = "openapi", feature = "derive"))]
use rydantic::{json, schema::openapi::OpenApiComponents, Model, SchemaMode};

#[derive(Model)]
struct Node {
    #[model(min_length = 2)]
    name: String,
    children: Vec<Node>,
}

#[derive(Model)]
struct Views {
    #[model(
        validation_alias = "inputName",
        serialization_alias = "outputName",
        trim
    )]
    name: String,
    #[model(exclude)]
    secret: String,
}

#[test]
fn recursive_components_validate_payloads_without_cross_registration_collisions() {
    let mut registry = OpenApiComponents::new();
    let first = registry.insert::<Node>("First", SchemaMode::Input).unwrap();
    let second = registry
        .insert::<Node>("Second", SchemaMode::Input)
        .unwrap();
    let document = json!({
        "openapi":"3.1.1", "info":{"title":"Example", "version":"1"}, "paths":{},
        "components":{"schemas":registry.into_schemas()}
    });
    for reference in [first.reference, second.reference] {
        // This is a payload schema wrapper with the same document-root paths,
        // NOT an assertion that jsonschema validates the OpenAPI document.
        let wrapper = json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "$ref":reference["$ref"], "components":document["components"]});
        let validator = jsonschema::validator_for(&wrapper).unwrap();
        assert!(validator.is_valid(&json!({"name":"ok","children":[{"name":"yes","children":[]}]})));
        assert!(!validator.is_valid(&json!({"name":"ok","children":[{"name":"x","children":[]}]})));
        assert!(!validator.is_valid(&json!({"name":"ok","children":[{"name":"yes"}]})));
    }
    let first_schema = document["components"]["schemas"]["First"].to_string();
    assert!(first_schema.contains("#/components/schemas/First/"));
    assert!(!first_schema.contains("#/components/schemas/Second/"));
}

#[test]
fn input_output_views_and_runtime_rule_paths_remain_distinct() {
    let mut registry = OpenApiComponents::new();
    let input = registry
        .insert::<Views>("ViewsInput", SchemaMode::Input)
        .unwrap();
    let output = registry
        .insert::<Views>("ViewsOutput", SchemaMode::Output)
        .unwrap();
    let document = json!({"components":{"schemas":registry.into_schemas()}});
    let compile = |reference: &serde_json::Value| {
        jsonschema::validator_for(&json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema", "$ref":reference["$ref"], "components":document["components"]
    })).unwrap()
    };
    let input_validator = compile(&input.reference);
    assert!(input_validator.is_valid(&json!({"inputName":" value ","secret":"s"})));
    assert!(!input_validator.is_valid(&json!({"outputName":"value","secret":"s"})));
    let output_validator = compile(&output.reference);
    assert!(output_validator.is_valid(&json!({"outputName":"value"})));
    assert!(!output_validator.is_valid(&json!({"inputName":"value"})));
    assert!(!input.local_rules.is_empty());
    for rule in input.local_rules.iter().chain(&output.local_rules) {
        assert!(document.pointer(&rule.schema_path).is_some());
        assert!(rule.schema_path.starts_with("/components/schemas/Views"));
    }
    let output_schema = &document["components"]["schemas"]["ViewsOutput"];
    let definition = output_schema["$defs"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert!(definition["properties"].get("secret").is_none());
}
