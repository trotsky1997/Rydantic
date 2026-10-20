//! Adapts Rydantic's document-root references to provider-friendly schema documents.
use crate::{Error, Result};
use rydantic::{Model, SchemaMode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConstraint {
    pub path: String,
    pub keyword: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedSchema {
    pub schema: Value,
    pub runtime_constraints: Vec<RuntimeConstraint>,
}

/// Preserves full validation in the model while projecting a provider-compatible schema.
pub fn prepare_model_schema<M: Model>() -> Result<PreparedSchema> {
    let mut schema = normalize(M::json_schema(SchemaMode::Input))?;
    let mut runtime_constraints = Vec::new();
    portable_bounds(&mut schema, "", &mut runtime_constraints)?;
    Ok(PreparedSchema {
        schema,
        runtime_constraints,
    })
}

pub fn model_schema<M: Model>() -> Result<Value> {
    Ok(prepare_model_schema::<M>()?.schema)
}

fn portable_bounds(
    value: &mut Value,
    path: &str,
    report: &mut Vec<RuntimeConstraint>,
) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    for keyword in ["const", "enum", "default", "examples"] {
        if object.get(keyword).is_some_and(unsafe_literal) {
            return Err(Error::Configuration(format!("provider schema {keyword} contains a numeric literal outside JavaScript's safe range")));
        }
    }
    for keyword in [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
    ] {
        let unsafe_number = object
            .get(keyword)
            .filter(|value| value.is_number())
            .is_some_and(|value| {
                value
                    .as_f64()
                    .is_none_or(|number| number.abs() > 9_007_199_254_740_991.0)
            });
        if unsafe_number {
            let bound = object.remove(keyword).expect("existing bound");
            let description = object
                .get("description")
                .map(|v| {
                    v.as_str().ok_or_else(|| {
                        Error::Configuration("schema description must be text".into())
                    })
                })
                .transpose()?
                .unwrap_or("");
            let description =
                format!("{description} Runtime-enforced constraint: {keyword} {bound}.");
            object.insert("description".into(), Value::String(description));
            report.push(RuntimeConstraint {
                path: path.into(),
                keyword: keyword.into(),
                value: bound,
            });
        }
    }
    for keyword in [
        "properties",
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
    ] {
        if let Some(Value::Object(children)) = object.get_mut(keyword) {
            for (key, child) in children {
                portable_bounds(
                    child,
                    &format!(
                        "{path}/{keyword}/{}",
                        key.replace('~', "~0").replace('/', "~1")
                    ),
                    report,
                )?;
            }
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(Value::Array(children)) = object.get_mut(keyword) {
            for (index, child) in children.iter_mut().enumerate() {
                portable_bounds(child, &format!("{path}/{keyword}/{index}"), report)?;
            }
        }
    }
    for keyword in [
        "items",
        "additionalItems",
        "additionalProperties",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
        "unevaluatedProperties",
        "unevaluatedItems",
    ] {
        if let Some(child) = object.get_mut(keyword) {
            portable_bounds(child, &format!("{path}/{keyword}"), report)?;
        }
    }
    Ok(())
}

fn unsafe_literal(value: &Value) -> bool {
    match value {
        Value::Number(number) => number
            .as_f64()
            .is_none_or(|n| n.abs() > 9_007_199_254_740_991.0),
        Value::Array(values) => values.iter().any(unsafe_literal),
        Value::Object(values) => values.values().any(unsafe_literal),
        _ => false,
    }
}

fn decode_fragment(fragment: &str) -> Result<String> {
    let bytes = fragment.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = fragment
                .get(i + 1..i + 3)
                .ok_or_else(|| Error::Configuration("invalid schema reference escape".into()))?;
            result.push(
                u8::from_str_radix(pair, 16)
                    .map_err(|_| Error::Configuration("invalid schema reference escape".into()))?,
            );
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result)
        .map(|s| s.replace("~1", "/").replace("~0", "~"))
        .map_err(|_| Error::Configuration("invalid schema reference UTF-8".into()))
}
fn rewrite(value: &mut Value, names: &BTreeMap<String, String>) -> Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get_mut("$ref") {
                let source = reference
                    .as_str()
                    .ok_or_else(|| Error::Configuration("schema $ref must be a string".into()))?;
                if source != "#" {
                    let key = source.strip_prefix("#/$defs/").ok_or_else(|| {
                        Error::Configuration(
                            "provider schemas require local $defs references".into(),
                        )
                    })?;
                    let key = decode_fragment(key)?;
                    let name = names.get(&key).ok_or_else(|| {
                        Error::Configuration("unresolved schema definition".into())
                    })?;
                    *reference = Value::String(format!("#/$defs/{name}"));
                }
            }
            for keyword in [
                "properties",
                "patternProperties",
                "dependentSchemas",
                "$defs",
                "definitions",
            ] {
                if let Some(Value::Object(children)) = object.get_mut(keyword) {
                    for child in children.values_mut() {
                        rewrite(child, names)?;
                    }
                }
            }
            for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
                if let Some(Value::Array(children)) = object.get_mut(keyword) {
                    for child in children {
                        rewrite(child, names)?;
                    }
                }
            }
            for keyword in [
                "items",
                "additionalItems",
                "additionalProperties",
                "contains",
                "not",
                "if",
                "then",
                "else",
                "propertyNames",
                "unevaluatedProperties",
                "unevaluatedItems",
            ] {
                if let Some(child) = object.get_mut(keyword) {
                    rewrite(child, names)?;
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                rewrite(child, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn normalize(mut source: Value) -> Result<Value> {
    let object = source
        .as_object_mut()
        .ok_or_else(|| Error::Configuration("expected a schema object".into()))?;
    let definitions = object
        .remove("$defs")
        .unwrap_or_else(|| Value::Object(Map::new()));
    let definitions = definitions
        .as_object()
        .ok_or_else(|| Error::Configuration("expected $defs object".into()))?;
    let names: BTreeMap<_, _> = definitions
        .keys()
        .enumerate()
        .map(|(index, name)| (name.clone(), format!("def_{index}")))
        .collect();
    let mut renamed = Map::new();
    for (old, value) in definitions {
        let mut value = value.clone();
        rewrite(&mut value, &names)?;
        renamed.insert(names[old].clone(), value);
    }
    rewrite(&mut source, &names)?;
    let object = source.as_object_mut().expect("schema object");
    object.remove("$schema");
    let mut root = if let Some(reference) = object.remove("$ref") {
        let key = reference
            .as_str()
            .and_then(|s| s.strip_prefix("#/$defs/"))
            .ok_or_else(|| Error::Configuration("unsupported root schema reference".into()))?;
        renamed
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| Error::Configuration("unresolved root schema".into()))?
    } else {
        Map::new()
    };
    for (key, value) in object.iter() {
        if root.get(key).is_some_and(|old| old != value) {
            return Err(Error::Configuration(
                "conflicting root reference constraints".into(),
            ));
        }
        root.insert(key.clone(), value.clone());
    }
    if !renamed.is_empty() {
        root.insert("$defs".into(), Value::Object(renamed));
    }
    Ok(Value::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_and_recursive_references_keep_constraints() {
        let result = normalize(serde_json::json!({"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/a%3A%3AB","$defs":{"a::B":{"type":"object","properties":{"next":{"$ref":"#/$defs/a%3A%3AB"},"count":{"type":"integer","minimum":4}},"required":["count"],"additionalProperties":false}}})).unwrap();
        assert_eq!(result["type"], "object");
        assert_eq!(result["properties"]["count"]["minimum"], 4);
        assert_eq!(result["properties"]["next"]["$ref"], "#/$defs/def_0");
        assert_eq!(
            result["$defs"]["def_0"]["properties"]["next"]["$ref"],
            "#/$defs/def_0"
        );
        assert!(result.get("$schema").is_none());
    }
    #[test]
    fn property_names_and_literal_values_are_not_schema_keywords() {
        let input = serde_json::json!({"type":"object","properties":{"$ref":{"type":"string"},"data":{"const":{"$ref":"literal"}}}});
        assert_eq!(normalize(input.clone()).unwrap(), input);
    }

    #[test]
    fn unsafe_integer_bounds_are_reported_and_remain_runtime_enforced() {
        #[derive(rydantic::Model)]
        struct Large {
            number: i64,
        }
        let prepared = prepare_model_schema::<Large>().unwrap();
        assert!(!prepared.runtime_constraints.is_empty());
        assert!(prepared.schema["properties"]["number"]
            .get("maximum")
            .is_none());
        assert!(prepared.schema["properties"]["number"]["description"]
            .as_str()
            .unwrap()
            .contains("9223372036854775807"));
        assert!(Large::parse_json(r#"{"number":9223372036854775808}"#).is_err());
        assert_eq!(
            Large::parse_json(r#"{"number":9223372036854775807}"#)
                .unwrap()
                .number,
            i64::MAX
        );
    }

    #[test]
    fn unsafe_literals_are_rejected_instead_of_rounded() {
        let mut schema = serde_json::json!({"type":"integer","const":9223372036854775807i64});
        assert!(portable_bounds(&mut schema, "", &mut vec![]).is_err());
    }
}
