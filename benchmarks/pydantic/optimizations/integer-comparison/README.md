# Exact integer comparison allocation experiment

Refs #119; draft #120, stacked on the static-layout optimization #118. This measures a second bounded optimization, **not** Rust versus Python as languages: Pydantic's core is also Rust.

## Change and semantics

`compare_numbers` previously always formatted both Numbers into owned Strings and collected their significant digits into Vecs. It now first tries both `Number::as_i128()` accessors and compares successful exact integers directly. In the locked serde_json arbitrary_precision implementation, that accessor parses its borrowed string; it neither allocates decimal buffers nor casts to floating point. Both operands must fit i128. Decimal/exponent forms and out-of-i128 values retain the original fallback unchanged, including existing extreme-exponent behavior. No new interpretation of arbitrary-precision or signed-zero inputs is introduced.

Baseline local commit `40370f2f061fad9f3578c07f620a68812fdfc81d` has tree `89b6b1475b9e9e8477b74ac80ff37a8b4cb90601`, identical to published static-layout source `f85169bfd16b9a3d52c468e23957032cac2a8987`. Candidate local commit `b255a8b24dd9b61b4cbf49df252874659d836168` adds only the five-line fast path and regression test. Published source `78ae5228f0415edb55c30087c1cfdd351bb9581d` has identical runtime/test blobs and additionally inherits the previous static-layout evidence. Original benchmark and previous optimization evidence are unchanged.

## Allocation diagnosis, not timing

The standalone `allocation_probe.rs.txt` uses a counting System allocator outside the unsafe-forbidden production workspace. Number construction is outside the count; each row counts one comparison. Calls include allocation/reallocation; bytes are cumulative requested sizes, not peak or live memory. Instrumented durations are never used.

| Operands | Before calls / bytes | Candidate calls / bytes |
|---|---:|---:|
| 42, 0 | 3 / 24 | 0 / 0 |
| 42, 1000000000 | 5 / 50 | 0 / 0 |
| 42.0, 0 | 3 / 24 | 3 / 24 |
| -0, 0 | 2 / 16 | 0 / 0 |
| i128::MAX + 1, 42 | 7 / 175 | 7 / 175 |

The two ordinary integer comparisons occur for each Flat.id in this benchmark. These measurements isolate avoided comparator allocations; they are not a sampled CPU profile or attribution of total parse time. `alloc-fresh-candidate.txt` confirms the candidate in its final isolated target.

## Verification

Default-feature `cargo test --locked -p rydantic --tests`, four derive unit tests, focused clippy with warnings denied, formatting and diff checks passed. The new regression suite checks 97 numeric representations / 9,409 Cartesian comparisons against a frozen byte-identical original algorithm, including full serialized validation errors for all four operators, and 10,000 deterministic full-width integer pairs against both i128 order and the original. Boundary cases cover i64/u64/i128/u128 limits, signed zero, integral decimals, exponent notation and arbitrarily large positive/negative exponents. Nonfinite numbers are not valid JSON and this patch does not touch parsing.

An independent reviewer found no source blocker and ran a separate no-default-features consumer: all three supplied tests plus exhaustive -128..128 integer pairs, equivalent decimal/exponent representations in both directions, and i128-extrema equality checks passed. Canonical benchmark verification on the final isolated candidate passed all 98 comparable checks and the four separately checked 42.0 known-difference checks (the runner prints only the 98 count). No all-feature workspace or final-head remote CI pass is claimed here.

## Timing method and limits

Baseline and candidate copied executables use Rust/Cargo 1.98.1, identical lockfile and release opt-level 3, debuginfo 0, debug assertions false, and no custom RUSTFLAGS. Cargo JSON artifacts and source/executable hashes are retained. A preflight attempt reused one Cargo target for separate source checkouts; the candidate was incorrectly reported fresh and its hash equaled the baseline. This was caught **before any timing**, rejected, and rebuilt in a new isolated candidate target. The rejected artifact is not performance evidence; its build receipts are preserved. Final baseline/candidate targets are distinct, and the fresh candidate's actual nonallocating comparator was independently rechecked.

`compare.py` uses the unchanged corpus and oracle, consumes/drops outputs, excludes compilation/startup, uses 1,000 warmup operations and 10,000 operations per batch, three batches in each of two processes per engine/case/mode. Process order is baseline/candidate/Pydantic/Pydantic/candidate/baseline. It records and checks unchanged source and executable hashes before/after. All 49 case-mode rows and all 294 results are retained. Six batch-average samples per engine are summarized by median and inclusive IQR, not individual-request latency.

