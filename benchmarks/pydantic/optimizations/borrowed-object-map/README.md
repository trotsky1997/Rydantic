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

## Complete canonical results

UTC: 2026-10-25T18:49:48.143418+00:00 to 2026-10-25T18:51:37.293019+00:00. Load start/end: [1.29052734375, 2.466796875, 1.94921875] / [1.04296875, 2.009765625, 1.837890625]. Raw JSON SHA256: `c350f26a7ea112f2a11b3b82946b8d14ebc3fc19cce0bac1ac7278d93888aef6`.

Executable hashes: `{'baseline': 'b66bcd1a737ba8f80d8fe31cad1b3683c87acc59229648de20d0416f05ed567d', 'previous': '2cbf459eae69a357a26ba94b0db9e40ebabda25cf67cf42b5bf98d797248dddc', 'candidate': 'cbe22102a8f0acfc0f066492c8e1c04a762c5c631a88837e9d1dfaa0e24f88cf', 'buffered': '52b79385a7450782381885e62f9112b79d13aaac35c7138e9bc867b47514ae55'}`. Python: `fa67443527ed9647f760d807e2a38f26340757123e643c4639cf273ed15d5ea7`.

All values ns/op: median (inclusive IQR). Direct is the semantics-preserving single-map candidate; buffered is an experimental alternative.

