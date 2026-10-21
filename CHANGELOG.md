# Rydantic 0.8.0-beta.1 · Published prerelease

Published on 2026-10-21 as a prerelease: [runtime](https://crates.io/crates/rydantic/0.8.0-beta.1) and [derive](https://crates.io/crates/rydantic-derive/0.8.0-beta.1). This is a breaking opt-in beta, not a new stable release. Stable/default remains 0.7.0. Install with `rydantic = "=0.8.0-beta.1"`; runtime and derive share that version with an exact `=0.8.0-beta.1` derive pin. See the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0-beta.1).

## Added and scope

- Ordered, nonempty callback lists for field/model hooks. `before` transforms pass outputs left to right and stop at the first error. Typed sync/async check lists run all callbacks sequentially in declaration order and collect errors; existing prerequisite gates are evaluated before the list. Independent sibling errors still accumulate, model checks require successful fields, and any synchronous failure prevents the async phase. Parsing and typed revalidation retain their distinct field gates.
- Explicit context-aware hook variants and owned `Model` entry points carry an `Arc` of `Any + Send + Sync` application data through nested validation and encoding. Exact-type retrieval reports `context_missing` or `context_type_mismatch` as ordinary validation errors. Models do not retain the data; supply it again for later validation/dumping. Ordinary entry points, Serde and existing high-level Axum/SQL/Cap’n Proto paths remain context-free; there are no high-level `BorrowModel` context entry points.
- Typed `Pipeline` and `AsyncPipeline` adapters offer `plain`, `then`, `check` and explicit `wrap` composition. Stages are sequential and fail-fast, preserve their error type, and do not implicitly validate models. Wrappers can skip or repeat their continuation; side effects are not rolled back. Dropping a pending async run prevents later stages, not completed effects or synchronous work already executing. No scheduler, implicit retries, derive-level plain/wrap hooks, or Pydantic parity is claimed.
- Existing single callbacks continue through the same implementation, not a compatibility layer. This core release includes only `rydantic` and `rydantic-derive`. `rydantic-ai` remains independently versioned at 0.6.0; its source path dependency tracks this beta solely for workspace resolution, not an AI publication. CLI/codegen remain at 0.1.0 and unpublished. Frozen Skill/evaluation recipes keep their original contracts.

## Breaking Context panic-boundary contract

`Context` no longer implements `UnwindSafe` or `RefUnwindSafe`, including a context constructed without business data. Its new erased `Arc<dyn Any + Send + Sync>` payload does not retain the unwind-safety marker traits. Capturing an owned or shared context across `std::panic::catch_unwind`, or requiring those marker bounds, can therefore stop compiling even if the concrete payload would have met the bounds. This is an API break from 0.7.0, not merely an additive context API.

Construct and consume `Context` inside the `catch_unwind` closure, or avoid capturing it across that boundary. Any other captured state must still satisfy the closure's requirements. Use `AssertUnwindSafe` only after explicitly auditing every captured value and shared payload for panic safety; do not blindly wrap a context or closure to silence the error. Ordinary validation errors are `Result` values and do not require panic catching. No compatibility layer restores the old auto-trait contract.

## Data and deployment responsibilities

Adopting new transforms or checks can change accepted inputs and outputs. Pipelines are not validated-value wrappers, do not synthesize portable JSON Schema for arbitrary business rules, and do not migrate persisted values. No lossless upgrade or automatic production-data migration is promised.

The 0.7.0 explicit `Model` derive requirement, configuration-specific Cap’n Proto wire schemas, SQLite unsupported-index guards and manual-migration/backup responsibilities remain. Keep shared wire contracts fixed or validate interoperability deliberately; never remove database constraints to bypass migration guards. Existing settings secret-handling, partial JSON completion/size/depth limits and domain-catalog provenance boundaries still apply. See the stable release notes below and the [context](docs/models/validation-context.mdx) and [pipeline](docs/models/validator-extensions.mdx) guides before upgrading.

## Release verification

The guarded [publication workflow](https://github.com/trotsky1997/Rydantic/actions/runs/39559766878) succeeded for source commit [9bc51b3](https://github.com/trotsky1997/Rydantic/commit/9bc51b31839aa76fd8db4037a8f4526cc5b3bc5b). Both downloaded registry archives matched their registry API and sparse-index checksums and the final local packages. Their VCS metadata identifies that source commit; both versions were confirmed unyanked, and the runtime pins derive to exactly `=0.8.0-beta.1`.

A fresh registry-only consumer passed 83 tests plus 3 isolated settings cases in each of its default and all-features modes. A separate renamed-dependency/direct-macro consumer passed 2 tests in each configuration mode. Cargo metadata confirmed both exact beta packages resolved from crates.io without path patches. The safe panic-boundary migration ran successfully; five negative compile probes rejected the intended `UnwindSafe`, `RefUnwindSafe`, `Rc`, borrowed-data and pipeline stage-type violations. See the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0-beta.1) for detailed verification results.

# Rydantic 0.7.0 · Stable

Published on 2026-10-21 as stable: [runtime](https://crates.io/crates/rydantic/0.7.0) and [derive](https://crates.io/crates/rydantic-derive/0.7.0). Runtime and derive use matching 0.7.0 versions, with an exact `=0.7.0` derive pin. See the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0) for verification details.

## Included

- Promote the previously published beta features: opt-in settings, bounded partial JSON snapshots, phone/country/currency domain types, exact zero-exponent normalization, Cap’n Proto compiler-path fixes and SQLite index-safety guards.
- Fix callback path hygiene across `Model` and generated integrations; bare callback names no longer collide with internal locals or assertion helpers. Preserve SQL skipped-field behavior.
- Evaluate consumer `cfg` / `cfg_attr` through Rust before automatic Cap’n Proto registry/schema generation. Disabled inline models, fields and variants no longer participate. Existing active-model validation and explicit ordinal gap/duplicate checks remain. Rust's existing restrictions on external file modules inside procedural-macro input still apply.
- Preserve direct `rydantic-derive` consumers with runtime derive re-exports disabled, including renamed dependencies; runtime helper resolution remains isolated from unrelated direct macro dependencies.
- Document and test the existing synchronous/asynchronous validator execution contract. This does not add a validator composition API or change callbacks into atomic operations.

## Breaking upgrade from 0.6.0

Every struct and enum inside `#[rydantic::capnp_models(...)]`, including nested models, must explicitly derive `Model`; automatic insertion was removed in beta.1. Add `#[derive(rydantic::Model)]` or an imported alias before upgrading. No legacy compatibility layer is provided.

Conditional model definitions now affect generated schemas and automatic ordinals. Different target/feature configurations may therefore have different wire layouts. Do not assume cross-configuration compatibility, even with the same namespace. For a shared protocol, keep definitions fixed, use schema-first bindings, or intentionally separate protocol identities and validate interoperability. The unconditional explicit-derive change alone does not change wire layout.

SQLite rebuilds reject unsupported custom index semantics, including non-BINARY collation and descending key order, instead of silently dropping constraints. Review migration plans and keep backups; application owners are responsible for existing data and deployments. Use a reviewed manual migration for unsupported layouts, never remove constraints merely to bypass guards. No production data is migrated by this release.

## Data boundaries and package scope

Settings source diagnostics redact values, but successful values can contain secrets. Partial JSON previews are not business-validated: require final producer input, `Complete` and Model validation before consequential use. Limits remain 1 MiB / 64 containers, incomplete strings/keys are rejected, and integer parts over 4300 digits are unsupported rather than rounded. Repeated prefix reparsing can cost quadratic work.

Domain catalogs are pinned offline snapshots: phones do not prove reachability or ownership; currency membership includes legacy, fund and special codes, not a current-tender guarantee. Preserve upstream attribution.

This promotion publishes only `rydantic` and `rydantic-derive`. RydanticAI has an independent version/publication lifecycle. `rydantic-cli` and `rydantic-codegen` remain unpublished source tools (`publish = false`); the Skill and frozen evaluation fixtures are repository assets, not extra crates. Their exact 0.6.0/beta.4 recipes retain their original version-specific contracts.

## Release verification

The guarded [publication workflow](https://github.com/trotsky1997/Rydantic/actions/runs/39545120139) completed successfully for source commit [09ad96d](https://github.com/trotsky1997/Rydantic/commit/09ad96d4852ae187d5ddeac096c690eef723a127). Both registry archives were independently downloaded, matched their registry API and sparse-index checksums, and were byte-for-byte equal to the final local packages. Their VCS metadata identifies that source commit.

A fresh registry-only consumer passed 48 tests plus 3 isolated settings cases, 2 alternate-configuration tests, and 2 direct-macro tests in each configuration mode. Cargo metadata confirmed both 0.7.0 packages resolved from crates.io without path patches, with the runtime's exact `=0.7.0` derive dependency. See the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0) for detailed verification results.

# Rydantic 0.7.0-beta.4

Published on 2026-10-20 as a prerelease: [GitHub Release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0-beta.4). The published release record reports verified archives and a fresh registry-only consumer (46 tests plus 3 isolated environment cases). The runtime pins derive to exactly the same version.

## Added and scope

- Individually optional `phone`, `country` and `currency` validated string types. Phone JSON is canonical E.164 with metadata validation, never proof of reachability or ownership. Explicit-region human parsing has a bounded permissive punctuation policy; extensions are rejected. Countries use a pinned ISO 3166-1 catalog. Currency membership includes legacy, fund and special codes, not a current-legal-tender guarantee or successor conversion.
- Catalogs are offline snapshots. Generic JSON Schema patterns cannot enforce local catalog/metadata rules. Successful phone values remain personal data. Preserve dependency/data-source attribution; see [domain types and provenance](docs/domain-types.mdx).
- The source tree includes the fail-closed JSON Schema-to-Rust developer tool. `rydantic-codegen` remains version 0.1.0 with `publish = false`; it is not part of either published core archive or a separately published Cargo-installable tool. Use its source README for supported subset and resource limits.
- AI has an independent release lifecycle: the source workspace contains the separately maintained `rydantic-ai` 0.2.0 and Jev integration. Neither is included in the two core archives or uploaded by this core release; consult its own release status.

## Upgrade and safety

No new breaking API change from beta.3 is introduced. The beta.1 breaking explicit `Model` derive contract still applies from 0.6.0. Back up databases and test migrations; users are responsible for existing data and deployments. Unsupported SQLite layouts require reviewed manual migration, never removing constraints to bypass protections.

Settings and partial JSON retain their previous contracts: previews are not business-validated, incomplete strings/keys are rejected, limits remain 1 MiB/64 containers, and integer parts over 4300 digits are unsupported rather than rounded. Require final producer input, `Complete` and Model validation; values may contain secrets and repeated prefix reparsing may cost quadratic work.

# Rydantic 0.7.0-beta.3

Published on 2026-10-20 as a prerelease: [GitHub Release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0-beta.3). Both archives and a fresh registry-only consumer were verified. Stable remains 0.6.0.

## Added

- Opt-in `partial-json` snapshots of accumulated buffers, recovering incomplete containers, numbers and literals without converting numeric lexemes through f64. The pinned jiter 0.17.0 parser handles grammar and raw numeric tokens; no new lexer is introduced. Strict streaming, NDJSON and Model required-field semantics remain unchanged.
- Hard ceilings of 1 MiB input and 64 nested containers; callers may lower these limits. All incomplete strings and object keys are rejected. jiter's 4300-digit integer-part ceiling (excluding a minus sign, including decimal/exponent forms) is reported as `UnsupportedNumberLimit`, never silently rounded. No matching fractional/exponent digit limit is claimed.

## Safety and upgrade

Use `rydantic = { version = "=0.7.0-beta.3", features = ["partial-json"] }`. Runtime and derive packages both use beta.3 with an exact derive pin. This opt-in API adds no breaking change from beta.2. A preview is not business-validated data: wait for the producer's final document, require `Complete`, then validate with Model before consequential actions. Successful snapshot values and Debug output may contain secrets. Calls reparse and allocate; repeated prefixes can cost quadratic total work. See [snapshot semantics and limits](docs/models/partial-json.mdx).

The beta.1 breaking explicit Model derive contract and SQLite migration protections remain. Back up databases and test migrations; users remain responsible for existing deployments and data. Unsupported SQLite index layouts require reviewed manual migration, not removal of constraints. New JSON Schema-to-Rust code generation and phone/country/currency domain types are not included.

# Rydantic 0.7.0-beta.2

Published on 2026-10-20 as a prerelease: [GitHub Release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0-beta.2). Both package archives and a fresh registry-only consumer were verified. Stable remains 0.6.0.

## Added

- Opt-in `settings` feature with explicit ordered JSON, environment, strict dotenv-subset and flat secrets-directory sources, validated with the existing `Model` contract. Source precedence and value parsing are caller-selected; source errors and builder diagnostics redact values. No added dependencies and no claim of full pydantic-settings compatibility. See the [settings guide](docs/models/settings.mdx).

## Fixed

- Exact integer normalization accepts zero with arbitrarily large positive or negative JSON exponents, including signed and decimal zero. Nonzero overflow, fractional values, invalid JSON numbers and numeric strings remain rejected by the existing integer contract. This intentionally corrects previously rejected valid zero inputs.

## Upgrade and scope

Use `rydantic = { version = "=0.7.0-beta.2", features = ["settings"] }`. Runtime and derive packages both use 0.7.0-beta.2, with an exact derive dependency pin. Settings are opt-in; no new breaking API change from beta.1 is introduced. The breaking explicit `Model` derive requirement from beta.1 still applies when upgrading from 0.6.0. Back up databases and test migration plans; users remain responsible for existing data and deployments. Unsupported SQLite index layouts require reviewed manual migration, not removal of constraints to bypass guards.

This release includes settings and the zero-exponent correction only as new runtime behavior. Partial JSON, the new JSON Schema-to-Rust code generator, and phone/country/currency domain types are not included. Publication tooling and the documentation URL are maintained separately.

# Rydantic 0.7.0-beta.1

Published on 2026-10-20 as a prerelease: [GitHub Release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0-beta.1). Both crates and a fresh registry-only consumer were verified. Stable remains 0.6.0.

This beta introduces an explicit derive contract for generated Cap'n Proto models, fixes compiler path resolution, and protects SQLite migrations from losing custom index semantics.

## Breaking change: explicit Model derives

Every struct and enum inside `#[rydantic::capnp_models(...)]`, including nested models, must explicitly derive `rydantic::Model` (or an imported alias). The attribute no longer injects the derive automatically. Add it alongside existing derives:

```rust
#[rydantic::capnp_models(namespace = "my_app.users")]
pub mod models {
    #[derive(Debug, PartialEq, rydantic::Model)]
    pub struct User {
        pub name: String,
        pub age: u8,
    }
}
```

This is a source compatibility break from 0.6. Existing wire schema layout is unchanged by the derive-contract change. Update all affected models before upgrading.

## Fixes

- Cap'n Proto compiler paths resolve correctly when given relative paths, including Windows drive-relative and root-relative paths. Windows CI exercises compiler-path resolution.
- SQLite migration planning rejects table rebuilds when an existing index uses non-BINARY collation or descending key order, including primary-key indexes. The explicit `migration_unsupported` error prevents silent loss of NOCASE uniqueness and `INTEGER PRIMARY KEY DESC` NULL-key/rowid semantics. Ordinary BINARY ascending indexes remain supported. No-op and native rename-only changes preserve custom indexes.

## Trying the beta

```toml
rydantic = { version = "=0.7.0-beta.1", features = ["capnp"] }
```

Both runtime and derive packages use 0.7.0-beta.1; the runtime pins the matching derive exactly. This is a prerelease: opt in explicitly and test your model derives and migration plans before upgrading production applications. Keep backups before schema changes. Unsupported custom SQLite index layouts require a reviewed manual migration; do not remove constraints just to bypass the guard.

## Known limitation

Conditional definitions inside `capnp_models` still need care: the macro may validate `#[cfg]`-disabled models before Rust removes them. A disabled generic model can therefore prevent compilation. This is a pre-existing limitation tracked in [issue #8](https://github.com/trotsky1997/Rydantic/issues/8); this beta does not claim to fix general conditional schema generation.

## Verification

Release gates (record exact results on the published release): Linux/macOS full CI; Windows compiler paths; real PostgreSQL 18/MySQL 8.4 tests; package verification; a registry-only renamed-dependency consumer with explicit and aliased derives; and published artifact checksum verification. Do not claim these passed until recorded.

## Release rhythm

Main stays releasable. Breaking or substantial changes first receive a numbered beta and a GitHub prerelease. Fixes advance beta.2, beta.3, and so on. Publish 0.7.0 stable only after the candidate passes the full matrix and independent packaged/registry consumer checks, with no unresolved release-blocking issues. Promotion changes package versions and gets a fresh exact-commit CI run. No permanent beta branch is required.
