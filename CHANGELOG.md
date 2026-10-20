# Rydantic 0.7.0-beta.3

Source candidate, not a publication announcement. Check crates.io and GitHub Releases for published artifacts. Stable remains 0.6.0.

## Added

- Opt-in `partial-json` snapshots of accumulated buffers, recovering incomplete containers, numbers and literals without converting numeric lexemes through f64. The pinned jiter 0.17.0 parser handles grammar and raw numeric tokens; no new lexer is introduced. Strict streaming, NDJSON and Model required-field semantics remain unchanged.
- Hard ceilings of 1 MiB input and 64 nested containers; callers may lower these limits. All incomplete strings and object keys are rejected. jiter's 4300-digit integer-part ceiling (excluding a minus sign, including decimal/exponent forms) is reported as `UnsupportedNumberLimit`, never silently rounded. No matching fractional/exponent digit limit is claimed.

## Safety and upgrade

Use `rydantic = { version = "=0.7.0-beta.3", features = ["partial-json"] }` after publication. Runtime and derive packages both use beta.3 with an exact derive pin. This opt-in API adds no breaking change from beta.2. A preview is not business-validated data: wait for the producer's final document, require `Complete`, then validate with Model before consequential actions. Successful snapshot values and Debug output may contain secrets. Calls reparse and allocate; repeated prefixes can cost quadratic total work. See [snapshot semantics and limits](docs/models/partial-json.mdx).

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