Primary JSON includes parse+validation. Secondary value_copy includes each language's input-copy costs. Rejection rows do not claim equal error-formatting work. Pydantic 2.14.1/core 2.50.1 on CPython 3.12.14 uses default string caching on repeated identical payloads; this does not represent high-cardinality or cold traffic. Other owned cloud workers paused builds, but the shared host is not exclusive or CPU-pinned. No universal speed/no-regression claim or CI performance threshold is made. Do not compute speedups against older runs; only the simultaneous paired baseline below is comparable.

## Complete paired results

UTC: 2026-10-25T18:01:50.658522+00:00 to 2026-10-25T18:03:00.121662+00:00. Load start/end: [3.0185546875, 1.97412109375, 1.22802734375] / [1.70751953125, 1.79638671875, 1.21875]. Uncompressed raw SHA256: `862e73a51e4e1008fd4c7a289aa93f3adf9d7d9e4782f199831c482a3d6f686e`.

Executable SHA256: {'baseline': '05bd24031be3eb6ce1c5c8a9486a638bd38dcc2484e5727611c716b106ab667e', 'candidate': '19d2e1fdc1440b3cbdbd6813fe21e1cc79b517c48f8d36e470d0c284426d47b2'}

All values ns/op: median (inclusive IQR). Before/after >1 favors candidate.

