//! Bounded model-schema components for OpenAPI 3.1 (not document generation).
use crate::{LocalRule, Model, SchemaMode};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// A reference relative to the complete OpenAPI document, plus runtime-only rules.
#[derive(Debug, Clone)]
pub struct ComponentRef {
    pub reference: Value,
    /// JSON Pointer locations relative to the complete OpenAPI document.
    pub local_rules: Vec<LocalRule>,
}

/// A rejected registration. The registry remains unchanged on error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiError {
    pub schema_path: String,
    pub message: String,
}
impl std::fmt::Display for OpenApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.schema_path, self.message)
    }
}
impl std::error::Error for OpenApiError {}
fn error(path: &str, message: &str) -> OpenApiError {
    OpenApiError {
        schema_path: path.into(),
        message: message.into(),
    }
}

/// Owns only the `components.schemas` map. No routes or full document are inferred.
#[derive(Debug, Default)]
pub struct OpenApiComponents {
    schemas: Map<String, Value>,
}
impl OpenApiComponents {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn schemas(&self) -> &Map<String, Value> {
        &self.schemas
    }
    pub fn into_schemas(self) -> Map<String, Value> {
        self.schemas
    }

    /// Register input and output models under distinct caller-selected names.
    ///
    /// References target this map only when placed at `/components/schemas` in
    /// an OpenAPI 3.1 document. The returned reference is not a standalone schema.
    /// Existing schema resource IDs, anchors, dynamic/external references and
    /// non-2020-12 dialects are rejected; assertions are never removed.
    pub fn insert<T: Model>(
        &mut self,
        name: &str,
        mode: SchemaMode,
    ) -> Result<ComponentRef, OpenApiError> {
        self.check_name(name)?;
        self.insert_schema(name, T::json_schema(mode))
    }

    fn check_name(&self, name: &str) -> Result<(), OpenApiError> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(error("", "Component name must match [A-Za-z0-9._-]+"));
        }
        if self.schemas.contains_key(name) {
            return Err(error("", "Duplicate component name"));
        }
        Ok(())
    }

    fn insert_schema(
        &mut self,
        name: &str,
        mut schema: Value,
    ) -> Result<ComponentRef, OpenApiError> {
        self.check_name(name)?;
        let mut paths = BTreeSet::new();
        let mut references = Vec::new();
        inspect(&schema, "", 0, &mut paths, &mut references)?;
        let prefix = format!("/components/schemas/{name}");
        for (path, target) in references {
            if !paths.contains(&target) {
                return Err(error(
                    &path,
                    "Reference does not target a supported schema location",
                ));
            }
            let reference = schema
                .pointer_mut(&path)
                .expect("inspected reference location");
            // Encode the fragment after JSON Pointer construction; preserve '/' separators.
            *reference = Value::String(format!("#{}", fragment(&format!("{prefix}{target}"))));
        }
        let mut local_rules = Vec::new();
        for path in &paths {
            let node = schema.pointer(path).expect("inspected schema location");
            if let Some(rules) = node.get("x-rydantic-local-rules").and_then(Value::as_array) {
                for rule in rules.iter().filter_map(Value::as_str) {
                    local_rules.push(LocalRule {
                        schema_path: format!("{prefix}{path}"),
                        rule: rule.into(),
                    });
                }
            }
        }
        self.schemas.insert(name.into(), schema);
        Ok(ComponentRef {
            reference: json!({"$ref": format!("#{prefix}")}),
            local_rules,
        })
    }
}

fn pointer_key(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
fn fragment(pointer: &str) -> String {
    let mut out = String::new();
    for byte in pointer.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut out, "%{byte:02X}").expect("String write");
        }
    }
    out
}
fn decode_reference(reference: &str, path: &str) -> Result<String, OpenApiError> {
    let raw = reference
        .strip_prefix('#')
        .ok_or_else(|| error(path, "Only local JSON Pointer references are supported"))?;
    let mut decoded = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .ok_or_else(|| error(path, "Invalid percent escape"))?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return Err(error(path, "Invalid percent escape"));
            }
            let hex =
                std::str::from_utf8(hex).map_err(|_| error(path, "Invalid percent escape"))?;
            decoded.push(
                u8::from_str_radix(hex, 16).map_err(|_| error(path, "Invalid percent escape"))?,
            );
            i += 3;
        } else {
            if !bytes[i].is_ascii_alphanumeric() && !b"-._~!$&'()*+,;=:@/?".contains(&bytes[i]) {
                return Err(error(path, "Invalid unescaped URI fragment character"));
            }
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let pointer = String::from_utf8(decoded).map_err(|_| error(path, "Reference is not UTF-8"))?;
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(error(path, "Anchors are not supported"));
    }
    // JSON Pointer escapes are validated, not normalized through lossy replacement.
    for token in pointer.split('/').skip(1) {
        let mut chars = token.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(error(path, "Invalid JSON Pointer escape"));
            }
        }
    }
    Ok(pointer)
}

