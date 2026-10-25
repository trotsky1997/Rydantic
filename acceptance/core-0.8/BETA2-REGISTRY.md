# Core 0.8.0-beta.2 registry acceptance receipt

Recorded 2026-10-25 by dot · AI 助手. This is maintainer-operated, first-party
application integration with synthetic inputs, not external adoption, production
traffic, or an all-platform registry-consumer run.

## Publication and provenance

- Exact source/tag: [`v0.8.0-beta.2`](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0-beta.2),
  commit `0a84cc42cadbe2bb4d3c86f207aebdb4827c2a69`,
  tree `1e69fbcd5224e7728c673237371f25b1b4cdb9e1`.
- [Final-main CI](https://github.com/trotsky1997/Rydantic/actions/runs/40197806534)
  completed all six jobs successfully. Source CI covered Linux/macOS, Windows
  compiler paths, PostgreSQL/MySQL, bridge and release safety; it is separate from
  the Linux registry acceptance below.
- [Guarded publication](https://github.com/trotsky1997/Rydantic/actions/runs/40199734576)
  succeeded, publishing derive then runtime with the existing trusted publisher.
  crates.io created derive at `2026-10-25T13:54:43.372133Z` and runtime at
  `2026-10-25T13:54:55.802095Z`.
- Both API and sparse-index records report not yanked. Exact `=0.8.0-beta.2`
  runtime-to-derive dependency verified. Actual downloads, API, sparse index and
  final clean-checkout local archives all have these SHA256 checksums:

| Package | Files | SHA256 |
| --- | ---: | --- |
| rydantic-derive | 13 | `2a9b30097620e5dfd3765f610eaeca8b700d7f1290f8f040ad27c0ae235a5d4b` |
| rydantic | 154 | `5b112a9df7551f93a05808f7f7ba0a97f3a366b57fdd83adef074bec359d9b56` |

Both archives include complete byte-identical committed crate source, clean VCS
metadata for the commit above, MIT license and actual package README. Runtime
contains optional OpenAPI and the normalized exact derive pin, without a path
substitute. No temporary registry/path leak or Node/AI/acceptance/site/Skill/eval
artifact entered either archive. Final local packaging compiled both archives;
actual registry validation did not substitute that local packaging for download.

## Registry application and migration results

On dot's isolated Linux cloud computer with Rust/Cargo 1.98.1, copied the exact
committed application and selected the published dependency:

```sh
python3 select-dependency.py registry 0.8.0-beta.2
cargo test --features openapi-acceptance
cargo test --locked --features openapi-acceptance
python3 scripts/context-migration.py --version 0.8.0-beta.2 --output results/beta2-migration
cargo metadata --locked --format-version 1
```

The first command changes the manifest only. Normal resolution updated the two
core entries in the source lock to registry entries, retaining the other resolved
versions; the locked repeat used that resolved registry lock, not the old source
lock. Metadata identifies both packages as exact beta.2 from
`registry+https://github.com/rust-lang/crates.io-index`, with no path/patch
substitution. Lock checksums match the table above.

- `pipeline_rejection_skips_later_stage`: PASS.
- `explicit_context_ownership_and_repeated_lifecycle`: PASS.
- `synthetic_order_service_acceptance`: PASS (real loopback HTTP, per-request
  context, model lifecycle, typed pipeline, isolated SQLite commit/rollback and
  actual application request/output OpenAPI components).
- `application_document_components_and_scope_boundary`: PASS (independent
  input/output projection fixture and runtime-only rule/unsupported-scope boundaries).
- Locked repeat: the same **4/4 tests PASS**; zero library/doc tests are not counted.
- Migration: **4/4 probes PASS**: safe panic-boundary execution plus intended
  compile errors for captured Context unwind bounds, non-thread-safe Rc and
  borrowed non-owned data. A generic dependency/compilation failure is not accepted
  as an expected negative diagnostic.

An independent reviewer used a separate application copy, repeated normal registry
resolution and locked tests (**4/4 each**), all four migration probes, metadata and
archive/API/index/local checksum audit. All passed. Application source identity
was checked against the exact release checkout:

| File | SHA256 |
| --- | --- |
| src/lib.rs | `e778cb023711a40327fe2d9ec0bfdf14f9008d5e64ee2037435c3a3d352917da` |
| tests/application.rs | `f447d47bb9a8c18c3806d4a3657f221c64dfcc150158335254913dc8807a164e` |
| tests/openapi.rs | `e8c79548dab656c9914ea7d9f3c9dbeb25f06cb43731262ce12c97f71bde7ee3` |
| scripts/context-migration.py | `7ed390e0a3dc357f33afab051a8219eed7f640e0e39a5804164bddeeef830df7` |

## Decision and limits

No library contract failure was confirmed in this bounded integration. Initial
fixture review friction and corrections are recorded in [VALIDATION.md](VALIDATION.md);
these registry runs reused that reviewed fixture rather than renaming standalone
unit tests as application feedback. No external beta adopter is claimed.

This closes the planned beta.2 registry-acceptance gate for an equivalent core
0.8.0 promotion. It does not publish stable 0.8.0, waive its exact-source CI,
independent version/package review or final registry-consumer verification, or
claim AI/CLI/provider milestones. Context's breaking ownership/unwind contract,
explicit per-call policy, shared effects and no automatic/lossless data migration
remain applicable. No user or third-party production data was accessed.
