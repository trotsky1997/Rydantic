# Core 0.8.0 stable registry acceptance receipt

Recorded 2026-10-25 by dot · AI 助手. These are maintainer-operated first-party
application checks with synthetic inputs on Linux, not external adoption,
production traffic, or cross-platform registry acceptance.

## Publication and provenance

- Exact source: `337c8b05798fa4d02eb75359ba9c9980901949df`, tree
  `1d8221b763010a54fd1b3dac23a7024908ffe9d0`, released as
  [`v0.8.0`](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0).
- [Exact-main source CI](https://github.com/trotsky1997/Rydantic/actions/runs/40202297651)
  passed all six jobs. Source platform/integration checks are separate evidence
  from the Linux registry-only consumers below.
- [Guarded publication](https://github.com/trotsky1997/Rydantic/actions/runs/40203697840)
  succeeded. crates.io created derive at `2026-10-25T14:45:42.120262Z` and runtime
  at `2026-10-25T14:45:53.002740Z`. GitHub release ID `423461905` was promoted to
  stable (not prerelease) and Latest only after registry verification; API readback
  confirmed that state.
- At 14:52 UTC, an independent HTTP check confirmed both exact-version API pages
  returned HTTP 200 and identified version 0.8.0:
  [runtime API](https://docs.rs/rydantic/0.8.0/rydantic/) and
  [derive API](https://docs.rs/rydantic-derive/0.8.0/rydantic_derive/).
- API and sparse-index records report both versions not yanked. Downloaded
  archives, API checksums, sparse-index checksums and final clean-checkout local
  archives match in both the author's audit and the independent audit:

| Package | Version | Archive files | SHA256 |
| --- | --- | ---: | --- |
| rydantic-derive | 0.8.0 | 13 | `b6112b5eeb820ed08259f97c56975474f367660a77276e04a98cd4071cd1e777` |
| rydantic | 0.8.0 | 154 | `3034eedcb1f538d9ec5e93fb8bc4cdec2e152259635fb6bab479bcd8ae7fbfd0` |

Both archives' `.cargo_vcs_info.json` identifies the exact source above. The
tracked crate source is complete and byte-identical to that commit. The normalized
runtime manifest pins derive exactly to `=0.8.0`. Registry metadata confirms that
both crates resolve from crates.io, not a path patch. Stable promotion preserves
the beta.2 runtime contract; version, metadata and documentation adjustments do
not introduce new runtime behavior. These archive checks do not claim that
arbitrary existing data or deployments are compatible or migrated losslessly.

## Fresh registry application and migration checks

The author and an independent reviewer used separately resolved standalone
consumers with Rust 1.98.1 and exact `rydantic = "=0.8.0"`, enabling `axum`,
`sqlite` and the optional OpenAPI acceptance feature. Neither substituted source
paths for the released core/derive packages.

| Check | Author | Independent reviewer |
| --- | --- | --- |
| Normal resolution: three application/lifecycle/pipeline tests + one OpenAPI test | 4 passed | 4 passed |
| Same consumer with `--locked` | 4 passed | 4 passed |
| Safe panic-boundary execution and three intended compile failures | 4 passed | 4 passed |

The real loopback HTTP application exercises explicit request policy, model
parse/revalidation/output, noncommutative pipeline normalization, fail-fast
rejection before writes, and isolated SQLite commit/rollback. Application OpenAPI
components are checked against the actual response, while a separate projection
fixture exercises aliases, excluded fields, nonempty runtime-rule locations and
unsupported-scope rejection. This is not complete OpenAPI document validation.

The four migration probes separately verify: context constructed and consumed
inside a panic boundary succeeds; capturing a context outside that boundary fails
with the intended `RefUnwindSafe` diagnostic; `Rc` payloads fail thread-safety
bounds; borrowed payloads fail the owned/`'static` requirement. Expected compiler
failures count as successful negative probes, not successful compilations.

Reproduce with [the application](README.md), its exact-version selector, and
`scripts/context-migration.py --version 0.8.0 --output <new-directory>`.
Run `cargo test --features openapi-acceptance`, then the same command with
`--locked`; inspect Cargo metadata for registry sources and exact paired versions.
Do not reinterpret this bounded receipt as a full all-features/platform consumer
matrix or an external usage campaign.

## Audit trail and limits

The release task retained author package audits (`registry-receipts.json`,
`archive-audit.json`), normal/locked application logs, Cargo metadata and migration
case logs. The independent reviewer separately retained its four-way archive
audit, registry metadata, lockfile, normal/locked logs and migration results.
The table above records actual completed outcomes from those artifacts, not
predicted outcomes. Public source, workflow and package links identify the
immutable inputs; local execution artifacts are not falsely presented as hosted
CI logs. No production data or paid provider calls were used.

**Breaking from 0.7.x:** `Context` no longer implements `UnwindSafe` or
`RefUnwindSafe`, even without data. Supply owned `Send + Sync` business context
explicitly to each operation. Do not introduce a blanket `AssertUnwindSafe`
compatibility wrapper. Failed trial branches can retain shared effects; pipelines
are not transactions or proofs of validity. Transformations/checks can change
accepted and emitted data. Review behavior, wire/schema choices and backups before
migrating your own data or deployment; no automatic/lossless upgrade is promised.
AI publication, CLI distribution and frozen Skills/evals remain independent.