fn inspect(
    schema: &Value,
    path: &str,
    depth: usize,
    paths: &mut BTreeSet<String>,
    refs: &mut Vec<(String, String)>,
) -> Result<(), OpenApiError> {
    if depth > 128 {
        return Err(error(path, "Schema nesting exceeds 128"));
    }
    paths.insert(path.into());
    if schema.is_boolean() {
        return Ok(());
    }
    let map = schema
        .as_object()
        .ok_or_else(|| error(path, "Schema must be an object or boolean"))?;
    for keyword in [
        "$id",
        "$anchor",
        "$dynamicAnchor",
        "$dynamicRef",
        "$recursiveRef",
        "$recursiveAnchor",
        "$vocabulary",
        "definitions",
        "dependencies",
        "additionalItems",
    ] {
        if map.contains_key(keyword) {
            return Err(error(
                path,
                "Unsupported schema resource or reference scope",
            ));
        }
    }
    if let Some(dialect) = map.get("$schema") {
        if !path.is_empty() || dialect != "https://json-schema.org/draft/2020-12/schema" {
            return Err(error(path, "Only JSON Schema 2020-12 is supported"));
        }
    }
    if let Some(reference) = map.get("$ref") {
        let at = format!("{path}/$ref");
        let reference = reference
            .as_str()
            .ok_or_else(|| error(&at, "$ref must be a string"))?;
        refs.push((at.clone(), decode_reference(reference, &at)?));
    }
    for keyword in [
        "$defs",
        "properties",
        "patternProperties",
        "dependentSchemas",
    ] {
        if let Some(children) = map.get(keyword) {
            let children = children
                .as_object()
                .ok_or_else(|| error(path, "Schema map keyword must be an object"))?;
            for (key, child) in children {
                inspect(
                    child,
                    &format!("{path}/{keyword}/{}", pointer_key(key)),
                    depth + 1,
                    paths,
                    refs,
                )?;
            }
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = map.get(keyword) {
            let children = children
                .as_array()
                .ok_or_else(|| error(path, "Schema array keyword must be an array"))?;
            for (index, child) in children.iter().enumerate() {
                inspect(
                    child,
                    &format!("{path}/{keyword}/{index}"),
                    depth + 1,
                    paths,
                    refs,
                )?;
            }
        }
    }
    for keyword in [
        "items",
        "contains",
        "additionalProperties",
        "unevaluatedItems",
        "unevaluatedProperties",
        "propertyNames",
        "if",
        "then",
        "else",
        "not",
        "contentSchema",
    ] {
        if let Some(child) = map.get(keyword) {
            inspect(child, &format!("{path}/{keyword}"), depth + 1, paths, refs)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BooleanSchema<const ACCEPT: bool>;
    static SCHEMA_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    impl<const ACCEPT: bool> crate::JsonType for BooleanSchema<ACCEPT> {
        fn decode(_: Value, _: &mut crate::Context, _: bool) -> Option<Self> {
            panic!("export must not decode")
        }
        fn encode(&self, _: &mut crate::Context) -> Value {
            panic!("export must not encode")
        }
        fn schema(_: &mut crate::SchemaContext, _: bool) -> Value {
            json!(ACCEPT)
        }
    }
    impl<const ACCEPT: bool> Model for BooleanSchema<ACCEPT> {
        fn json_schema(_: SchemaMode) -> Value {
            SCHEMA_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            json!(ACCEPT)
        }
    }

    #[test]
    fn public_custom_boolean_models_and_name_checks() {
        let mut components = OpenApiComponents::new();
        components
            .insert::<BooleanSchema<true>>("True", SchemaMode::Input)
            .unwrap();
        components
            .insert::<BooleanSchema<false>>("False", SchemaMode::Output)
            .unwrap();
        let calls = SCHEMA_CALLS.load(std::sync::atomic::Ordering::SeqCst);
        assert!(components
            .insert::<BooleanSchema<true>>("True", SchemaMode::Input)
            .is_err());
        assert!(components
            .insert::<BooleanSchema<true>>("bad/name", SchemaMode::Input)
            .is_err());
        assert_eq!(
            SCHEMA_CALLS.load(std::sync::atomic::Ordering::SeqCst),
            calls
        );
        assert_eq!(components.schemas()["True"], true);
        assert_eq!(components.schemas()["False"], false);
    }

    #[test]
    fn nesting_boundary_is_explicit_and_atomic() {
        let nested = |count| (0..count).fold(json!(true), |child, _| json!({"not":child}));
        let mut components = OpenApiComponents::new();
        components.insert_schema("Depth128", nested(128)).unwrap();
        let before = components.schemas().clone();
        assert!(components.insert_schema("Depth129", nested(129)).is_err());
        assert_eq!(components.schemas(), &before);
    }

    #[test]
    fn references_are_relocated_only_in_schema_positions() {
        let literal = json!({"$ref":"https://example.invalid/untouched", "$id":"literal"});
        let schema = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "$defs": {"a/雪~%+": {"type":"string", "minLength":2}},
            "$ref":"#%2F$defs%2Fa~1%E9%9B%AA~0%25+",
            "default":literal, "examples":[literal], "const":literal,
            "enum":[literal], "x-vendor":literal,
            "properties":{"x":{"$ref":"#/$defs/a~1%E9%9B%AA~0%25+"}},
            "additionalProperties":false
        });
        let mut components = OpenApiComponents::new();
        let registered = components.insert_schema("Input", schema.clone()).unwrap();
        assert_eq!(
            registered.reference,
            json!({"$ref":"#/components/schemas/Input"})
        );
        let result = &components.schemas()["Input"];
        for key in [
            "default",
            "examples",
            "const",
            "enum",
            "x-vendor",
            "$defs",
            "additionalProperties",
        ] {
            assert_eq!(result[key], schema[key]);
        }
        assert_eq!(
            result["$ref"],
            "#/components/schemas/Input/%24defs/a~1%E9%9B%AA~0%25%2B"
        );
        assert_eq!(result["properties"]["x"]["$ref"], result["$ref"]);
    }

    #[test]
    fn pointer_tokens_and_lowercase_percent_escapes_are_unambiguous() {
        for (token, fragment, encoded) in [
            ("", "#/$defs/", ""),
            ("~1", "#/$defs/~01", "~01"),
            ("01", "#/$defs/01", "01"),
            ("-", "#/$defs/-", "-"),
            ("雪", "#/$defs/%e9%9b%aa", "%E9%9B%AA"),
        ] {
            let schema = json!({"$defs": {token: true}, "$ref": fragment});
            let mut components = OpenApiComponents::new();
            components.insert_schema("Tokens", schema).unwrap();
            assert_eq!(
                components.schemas()["Tokens"]["$ref"],
                format!("#/components/schemas/Tokens/%24defs/{encoded}")
            );
        }
    }

    #[test]
    fn root_reference_and_boolean_schemas_survive() {
        let mut components = OpenApiComponents::new();
        components.insert_schema("Root", json!({"type":"object","properties":{"next":{"$ref":"#"}},"additionalProperties":false})).unwrap();
        assert_eq!(
            components.schemas()["Root"]["properties"]["next"]["$ref"],
            "#/components/schemas/Root"
        );
        components.insert_schema("Never", json!(false)).unwrap();
        components.insert_schema("Anything", json!(true)).unwrap();
        assert_eq!(components.schemas()["Never"], false);
        assert_eq!(components.schemas()["Anything"], true);
    }

    #[test]
    fn unsupported_scopes_and_bad_targets_fail_atomically() {
        let mut components = OpenApiComponents::new();
        components.insert_schema("Existing", json!(true)).unwrap();
        let before = components.schemas().clone();
        for schema in [
            json!({"$id":"https://example.invalid/root"}),
            json!({"properties":{"x":{"$anchor":"x"}}}),
            json!({"$dynamicRef":"#x"}),
            json!({"$dynamicAnchor":"x"}),
            json!({"$vocabulary":{}}),
            json!({"$schema":"http://json-schema.org/draft-07/schema#"}),
            json!({"properties":{"x":{"$schema":"https://json-schema.org/draft/2020-12/schema"}}}),
            json!({"$ref":"https://example.invalid/s"}),
            json!({"$ref":"other.json#/x"}),
            json!({"$ref":"#anchor"}),
            json!({"$ref":4}),
            json!({"$ref":"#/missing"}),
            json!({"default":{},"$ref":"#/default"}),
            json!({"x-vendor":{"type":"string"},"$ref":"#/x-vendor"}),
            json!({"properties":{"a":{}},"$ref":"#/properties"}),
            json!({"$defs":{"a":{}},"$ref":"#/$defs/a~2"}),
            json!({"$ref":"#/%"}),
            json!({"$ref":"#/%GG"}),
            json!({"$defs":{"\n":true},"$ref":"#/$defs/%+A"}),
            json!({"$defs":{"\u{0}":true},"$ref":"#/$defs/%+0"}),
            json!({"$ref":"#/%FF"}),
            json!({"$defs":{"a b":true},"$ref":"#/$defs/a b"}),
            json!({"$defs":{"a#b":true},"$ref":"#/$defs/a#b"}),
            json!({"$defs":{"雪":true},"$ref":"#/$defs/雪"}),
            json!({"definitions":{"old":{"$ref":"#"}}}),
            json!({"dependencies":{}}),
            json!({"additionalItems":true}),
            json!({"allOf":[true,true],"$ref":"#/allOf/01"}),
            json!({"allOf":[true],"$ref":"#/allOf/-"}),
            json!({"items":[]}),
            json!({"properties":[]}),
            json!({"allOf":{}}),
        ] {
            assert!(
                components.insert_schema("Bad", schema.clone()).is_err(),
                "{schema}"
            );
            assert_eq!(components.schemas(), &before);
        }
        for name in ["", "space name", "a/b", "snow雪", "percent%", "hash#"] {
            assert!(components.insert_schema(name, json!(true)).is_err());
            assert_eq!(components.schemas(), &before);
        }
        assert!(components.insert_schema("Existing", json!(false)).is_err());
        assert_eq!(components.schemas(), &before);
    }

    #[test]
    fn local_rules_use_document_pointer_paths_not_instance_literals() {
        let mut components = OpenApiComponents::new();
        let registered = components
            .insert_schema(
                "Input",
                json!({
                    "properties":{"a/雪~%":{"x-rydantic-local-rules":["custom"],"type":"string"}},
                    "default":{"x-rydantic-local-rules":["not-a-rule"]},
                    "x-vendor":{"x-rydantic-local-rules":["also-not-a-rule"]}
                }),
            )
            .unwrap();
        assert_eq!(
            registered.local_rules,
            vec![LocalRule {
                schema_path: "/components/schemas/Input/properties/a~1雪~0%".into(),
                rule: "custom".into()
            }]
        );
        let document = json!({"components":{"schemas":components.into_schemas()}});
        assert_eq!(
            document
                .pointer(&registered.local_rules[0].schema_path)
                .unwrap()["x-rydantic-local-rules"],
            json!(["custom"])
        );
    }

    #[test]
    fn every_supported_schema_container_is_visited() {
        let mut schema = json!({"$defs":{"target":true}});
        for key in ["properties", "patternProperties", "dependentSchemas"] {
            schema[key] = json!({"child":{"$ref":"#/$defs/target"}});
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            schema[key] = json!([{"$ref":"#/$defs/target"}]);
        }
        for key in [
            "items",
            "contains",
            "additionalProperties",
            "unevaluatedItems",
            "unevaluatedProperties",
            "propertyNames",
            "if",
            "then",
            "else",
            "not",
            "contentSchema",
        ] {
            schema[key] = json!({"$ref":"#/$defs/target"});
        }
        let mut components = OpenApiComponents::new();
        components.insert_schema("All", schema).unwrap();
        let text = components.schemas()["All"].to_string();
        assert!(!text.contains("\"#/$defs/target\""));
        assert_eq!(
            text.matches("#/components/schemas/All/%24defs/target")
                .count(),
            18
        );
    }
}