| Case | Mode | Before | Candidate | Pydantic | Before/after |
|---|---|---:|---:|---:|---:|
| flat_valid | json | 1218.8 (43.2) | 998.0 (30.7) | 1037.5 (61.3) | 1.221 |
| flat_valid | value_copy | 895.2 (56.5) | 648.9 (29.8) | 2969.4 (145.2) | 1.380 |
| flat_unicode | json | 1160.0 (37.9) | 1050.6 (73.9) | 1075.5 (21.1) | 1.104 |
| flat_unicode | value_copy | 891.6 (10.3) | 727.6 (81.1) | 3042.6 (154.9) | 1.225 |
| nested_valid | json | 2360.8 (133.4) | 2009.4 (167.6) | 1908.7 (148.6) | 1.175 |
| nested_valid | value_copy | 1438.7 (55.4) | 1209.2 (18.7) | 5370.1 (195.0) | 1.190 |
| collection_100 | json | 127391.4 (5707.9) | 104417.7 (2053.6) | 60752.7 (2946.7) | 1.220 |
| collection_100 | value_copy | 92556.9 (4722.3) | 72674.5 (2143.7) | 225752.5 (3659.4) | 1.274 |
| flat_wrong_type | json | 1247.3 (72.3) | 1158.6 (45.7) | 1460.0 (65.7) | 1.077 |
| flat_wrong_type | value_copy | 786.9 (20.4) | 874.3 (25.6) | 3064.9 (29.1) | 0.900 |
| flat_bool_as_int | json | 1066.9 (67.8) | 1143.5 (21.6) | 1342.8 (13.6) | 0.933 |
| flat_bool_as_int | value_copy | 816.7 (147.7) | 838.2 (22.1) | 3004.6 (120.8) | 0.974 |
| flat_negative | json | 1548.2 (166.1) | 1243.9 (217.1) | 1374.9 (163.5) | 1.245 |
| flat_negative | value_copy | 1180.5 (77.2) | 863.8 (31.1) | 3015.8 (66.0) | 1.367 |
| flat_upper_bound | json | 1792.2 (272.7) | 1345.3 (61.3) | 1459.6 (108.6) | 1.332 |
| flat_upper_bound | value_copy | 1289.7 (49.9) | 944.7 (27.6) | 3145.4 (131.1) | 1.365 |
| flat_empty_name | json | 1597.7 (71.1) | 1312.5 (172.6) | 1339.0 (71.6) | 1.217 |
| flat_empty_name | value_copy | 1099.1 (53.6) | 975.9 (61.8) | 3027.9 (122.2) | 1.126 |
| flat_long_name | json | 1661.5 (101.1) | 1390.1 (210.9) | 1444.1 (80.4) | 1.195 |
| flat_long_name | value_copy | 1184.4 (204.0) | 928.3 (30.7) | 3543.2 (216.4) | 1.276 |
| flat_missing | json | 1086.6 (35.2) | 936.6 (128.0) | 1454.5 (48.7) | 1.160 |
| flat_missing | value_copy | 853.0 (19.0) | 685.4 (43.0) | 2866.5 (164.6) | 1.244 |
| flat_null | json | 1587.5 (377.7) | 1309.4 (202.2) | 1488.1 (177.3) | 1.212 |
| flat_null | value_copy | 1158.1 (91.3) | 855.7 (93.6) | 3260.0 (258.6) | 1.353 |
| flat_extra | json | 1454.6 (192.1) | 1295.8 (48.3) | 1466.7 (181.9) | 1.123 |
| flat_extra | value_copy | 1134.6 (80.2) | 947.0 (72.9) | 3600.9 (343.4) | 1.198 |
| flat_multiple | json | 2018.6 (85.2) | 1761.7 (201.3) | 1703.9 (117.1) | 1.146 |
| flat_multiple | value_copy | 1584.8 (163.9) | 1333.0 (172.1) | 3246.3 (92.8) | 1.189 |
| nested_invalid | json | 2932.5 (67.6) | 2585.8 (120.2) | 2095.6 (147.8) | 1.134 |
| nested_invalid | value_copy | 2041.2 (115.9) | 1640.7 (49.2) | 5287.9 (162.9) | 1.244 |
| collection_invalid | json | 3313.1 (107.6) | 2933.9 (82.6) | 2569.8 (254.3) | 1.129 |
| collection_invalid | value_copy | 2424.2 (468.8) | 1849.0 (69.4) | 6605.4 (110.3) | 1.311 |
| malformed_json | json | 295.5 (27.2) | 291.0 (23.5) | 975.5 (63.4) | 1.016 |
| flat_min | json | 1258.6 (48.6) | 1007.9 (22.6) | 1038.1 (85.9) | 1.249 |
| flat_min | value_copy | 792.0 (14.6) | 671.8 (54.9) | 2829.4 (50.3) | 1.179 |
| flat_max | json | 1458.2 (187.9) | 1109.7 (46.6) | 1112.5 (118.0) | 1.314 |
| flat_max | value_copy | 1013.4 (63.6) | 706.4 (57.4) | 2929.2 (105.7) | 1.435 |
| flat_unicode64 | json | 1381.6 (129.1) | 1207.0 (27.3) | 2059.3 (30.9) | 1.145 |
| flat_unicode64 | value_copy | 822.5 (72.9) | 787.9 (33.1) | 3001.4 (158.2) | 1.044 |
| flat_unicode65 | json | 1807.0 (163.8) | 1720.9 (609.7) | 2778.1 (303.2) | 1.050 |
| flat_unicode65 | value_copy | 1401.6 (317.3) | 927.2 (18.3) | 3302.2 (89.9) | 1.512 |
| flat_huge_int | json | 1251.5 (50.5) | 1351.5 (93.6) | 1548.1 (75.2) | 0.926 |
| flat_huge_int | value_copy | 833.0 (34.5) | 814.2 (50.4) | 3481.0 (215.4) | 1.023 |
| flat_root_array | json | 287.9 (38.0) | 307.7 (34.1) | 897.8 (42.7) | 0.936 |
| flat_root_array | value_copy | 155.0 (6.9) | 178.4 (43.6) | 1625.1 (129.1) | 0.869 |
| nested_extra | json | 2266.7 (259.2) | 2072.5 (335.7) | 1824.4 (63.8) | 1.094 |
| nested_extra | value_copy | 1543.5 (199.5) | 1329.3 (24.0) | 5024.4 (142.8) | 1.161 |
| flat_nonintegral_float | json | 1207.4 (59.7) | 1085.5 (18.7) | 1425.5 (78.5) | 1.112 |
| flat_nonintegral_float | value_copy | 743.7 (35.6) | 723.5 (38.6) | 3057.7 (138.7) | 1.028 |

Primary successful-input medians fell about 18.1%, 14.9%, and 18.0% for flat, nested, and collection100 respectively relative to the static-layout baseline in this run. Flat candidate was slightly below same-run Pydantic; nested and collection100 remained slower (about 1.05× and 1.72× Pydantic). These are fixture-specific observations, not a general superiority claim. All rejection/control rows, including any slower medians, are shown above; no causal claim is made for noise or paths that do not reach integer comparison.

Root-array rejection bypasses this comparator and did not improve: JSON median 287.9→307.7 ns/op and value_copy 155.0→178.4 ns/op. These results are retained rather than hidden; this experiment does not establish their cause.

Independent evidence audit recomputed all 49 median/IQR/ratio rows, verified all 294 outputs/checksums against the oracle, all 205 source hashes against the tested local commit, both actual artifact paths/profiles and executable hashes, and remote source blob identity. The rejected stale preflight artifact was confirmed excluded. Audit scripts/logs are retained alongside the evidence.