| Case | Mode | Original | Three fixes | Direct | Buffered | Pydantic | Three/direct |
|---|---|---:|---:|---:|---:|---:|---:|
| flat_valid | json | 1892.6 (24.4) | 965.2 (193.3) | 873.1 (53.5) | 845.6 (41.9) | 1028.3 (23.0) | 1.105 |
| flat_valid | value_copy | 1567.2 (21.6) | 569.4 (6.6) | 630.0 (39.3) | 567.5 (3.0) | 2941.8 (65.1) | 0.904 |
| flat_unicode | json | 1835.8 (97.7) | 939.5 (29.9) | 870.9 (33.3) | 846.9 (24.6) | 1159.4 (56.6) | 1.079 |
| flat_unicode | value_copy | 1512.6 (50.0) | 617.3 (56.4) | 566.8 (24.0) | 589.0 (27.0) | 2919.3 (45.4) | 1.089 |
| nested_valid | json | 3471.6 (177.1) | 1950.8 (54.6) | 1723.7 (44.6) | 1939.7 (331.7) | 1716.8 (54.6) | 1.132 |
| nested_valid | value_copy | 2721.4 (100.5) | 1126.2 (26.4) | 1232.6 (112.9) | 1235.7 (102.3) | 5541.9 (491.8) | 0.914 |
| collection_100 | json | 214674.0 (22132.5) | 106368.2 (4036.4) | 94132.0 (2365.5) | 97775.8 (9528.3) | 62905.4 (740.3) | 1.130 |
| collection_100 | value_copy | 170952.2 (10607.4) | 70052.9 (2303.4) | 66680.1 (5767.1) | 65533.8 (2405.7) | 224836.7 (14398.7) | 1.051 |
| flat_wrong_type | json | 1494.5 (68.2) | 1214.6 (160.1) | 1040.9 (69.5) | 1066.1 (58.6) | 1393.9 (94.7) | 1.167 |
| flat_wrong_type | value_copy | 1170.1 (24.8) | 864.7 (211.3) | 873.5 (84.6) | 813.8 (72.3) | 3090.2 (100.6) | 0.990 |
| flat_bool_as_int | json | 1537.2 (263.0) | 1112.5 (50.3) | 964.9 (20.2) | 1007.6 (34.7) | 1395.3 (96.1) | 1.153 |
| flat_bool_as_int | value_copy | 1308.2 (362.8) | 804.8 (16.0) | 782.0 (17.8) | 743.5 (22.5) | 3054.5 (100.5) | 1.029 |
| flat_negative | json | 2213.5 (141.0) | 1163.9 (39.4) | 1105.6 (51.2) | 1068.2 (72.5) | 1465.8 (32.8) | 1.053 |
| flat_negative | value_copy | 1797.3 (74.5) | 858.7 (45.5) | 787.8 (208.4) | 838.7 (95.9) | 3056.6 (271.9) | 1.090 |
| flat_upper_bound | json | 2207.8 (290.8) | 1247.6 (187.5) | 1137.8 (69.0) | 1164.9 (52.8) | 1430.3 (117.9) | 1.097 |
| flat_upper_bound | value_copy | 1923.5 (219.0) | 826.9 (24.7) | 806.7 (24.3) | 809.6 (67.8) | 3027.2 (34.2) | 1.025 |
| flat_empty_name | json | 2249.5 (242.5) | 1202.4 (27.1) | 1101.1 (59.0) | 1098.1 (32.4) | 1409.6 (121.5) | 1.092 |
| flat_empty_name | value_copy | 1832.5 (121.7) | 827.7 (41.5) | 879.9 (46.6) | 859.8 (53.8) | 3150.3 (245.2) | 0.941 |
| flat_long_name | json | 2272.5 (95.9) | 1382.2 (119.6) | 1272.5 (50.9) | 1204.0 (39.7) | 1585.5 (94.2) | 1.086 |
| flat_long_name | value_copy | 2311.2 (393.8) | 939.2 (200.9) | 1069.0 (204.5) | 949.8 (104.9) | 3260.6 (63.9) | 0.879 |
| flat_missing | json | 1519.4 (488.1) | 939.1 (19.8) | 741.3 (19.3) | 868.3 (233.3) | 1406.8 (30.0) | 1.267 |
| flat_missing | value_copy | 1114.9 (36.7) | 618.6 (139.4) | 548.3 (28.2) | 545.5 (29.1) | 2923.1 (358.1) | 1.128 |
| flat_null | json | 1855.9 (171.2) | 1125.8 (66.6) | 1300.7 (646.4) | 1124.6 (195.8) | 1740.6 (279.2) | 0.866 |
| flat_null | value_copy | 1542.2 (80.0) | 877.6 (34.8) | 884.8 (28.6) | 955.4 (89.9) | 3351.0 (254.2) | 0.992 |
| flat_extra | json | 2212.4 (102.5) | 1320.3 (91.0) | 1132.3 (65.1) | 1134.0 (33.7) | 1464.2 (13.4) | 1.166 |
| flat_extra | value_copy | 2043.5 (267.8) | 851.2 (113.1) | 882.1 (91.9) | 863.9 (105.6) | 3690.2 (232.1) | 0.965 |
| flat_multiple | json | 2440.8 (42.2) | 1799.5 (272.8) | 1801.1 (187.0) | 1827.0 (160.2) | 1799.9 (358.0) | 0.999 |
| flat_multiple | value_copy | 2113.3 (352.9) | 1401.2 (94.5) | 1446.9 (134.9) | 1398.0 (77.9) | 3426.1 (425.4) | 0.968 |
| nested_invalid | json | 3371.0 (665.1) | 2475.0 (222.2) | 2189.8 (405.2) | 2423.1 (173.6) | 2255.8 (103.8) | 1.130 |
| nested_invalid | value_copy | 2537.2 (199.8) | 1858.4 (169.0) | 1775.1 (129.7) | 1825.8 (212.4) | 5484.8 (272.1) | 1.047 |
| collection_invalid | json | 5769.8 (826.9) | 2855.6 (396.6) | 2560.7 (323.1) | 2723.1 (383.4) | 2591.6 (98.9) | 1.115 |
| collection_invalid | value_copy | 4237.1 (333.2) | 1765.5 (11.9) | 1735.5 (55.6) | 1823.8 (85.9) | 6769.5 (108.7) | 1.017 |
| malformed_json | json | 288.8 (12.5) | 300.5 (21.7) | 323.2 (17.9) | 314.7 (26.3) | 918.7 (28.0) | 0.930 |
| flat_min | json | 1799.0 (130.1) | 948.4 (186.1) | 862.7 (9.6) | 985.8 (86.1) | 1133.1 (44.2) | 1.099 |
| flat_min | value_copy | 1490.5 (34.4) | 627.2 (15.4) | 579.6 (42.3) | 624.0 (27.9) | 2892.0 (62.4) | 1.082 |
| flat_max | json | 2211.2 (449.5) | 1098.8 (60.3) | 937.8 (25.6) | 925.8 (27.4) | 1153.5 (89.0) | 1.172 |
| flat_max | value_copy | 1687.4 (223.0) | 659.3 (49.4) | 726.8 (133.0) | 635.5 (61.4) | 2990.7 (29.3) | 0.907 |
| flat_unicode64 | json | 2014.3 (72.3) | 1074.1 (37.4) | 1035.4 (65.7) | 958.2 (35.4) | 2205.6 (68.0) | 1.037 |
| flat_unicode64 | value_copy | 1555.5 (49.2) | 639.8 (66.7) | 657.8 (120.8) | 692.8 (25.1) | 2975.5 (59.1) | 0.973 |
| flat_unicode65 | json | 2356.4 (58.6) | 1400.6 (37.9) | 1337.2 (76.1) | 1318.0 (35.6) | 2501.0 (185.8) | 1.047 |
| flat_unicode65 | value_copy | 1906.1 (71.2) | 911.0 (32.7) | 1002.2 (104.2) | 909.7 (43.4) | 3252.8 (211.7) | 0.909 |
| flat_huge_int | json | 1531.4 (23.7) | 1149.0 (46.0) | 1151.6 (205.5) | 1015.4 (32.8) | 1575.6 (61.3) | 0.998 |
| flat_huge_int | value_copy | 1159.7 (35.8) | 655.3 (41.8) | 664.0 (7.3) | 716.7 (22.4) | 3586.2 (438.0) | 0.987 |
| flat_root_array | json | 278.2 (10.1) | 299.5 (34.2) | 282.8 (5.1) | 284.9 (10.7) | 853.0 (33.2) | 1.059 |
| flat_root_array | value_copy | 142.7 (18.0) | 142.5 (153.3) | 145.2 (13.2) | 141.1 (7.5) | 1576.9 (92.7) | 0.981 |
| nested_extra | json | 3329.8 (145.4) | 1756.5 (36.0) | 1519.7 (67.8) | 1562.1 (63.0) | 1912.0 (147.3) | 1.156 |
| nested_extra | value_copy | 2821.5 (489.0) | 1143.7 (62.8) | 1149.1 (49.4) | 1149.5 (46.3) | 5049.2 (121.3) | 0.995 |
| flat_nonintegral_float | json | 1518.2 (33.8) | 1180.1 (63.9) | 1038.0 (23.6) | 1072.6 (97.4) | 1398.6 (84.9) | 1.137 |
| flat_nonintegral_float | value_copy | 1062.9 (19.7) | 765.5 (63.0) | 752.7 (76.3) | 717.0 (23.2) | 3200.1 (283.4) | 1.017 |

