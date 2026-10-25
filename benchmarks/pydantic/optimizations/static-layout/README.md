# Static-layout collision bookkeeping experiment

Refs #117. This is a bounded optimization of Rydantic, not a language-wide Rust/Python race or a claim that all Rydantic workloads are faster.

## What changed and why

The derive already rejects duplicate fixed input aliases, fixed non-excluded output aliases, and enum-tag collisions. Nevertheless each successful named-model parse rebuilt both input and output name vectors/hash sets twice. The candidate omits this bookkeeping **only when there are no flattened or extra fields**. Dynamic names, extra-key collisions, field decoding, normalization, validation and error construction are unchanged. No parser rewrite or rule weakening is included.

Baseline: `862907a2aef111a6e29d9a7df5338c0e33819c2a`; candidate source: `f85169bfd16b9a3d52c468e23957032cac2a8987` (tree `89b6b1475b9e9e8477b74ac80ff37a8b4cb90601`, identical to locally tested `40370f2f061fad9f3578c07f620a68812fdfc81d`). Original benchmark evidence in `../../evidence` is unchanged.

## Allocation diagnosis, separate from timing

A standalone System global allocator counting probe measured one `parse_json` call, before correctness dumping or warmup. Calls include allocations and reallocations; bytes are cumulative requested allocation/reallocation sizes, **not peak/live memory**. The probe is instrumented and its timings are not used. `allocation_probe.rs.txt` retains the exact diagnostic worker; it is deliberately not compiled into the production library or normal benchmark. To reproduce, put it in a separate Cargo binary package with a path dependency on the selected `rydantic` source, serde with derive, and serde_json; run the fixture request protocol with iterations=1, warmup=0, repeats=1. Dependencies resolved to the same versions used by the locked workspace for this run.

| JSON fixture | Before calls | After calls | Before requested bytes | After requested bytes |
|---|---:|---:|---:|---:|
| flat_valid | 62 | 30 | 2,563 | 1,619 |
| nested_valid | 100 | 44 | 4,951 | 3,095 |
| collection_100 | 5,635 | 2,419 | 248,650 | 153,362 |

Only the two generator early returns changed between these allocation probes. Thus the flat fixture isolates 32 avoided allocation/reallocation calls to redundant name bookkeeping. This is allocation attribution, not a sampled CPU profile or a percentage breakdown of total execution time. No sampling profiler was installed in this executor.

## Timing method and limitations

Both Rydantic binaries were built with the same Rust/Cargo 1.98.1, workspace Cargo.lock, release opt-level 3, target and flags. Separate copied executable paths and SHA-256 hashes prevent artifact confusion. A final-source rebuild produced a byte-identical candidate binary. Compilation is outside measurements. All engines use the unchanged fixture corpus, same bytes, same 1,000-operation warmup and 10,000 operations per timed batch, consuming and dropping results. Each engine runs in two independent processes per case/mode, with three batches per process, in counterbalanced baseline/candidate/Pydantic/Pydantic/candidate/baseline order. Six batch-average samples per engine are aggregated; medians/IQRs below are not per-request tail latency.

Primary `json` includes parsing and validation. Secondary `value_copy` additionally includes language-specific input-copy costs and is not a pure validation comparison. Pydantic 2.14.1 / pydantic-core 2.50.1 on CPython 3.12.14 uses default string caching on repeated payloads; Rydantic produces owned strings. This is a warm identical-payload API workload, not high-cardinality traffic. Native error creation is included but diagnostic formatting differs; no equal error-rendering cost is claimed. Pydantic's core is itself Rust.

Other owned cloud workers paused local builds during timing. The shared cloud host was not exclusive or CPU-pinned; scheduling and unobserved host contention remain uncontrolled. Do not compare these numbers directly with the older standalone evidence run to calculate speedups. All 49 valid/invalid case-mode combinations are reported, including noisy/unimproved cases. No CI speed threshold is introduced.

`compare.py` reproduces the paired experiment using explicit baseline/candidate binaries and the pinned Python interpreter. It imports the original correctness oracle and verifies every operation result and checksum before retaining timings. Before the experiment, the canonical runner also passed 98 parity checks and four separately checked integral-42.0 known-difference checks. The known difference is unchanged and never timed as comparable work.

Validation: default-feature `cargo test --locked -p rydantic --tests` passed (including compile UI, flatten/extra, borrowed, normalization, error-path and new cfg/alias/exclusion regressions); derive unit tests 4/4; focused clippy with warnings denied and formatting passed. This does not claim a full all-feature workspace or remote CI pass. Independent source review found no blocker.

