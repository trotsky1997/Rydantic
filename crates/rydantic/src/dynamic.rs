use crate::{Context, PathSegment, ValidationError, ValidationErrors, Value};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError(pub String);
impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SchemaError {}

/// Runtime JSON Schema validation. Compile once and reuse the validator.
///
/// This path checks standard JSON Schema assertions. It does not apply defaults,
/// transformations, or `x-rydantic-local-rules`. Network/file resolvers are disabled.
pub struct DynamicModel {
    schema: Value,
    validator: jsonschema::Validator,
}
impl DynamicModel {
    pub fn compile(schema: Value) -> Result<Self, SchemaError> {
        let report = crate::SchemaReport::new(schema);
        if !report.is_portable() {
            return Err(SchemaError("schema contains Rydantic local rules; use the typed model, or explicitly choose compile_structural".into()));
        }
        Self::compile_structural(report.schema)
    }
    /// Explicitly check standard JSON Schema assertions while allowing local annotations.
    pub fn compile_structural(schema: Value) -> Result<Self, SchemaError> {
        let validator =
            jsonschema::validator_for(&schema).map_err(|error| SchemaError(error.to_string()))?;
        Ok(Self { schema, validator })
    }
    pub fn schema(&self) -> &Value {
        &self.schema
    }
    pub fn is_valid(&self, input: &Value) -> bool {
        self.validator.is_valid(input)
    }
    pub fn validate(&self, input: &Value) -> Result<(), ValidationErrors> {
        let mut ctx = Context::new();
        for error in self.validator.iter_errors(input) {
            let mut converted =
                ValidationError::new("json_schema", "value failed a JSON Schema assertion")
                    .with_param("schema_path", error.schema_path.to_string());
            let mut current = input;
            for part in error.instance_path.to_string().split('/').skip(1) {
                let part = part.replace("~1", "/").replace("~0", "~");
                if current.is_array() {
                    if let Ok(index) = part.parse::<usize>() {
                        converted.loc.push(PathSegment::Index(index));
                        current = current.get(index).unwrap_or(&Value::Null);
                        continue;
                    }
                }
                current = current.get(&part).unwrap_or(&Value::Null);
                converted.loc.push(PathSegment::Field(part));
            }
            ctx.push(converted);
        }
        ctx.result()
    }
}
