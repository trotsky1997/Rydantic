# Borrowed JSON object materialization: direct versus buffered map construction

Refs #123. This experiment compares the original runtime, the three previous fixes, direct single-map insertion, a local buffered-entry alternative, and pinned Pydantic in the same session. All earlier evidence is unchanged. This is a bounded allocation/parse-path optimization, not a streaming parser rewrite or validation shortcut.

## Implementation and choice criteria

Previously, `Input::object` parsed `BTreeMap<String, &RawValue>` and then collected a second map with `Input::Borrowed` values. The direct candidate uses `deserialize_map` with exactly the same `String` key and `&RawValue` value types, inserting into the final map once. Ordered keys, eager replacement with the last duplicate, raw slice lifetimes and bytes, and the existing type-error mapping remain unchanged. Typed `deserialize_map`, rather than `deserialize_any`, is intentional: objects with serde_json private number/raw marker keys must remain ordinary objects. Empty, malformed, deep, nonobject, escaped-string, exact-decimal and huge-number cases remain on the same serde parser path.

The local alternative collects entries in a Vec before constructing one compact BTreeMap. It is **not** a production recommendation merely because a microbenchmark is faster. It retains every duplicate until collection, and current Rust collect's duplicate survivor is not a public last-wins guarantee. The direct visitor explicitly preserves eager last-wins semantics and bounds intermediate map storage by distinct keys. No key-count thresholds or fixture-specific paths were introduced.

## Source identity

- Original runtime local `862907a2aef111a6e29d9a7df5338c0e33819c2a`, unchanged runtime from `4d61843`; existing dedicated original target.
- Three-fix local `ca682a51f7686303828089a48d607725a4d19fd2`, published source `2e65b2ac2eb204fdfd6d3cac30a42ee3f4d89a1e`; existing dedicated scalar target.
- Direct candidate local `69a3fb482a704d60bb7619ddc97d985c084a3fac` (all source-file hashes in raw evidence); separate `rydantic-single-map-release-target`.
- Buffered alternative local `0b6789004dc265bd470fd666f4b4ef2ab28dcceb`; separate `rydantic-single-map-buffered-target`.

The five-engine public raw-sample JSON includes executable hashes, complete source-file hashes checked before/after, logical command artifact IDs, pinned engine metadata and the Python executable hash. Absolute local paths and unrelated OS build identifiers are removed from this public derivative; its private-original checksum is retained in public_export metadata. Samples, outputs, hashes and timing configuration are unchanged. Both newly built benchmark artifacts report opt-level 3, debuginfo 0, debug assertions false, overflow checks false; no RUSTFLAGS override. Compiler is Rust 1.98.1, Python 3.12.14, Pydantic 2.14.1 / core 2.50.1. No instrumented executable is used for timing.

## Verification

Direct candidate: full default core test suite (39 test suites), full workspace `--all-features --locked --offline` (90 test suites including doctests), and independent focused review/rerun passed. The first all-feature attempt failed only because capnp was not on PATH; the rerun used the existing official capnp 1.5.0 compiler and passed. The original failed log is retained, not reported as a test pass.

Both candidates passed the same 6,144 generated parser differential comparisons, edge/deep/malformed and 12/32/128/1,024-key split cases, identical raw slice pointers/text, and alias/flatten/duplicate/full serialized-error integration tests. The integration oracle substitutes the previous root map path; it is not an entire old-binary oracle. Formatting, all-feature core library clippy with warnings denied, canonical parity (98 comparable checks plus four known-difference controls), and independent review passed. Independent review approved direct correctness and buffered benchmarking, recording the latter's duplicate-memory and contract caveats.

## Measurement protocol and limitations

All 49 case/mode pairs run serially in order original → three-fix → direct → buffered → Pydantic → Pydantic → buffered → direct → three-fix → original. Each position uses 1,000 warmups and three 10,000-operation batches, giving six samples per engine/case/mode and 490 independently checked process results. Results are median batch-average ns/op and inclusive IQR, not request latency percentiles. Baseline loop timings are recorded but not subtracted. Fixed case order and shared-host noise remain limitations; other owned workers paused CPU work during measurements, but the machine was not exclusive or CPU-pinned.

