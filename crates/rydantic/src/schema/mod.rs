#[cfg(feature = "openapi")]
pub mod openapi;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaMode {
    Input,
    Output,
}

/// A rule which requires Rydantic execution beyond portable JSON Schema assertions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRule {
    pub schema_path: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaReport {
    pub schema: Value,
    pub local_rules: Vec<LocalRule>,
}
impl SchemaReport {
    pub fn new(schema: Value) -> Self {
        fn collect(value: &Value, path: &str, out: &mut Vec<LocalRule>) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::Array(rules)) = map.get("x-rydantic-local-rules") {
                        for rule in rules.iter().filter_map(Value::as_str) {
                            out.push(LocalRule {
                                schema_path: path.to_owned(),
                                rule: rule.to_owned(),
                            });
                        }
                    }
                    // Descend through schema keywords; defaults and examples are instance data.
                    for keyword in [
                        "$defs",
                        "definitions",
                        "properties",
                        "patternProperties",
                        "dependentSchemas",
                        "dependencies",
                    ] {
                        if let Some(Value::Object(children)) = map.get(keyword) {
                            for (key, child) in children {
                                collect(
                                    child,
                                    &format!(
                                        "{path}/{keyword}/{}",
                                        key.replace('~', "~0").replace('/', "~1")
                                    ),
                                    out,
                                );
                            }
                        }
                    }
                    for keyword in [
                        "allOf",
                        "anyOf",
                        "oneOf",
                        "prefixItems",
                        "items",
                        "contains",
                        "additionalItems",
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
                            collect(child, &format!("{path}/{keyword}"), out);
                        }
                    }
                }
                Value::Array(items) => {
                    for (index, value) in items.iter().enumerate() {
                        collect(value, &format!("{path}/{index}"), out);
                    }
                }
                _ => {}
            }
        }
        let mut local_rules = vec![];
        collect(&schema, "", &mut local_rules);
        Self {
            schema,
            local_rules,
        }
    }
    pub fn is_portable(&self) -> bool {
        self.local_rules.is_empty()
    }
}

/// Per-export registry. Registering before descending makes recursive models finite.
pub struct SchemaContext {
    pub mode: SchemaMode,
    definitions: Map<String, Value>,
}
impl SchemaContext {
    pub fn new(mode: SchemaMode) -> Self {
        Self {
            mode,
            definitions: Map::new(),
        }
    }
    pub fn reference(&mut self, key: &str, build: impl FnOnce(&mut Self) -> Value) -> Value {
        if !self.definitions.contains_key(key) {
            self.definitions.insert(key.to_owned(), Value::Bool(true));
            let definition = build(self);
            self.definitions.insert(key.to_owned(), definition);
        }
        json!({"$ref": format!("#/$defs/{}", encode_fragment(&key.replace('~', "~0").replace('/', "~1")))})
    }
    /// Resolve a reference within this export registry.
    pub fn resolve(&self, schema: &Value) -> Value {
        if let Some(reference) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("#/$defs/"))
        {
            let key = decode_fragment(reference)
                .replace("~1", "/")
                .replace("~0", "~");
            self.definitions
                .get(&key)
                .cloned()
                .unwrap_or_else(|| schema.clone())
        } else {
            schema.clone()
        }
    }
    pub fn finish(self, root: Value) -> Value {
        let mut object = match root {
            Value::Object(map) => map,
            value => {
                let mut map = Map::new();
                map.insert("allOf".into(), json!([value]));
                map
            }
        };
        object.insert(
            "$schema".into(),
            json!("https://json-schema.org/draft/2020-12/schema"),
        );
        if !self.definitions.is_empty() {
            object.insert("$defs".into(), Value::Object(self.definitions));
        }
        Value::Object(object)
    }
}

pub fn annotate_local(schema: &mut Value, rule: &str) {
    if !schema.is_object() {
        *schema = json!({"allOf": [schema.clone()]});
    }
    let object = schema.as_object_mut().expect("schema object");
    let entries = object
        .entry("x-rydantic-local-rules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("rule array");
    if !entries.iter().any(|entry| entry == rule) {
        entries.push(json!(rule));
    }
}

fn encode_fragment(text: &str) -> String {
    let mut result = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut result, "%{byte:02X}").expect("String write");
        }
    }
    result
}
fn decode_fragment(text: &str) -> String {
    let mut bytes = Vec::new();
    let mut input = text.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = input.by_ref().take(2).collect::<Vec<_>>();
            if let Ok(hex) = std::str::from_utf8(&hex) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    bytes.push(value);
                    continue;
                }
            }
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_owned())
}
