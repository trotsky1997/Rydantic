# Default-core runtime coverage

This report measures executable production source in `crates/rydantic/src` with
only `derive` enabled. It is **not whole-workspace or all-feature coverage**.
The comparison adds 38 regression tests without changing production code.

## Reproduce

Use an installed Rust toolchain and its matching official LLVM tools:

```sh
rustup component add llvm-tools-preview
python3 scripts/core_coverage.py
# With already cached locked dependencies:
python3 scripts/core_coverage.py --offline
# Historical PR #76 comparison only:
python3 scripts/core_coverage.py --offline --compare-regressions
```

The script uses a separate `target/core-coverage` directory by default. An explicit
`CARGO_TARGET_DIR` overrides that location. Debug information and incremental
compilation are disabled to limit disk use. Each invocation creates fresh profile
and report directories; previous runs are not merged. The script preserves the
active toolchain, records `rustc -vV`, and fails if either test phase fails.
Reports include LLVM text summaries, JSON exports, annotated sources and test logs.
No real provider, external database, credentials or production data is needed.

In historical comparison mode, the baseline executes the existing `models`, `features`, `raw_input`,
`integer_exponents`, `schema`, `schema_paths`, `async_stream`, `callback_hygiene`
and `validator_execution` suites plus library tests. The expanded run adds
`core_contracts`, `stream_failures`, `borrowed_contracts` and
`collection_contracts`. Both reports use the **same union of executable objects
and production-source paths**. New generic instantiations therefore do not create
an easier denominator only in the expanded report. Test source and dependencies
are excluded from the reported source scope.

## Current-suite CI contract

The default command now discovers top-level integration `.rs` files and
`tests/<suite>/main.rs` targets in `crates/rydantic/tests`, except the `compile`
target (the separately tested trybuild harness), and also runs library tests.
New auto-discovered runtime targets are included without editing the historical
list. Custom Cargo `[[test]]` paths require an explicit extension of discovery
before they are part of this contract. Feature-gated suites may run zero tests:
this does **not** enable their features or credit their source as covered.
`--compare-regressions` retains the original two-phase comparison for historical
investigation, not the current CI gate.

A separate `Default core coverage` workflow runs on relevant pull requests and
main changes, independently of the existing all-feature/platform CI. It pins Rust
1.98.1 and matching official LLVM tools, runs Python measurement-contract tests,
and enforces 85% line and 85% function coverage for this bounded runtime scope.
These are regression floors below the measured result, not an assertion that
85% is sufficient testing. No branch threshold or all-repository percentage is
invented. The existing CI still owns trybuild and optional-feature matrices.

On main source `f1f92b26f458e92e19acc003beb883ed38dce3c9`, Linux x86-64,
Rust 1.98.1 / LLVM 22.1.8, locked offline dependencies:

| Metric | Historical 136-test selection | Current 167-test selection |
| --- | ---: | ---: |
| Lines | 2,037 / 2,583 (78.86%) | 2,297 / 2,583 (88.93%) |
| Functions | 427 / 533 (80.11%) | 475 / 533 (89.12%) |
| Regions | 3,371 / 4,373 (77.09%) | 3,725 / 4,373 (85.18%) |

The 31 additional tests already existed in the merged validator extension;
this change brings its four suites into measurement. It does not claim to have
written those tests or newly improved production behavior. The source universe
now includes `pipeline.rs`; this table must not be compared directly with the
older 2,310-line denominator below. All 167 tests pass. Twelve other discovered
feature-gated integration targets and the library target have zero enabled tests.

`manifest.json` records toolchain, source revision and dirty state, lockfile hash,
effective compiler flags, Cargo commands, feature selection, selected suites and
requested floors. `summary.json` records validated production-file
counters, actual reported source files and unreported source files. File counters
are validated before aggregation; dependency files, duplicates, empty metrics
and malformed counters fail closed. Zero branch counters produce a null
percentage. Thresholds use exact counts, not displayed rounded percentages.
CI retains JSON reports, annotated source, logs and toolchain metadata for seven
days, including failure logs when tests or thresholds fail. It does not upload
raw profiles or send results to an external coverage service.

For a stable report destination or a local threshold gate:

```sh
python3 scripts/core_coverage.py --output-dir coverage/core \
  --min-line-coverage 85 --min-function-coverage 85
python3 -m unittest discover -s scripts -p 'test_core_coverage.py' -v
```

