# Combined static-layout and exact-integer optimization experiment

Refs #117 and #119. This final paired run compares the **original unoptimized runtime directly with both fixes together**, alongside Pydantic in the same session. It does not multiply percentages from previous runs. Original benchmark evidence and both incremental experiments remain immutable.

## Scope and provenance

Original baseline: `862907a2aef111a6e29d9a7df5338c0e33819c2a`, runtime unchanged from main `4d61843`; fresh checkout `/workspace/shared/rydantic-combined-original` and dedicated target `/workspace/shared/rydantic-combined-original-target`. Candidate runtime/test source: published `78ae5228f0415edb55c30087c1cfdd351bb9581d` (local `b255a8b24dd9b61b4cbf49df252874659d836168`), identical to evidence head `a8369a029e45949333f6c8bcf8b25b34f6d61667` except evidence documents. Candidate executable is the independently audited isolated build from `../integer-comparison`, not rebuilt or changed for this run.

The candidate includes exactly two runtime optimizations:
1. Omit redundant collision-name bookkeeping for statically checked model layouts; retain flatten/extra dynamic checks.
2. Compare lexical integer bounds fitting i128 without allocating decimal buffers; retain the unchanged exact decimal/exponent/out-of-i128 fallback.

The proposed scalar integer decode optimization is **not included**. Both engines use Rust internally; these are API/workload measurements, not a language-wide Rust-versus-Python result.

## Method

Both Rust artifacts use Rust/Cargo 1.98.1, the same Cargo.lock, release opt-level 3, no debug assertions, debuginfo 0 and no custom RUSTFLAGS. The original was freshly built into its own target; the candidate retains its separately verified target/artifact. Build JSON receipts and independent source, binary and fixture hashes are retained. No instrumented executable is timed. See the incremental experiment for isolated allocation counts and full local correctness coverage.

The unchanged 49 case-mode fixture corpus is validated against the existing expected-output/error-category/path oracle on every worker result. Baseline/candidate/Pydantic/Pydantic/candidate/baseline counterbalanced order; two independent processes per engine/case/mode; each uses 1,000 warmup operations and three batches of 10,000 operations. Results are consumed and disposed; compilation and process startup are outside timing. Six batch-average samples per engine are summarized by median and inclusive IQR, not request-tail latency. All 294 worker results are retained.

`compare.py` verifies both original and candidate source snapshots and executable hashes before/after the experiment. The canonical candidate verification had separately passed 98 comparable checks plus four known-difference checks for integral decimal 42.0; the known difference is not timed as comparable work. Baseline and candidate use exactly the same fixture bytes.

Primary `json` includes parsing and validation. Secondary `value_copy` includes language-specific input-copy costs and is not pure validation. Rejection paths do not render identical diagnostic detail: Rydantic eagerly constructs errors while Pydantic `.errors()` materialization remains outside timing. Pydantic 2.14.1/core 2.50.1 on CPython 3.12.14 uses default string caching on repeated payloads; these results do not describe cold-cache/high-cardinality traffic. Other owned workers paused CPU-heavy work, but shared-host scheduling and contention remain uncontrolled; no CPU pinning or exclusive host was used. No universal speed/no-regression claim or CI speed threshold follows.

## Complete paired results

UTC: 2026-10-25T18:09:37.532620+00:00 to 2026-10-25T18:10:59.624045+00:00. Load start/end: [0.81689453125, 1.57958984375, 1.29443359375] / [0.9541015625, 1.439453125, 1.26513671875]. Uncompressed raw SHA256: `b4eeedffccebe9ca3ed4fcf784008cd6a8a06e131de991756590a3852ce27bcd`.

Executable SHA256: {'baseline': 'b66bcd1a737ba8f80d8fe31cad1b3683c87acc59229648de20d0416f05ed567d', 'candidate': '19d2e1fdc1440b3cbdbd6813fe21e1cc79b517c48f8d36e470d0c284426d47b2'}

