# Stable API: published rydantic 0.6.0

This reference is deliberately not a description of current main or any 0.7 beta.
The consumer in `assets/stable-0.6.0` uses registry dependencies, not workspace paths.
[API source](https://docs.rs/crate/rydantic/0.6.0/source/src/lib.rs).

## Typed entry points

Import `rydantic::Model` to use trait methods. With default `derive`:

    use rydantic::{json, Model, SchemaMode};
    #[derive(Debug, Model)]
    struct User {
        #[model(trim, min_length = 2)]
        name: String,
        #[model(ge = 18, le = 120)]
        age: u8,
    }
    let user = User::parse_value(json!({"name":" Ada ","age":18}))?;
    let output = user.dump()?;
    let schema = User::json_schema(SchemaMode::Output);

This fragment belongs in a function returning a compatible Result. The bundled
consumer contains complete runnable tests rather than pretending this is a binary.

- `T::parse_json(&str)`, `parse_slice(&[u8])`, `parse_value(Value)`,
  `parse_raw(&RawValue)`, `parse_reader(reader)` return `Result<T, ValidationErrors>`.
- `value.dump()` returns `Result<Value, ValidationErrors>`;
  `dump_json()` returns `Result<String, ValidationErrors>`.
- `T::json_schema(SchemaMode::Input | SchemaMode::Output)` returns a JSON Value.
- `T::schema_report(mode)` returns schema plus local rules, with `is_portable()`.
- Async counterparts include `parse_json_async`, `parse_raw_async`,
  `parse_value_async`, `dump_async`, `dump_json_async`. Inspect the precise
  signature and concrete callback contract before writing async custom validation;
  synchronous success does not prove an async validator executed.

Do not invent `Model::validate`, `model_json_schema`, `JsonSchema` derive,
`DynamicModel::parse`, or an automatic typed CLI adapter. Calling Serde directly
or constructing a struct is not equivalent to running the Model parse pipeline.

## Presence and constraints

- Ordinary `Option<T>` can be missing or null. `#[model(required)]` on an Option
  requires the key but still permits null.
- `#[model(default)]` or `#[model(default = 3)]` supplies a missing field; explicit
  null is not the same as missing. Defaults are subject to validation.
- `#[model(ge = 18, le = 120)]` sets inclusive numeric bounds.
- `min_length`/`max_length`, `pattern`, `email` are supported field attributes.
  Test Unicode boundaries and do not replace typed email checks with format annotations.
- `trim`, `lowercase`, and `coerce` change runtime interpretation and can produce
  local-rule annotations. Coercion is explicit, not a default promise.
- Unknown fields are forbidden by default. `#[model(extra = "ignore")]` is a
  contract change, not a generic repair for failing fixtures.
- `validation_alias` and `serialization_alias` select different input/output keys;
  `exclude` omits output. Never substitute one schema mode for the other.

## Stable limitations stay stable

Later fixes for callback path hygiene, alias handling, validator execution,
settings, partial JSON or other evolving areas must be checked against published
0.6.0 before recommending them. This package makes no promise of beta fixes in
stable. For an unsupported combination, retain a minimal reproducer and explain
the verified limitation or seek an explicitly chosen upgrade. Never rewrite the
user's constraints to hide a failure.