The explicit output directory must not already exist, preventing accidental
mixing with stale profiles. Coverage artifacts are generated data, not source
files to commit. Threshold changes should include a fresh measurement and an
explanation of scope/denominator changes rather than silently lowering a gate.

## Historical PR #76 comparison

Base: `09ad96d4852ae187d5ddeac096c690eef723a127` (2026-10-21).
Cloud Linux x86-64; Rust 1.99.1, LLVM 23.1.1; locked offline dependencies.
All 98 existing tests and 38 new tests pass. Production files are unchanged.

| Metric | Existing suites | With regression suites |
| --- | ---: | ---: |
| Executable lines | 1,702 / 2,310 (73.68%) | 2,029 / 2,310 (87.84%) |
| Functions | 320 / 483 (66.25%) | 426 / 483 (88.20%) |
| Regions | 2,769 / 3,990 (69.40%) | 3,364 / 3,990 (84.31%) |
| Branches | Not measured | Not measured |

LLVM reported zero branch counters, not a meaningful zero denominator or 100%
branch coverage. Region coverage is separately reported, not relabeled as branch
coverage. Function counts treat a function as covered when an emitted
instantiation executes; they do not imply every generic type combination ran.
Compiler/toolchain changes may alter executable-line and function denominators.

| Production source | Before line coverage | After line coverage |
| --- | ---: | ---: |
| `asynchronous.rs` | 61 / 85 (71.76%) | 81 / 85 (95.29%) |
| `borrowed.rs` | 338 / 587 (57.58%) | 507 / 587 (86.37%) |
| `direct.rs` | 22 / 26 (84.62%) | 22 / 26 (84.62%) |
| `error.rs` | 131 / 137 (95.62%) | 137 / 137 (100.00%) |
| `lib.rs` | 180 / 203 (88.67%) | 198 / 203 (97.54%) |
| `object.rs` | 109 / 141 (77.30%) | 109 / 141 (77.30%) |
| `rules.rs` | 267 / 290 (92.07%) | 267 / 290 (92.07%) |
| `schema/mod.rs` | 147 / 156 (94.23%) | 147 / 156 (94.23%) |
| `stream.rs` | 142 / 170 (83.53%) | 158 / 170 (92.94%) |
| `types.rs` | 305 / 515 (59.22%) | 403 / 515 (78.25%) |

## What the added tests protect

- Context scopes, child collectors and error aggregation preserve relative paths,
  error order, stages and parameters. Invalid custom decoders cannot silently
  succeed, and custom encoder failures propagate through validated serde wrappers.
- Borrowed models preserve input references and input/output aliases through
  collections, defaults, revalidation and synchronous/asynchronous entry points.
- Owned collection defaults use input representations rather than lossy output
  representations. Parse, default, validation and encoding failures collect
  sibling diagnostics; nullable boxes and fixed-array cardinality stay checked.
- Streams retain callback errors, recover only where documented, stop after
  terminal reader/syntax errors, and reject trailing input without losing paths.

## Remaining gaps and limits

The historical PR #76 measurement above has 281 uncovered executable lines and
57 uncovered functions; the current pinned-toolchain run above has 286 and 58,
respectively. The largest line gaps are `types.rs` (112), `borrowed.rs` (80), and
`object.rs` (32). Annotated reports identify the exact remaining locations; no
claim is made that all error paths, generic instances or behavior combinations
are covered.

Excluded: proc-macro implementation source (derive expansion is exercised but
not measured as compiler-process coverage), compile-fail/trybuild tests,
doctests, examples, Cap'n Proto fixtures and optional `capnp`, settings,
partial-JSON, dynamic schema, date/decimal/domain, Axum and SQL features; also AI,
CLI, codegen, JavaScript/Python and other workspace packages. Feature-disabled
files are absent from the denominator, not credited as covered. These need
separate instrumented matrices. Local disk constraints motivated the bounded
core scope; full workspace and supported-platform regressions remain CI's job.

The four new integration suites pass Clippy with warnings denied. Formatting and
diff checks pass. This report does not claim a new release, merge, or CI result.

Implementation follows Rust's official
[instrumentation-based coverage documentation](https://doc.rust-lang.org/rustc/instrument-coverage.html).
