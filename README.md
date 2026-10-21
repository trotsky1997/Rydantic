<div align="center">

# Rydantic

**Validated Rust models, from JSON to HTTP, SQL, and Cap'n Proto.**

[![CI](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml/badge.svg)](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rydantic.svg)](https://crates.io/crates/rydantic)
[![License: MIT](https://img.shields.io/badge/license-MIT-31A879.svg)](https://github.com/trotsky1997/Rydantic/blob/main/LICENSE)

[Documentation](https://rydantic.mintlify.site/introduction) · [Quick start](https://rydantic.mintlify.site/quickstart) · [Stable API](https://docs.rs/rydantic/0.6.0/rydantic/) · [Homepage](https://trotsky1997.github.io/Rydantic/)

</div>

Define a model once. Keep parsing, normalization, validation, and serialization rules next to its fields. Add the integrations your application needs without maintaining a separate validation layer for every boundary.

```rust
#[derive(rydantic::Model)]
struct User {
    #[model(trim, min_length = 3)]
    name: String,
    #[model(coerce, ge = 18, le = 120)]
    age: u8,
}
```

## Coding Agent toolkit (P0)

The portable [Rydantic Skill](skills/rydantic/SKILL.md) includes an exact stable
0.6.0 consumer recipe and adoption guidance. The unpublished [CLI](crates/rydantic-cli/README.md)
provides offline `doctor`, `explain` and versioned JSON diagnostics, not validation
or code generation. The [three-arm evaluation harness](evals/rydantic-agent/README.md)
keeps harness smoke tests separate from actual Agent trials; no measured Agent
improvement is claimed. Nothing is installed or executed automatically.

## What you get

| Capability | What it does |
| --- | --- |
| **Validated JSON** | Explicit coercion, string normalization, defaults, nested models, and structured errors. |
| **Serde + JSON Schema** | Generated serialization/deserialization and input/output schemas from the same model. |
| **HTTP with Axum** | Validated request extractors, responses, and structured rejections. |
| **SQL with SQLx** | Model metadata, validated CRUD, queries, and migration planning for SQLite, PostgreSQL, and MySQL. |
| **Native Cap'n Proto** | Generate schemas and official Rust bindings, or connect an existing protocol. |
| **More model shapes** | Borrowed strings, enums, generics, recursive models, async validators, and streaming JSON. |
| **RydanticAI** | Independently versioned `rydantic-ai`: typed agents and tools, [image inputs](https://rydantic.mintlify.site/integrations/images), [PDF inputs in development](https://rydantic.mintlify.site/integrations/pdf), [structured output streams](https://rydantic.mintlify.site/integrations/structured-output), resumable graphs, an authenticated three-protocol gateway, and a [TypeSafe Jev SDK](https://rydantic.mintlify.site/integrations/jev). |

Integrations are opt-in Cargo features. The default `derive` feature covers the core model API.

## Start with a validated model

For the current **stable 0.6.0** release:

```toml
[dependencies]
rydantic = "0.6.0"
```

```rust
use rydantic::{Model, SchemaMode};

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 3)]
    name: String,
    #[model(coerce, ge = 18, le = 120)]
    age: u8,
}

fn main() -> Result<(), rydantic::ValidationErrors> {
    let user = User::parse_json(r#"{"name":" Alice ","age":"25"}"#)?;

    assert_eq!(user.name, "Alice");
    assert_eq!(user.age, 25);
    println!("{}", user.dump_json()?);
    // {"age":25,"name":"Alice"}

    let schema = User::json_schema(SchemaMode::Output);
    assert!(schema.is_object());
    Ok(())
}
```

`coerce` makes conversion explicit. Invalid fields produce structured errors with locations and codes. `Model` also generates Serde implementations: deserialization validates input, and serialization revalidates the current object.

Continue with [validation rules](https://rydantic.mintlify.site/models/validation), [model shapes](https://rydantic.mintlify.site/models/structure), or [JSON Schema](https://rydantic.mintlify.site/models/schema).

## Choose your integrations

For AI applications, add the separate `rydantic-ai` crate. It supports Responses, Chat Completions and Anthropic Messages APIs through BASEURL and APIKEY, accepts ordered URL/Base64 image inputs, validates typed outputs and tool arguments, streams partial JSON snapshots and validated list items, and provides graph checkpoints and a gateway that converts between all three protocols with routing and budgets. See the [RydanticAI guide](https://rydantic.mintlify.site/integrations/ai) and [crate README](crates/rydantic-ai/README.md).

The development source adds [PDF document inputs](https://rydantic.mintlify.site/integrations/pdf), with embedded PDF conversion across all nine protocol combinations and URL references for Responses/Messages.

Enable features on the version you selected:

```toml
# Stable HTTP + PostgreSQL example
rydantic = { version = "0.6.0", features = ["axum", "postgres"] }
```

| Build with | Features | Guide |
| --- | --- | --- |
| HTTP APIs | `axum` | [Axum](https://rydantic.mintlify.site/integrations/axum) |
| Relational data | `sqlite`, `postgres`, or `mysql` | [SQL models](https://rydantic.mintlify.site/integrations/sql) · [Migrations](https://rydantic.mintlify.site/integrations/migrations) |
| Binary protocols | `capnp` | [Generated schemas](https://rydantic.mintlify.site/integrations/capnp) · [Existing schemas](https://rydantic.mintlify.site/integrations/existing-capnp) |
| Dates and exact decimals | `chrono`, `decimal` | [Dates and Decimal](https://rydantic.mintlify.site/models/dates-decimal) |
| Runtime JSON Schema | `dynamic` | [JSON Schema](https://rydantic.mintlify.site/models/schema) |

See the [complete feature reference](https://rydantic.mintlify.site/reference/features). Automatic Cap'n Proto binding generation also requires the official `capnp` compiler on `PATH`.

## Stable and beta

| Channel | Version | Use it for |
| --- | --- | --- |
| **Stable** | `0.6.0` | Published API, with [versioned source documentation](https://github.com/trotsky1997/Rydantic/tree/v0.6.0) and [Rust API reference](https://docs.rs/rydantic/0.6.0/rydantic/). |
| **Source candidate** | `0.7.0-beta.4` | Testing opt-in phone, country and currency types; retaining partial JSON, settings and the beta.1 upgrade protections. |

A version in this branch is **not a publication announcement**. Check [published versions](https://crates.io/crates/rydantic/versions) before installing a candidate. Once published, opt in exactly with `rydantic = "=0.7.0-beta.4"`. The guides follow the working source; use the versioned links above for stable behavior.

> **Breaking upgrade:** 0.7.0-beta.1 removes automatic `Model` derive insertion inside `#[rydantic::capnp_models]`. Add an explicit `#[derive(rydantic::Model)]` to every model struct and enum, including nested models. The beta also rejects unsupported custom SQLite index layouts before a table rebuild. Read the [changelog and migration guidance](https://github.com/trotsky1997/Rydantic/blob/main/CHANGELOG.md) before upgrading.

Breaking changes are allowed in this early-stage library. Application owners are responsible for backups, migration review, and deployment validation. Unsupported SQLite layouts need a reviewed migration, not removal of constraints to bypass the guard. We do not describe destructive changes as lossless.

## Know the boundaries

- JSON Schema cannot express every normalization, coercion, or custom-validation rule. Schema validation and model validation are not interchangeable.
- Reusing models reduces repeated rules; it does not replace separate public, internal, and storage models when their contracts differ.
- Native Cap'n Proto bindings use concrete, owned models. Keep protocol IDs and ordinals stable when wire compatibility matters.
- Conditional definitions inside `capnp_models` have a [known limitation](https://github.com/trotsky1997/Rydantic/issues/8); this beta does not claim general conditional schema generation.

## Explore and contribute

The workspace contains the `rydantic` runtime, `rydantic-derive` macros, and executable integration fixtures. Browse [examples](https://github.com/trotsky1997/Rydantic/tree/main/crates/rydantic/examples) for tool arguments, HTTP, databases, and binary protocols.

```sh
cargo run -p rydantic --example user
cargo run -p rydantic --example tool_call -- --schema
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

Use Rust 1.98.1 to match the repository's tested toolchain, and install the official Cap'n Proto compiler for all-feature checks. Database integration tests require their configured services; see [CI](https://github.com/trotsky1997/Rydantic/blob/main/.github/workflows/ci.yml) for the full matrix. To work on the site, see [documentation development](https://github.com/trotsky1997/Rydantic/blob/main/docs/README.md#preview-and-validate).

Found a bug? [Open an issue](https://github.com/trotsky1997/Rydantic/issues) with a minimal model, input, expected behavior, and crate version. Small fixes with regression tests are welcome.

---

[MIT license](https://github.com/trotsky1997/Rydantic/blob/main/LICENSE)
