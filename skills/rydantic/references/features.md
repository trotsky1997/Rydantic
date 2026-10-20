# Version and feature discovery

1. Read the target package's manifest and workspace inheritance. A dependency
   may be renamed: `rd = { package = "rydantic", version = "=0.6.0" }`. Resolve
   package identity separately from the Rust import name; inspect exact-version
   derive support before promising that every renamed-dependency case works.
2. Read the lockfile's version and source for **all** matching core and derive
   packages. A Git/path dependency is not the published crate merely because its
   version field matches. Without a lockfile, report the resolved version unknown.
3. Read requested/default features and target/optional declarations. Manifest
   discovery cannot establish the active feature graph. Frozen Cargo metadata,
   when separately authorized and available, establishes resolution, not a build.
   P0 doctor intentionally does not run it (`--resolve` is unsupported).
4. Check installed toolchains before invoking rustup shims; do not trigger an
   automatic toolchain installation for discovery. Missing cache or lock data is
   an environment blocker, not evidence of a source bug.

## Published 0.6.0 feature names

Default: `derive`. Opt-in: `dynamic`, `chrono`, `decimal`, `capnp`, `axum`,
`sqlx`, `sqlite`, `postgres`, `mysql`.

- `derive`: `#[derive(Model)]`. Turning off default features removes this macro
  unless explicitly restored; it does not remove the Model trait.
- `dynamic`: runtime JSON Schema validation (`DynamicModel`).
- `chrono`, `decimal`: corresponding optional type support.
- `capnp`: Cap'n Proto integration; inspect compiler/tool requirements separately.
- `axum`: HTTP integration. Do not enable it for pure model validation.
- `sqlx`: SQL support; backend features select SQLite/PostgreSQL/MySQL integrations.

Enable only the integration needed. There are **no** `settings`, `partial-json`,
or `async` features in 0.6.0. Async Model methods exist without a feature named
`async`; execution still needs the project's async runtime. `rydantic-ai` is a
separate package, not a core feature. Domain/streaming APIs documented on current
main are not automatically part of this stable reference.

Source: published [0.6.0 manifest](https://docs.rs/crate/rydantic/0.6.0/source/Cargo.toml).
The bundled consumer pins both core and derive: core's transitive derive range
otherwise allows compatible newer 0.6.x macros. Do not impose an upgrade or
exact-pin policy on the user's application without a task-specific reason.

## Built-in capability boundaries, not just feature names

Verified against the published 0.6.0 package's module declarations, reexports,
Model methods and source files (2026-10-20):

| Requested capability | Published 0.6.0 evidence | Decision while retaining the exact pin |
| --- | --- | --- |
| Built-in settings loader (environment/dotenv source layering) | No settings module, settings loader export or settings derive in core/derive 0.6.0. | Do not invent a `Settings` derive, loader method or feature. Existing application configuration code or a separately selected library is a different solution. |
| Partial/incomplete JSON snapshots | No partial-JSON module, snapshot parser or partial-JSON API export. | Do not invent `PartialJson` or treat truncated JSON as a validated model. An independently implemented parser would be new application work, not a hidden stable API. |
| Complete JSON document sequences / NDJSON | `stream::json_stream`, `borrow_stream` and `ndjson` exist; they parse complete records and pass them to Model/BorrowModel parsing. | Use only when complete-record streaming is the requirement. This is not incremental recovery of an unfinished JSON value. |

Evidence: [core module/export surface and Model methods](https://docs.rs/crate/rydantic/0.6.0/source/src/lib.rs),
[complete-record stream implementation](https://docs.rs/crate/rydantic/0.6.0/source/src/stream.rs),
and [derive entrypoints](https://docs.rs/crate/rydantic-derive/0.6.0/source/src/lib.rs).
These are capability claims about this published library version, not claims that
Rust applications cannot implement configuration loading or partial parsing.
Do not silently expand the task into either implementation or upgrade. Establish
the desired alternative and preserve final typed validation where it is required.
