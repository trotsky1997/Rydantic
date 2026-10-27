<div align="center">

# Rydantic

**Validated Rust models, from JSON to HTTP, SQL, and Cap'n Proto.**

[![CI](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml/badge.svg)](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rydantic.svg)](https://crates.io/crates/rydantic)
[![License: MIT](https://img.shields.io/badge/license-MIT-31A879.svg)](https://github.com/trotsky1997/Rydantic/blob/main/LICENSE)

[Documentation](https://rydantic.mintlify.site/introduction) · [Quick start](https://rydantic.mintlify.site/quickstart) · [Tutorials](https://rydantic.mintlify.site/tutorials/overview) · [Examples](https://rydantic.mintlify.site/examples/overview) · [API documentation](https://docs.rs/rydantic/latest/rydantic/) · [Homepage](https://trotsky1997.github.io/Rydantic/)

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
| **RydanticAI** | Independently versioned `rydantic-ai`: typed agents and tools, [image inputs](https://rydantic.mintlify.site/integrations/images), [PDF inputs in the unpublished 0.7.0 source candidate](https://rydantic.mintlify.site/integrations/pdf), [structured output streams](https://rydantic.mintlify.site/integrations/structured-output), resumable graphs, an authenticated three-protocol gateway, and a [TypeSafe Jev SDK](https://rydantic.mintlify.site/integrations/jev). |

Integrations are opt-in Cargo features. The default `derive` feature covers the core model API.

## Start with a validated model

The default installation selects **0.8.0 stable**. Consult the
[live release record](https://rydantic.mintlify.site/reference/releases) and
[stable registry receipt](https://github.com/trotsky1997/Rydantic/blob/main/acceptance/core-0.8/STABLE-REGISTRY.md)
for publication and independent verification before installation:

```toml
[dependencies]
rydantic = "=0.8.0"
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

The unpublished RydanticAI 0.7.0 source candidate adds [PDF document inputs](https://rydantic.mintlify.site/integrations/pdf), with embedded PDF conversion across all nine protocol combinations and URL references for Responses/Messages. It pins the published core release `=0.8.0`.

Enable features on the version you selected:

```toml
# 0.8.0 HTTP + PostgreSQL (see live release status)
rydantic = { version = "=0.8.0", features = ["axum", "postgres"] }
```

| Build with | Features | Guide |
| --- | --- | --- |
| HTTP APIs | `axum` | [Axum](https://rydantic.mintlify.site/integrations/axum) |
| Relational data | `sqlite`, `postgres`, or `mysql` | [SQL models](https://rydantic.mintlify.site/integrations/sql) · [Migrations](https://rydantic.mintlify.site/integrations/migrations) |
| Binary protocols | `capnp` | [Generated schemas](https://rydantic.mintlify.site/integrations/capnp) · [Existing schemas](https://rydantic.mintlify.site/integrations/existing-capnp) |
| Dates and exact decimals | `chrono`, `decimal` | [Dates and Decimal](https://rydantic.mintlify.site/models/dates-decimal) |
| Runtime JSON Schema | `dynamic` | [JSON Schema](https://rydantic.mintlify.site/models/schema) |

> **Unreleased source integration:** optional `grpc` adds [Cap’n Proto over gRPC unary transport](docs/integrations/grpc-capnp.mdx) with tonic custom codecs. It is not included in the published 0.8.0 package and is not native Cap’n Proto RPC.

See the [complete feature reference](https://rydantic.mintlify.site/reference/features). Automatic Cap'n Proto binding generation also requires the official `capnp` compiler on `PATH`.

## Core 0.8.0 contract

This package describes the **0.8.0 contract**, retaining the verified beta.2 runtime behavior. Source metadata alone is not proof of publication. Consult the [live release status](https://rydantic.mintlify.site/reference/releases) and verify matching [runtime](https://crates.io/crates/rydantic/0.8.0) and [derive](https://crates.io/crates/rydantic-derive/0.8.0) availability before using the exact version pin:

```toml
[dependencies]
rydantic = "=0.8.0"
```

Runtime and derive share this version, with an exact `=0.8.0` derive dependency. The 0.8 contract includes [ordered callback lists](docs/models/validation.mdx#ordered-callback-lists), [typed per-call business context](docs/models/validation-context.mdx), and [synchronous/asynchronous pipelines](docs/models/validator-extensions.mdx). `before` lists stop at the first failure; typed check lists accumulate errors in declaration order, behind the existing prerequisite gates. Pipelines are sequential and fail-fast; their explicit wrappers may skip or repeat stages, without automatic rollback or a guarantee of model validity. Existing single callbacks use the same mechanism, not a compatibility layer.

> **Breaking upgrade from 0.7.0:** `Context` no longer implements `UnwindSafe` or `RefUnwindSafe`, even when it carries no application data. Its erased `Any + Send + Sync` payload does not retain those unwind-safety markers. Code capturing a context across `catch_unwind` may stop compiling. Construct and consume it inside the closure or avoid capturing it. Use `AssertUnwindSafe` only after auditing every captured value and shared payload for panic safety, never as a blind wrapper. Ordinary validation failures remain `Result` errors.

The 0.8 contract does not migrate stored data or promise lossless conversion. The explicit Cap’n Proto derives, wire-configuration limits, SQLite guards and backup responsibilities below still apply. Only the runtime and derive are part of this core release; RydanticAI remains independently versioned, and CLI/codegen remain unpublished source tools. See the [release notes](https://rydantic.mintlify.site/reference/releases) for publication status and verification, not source metadata alone.

The optional `openapi` feature in this source adds bounded input/output schema components, local reference relocation and runtime-only rule metadata. It is not route discovery or complete OpenAPI document validation, and unsupported schema scopes fail closed. Published beta.1 predates this feature; published beta.2 includes it. See [beta.2 registry acceptance](https://github.com/trotsky1997/Rydantic/blob/main/acceptance/core-0.8/BETA2-REGISTRY.md) for verified packages and maintainer-operated integration results. See the [first-party application acceptance](https://github.com/trotsky1997/Rydantic/blob/v0.8.0-beta.2/acceptance/core-0.8/README.md) for integration feedback and limits; synthetic requests are not external adoption evidence.

## Historical 0.7.0 installation and migration snapshot

At this README's 2026-10-25 preparation checkpoint, before 0.8.0 publication, the default stable release was 0.7.0: tested settings, partial JSON and domain types, callback hygiene, and conditional Cap’n Proto generation. Its runtime and derive use matching 0.7.0 versions, with an exact derive dependency pin. The 0.8 validator APIs above are not included in 0.7.0.

Source metadata alone does not prove registry availability. Before installing, verify both [runtime](https://crates.io/crates/rydantic/0.7.0) and [derive](https://crates.io/crates/rydantic-derive/0.7.0) packages and the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0). Use `rydantic = "=0.7.0"` for this release. For older 0.6.0 deployments, consult the [versioned source](https://github.com/trotsky1997/Rydantic/tree/v0.6.0) and [API reference](https://docs.rs/rydantic/0.6.0/rydantic/); consult the live guides and release status for the current default; the versioned 0.7.0 guides do not include the 0.8 APIs.

> **Breaking upgrade from 0.6.0:** 0.7.0 removes automatic `Model` derive insertion inside `#[rydantic::capnp_models]`. Add an explicit `#[derive(rydantic::Model)]` to every model struct and enum, including nested models. The release also rejects unsupported custom SQLite index layouts before a table rebuild. Conditional Cap’n Proto definitions produce configuration-specific wire schemas; different feature/target configurations are not automatically wire-compatible. Read the [changelog and migration guidance](https://github.com/trotsky1997/Rydantic/blob/main/CHANGELOG.md) before upgrading.

Breaking changes are allowed in this early-stage library. Application owners are responsible for backups, migration review, and deployment validation. Unsupported SQLite layouts need a reviewed migration, not removal of constraints to bypass the guard. We do not describe destructive changes as lossless.

## Know the boundaries

- JSON Schema cannot express every normalization, coercion, or custom-validation rule. Schema validation and model validation are not interchangeable.
- Reusing models reduces repeated rules; it does not replace separate public, internal, and storage models when their contracts differ.
- Native Cap'n Proto bindings use concrete, owned models. Keep protocol IDs and ordinals stable when wire compatibility matters.
- `capnp_models` evaluates `cfg` and `cfg_attr` in the consuming crate and generates only the active configuration. Conditional fields or variants can change wire layouts; a shared protocol ID does not make different configurations compatible. See the [conditional-compilation boundaries](docs/integrations/capnp.mdx#conditional-compilation) before deploying different builds.

## Explore and contribute

See the [project roadmap](ROADMAP.md) for Now / Next / Later priorities, acceptance gates and explicit non-goals.

The workspace contains the `rydantic` runtime, `rydantic-derive` macros, and executable integration fixtures. Browse [API examples](https://github.com/trotsky1997/Rydantic/tree/main/crates/rydantic/examples) for tool arguments, HTTP, databases, and binary protocols. The standalone [Dioxus forum](examples/forum/README.md) combines validated models with SQLite persistence in a fullstack app.

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

## Performance comparisons

The [Rydantic / Pydantic benchmark suite](benchmarks/pydantic/README.md) checks
shared fixture semantics before measuring JSON parse + validation, and reports
predecoded copy + validation separately. It records raw samples and provenance;
CI checks correctness, not speed. Known semantic differences are explicit and
excluded from comparative timings. No universal performance ranking is implied.
