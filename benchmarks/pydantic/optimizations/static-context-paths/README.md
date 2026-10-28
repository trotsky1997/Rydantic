# Static Context field-path labels

Refs #126. dot · AI 助手

## Bounded change

Baseline: main `2ce66b92b281ff5f0e391844bf1ce8cce8cde728`.
Candidate runtime/tests: `ef7194a95e89541f3b343cde6dc69eebe94c0e64`.

Derived JSON/borrowed parsing, validation, encoding and async child collectors
know named field labels at compile time. Previously entering each such label
constructed an owned String even when no error occurred. Context now privately
stores static or owned field labels; public owned `PathSegment` values are
materialized while pushing errors. Dynamic callers still use `Context::at` and
`child` without a changed signature. Static generated calls share the same
implementation, not a compatibility shim or a weaker validation path.

Public error variants, field/index distinction, exact payloads and ordering,
input/output aliases, dynamic keys, flatten/newtype semantics and async
cancellation behavior are unchanged. Normal-return path restoration is preserved;
no new panic/unwind guarantee is introduced. Native Cap'n Proto and SQL-generated
field entry sites were not changed, and no benefit for those routes is claimed.

## Separate allocation evidence

[`allocation_probe.rs.txt`](allocation_probe.rs.txt) is a standalone
System-allocator instrument, never a timing worker. It uses the same Flat,
Nested and Collection declarations and canonical fixture text. Each parse is
warmed once to initialize rule caches before counting; setup and fixture parsing
are excluded, while value_copy still includes cloning. Counts are calls to
alloc/alloc_zeroed/realloc; requested bytes are cumulative, not peak memory or RSS.

| Operation | Main calls → candidate | Requested bytes, main → candidate |
|---|---:|---:|
| 1,000 successful static labels (path-vector capacity prewarmed) | 1,000 → 0 | 11,000 → 0 |
| 1,000 dynamic borrowed-string labels, control | 1,000 → 1,000 | 11,000 → 11,000 |
| One error at a static label with relative index | 8 → 7 | 667 → 656 |
| Warmed flat_valid JSON | 14 → 11 | 862 → 850 |
| Warmed nested_valid JSON | 26 → 21 | 1,770 → 1,750 |
| Warmed collection_100 JSON | 1,215 → 914 | 81,430 → 80,225 |

[`allocations.json`](allocations.json) retains all 49 parse rows, full errors,
controls and the exact probe source hash. Baseline/candidate acceptance and full
serialized errors match in every row. This removes small label allocations, not
all parse allocations, and does not by itself establish end-to-end speed.

## Measured timing is mixed; no latency approval