## Canonical interpretation

- flat_valid: direct incremental reduction 9.5%; direct original-to-four-fix reduction 53.9%; direct/Pydantic median ratio 0.849.
- nested_valid: direct incremental reduction 11.6%; direct original-to-four-fix reduction 50.3%; direct/Pydantic median ratio 1.004.
- collection_100: direct incremental reduction 11.5%; direct original-to-four-fix reduction 56.2%; direct/Pydantic median ratio 1.496.

Rows with slower direct medians than three fixes (not hidden): flat_valid/value_copy, nested_valid/value_copy, flat_wrong_type/value_copy, flat_empty_name/value_copy, flat_long_name/value_copy, flat_null/json, flat_null/value_copy, flat_extra/value_copy, flat_multiple/json, flat_multiple/value_copy, malformed_json/json, flat_max/value_copy, flat_unicode64/value_copy, flat_unicode65/value_copy, flat_huge_int/json, flat_huge_int/value_copy, flat_root_array/value_copy, nested_extra/value_copy.

These are same-session direct comparisons, not products of percentages from older sessions. IQR, workload semantics and shared-host caveats prevent a universal ranking or no-regression claim.

## Isolated object materialization

Median (inclusive IQR), ns/op; 10 batch samples per cell.

| Entries | Reverse | Old | Direct | Buffered |
|---|---|---:|---:|---:|
| 3 | false | 375.0 (71.4) | 209.9 (40.4) | 276.1 (73.7) |
| 3 | true | 405.7 (48.4) | 301.1 (89.0) | 280.3 (66.0) |
| 12 | false | 1357.2 (156.2) | 968.9 (25.8) | 961.5 (98.5) |
| 12 | true | 1468.7 (135.3) | 1207.8 (206.0) | 1154.6 (96.7) |
| 32 | false | 3929.6 (530.2) | 2861.3 (173.0) | 2458.4 (265.3) |
| 32 | true | 3740.7 (217.3) | 3029.7 (686.8) | 2300.5 (172.0) |
| 128 | false | 18667.7 (1809.4) | 14251.6 (583.9) | 9790.4 (473.8) |
| 128 | true | 15106.2 (614.7) | 13165.4 (1385.2) | 11388.0 (532.6) |
| 1024 | false | 194867.7 (38307.0) | 153373.5 (13104.3) | 85771.6 (3066.2) |
| 1024 | true | 126033.3 (5415.6) | 111022.2 (6672.0) | 82434.0 (3239.5) |

