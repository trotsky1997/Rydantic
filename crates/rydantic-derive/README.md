# Rydantic derive

Procedural macros for [Rydantic](https://crates.io/crates/rydantic), a Rust library for data modeling, validation, JSON Schema and native Cap'n Proto.

Use the `rydantic` crate, which enables and re-exports `#[derive(Model)]` by default. This package describes the **0.8.0-beta.1 prerelease contract**, including ordered callback lists, typed per-call context callbacks and integration with synchronous/asynchronous pipelines. Runtime and derive use matching versions with an exact `=0.8.0-beta.1` derive dependency. Source metadata alone does not prove registry availability: verify both [runtime](https://crates.io/crates/rydantic/0.8.0-beta.1) and [derive](https://crates.io/crates/rydantic-derive/0.8.0-beta.1) packages and the [release notes](https://rydantic.mintlify.site/reference/releases) before using this exact beta pin:

```toml
[dependencies]
rydantic = "=0.8.0-beta.1"
```

The default stable release remains 0.7.0 (`rydantic = "=0.7.0"`); it does not include these new validator APIs. `before` lists run left to right and stop on failure; typed validator lists run sequentially and accumulate errors behind the existing prerequisite gates. Pipelines instead fail fast between stages. Existing single callbacks use the same mechanism, not a compatibility layer.

> **Breaking from 0.7.0:** runtime `Context` no longer implements `UnwindSafe` or `RefUnwindSafe`, even without data, because its erased `Any + Send + Sync` payload omits those markers. Code capturing it across `catch_unwind` may stop compiling. Construct and consume the context inside the closure or avoid capturing it. Use `AssertUnwindSafe` only after auditing all captured values and shared payloads, never as a blind wrapper.

```rust
use rydantic::Model;

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(ge = 18, le = 120)]
    age: u8,
}

let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
assert_eq!(user.name, "Alice");
# Ok::<(), rydantic::ValidationErrors>(())
```

See the [API documentation](https://docs.rs/rydantic) and [complete guide](https://github.com/trotsky1997/Rydantic) for borrowing, validators, model attributes, schemas and Cap'n Proto mappings.

With `rydantic`'s `capnp` feature, `#[rydantic::capnp_models]` on an inline model module also infers the complete Cap'n Proto schema, invokes the installed official `capnp` compiler, and generates native Rust bindings automatically. Each struct and enum must explicitly derive `rydantic::Model` (or an imported alias). Consumers maintain Rust model definitions without a separate schema, generated source file, or build script. **Breaking change in 0.7.0-beta.1:** `capnp_models` no longer inserts `Model` derives; add the derive to models that previously relied on automatic insertion before upgrading. The schema is available through `CapnpSchema` and the model module's `CAPNP_SCHEMA` constant.

The existing data and deployment boundaries remain: conditional Cap’n Proto models can produce incompatible wire schemas across configurations, and SQLite rebuilds reject unsupported custom index layouts. Review migration plans and keep backups; unsupported schemas need reviewed manual migration, not removal of constraints to bypass guards. Callback transformations do not migrate stored data or promise lossless conversion.

License: MIT.

Enable `sqlite`, `postgres` or `mysql` for `#[derive(Model, SqlModel)]`: the macro generates portable table metadata, validated SQLx row decoding for enabled drivers, and asynchronous CRUD with transactions. The pool type selects SQLite, PostgreSQL or MySQL while model definitions stay the same. `migrate` derives upgrades from the live catalog; `rename_from` preserves renamed columns and constant SQL defaults backfill required additions. Preview changes with `migration_plan` and explicitly opt in to column removal or storage conversions. Combine it with the `axum` feature to use one model for HTTP JSON validation and database persistence. See the complete guide for `#[sql(...)]` attributes, queries, and the Tokio HTTP server example.