Both complete runs and the subsequently failed A/A diagnostic remain visible.
This source excludes the separate numeric-rule optimization (#125); no combined
speedup is inferred.

| Primary JSON case | Initial main → candidate | Longer confirmation main → candidate |
|---|---:|---:|
| flat_valid | 857.5 → 773.6 ns (-9.8%) | 898.7 → 829.5 ns (-7.7%) |
| nested_valid | 1,740.0 → 1,621.2 ns (-6.8%) | 1,827.3 → 2,079.8 ns (+13.8%) |
| collection_100 | 96.41 → 89.09 µs (-7.6%) | 100.03 → 95.18 µs (-4.8%) |

There are **19 slower initial medians and 7 slower confirmation medians**. The
confirmation also records flat_wrong_type/json +67.2%, flat_negative/json +2.1%,
flat_long_name/json +4.9%, nested_invalid/json +0.1%, malformed_json/json +0.5%
and flat_root_array/value_copy +0.6%. No rows are dropped or replaced. A
byte-identical candidate-control executable shows substantial variation too;
this makes attribution uncertain, not the slower rows irrelevant.

The pinned/interleaved A/A diagnostic failed the predeclared stability gate for all three cases
and **stopped before baseline/Pydantic pairing**. Allocation reduction is
established; this environment does not resolve modest incremental latency
reliably. This is not a clean no-regression, superiority, or merge recommendation.

- [Initial full table](initial/results.md) / [raw](initial/results.json.gz).
- [Longer confirmation full table](confirmation/results.md) / [raw](confirmation/results.json.gz).
- [Instability analysis and failed pinned diagnostic](instability-analysis.md) /
  [raw telemetry](pinned-diagnostic.json.gz).

## Timing protocol

Uninstrumented release workers, same locked dependencies and toolchain, isolated
target directories; same canonical 49 case/mode rows and pinned Pydantic.
The initial run uses baseline / candidate / identical-candidate-control /
Pydantic, followed by the reverse order for every row; each position warms 1,000
operations and records three batches of 10,000 (six samples/engine/row).
The confirmation reverses that order, sizes each row once from the initial
baseline median toward 100 ms (minimum 10,000, maximum 1,000,000 operations), and
records five batches/position (ten samples/engine/row). Both retain 392 process
outputs across 49 rows. The control is a separately built, byte-identical
candidate executable, not a distinct code-layout variant. Correctness is checked
before results are retained. Source and executable hashes must remain
unchanged through the run. No baseline subtraction or multiplication of earlier
optimization ratios.

Results are warmed repeated-payload microbenchmarks on shared cloud hardware,
not cold/high-cardinality traffic or request-tail latency. Pydantic retains its
default string cache and compiled Rust core. Native rejected-input diagnostic
work differs. Secondary value_copy includes copying and is not parser-only
performance. The complete table retains all slower medians and IQRs.

## Verification and reproduction

- Full default core tests: 40 passing test harnesses.
- Full workspace `--all-features --locked --offline`: 91 passing harnesses,
  including doctests. Feature/database tests already marked ignored retain that
  status; no live external database claim.
- Derive tests: 4 passed; no-default runtime library check passed.
- All-feature/all-target core + derive Clippy with warnings denied, formatting
  and diff checks passed. An initial Clippy attempt lacked the installed capnp
  compiler on PATH; the corrected-environment rerun passed without source change.
- Canonical 98 engine/case/mode parity and four known-difference checks passed.
- Five new full-payload contract tests also pass against unchanged baseline.
  Context unit tests cover static storage, dynamic ownership, child/data/trial/
  absorb, full errors and normal-return stack restoration.
- Independent source review: 57 output records / 168 full errors byte-identical
  against the frozen baseline, including async cancellation and borrowed models.
- Independent evidence audit verifies both runs' 784 outputs / 98 table rows,
  all error/allocation parity, artifacts, source hashes and all slower rows.

No release, merge or private/user-computer work is part of this change.

Use the scripts with explicit baseline/candidate source roots, isolated release
workers and the pinned benchmark Python environment. `initial/compare.py` is the
initial protocol. `compare.py` reproduces the longer protocol; exact measured
bytes are retained in `confirmation/measured-compare.py`, matching the raw
`driver_sha256`. The current copy only corrects a stale comment from “three” to
“five”; it does not alter protocol code. The initial result did not record a
self-hash of its driver at execution time; its retained driver and artifact/source
checks are evidence, but that missing self-hash is an explicit provenance limit.
`report.py` recomputes the confirmation table; `initial/report.py` recomputes the
initial table. `pinned_diagnostic.py` refuses output overwrite, gates any paired
phase on the predeclared A/A thresholds, and was run exactly once.

[`build-provenance.json`](build-provenance.json) identifies the release artifacts.
An early target named baseline-release actually held candidate source; it was
never used as baseline and was explicitly verified/reclassified as the identical
candidate control. The true baseline was built in baseline-final-release through
its explicit manifest. Both original build logs and clean artifact receipts were
independently checked. Initial local test-authoring mistakes (an unsupported raw
Rust tuple and ambiguous trait methods) were corrected before final verification;
production behavior was not weakened.

For allocation reproduction, create an independent Cargo package outside the
repository workspace, copy `allocation_probe.rs.txt` to `src/main.rs`, use
`allocation-probe.Cargo.toml.template` as its manifest (replace only
`REPOSITORY_ROOT`), and copy `allocation-probe.Cargo.lock` to `Cargo.lock`.
Build release with `--locked`; baseline omits `--features static-path`, candidate
includes it. Pipe the unmodified canonical `benchmarks/pydantic/fixtures.json`
into the probe. Use separate targets and record hashes. This allocator instrument
uses unsafe System delegation only in the standalone probe, never production.

## Public evidence privacy boundary

These are path-redacted derivatives, not altered measurements. Each adjacent
privacy manifest records compressed/decompressed SHA256, size, original private
checksum, schema fields and exact changes. Only the five command-path strings
are replaced; all other fields, every sample, hash, protocol, regression and
telemetry value are deep-equal to the private originals. Fixtures are synthetic
public repository data; machine metadata describes only the assistant-owned cloud.
No user device, credentials or private business data was used. Public upload of
these prepared artifact candidates remains gated by the parent's authorization
check; their local existence is not a publication claim.
