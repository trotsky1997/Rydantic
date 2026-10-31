# Stable API: published rydantic 0.8.0

Select this track only for registry core and derive exactly **0.8.0**, not a beta,
a same-version Git/path package or current main. The independent
[consumer](../assets/stable-0.8.0/Cargo.toml) pins both packages, has its own
workspace/lock and uses Rust 1.98.1 (verification toolchain, not an MSRV claim).
The [0.6.0](stable-0.6.0.md), [0.7.0](stable-0.7.0.md) and
[source snapshot](source-6006720.md) contracts remain separate.

Sources: published [manifest](https://docs.rs/crate/rydantic/0.8.0/source/Cargo.toml),
[Model methods](https://docs.rs/crate/rydantic/0.8.0/source/src/lib.rs),
[validation context](https://docs.rs/crate/rydantic/0.8.0/source/src/error.rs),
[pipelines](https://docs.rs/crate/rydantic/0.8.0/source/src/pipeline.rs) and
[derive](https://docs.rs/crate/rydantic-derive/0.8.0/source/src/lib.rs).

## Typed validation and call context

Import `rydantic::Model`; `derive` is enabled by default. Typed parsing runs model
rules. `model.validate()` revalidates an existing value without rerunning normalization; direct struct construction
is not validation. Dump methods also validate. Input and output schemas are
separate; structural validation never substitutes for typed callbacks/transforms.

Use `T::parse_value_with_context(value, Arc::new(policy))` or
`T::parse_json_with_context(text, Arc::new(policy))`. The data must be
`Any + Send + Sync` (therefore owned, `'static`), not a borrowed request reference
or `Rc`-backed non-thread-safe value. Callbacks receive `&rydantic::Context`:

- Field `before_with_context`: `fn(Value, &Context) -> Result<Value, ValidationError>`.
- Field `validate_with_context`: `fn(&T, &Context) -> Result<(), ValidationError>`.
- Model `validate_with_context`: `fn(&Self, &Context) -> Result<(), ValidationErrors>`.
- Validators also have `async_validate_with_context` variants; use and await
  `parse_value_async_with_context` / `parse_json_async_with_context`, not synchronous
  parsing that returns `async_validation_required` rather than skipping the check.

The derive attributes accept callback lists. Before hooks can transform defaults;
order matters. `ctx.require_data::<Policy>()?` reports `context_missing` or
`context_type_mismatch`. Do not swallow these errors or infer policy from schemas.
The parsed model does **not** retain the context: resupply it for
`validate_with_context`, `dump_with_context`, `dump_json_with_context` and async
counterparts (`validate_async_with_context`, `dump_async_with_context`,
`dump_json_async_with_context`). Shared context side effects are not rolled back on validation
failure. Context is caller data, not a global store or a secret-redaction guarantee.

## Typed pipelines: explicit composition, not implicit enforcement

`pipeline::Pipeline::plain` constructs a typed stage; `.then` transforms,
`.check` validates without replacing the value and `.run(input, &context)` executes.
Ordinary chains short-circuit on errors. `AsyncPipeline` accepts boxed futures and
its `.run(...).await` is asynchronous. These APIs require no `pipeline` or `async`
feature. They do not automatically execute a Model: explicitly compose a Model
parse/validation stage when that is the contract. Wrapping a callback does not
bypass built-in field constraints outside that callback.

`.wrap` deliberately allows skipping or retrying the inner stage. It is **not** a
promise that the wrapped validator runs exactly once, or at all. Retried side
effects are not transactional; async cancellation is not rollback. Do not describe
normalization, dropped fields, changed constraints or wrapper bypass as lossless.

## Features and limitations

Default: `derive`. Opt-ins: `axum`, `capnp`, `chrono`, `country`, `currency`,
`decimal`, `dynamic`, `mysql`, `openapi`, `partial-json`, `phone`, `postgres`,
`settings`, `sqlite`, `sqlx`. There are no `async`, `pipeline`, `grpc` or `jsonrpc`
features in this published package. Do not import later main features into it.

`openapi` is now available (absent from 0.6.0/0.7.0). Use
`schema::openapi::OpenApiComponents` for schema components and explicit
`SchemaMode::Input` / `Output`. It is not a complete endpoint/router generator,
transport server or proof of runtime validation. Dynamic schemas still cannot
execute Rust context callbacks. Settings uses `settings::Settings`, not a Settings
derive; partial-JSON snapshots remain provisional until final typed validation.

The consumer covers derive/dynamic/settings/partial-json/openapi, context and
pipelines. It does not certify every optional integration, every feature
combination, all async scheduling behavior or platform/toolchain compatibility.

## Breaking upgrade warning

0.8.0 is a breaking minor upgrade from 0.7.x under pre-1.0 semantic versioning.
`Context` no longer implements `UnwindSafe` / `RefUnwindSafe`: it can carry
caller-owned dynamically typed shared state. Existing generic bounds or
`catch_unwind` closures capturing it may stop compiling. Review panic boundaries
and state invariants; do not mechanically add `AssertUnwindSafe` or claim automatic
recovery. The consumer includes negative compile checks for both traits.

Upgrade core and derive together in the application, review its lock/source and
feature choices, then test real models, callbacks, serialization and error paths.
Back up application data/configuration before changing validation or normalization
in a real deployment; this fixture does not migrate or touch production data.
It does not establish old-data compatibility or authorize weakening constraints.

## Verification boundary

Run `cargo +1.98.1 test --manifest-path assets/stable-0.8.0/Cargo.toml --frozen`
from the skill root with prepared dependencies/toolchain; see [testing](testing.md).
CLI knowledge is static discovery/explanation only. No CLI release, installation,
real Agent trial or measured efficacy improvement is implied by this track.
Report actual local/CI results, not source inspection as executed tests.