## Wide typed-map parse and decode

Median (inclusive IQR), ns/op; 10 batch samples per cell.

| Entries | Reverse | Old | Direct | Buffered |
|---|---|---:|---:|---:|
| 3 | false | 873.3 (72.8) | 768.3 (103.1) | 733.1 (44.2) |
| 3 | true | 886.1 (128.1) | 771.4 (85.7) | 724.1 (69.0) |
| 12 | false | 3338.6 (310.0) | 3423.5 (283.0) | 3194.7 (437.5) |
| 12 | true | 3449.0 (353.1) | 3162.0 (170.7) | 3028.5 (196.3) |
| 32 | false | 8838.2 (832.5) | 7904.8 (218.2) | 7141.7 (311.1) |
| 32 | true | 9073.8 (796.2) | 8447.4 (259.9) | 7480.4 (355.6) |
| 128 | false | 38010.5 (2307.7) | 37257.2 (1562.6) | 32472.4 (649.2) |
| 128 | true | 41557.6 (3206.2) | 36039.5 (2424.5) | 33667.9 (1621.0) |
| 1024 | false | 375201.7 (12482.9) | 383519.9 (14733.8) | 289950.1 (17026.9) |
| 1024 | true | 347543.7 (15007.7) | 326554.9 (8188.6) | 296466.1 (13222.9) |

## Duplicate-key typed-map parse and decode

Median (inclusive IQR), ns/op; 10 batch samples per cell.

| Entries | Reverse | Old | Direct | Buffered |
|---|---|---:|---:|---:|
| 3 | false | 445.5 (14.6) | 393.6 (41.8) | 449.4 (60.0) |
| 3 | true | 438.8 (44.2) | 390.9 (13.7) | 428.7 (22.2) |
| 12 | false | 808.7 (16.0) | 828.5 (63.3) | 1028.9 (57.0) |
| 12 | true | 821.2 (44.8) | 814.8 (33.4) | 991.8 (36.9) |
| 32 | false | 1724.8 (85.2) | 1710.3 (108.4) | 2356.6 (282.0) |
| 32 | true | 1724.1 (69.2) | 1718.8 (68.8) | 2354.9 (149.5) |
| 128 | false | 5753.5 (145.1) | 6119.9 (230.3) | 8817.0 (406.5) |
| 128 | true | 5670.7 (134.7) | 6179.4 (212.4) | 8771.6 (300.2) |
| 1024 | false | 44741.3 (5105.7) | 47468.4 (760.8) | 71191.5 (5465.7) |
| 1024 | true | 47733.9 (5761.0) | 53896.1 (11157.1) | 83073.3 (7611.2) |