Primary JSON includes parsing/validation and result disposal. Secondary `value_copy` includes different language-specific copying costs and is not pure validator cost. Rejection workloads do not materialize identical diagnostic detail. Repeated fixture strings permit Pydantic's default string caching; earlier cache-disabled diagnostics did not explain the collection gap, but these results are not cold/high-cardinality evidence. No universal speed ranking or CI speed threshold follows; all rows including regressions are retained.

## Allocation and wide-object controls

Standalone System allocator probes count calls and cumulative requested bytes; object controls additionally count tracked live requested bytes retained at return and their peak during materialization. These counters do not measure allocator overhead, RSS, or transient internal overlap inside realloc. Input construction is excluded. These are different metrics, not interchangeable claims. The materialized Input map is temporary during normal decode; retained-at-return is not final model memory. Instrumented durations are not timing evidence.

Full parse allocation counts on canonical fixtures (three fixes → direct): flat 20→18 calls / 1,529→905 requested bytes; nested 34→30 / 3,005→1,813; collection_100 1,421→1,219 / 144,385→81,473. Complete outputs are in `parse-allocations.json`.

`object-allocations.tsv` and `duplicate-allocations.tsv` columns: entry count, reverse order, engine, allocation/reallocation calls, cumulative requested bytes, live bytes retained at return, peak live bytes. The duplicate corpus repeats one key, so entry count is deliberately different from distinct-key count; reversing this repeated-key vector yields identical input, making its two reverse blocks repeats rather than distinct ordering controls. In auxiliary tables, Old means the previous object-map algorithm with the same three-fix scalar decoding, not the original runtime in the canonical table. Wide typed-map end-to-end controls include root RawValue parsing, map materialization, scalar decoding, error context and model disposal; they use `BTreeMap<String, i64>` rather than a fixture-specific derive schema. Diagnostics use 2,000 operations × five batches at each of two counterbalanced positions, 100 warmups, and no allocator instrumentation. They are separate controls, not substituted for canonical cases.

### Selected implementation and memory tradeoff

The direct visitor is selected; buffered remains an experimental diagnostic only. On 1,024 distinct keys, materialization cumulative bytes fall 263,136→117,640 and peak live bytes fall 144,888→117,640, but retained-at-return bytes rise 68,072→117,640 because the old sorted collect packed final nodes more tightly. This is **not universally lower retained memory**. On 1,024 repetitions of one key, direct materialization peak is 646 bytes versus old 695 and buffered 121,856; every implementation returns 639 live bytes. Full typed-map decode peak controls are retained separately in `wide-validation-allocations.tsv` (columns: entries, duplicate-key workload, mode 0=old/1=direct/2=buffered, calls, cumulative bytes, final model retained bytes, peak live bytes).

The direct candidate's wide ascending 1,024-key typed-map decode median is slower than old in this run; reverse order is faster. These complete controls and their IQR are reported below rather than hidden behind small-object gains. Parent engineering selection prioritizes defined eager duplicate semantics and bounded duplicate-heavy memory over buffered microbenchmark wins. Full canonical collection parsing remains slower than Pydantic; this change does not close the entire gap.

Reproduction: `compare.py --help` lists explicit source roots and separately built worker binaries. `gzip -dc results.json.gz > results.json` restores the redacted public JSON; `report.py` recomputes every table from raw files. Diagnostic probe sources and Cargo manifest are provided under `probes/`; substitute the local Rydantic dependency path when rebuilding. Timings and allocator probes are separate binaries. Original build and verification logs are retained in the task workspace; the report states their actual scopes.

Public export: `export_public.py PRIVATE_RESULTS PUBLIC_RESULTS.json.gz` creates a clearly marked path-redacted derivative without overwriting its input. Public evidence is independently audited against the private original; no claim of byte-for-byte identity between those two JSON files is made. Logical artifact IDs replace local paths, but all source/executable SHA256 values and measurements remain exact.
