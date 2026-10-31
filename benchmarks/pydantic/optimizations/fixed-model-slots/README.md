dot · AI 助手

# Fixed-layout borrowed model slots: allocation evidence, latency unknown

Refs #128. Baseline is main `2ce66b92b281ff5f0e391844bf1ce8cce8cde728`.
The baseline checkout additionally contains the copied regression test; runtime sources are exact main.
The allocation and compatibility artifacts below are preserved from related PR #130
(`9fe395a`), whose candidate contains the slot-only runtime change against this exact
baseline. This follow-on branch also adds streamed borrowed arrays, direct integer
and string rule validation, scalar decode, and static error paths. The archived
counts and oracle describe the slot-only candidate; use the
benchmark runner to time the combined branch.
The slot-only candidate is independent of integer-rule #125/#127 and static-context-path #126 work.
Its artifacts do not claim to close the Pydantic performance gap or to improve latency.
The shared cloud failed prior same-binary timing noise controls; **no timing campaign
was performed for this change**.

## Bounded implementation

Fixed-layout named structs stage borrowed JSON in one exact-length boxed slice of
`Option<&RawValue>` slots. A typed map visitor and key seed match decoded validation
aliases without allocating a String for each known key. The last decoded duplicate
replaces its slot. Every value, including ignored/discarded values, is consumed as
RawValue before any field callback runs. Unknown names use a sorted, deduplicating
BTreeSet only when diagnostics require them.

There is one generated declaration-order field/default/coercion/validation body,
including one set of rule caches. Owned/Object inputs retain map decoding;
flatten/extra layouts and tagged/untagged enum dispatch retain their existing paths.
Nested fixed structs can still select slots. This is **not single-pass parsing**:
root RawValue scanning, object scans, and nested/scalar scans remain.

An initially explored inline slot array was replaced before allocation collection.
`Option<&RawValue>` is 16 bytes on the measured target; an inline N-slot array could
add 16N bytes per recursive frame. Always heap-backed slots avoid that new
width-dependent stack storage, without an arbitrary width threshold. Models declaring one or more fields allocate a slot buffer even for an empty
borrowed object; zero-field models allocate no slots.
This intentionally trades some sparse-object allocation overhead for bounded
additional slot stack storage.

## Measured allocation evidence (not timing or RSS)

[All 77 case/mode rows](allocation-results.md) · [raw counts, complete outcomes,
inputs and provenance](allocations.json.gz).

These are release-profile counts from the exact instrumented binaries, not uninstrumented latency. Each case/mode uses a fresh process for its cold parse, then one additional unmeasured
prewarm and five measured parses. All five warm samples match exactly. Counts cover
System allocator alloc/alloc_zeroed/realloc calls and cumulative requested bytes
inside parse; input preparation, outcome serialization and result disposal are outside
counters. Requested bytes are not live/peak memory, retained memory or RSS.

| Warm borrowed JSON | Allocation calls, baseline → candidate | Requested bytes, baseline → candidate |
| --- | ---: | ---: |
| Flat valid | 14 → 11 | 862 → 266 |
| Nested valid | 26 → 21 | 1,770 → 566 |
| Collection of 100 | 1,215 → 914 | 81,430 → 21,209 |
| 80-field dense, ascending or descending | 334 → 243 | 9,930 → 3,210 |
| **80 defaulted fields, empty object** | **82 → 83** | **500 → 1,780** |
| **80 defaulted fields, one present** | **86 → 85** | **1,154 → 1,798** |

The collection removes 301 owned-key allocations, replacing 101 single-node model
maps with 101 slot allocations. This is a structural/count observation, not CPU-share
attribution. All 49 canonical case/mode rows and 28 synthetic controls retain full
outcome equality. All value_copy rows and explicit flatten/extra/tagged controls have
identical counts and bytes. The two adverse sparse/default-heavy controls above are
retained, not treated as noise. No universal memory improvement is claimed.

Ignored keys are not collected; 1,000 synthetic ignored keys retain only raw scanning
and serde scratch costs rather than a 1,000-entry model map. Escaped-key scratch and
raw parsing still cost work and can allocate. Forbidden unknown duplicates still
allocate decoded names while being deduplicated; they are not a zero-allocation path.