An initial three-engine driver attempt stopped on differently ordered but equivalent error lists; it was corrected to use the existing normalized path/category oracle. That incomplete attempt is not performance evidence. The prior completed Rydantic-only ABBA raw file is retained as exploratory evidence; final results below use the complete three-engine run only.

## Complete final paired results

UTC: 2026-10-25T17:52:20.668000+00:00 to 2026-10-25T17:53:48.281732+00:00. Load start/end: [0.90576171875, 1.15478515625, 0.833984375] / [1.13232421875, 1.17041015625, 0.8720703125]. Uncompressed raw SHA256: `a4735a528cdd476844757b6460a6c3386448d48d953fd4f242ac4861d79f14fc`.

Binary SHA256: {'baseline': 'b66bcd1a737ba8f80d8fe31cad1b3683c87acc59229648de20d0416f05ed567d', 'candidate': '05bd24031be3eb6ce1c5c8a9486a638bd38dcc2484e5727611c716b106ab667e'}

All values are ns/op: median (inclusive IQR). Before/after ratio > 1 indicates candidate speedup.

| Case | Mode | Before | Candidate | Pydantic | Before/after |
|---|---|---:|---:|---:|---:|
| flat_valid | json | 1919.1 (86.9) | 1268.5 (98.0) | 1109.0 (96.8) | 1.513 |
| flat_valid | value_copy | 1672.5 (73.8) | 953.3 (35.4) | 3128.2 (213.3) | 1.754 |
| flat_unicode | json | 1990.1 (276.1) | 1176.9 (234.7) | 1159.3 (74.2) | 1.691 |
| flat_unicode | value_copy | 1669.1 (91.0) | 846.2 (4.5) | 2939.0 (242.2) | 1.972 |
| nested_valid | json | 3465.1 (983.3) | 2291.7 (243.1) | 1891.4 (100.3) | 1.512 |
| nested_valid | value_copy | 2700.5 (136.8) | 1635.9 (42.6) | 7123.3 (676.0) | 1.651 |
| collection_100 | json | 216549.3 (8188.1) | 133946.9 (12830.3) | 61144.9 (746.4) | 1.617 |
| collection_100 | value_copy | 167939.1 (8012.6) | 94595.0 (2430.5) | 229649.8 (4375.5) | 1.775 |
| flat_wrong_type | json | 1541.0 (117.4) | 1267.1 (48.0) | 1526.3 (97.8) | 1.216 |
| flat_wrong_type | value_copy | 1292.4 (107.8) | 905.0 (69.1) | 3438.7 (318.6) | 1.428 |
| flat_bool_as_int | json | 1561.8 (53.7) | 1249.4 (50.5) | 1558.5 (282.7) | 1.250 |
| flat_bool_as_int | value_copy | 1119.9 (50.7) | 915.5 (100.1) | 3350.7 (224.0) | 1.223 |
| flat_negative | json | 2520.6 (505.1) | 1683.3 (456.1) | 1454.7 (42.1) | 1.497 |
| flat_negative | value_copy | 1871.4 (67.4) | 1260.3 (57.1) | 3310.9 (87.8) | 1.485 |
| flat_upper_bound | json | 2529.9 (241.8) | 1949.5 (544.9) | 1524.9 (217.8) | 1.298 |
| flat_upper_bound | value_copy | 2076.3 (177.1) | 1188.1 (16.6) | 3469.7 (391.5) | 1.748 |
| flat_empty_name | json | 2279.9 (157.9) | 1606.7 (176.0) | 1686.9 (584.5) | 1.419 |
| flat_empty_name | value_copy | 2283.4 (388.0) | 1231.1 (53.0) | 3155.1 (117.0) | 1.855 |
| flat_long_name | json | 2361.3 (213.8) | 1768.8 (234.8) | 1394.2 (250.4) | 1.335 |
| flat_long_name | value_copy | 1983.2 (97.0) | 1240.5 (122.5) | 3326.5 (139.9) | 1.599 |
| flat_missing | json | 1558.8 (79.6) | 1252.3 (458.9) | 1491.9 (77.2) | 1.245 |
| flat_missing | value_copy | 1227.5 (59.0) | 892.9 (55.3) | 2900.5 (109.0) | 1.375 |
| flat_null | json | 1897.7 (59.4) | 1573.3 (28.8) | 1453.1 (60.4) | 1.206 |
| flat_null | value_copy | 1549.5 (91.5) | 1075.5 (40.5) | 3266.8 (119.9) | 1.441 |
| flat_extra | json | 2215.9 (175.4) | 1537.7 (183.7) | 1424.6 (26.4) | 1.441 |
| flat_extra | value_copy | 1979.6 (147.2) | 1169.9 (45.5) | 3614.2 (167.7) | 1.692 |
| flat_multiple | json | 2480.3 (209.9) | 2226.4 (326.5) | 1788.5 (206.4) | 1.114 |
| flat_multiple | value_copy | 2060.1 (306.3) | 1644.0 (98.3) | 3734.7 (151.9) | 1.253 |
| nested_invalid | json | 3759.2 (256.5) | 2685.0 (96.2) | 2166.2 (63.8) | 1.400 |
| nested_invalid | value_copy | 2749.0 (418.5) | 2061.9 (309.2) | 5387.2 (648.2) | 1.333 |
| collection_invalid | json | 5858.1 (506.1) | 3735.3 (196.1) | 2633.4 (143.2) | 1.568 |
| collection_invalid | value_copy | 4292.8 (295.4) | 2230.9 (53.6) | 7185.1 (476.1) | 1.924 |
| malformed_json | json | 294.9 (27.8) | 290.8 (45.3) | 1000.6 (147.0) | 1.014 |
| flat_min | json | 2036.9 (150.4) | 1229.6 (47.3) | 1141.2 (123.5) | 1.657 |
| flat_min | value_copy | 1672.9 (57.4) | 887.5 (8.4) | 3154.6 (560.0) | 1.885 |
| flat_max | json | 2321.0 (49.4) | 1328.3 (190.6) | 1130.7 (57.8) | 1.747 |
| flat_max | value_copy | 1751.0 (81.7) | 995.5 (113.6) | 2898.5 (70.9) | 1.759 |
| flat_unicode64 | json | 2213.6 (146.4) | 1323.9 (55.4) | 2128.8 (170.4) | 1.672 |
| flat_unicode64 | value_copy | 1553.5 (51.7) | 840.4 (40.0) | 3271.2 (130.4) | 1.849 |
| flat_unicode65 | json | 2386.9 (63.7) | 1809.2 (45.7) | 2379.5 (29.6) | 1.319 |
| flat_unicode65 | value_copy | 1936.1 (67.4) | 1239.3 (175.1) | 3702.0 (540.5) | 1.562 |
| flat_huge_int | json | 1608.8 (88.7) | 1356.3 (75.0) | 1560.9 (54.8) | 1.186 |
| flat_huge_int | value_copy | 1303.9 (228.4) | 801.7 (46.3) | 3467.1 (697.6) | 1.627 |
| flat_root_array | json | 280.7 (34.4) | 309.8 (522.5) | 1002.8 (35.1) | 0.906 |
| flat_root_array | value_copy | 152.2 (24.2) | 184.2 (32.2) | 1654.4 (127.4) | 0.827 |
| nested_extra | json | 3577.4 (71.2) | 2213.7 (27.7) | 2069.9 (350.3) | 1.616 |
| nested_extra | value_copy | 2973.0 (252.6) | 1471.6 (66.7) | 5482.7 (384.7) | 2.020 |
| flat_nonintegral_float | json | 1422.4 (10.5) | 1218.5 (104.2) | 1526.8 (43.1) | 1.167 |
| flat_nonintegral_float | value_copy | 1170.2 (163.3) | 715.3 (32.0) | 3173.6 (223.1) | 1.636 |

The candidate reduces primary valid-input medians by about 34%, 34%, and 38% for flat, nested, and collection100 respectively in this paired run, but remains about 1.14×, 1.21×, and 2.19× Pydantic's medians. These are fixture-specific observations, not universal speed claims. Root-array rejection did not improve: its JSON median rose 280.7→309.8 ns (candidate IQR522.5 ns), and copy-inclusive median152.2→184.2 ns (IQR24.2/32.2 ns). Those paths reject before the eliminated model-name work; the experiment does not establish the cause of that timing variation or guarantee no regression on every workload.

Independent evidence review recomputed every table cell, checked all 294 results against the original oracle, and verified all 204 captured source hashes against the tested commit plus actual executable hashes. Build/toolchain/profile claims above are separately observed build-step evidence (including the canonical runner's opt-level/debug assertions guard), not fields independently attested by the raw result. The focused script records hashes at start; a post-run independent comparison verified the same source and binaries, but it does not provide continuous mutation detection.
