# Stable API: published rydantic 0.7.0

Select this reference only for core and derive exactly 0.7.0, not current main or
0.8 beta. The independent [consumer](../assets/stable-0.7.0/Cargo.toml) pins both
registry packages and Rust 1.98.1. The [0.6.0 track](stable-0.6.0.md) remains
separate; evaluation tasks pinned to that version must not be upgraded.

Sources: published [core manifest](https://docs.rs/crate/rydantic/0.7.0/source/Cargo.toml),
[core API](https://docs.rs/crate/rydantic/0.7.0/source/src/lib.rs) and
[derive API](https://docs.rs/crate/rydantic-derive/0.7.0/source/src/lib.rs).

## Typed contracts

Import `rydantic::Model` for trait methods and use `#[derive(Model)]` with the
`derive` feature (enabled by default). `T::parse_value` returns a validated model;
`model.dump()` returns output JSON. `T::json_schema(SchemaMode::Input)` and
`SchemaMode::Output` describe different contracts. `schema_report` records local
rules; structural-only validation does not run typed transforms or callbacks.
Do not invent `Model::validate`, `JsonSchema` derive or a typed CLI adapter.

The consumer exercises actual trim/coerce results, inclusive bounds, default
insertion, missing/null distinctions, required Option fields, unknown-field
rejection, input/output aliases, output exclusion, dynamic-schema limitations
and structured diagnostic code/stage/location. Direct struct construction or
Serde deserialization is not proof that this parse pipeline ran.

## Feature names and newly available routes

Default: `derive`. Available opt-ins: `dynamic`, `chrono`, `decimal`, `capnp`,
`axum`, `sqlx`, `sqlite`, `postgres`, `mysql`, `settings`, `partial-json`, `phone`,
`country`, `currency`. There is no `async` feature. Enable only what the task needs;
this consumer exercises `derive`, `dynamic`, `settings` and `partial-json`, not
all optional integrations or domain validators.

- [Settings](https://docs.rs/crate/rydantic/0.7.0/source/src/settings.rs): use
  `settings::Settings`, not an invented `Settings` derive. Sources are explicit
  and ordered; later sources override earlier ones, then `load::<T>()` performs
  Model validation/defaults. Use `pairs` with `ValueMode` in deterministic tests
  rather than modifying process-global environment. Null is not a missing value.
  Source/validation errors are redacted, but the returned model's Debug/dump is
  not automatically secret-safe. The consumer tests layering and rejection
  without reading environment variables or files.
- [Partial JSON](https://docs.rs/crate/rydantic/0.7.0/source/src/partial_json.rs):
  `partial_json::snapshot` accepts bytes and returns a provisional value and
  syntactic `Status`. Incomplete strings/keys are unsupported. A Complete status
  means the current buffer is syntactically complete, not that the transport
  finished or the model is valid. Run the final complete value through typed
  parsing. The consumer checks incomplete recovery, incomplete-string rejection,
  a syntactically complete but typed-invalid value, and final normalization.

## Boundaries and verification

The optional toolkit CLI still bundles only 0.6.0 knowledge. For 0.7.0, its
unknown-version response is not evidence that settings or partial-json is absent.
Use this exact source and the consumer. No automatic install, release, provider
execution or application upgrade is authorized by this reference.

Run the consumer as described in [testing](testing.md). Source inspection and
fixture code are not execution evidence; report the actual CI/local commands and
outcomes. These are library contract tests, not A/B/C Agent performance trials.
