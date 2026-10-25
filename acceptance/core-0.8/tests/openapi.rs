#![cfg(feature = "openapi-acceptance")]
use rydantic::{
    Context, JsonType, Model, SchemaContext, SchemaMode, Value, json,
    schema::openapi::OpenApiComponents,
};
#[derive(Model)]
struct PublicOrder {
    #[model(
        validation_alias = "inputName",
        serialization_alias = "outputName",
        trim
    )]
    name: String,
    #[model(exclude)]
    secret: String,
}
struct Scoped;
impl JsonType for Scoped {
    fn decode(_: Value, _: &mut Context, _: bool) -> Option<Self> {
        Some(Self)
    }
    fn encode(&self, _: &mut Context) -> Value {
        json!(null)
    }
    fn schema(_: &mut SchemaContext, _: bool) -> Value {
        json!({"$id":"https://example.invalid/order","type":"string"})
    }
}
impl Model for Scoped {}
#[test]
fn application_document_components_and_scope_boundary() {
    let mut components = OpenApiComponents::new();
    let input = components
        .insert::<PublicOrder>("OrderInput", SchemaMode::Input)
        .unwrap();
    let output = components
        .insert::<PublicOrder>("OrderOutput", SchemaMode::Output)
        .unwrap();
    let before = components.schemas().clone();
    let error = components
        .insert::<Scoped>("Unsupported", SchemaMode::Input)
        .unwrap_err();
    assert!(
        error.message.contains("scope") || error.message.contains("Unsupported"),
        "{error}"
    );
    assert_eq!(components.schemas(), &before, "rejection must be atomic");
    let doc = json!({"openapi":"3.1.1", "info":{"title":"Separate projection schema fixture", "version":"0.0.0"},
        "paths":{"/projection-fixture":{"post":{"requestBody":{"content":{"application/json":{"schema":input.reference}}},
        "responses":{"201":{"description":"Order created","content":{"application/json":{"schema":output.reference}}}}}}},
        "components":{"schemas":components.into_schemas()}});
    for name in ["OrderInput", "OrderOutput"] {
        assert!(
            doc.pointer(&format!("/components/schemas/{name}"))
                .is_some()
        );
    }
    let input_text = doc["components"]["schemas"]["OrderInput"].to_string();
    let output_text = doc["components"]["schemas"]["OrderOutput"].to_string();
    assert!(input_text.contains("inputName"));
    assert!(input_text.contains("secret"));
    assert!(output_text.contains("outputName"));
    assert!(!output_text.contains("\"secret\""));
    assert!(
        !input.local_rules.is_empty(),
        "trim must be recorded as a runtime-only rule"
    );
    for rule in input.local_rules.iter().chain(&output.local_rules) {
        assert!(
            doc.pointer(&rule.schema_path).is_some(),
            "runtime-only rule location must resolve"
        );
    }
    // This checks bounded component integration, not complete OpenAPI document conformance.
}
