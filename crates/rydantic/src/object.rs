use crate::{Context, SchemaContext, SchemaMode, Stage, ValidationError, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::BuildHasher;

/// Object shape information used by flattened fields, independent of ownership.
pub trait ObjectType {
    fn field_names(mode: SchemaMode, seen: &mut HashSet<&'static str>) -> Vec<String>;
    fn captures_extra(seen: &mut HashSet<&'static str>) -> bool;
    fn reserved_names(&self, mode: SchemaMode) -> Vec<String> {
        Self::field_names(mode, &mut HashSet::new())
    }
}
/// Storage for `#[model(extra)]`. Keys are merged into the surrounding object.
pub trait ExtraFields: ObjectType {}
impl<T> ObjectType for BTreeMap<String, T> {
    fn field_names(_: SchemaMode, _: &mut HashSet<&'static str>) -> Vec<String> {
        vec![]
    }
    fn captures_extra(_: &mut HashSet<&'static str>) -> bool {
        true
    }
    fn reserved_names(&self, _: SchemaMode) -> Vec<String> {
        self.keys().cloned().collect()
    }
}
impl<T> ExtraFields for BTreeMap<String, T> {}
impl<T, S: BuildHasher> ObjectType for HashMap<String, T, S> {
    fn field_names(_: SchemaMode, _: &mut HashSet<&'static str>) -> Vec<String> {
        vec![]
    }
    fn captures_extra(_: &mut HashSet<&'static str>) -> bool {
        true
    }
    fn reserved_names(&self, _: SchemaMode) -> Vec<String> {
        self.keys().cloned().collect()
    }
}
impl<T, S: BuildHasher> ExtraFields for HashMap<String, T, S> {}
impl<T: ObjectType> ObjectType for Option<T> {
    fn field_names(mode: SchemaMode, seen: &mut HashSet<&'static str>) -> Vec<String> {
        T::field_names(mode, seen)
    }
    fn captures_extra(seen: &mut HashSet<&'static str>) -> bool {
        T::captures_extra(seen)
    }
    fn reserved_names(&self, mode: SchemaMode) -> Vec<String> {
        self.as_ref().map_or_else(
            || T::field_names(mode, &mut HashSet::new()),
            |v| v.reserved_names(mode),
        )
    }
}
impl<T: ObjectType> ObjectType for Box<T> {
    fn field_names(mode: SchemaMode, seen: &mut HashSet<&'static str>) -> Vec<String> {
        T::field_names(mode, seen)
    }
    fn captures_extra(seen: &mut HashSet<&'static str>) -> bool {
        T::captures_extra(seen)
    }
    fn reserved_names(&self, mode: SchemaMode) -> Vec<String> {
        (**self).reserved_names(mode)
    }
}

#[doc(hidden)]
pub fn check_names(names: Vec<String>, ctx: &mut Context) {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name.clone()) {
            ctx.at(name, |ctx| {
                ctx.push(ValidationError::new(
                    "field_collision",
                    "flattened field names overlap",
                ))
            });
        }
    }
}
#[doc(hidden)]
pub fn merge(target: &mut serde_json::Map<String, Value>, value: Value, ctx: &mut Context) {
    if value.is_null() {
        return;
    }
    let Value::Object(object) = value else {
        ctx.push(
            ValidationError::new("flatten_type", "flattened value must encode as an object")
                .with_stage(Stage::Serialize),
        );
        return;
    };
    for (key, value) in object {
        if target.contains_key(&key) {
            ctx.at(key, |ctx| {
                ctx.push(
                    ValidationError::new(
                        "field_collision",
                        "flattened output would overwrite a field",
                    )
                    .with_stage(Stage::Serialize),
                )
            });
        } else {
            target.insert(key, value);
        }
    }
}

/// Merge an object schema into a containing object. Optional objects have conditional requirements.
#[doc(hidden)]
pub fn flatten_schema(
    target: &mut Value,
    child: Value,
    optional: bool,
    capture: bool,
    ctx: &SchemaContext,
) {
    let mut child = ctx.resolve(&child);
    if let Some(branches) = child.get("anyOf").and_then(Value::as_array) {
        if let Some(non_null) = branches
            .iter()
            .find(|v| v.get("type").and_then(Value::as_str) != Some("null"))
        {
            child = ctx.resolve(non_null);
        }
    }
    if let Some(properties) = child.get("properties").and_then(Value::as_object) {
        let mut collision = false;
        let parent = target["properties"].as_object_mut().expect("object schema");
        for (key, value) in properties {
            if parent.insert(key.clone(), value.clone()).is_some() {
                collision = true;
            }
        }
        if collision {
            target["allOf"]
                .as_array_mut()
                .expect("allOf")
                .push(Value::Bool(false));
        }
        if let Some(required) = child.get("required").and_then(Value::as_array) {
            if optional && !required.is_empty() {
                let absent = properties
                    .keys()
                    .map(|key| serde_json::json!({"required":[key]}))
                    .collect::<Vec<_>>();
                target["allOf"].as_array_mut().expect("allOf").push(
                    serde_json::json!({"anyOf":[{"not":{"anyOf":absent}},{"required":required}]}),
                );
            } else {
                target["required"]
                    .as_array_mut()
                    .expect("required")
                    .extend(required.iter().cloned());
            }
        }
    }
    if let Some(extra) = child.get("additionalProperties") {
        if capture && extra != &Value::Bool(false) {
            target["additionalProperties"] = extra.clone();
        }
    }
    if let Some(all) = child.get("allOf").and_then(Value::as_array) {
        target["allOf"]
            .as_array_mut()
            .expect("allOf")
            .extend(all.iter().cloned());
    }
    if let Some(rules) = child
        .get("x-rydantic-local-rules")
        .and_then(Value::as_array)
    {
        for rule in rules.iter().filter_map(Value::as_str) {
            crate::schema::annotate_local(target, rule);
        }
    }
    if child.get("type") != Some(&serde_json::json!("object")) {
        crate::schema::annotate_local(target, "flatten_recursive_or_custom_shape");
    }
}
