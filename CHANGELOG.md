# Rydantic 0.7.0-beta.1

Source candidate, not a publication announcement. Check crates.io and GitHub Releases for published artifacts. Stable remains 0.6.0.

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