All values ns/op: median (inclusive IQR). Before/after >1 favors candidate.

| Case | Mode | Before | Candidate | Pydantic | Before/after |
|---|---|---:|---:|---:|---:|
| flat_valid | json | 2182.4 (411.5) | 1315.5 (184.5) | 1173.9 (81.1) | 1.659 |
| flat_valid | value_copy | 1712.6 (239.2) | 725.4 (101.4) | 3244.4 (195.5) | 2.361 |
| flat_unicode | json | 2001.9 (130.4) | 1154.9 (174.7) | 1272.4 (182.9) | 1.733 |
| flat_unicode | value_copy | 1650.5 (109.8) | 646.9 (86.6) | 3327.4 (644.0) | 2.552 |
| nested_valid | json | 3610.3 (836.3) | 2437.1 (662.8) | 1900.2 (497.9) | 1.481 |
| nested_valid | value_copy | 2486.4 (563.8) | 1240.7 (333.1) | 5179.9 (257.6) | 2.004 |
| collection_100 | json | 208013.3 (7949.1) | 109471.5 (4898.9) | 62467.8 (4673.2) | 1.900 |
| collection_100 | value_copy | 161865.7 (4364.7) | 71186.7 (4404.0) | 230343.7 (3939.1) | 2.274 |
| flat_wrong_type | json | 1468.2 (23.7) | 1074.1 (18.2) | 1436.5 (18.7) | 1.367 |
| flat_wrong_type | value_copy | 1136.9 (41.6) | 817.3 (21.6) | 3068.6 (105.3) | 1.391 |
| flat_bool_as_int | json | 1534.5 (115.2) | 1144.4 (46.8) | 1452.8 (74.7) | 1.341 |
| flat_bool_as_int | value_copy | 1190.8 (75.4) | 788.6 (23.7) | 3150.2 (143.2) | 1.510 |
| flat_negative | json | 2144.7 (145.2) | 1274.6 (91.0) | 1392.1 (77.7) | 1.683 |
| flat_negative | value_copy | 1830.5 (63.9) | 885.6 (76.4) | 3045.8 (100.1) | 2.067 |
| flat_upper_bound | json | 2183.6 (99.1) | 1344.3 (112.6) | 1421.0 (92.6) | 1.624 |
| flat_upper_bound | value_copy | 1990.6 (132.3) | 913.1 (52.8) | 3253.0 (494.4) | 2.180 |
| flat_empty_name | json | 2110.6 (208.8) | 1266.3 (86.4) | 1435.0 (68.9) | 1.667 |
| flat_empty_name | value_copy | 1762.6 (44.3) | 906.3 (112.1) | 3054.2 (162.4) | 1.945 |
| flat_long_name | json | 2224.1 (637.0) | 1471.4 (203.5) | 1583.5 (312.6) | 1.512 |
| flat_long_name | value_copy | 1904.7 (256.0) | 1004.6 (131.6) | 3116.6 (112.3) | 1.896 |
| flat_missing | json | 1421.7 (38.2) | 874.5 (93.3) | 1481.6 (62.9) | 1.626 |
| flat_missing | value_copy | 1137.2 (57.0) | 667.5 (11.5) | 2673.8 (248.9) | 1.704 |
| flat_null | json | 1780.8 (106.5) | 1185.7 (82.5) | 1340.1 (43.7) | 1.502 |
| flat_null | value_copy | 1507.4 (202.1) | 1019.9 (351.7) | 3432.2 (623.1) | 1.478 |
| flat_extra | json | 2158.8 (76.8) | 1408.6 (133.3) | 1397.1 (19.8) | 1.533 |
| flat_extra | value_copy | 1851.6 (258.3) | 830.4 (24.7) | 3507.7 (130.0) | 2.230 |
| flat_multiple | json | 2373.7 (185.9) | 1783.9 (44.7) | 1623.3 (49.0) | 1.331 |
| flat_multiple | value_copy | 1915.5 (371.0) | 1415.4 (244.7) | 3228.2 (114.8) | 1.353 |
| nested_invalid | json | 3165.3 (48.0) | 2538.0 (238.6) | 2051.1 (162.8) | 1.247 |
| nested_invalid | value_copy | 2412.4 (59.1) | 1703.8 (146.5) | 5342.7 (212.2) | 1.416 |
| collection_invalid | json | 5607.7 (458.7) | 3047.2 (163.4) | 2469.0 (150.8) | 1.840 |
| collection_invalid | value_copy | 4172.5 (135.4) | 1785.0 (37.1) | 6617.8 (49.2) | 2.338 |
| malformed_json | json | 289.3 (12.6) | 293.1 (16.0) | 937.6 (41.0) | 0.987 |
| flat_min | json | 1878.0 (52.8) | 1022.6 (67.3) | 1012.1 (58.7) | 1.836 |
| flat_min | value_copy | 1453.9 (41.8) | 721.4 (97.4) | 2791.4 (98.3) | 2.015 |
| flat_max | json | 2114.2 (137.7) | 1102.3 (33.5) | 1126.7 (93.5) | 1.918 |
| flat_max | value_copy | 1887.9 (365.2) | 761.8 (122.5) | 3034.0 (212.1) | 2.478 |
| flat_unicode64 | json | 1976.8 (175.2) | 1185.4 (203.1) | 2053.1 (22.3) | 1.668 |
| flat_unicode64 | value_copy | 1648.1 (123.1) | 668.5 (36.2) | 3063.7 (220.2) | 2.465 |
| flat_unicode65 | json | 2690.9 (565.9) | 1537.6 (295.0) | 2547.0 (166.6) | 1.750 |
| flat_unicode65 | value_copy | 2021.7 (94.5) | 1069.8 (156.9) | 3600.0 (999.1) | 1.890 |
| flat_huge_int | json | 1776.1 (113.9) | 1320.8 (62.0) | 1635.4 (55.0) | 1.345 |
| flat_huge_int | value_copy | 1176.4 (7.7) | 925.0 (53.0) | 3503.6 (134.1) | 1.272 |
| flat_root_array | json | 280.9 (7.3) | 274.0 (33.5) | 888.2 (46.4) | 1.025 |
| flat_root_array | value_copy | 166.5 (13.9) | 137.9 (24.6) | 1595.8 (405.5) | 1.208 |
| nested_extra | json | 3326.3 (534.2) | 2002.7 (38.7) | 1893.0 (114.1) | 1.661 |
| nested_extra | value_copy | 2831.4 (162.5) | 1491.5 (556.6) | 6340.0 (962.9) | 1.898 |
| flat_nonintegral_float | json | 1545.8 (28.2) | 1278.6 (175.9) | 1675.3 (81.2) | 1.209 |
| flat_nonintegral_float | value_copy | 1377.1 (227.9) | 934.1 (123.6) | 3469.7 (556.3) | 1.474 |

Direct original-to-combined primary successful-input medians fell about 39.7%, 32.5%, and 47.4% for flat, nested, and collection100 respectively in this paired run. The combined candidate nevertheless remained slower than same-run Pydantic on all three: about 1.12×, 1.28× and 1.75× its medians. The earlier incremental run placed flat slightly below Pydantic; this final run does not. Both raw runs are preserved, and the variation reinforces that a shared-cloud warmed microbenchmark does not establish a universal ranking. No percentages from different sessions are multiplied.

Not every row improved: malformed-JSON rejection median rose 289.3→293.1 ns/op. It rejects before the optimized model/comparator paths; this experiment does not establish the cause of that variation or guarantee no regressions.

Independent audit recomputed all 49 table rows and verified all 294 oracle outputs/checksums, both source snapshots against their commits, identical fixture/lockfile/worker hashes, the fresh original and unchanged audited candidate artifacts/profiles, gzip fidelity, and raw hash. The final three-workload ranking and earlier flat ranking reversal were explicitly checked. Audit script/log are retained here.