### Code-size tradeoff

The exact allocation-probe ELF includes canonical models plus synthetic/80-field
controls. GNU `size` reports text **991,295 → 1,058,687 bytes (+6.80%)**, data
22,280 → 22,744, bss 968 → 2,760; executable file size is 1,376,280 → 1,449,192 bytes.
[Size record](binary-sizes.json); executable hashes are in the raw allocation report.
This observed increase is not a general library-size estimate or latency evidence.

## Compatibility evidence

- The new repository regression suite passes unchanged on exact main and candidate:
  8 tests, including decoded duplicate survivors, callback/default order and count,
  complete serialized errors/display, prior errors, aliases, unknown ordering,
  malformed keys, ignored values, exact raw text, pointer identity, u128, generic,
  empty/80-field and recursive depth-24 layouts.
- [1,172 whole-program records](compatibility-oracle.jsonl.gz) match byte-for-byte
  between independently built baseline and candidate, not merely root-map routing.
  Uncompressed SHA-256:
  `1574834b6c70e13edb4288ac5d3a7a765488bcf6fd44792fe11ebe9f343acb4c`.
  [Exact source manifest](compatibility-manifest.json).
- Runtime unit tests additionally compare 2,048 generated raw objects in ignore and
  forbid modes with the previous map boundary, including raw pointer identity and
  unknown sets; existing 6,144 parser comparisons remain intact.
- An independent no-default, renamed-dependency/direct-derive consumer passed 5 tests
  on both builds: multiple lifetimes/const generics, callback-name hygiene, full prior
  errors, 512-field dense/sparse/duplicate and recursive 64-field/depth-24 models.
  Its ten full outcome records match. An initial incorrect lone-surrogate expectation
  failed on both builds; the corrected probe preserves the existing RawValue behavior,
  rather than tightening or weakening validation.
- Final local workspace all-features locked/offline: **534 passed, 9 ignored**
  across 91 successful suite summaries. The official Cap’n Proto CLI ignored test
  was subsequently run explicitly and passed; the remaining 8 live-database/provider
  tests were not run. Default core/derive: **197 passed**.
- Workspace all-target/all-feature Clippy with warnings denied, no-default library
  check, no-default chrono/decimal/capnp check, all-feature documentation with
  warnings denied, formatting and diff checks passed on the final source.
- Independent evidence review reran all 154 allocation processes and exactly
  reproduced every counter and full outcome; all 616 numeric table cells, binary,
  compiler, source and artifact hashes were checked. This is not performance or
  merge approval. Exact-head platform CI remains a separate gate.

## Reproduction and provenance

Build two standalone consumers from `allocation-probe.Cargo.toml.template`, replacing
`REPOSITORY_ROOT` with the baseline/candidate checkout. Use the **same** archived
`allocation_probe.rs.txt` as `src/main.rs` and `allocation-probe.Cargo.lock` as
`Cargo.lock`. Build each with the recorded Rust toolchain using
`cargo build --release --locked --offline`; the package is separate from the workspace
because allocation instrumentation necessarily implements GlobalAlloc. Production
code continues to forbid unsafe code.

From a configured Rust environment:

```sh
python3 collect_allocations.py BASELINE_ROOT CANDIDATE_ROOT \
  BASELINE_PROBE_BINARY CANDIDATE_PROBE_BINARY allocations.json
```

The driver checks full outcomes and stable samples, hashes source trees and executables
before and after, and records compiler/version/hash, fixture, probe, driver and lock
hashes. It records no clock durations. Compress with Python `gzip.compress(data,
mtime=0)` to reproduce the published archive format. Public artifacts contain only
synthetic fixtures/results and permitted source/tool metadata; local build logs and
absolute execution paths are excluded. Re-run compatibility tests on both roots with
`cargo test -p rydantic --test fixed_model_slots --locked --offline -- --nocapture --test-threads=1`
and compare the `SLOT_ORACLE ` JSON records.
