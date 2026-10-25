# Scalar integer decode and final three-fix experiment

Refs #121; draft #122. This experiment directly compares **original runtime, the two previous fixes, all three fixes, and Pydantic in one session**. It provides both the incremental scalar-decode effect and direct original-to-three-fix change without multiplying percentages from older sessions. Every earlier experiment remains immutable.

## Bounded change and semantics

The scalar integer decoder formerly always normalized Number through an owned decimal String and digits String. It now first tries exact borrowed `as_i128`, then `as_u128`, and converts a successful integer through `TryFrom` into the target primitive type. Decimal/exponent forms and integers outside both accessors retain the unchanged `integer_text` fallback. Out-of-range successful-accessor conversions still produce the identical error. No float cast, parser rewrite, coercion-policy change or unsafe production code is introduced.

The implementation covers all 12 primitive signed/unsigned integer types. Regression tests compare the frozen old decoder on boundaries, deterministic generated full-width inputs, owned and borrowed values, coercion modes, and full serialized errors. Controls include signed zero, integral/nonintegral decimals, large exponents/zero, i128/u128 extrema, numeric strings and non-numbers. Canonical benchmark checks include 98 comparable checks and four separate integral-42.0 known-difference checks; the latter are not timed as comparable work.

## Source and artifacts

- Original: `862907a2aef111a6e29d9a7df5338c0e33819c2a`, runtime unchanged from main `4d61843`; dedicated `/workspace/shared/rydantic-combined-original-target`.
- Two fixes: published runtime/test source `78ae5228f0415edb55c30087c1cfdd351bb9581d`, local `b255a8b24dd9b61b4cbf49df252874659d836168`; static-layout collision elision plus exact integer comparison; dedicated `/workspace/shared/rydantic-integer-candidate-target`.
- Three fixes: published source `2e65b2ac2eb204fdfd6d3cac30a42ee3f4d89a1e`, local `ca682a51f7686303828089a48d607725a4d19fd2`; adds only scalar integer decoding and its tests; dedicated `/workspace/shared/rydantic-scalar-target`.

The scalar commit inherits final two-fix evidence head `9e9725c459ee90d6ba9254532b23549d56de74b7`; prior evidence is preserved. Independent review verified exact published scalar runtime/test blobs against tested local files. All three copied Rust executables have separately verified source identity and permanent distinct build targets; Rust/Cargo 1.98.1, identical Cargo.lock, release opt-level 3, no debug assertions, debuginfo 0, and no custom RUSTFLAGS. Build artifacts/logs are retained. No instrumented executable is timed.

## Separate allocation diagnosis

Standalone System allocator probes, outside unsafe-forbidden production code, count allocation/reallocation calls and cumulative requested bytes (not peak/live memory). Scalar inputs are constructed outside the measurement. Instrumented durations are never timing evidence.

| Scalar input | Before calls / bytes | After calls / bytes |
|---|---:|---:|
| i64 or u128: 42 | 2 / 16 | 0 / 0 |
| i64 or u128: -0 | 2 / 9 | 0 / 0 |
| u128::MAX | 5 / 159 | 0 / 0 |

Integral-decimal/exponent and huge-exponent-zero controls retain their allocation counts and outputs; complete controls are in `alloc-before.txt` / `alloc-after.txt`.

Independent full `parse_json` probes on the same corpus measured:

| JSON fixture | Two-fix calls | Three-fix calls | Two-fix requested bytes | Three-fix requested bytes |
|---|---:|---:|---:|---:|
| flat_valid | 22 | 20 | 1,545 | 1,529 |
| nested_valid | 36 | 34 | 3,021 | 3,005 |
| collection_100 | 1,621 | 1,421 | 145,978 | 144,385 |

These are observed end-to-end allocation deltas, not extrapolations from the helper probe or a sampled CPU profile. Full outputs remain in `parse-allocations.json`.

## Verification and timing method

Default-feature core --tests, four derive tests, focused clippy with warnings denied, formatting and canonical parity passed. Independent source review and four separate no-default-features consumer tests passed with no blocker. Exact published scalar blob identity was checked. This does not claim a full all-feature workspace or final evidence-head remote CI pass.

Unchanged 49 fixture/mode combinations, expected-output/error category/path oracle, 1,000 warmup operations, 10,000 operations per timed batch, three batches in each of two processes per engine/case/mode. Fixed symmetric order: original / two fixes / three fixes / Pydantic / Pydantic / three fixes / two fixes / original. All 392 process results are retained and verified. Results are consumed and disposed; compilation and process startup excluded. Six batch-average samples per engine summarized by median and inclusive IQR, not per-request latency.

`compare.py` records and checks all three source snapshots, all Rust executable hashes and the Python interpreter hash before/after timing. Worker/fixture/lockfile equality is independently checked. Pydantic 2.14.1/core 2.50.1 uses CPython 3.12.14 and default string caching on repeated identical payloads. This is not cold-cache or high-cardinality traffic; Pydantic's core itself is Rust. Primary JSON includes parsing/validation; secondary value_copy includes language-specific copying costs. Rejection workloads do not materialize identical diagnostic detail. Other owned workers paused CPU-heavy work, but the shared host was neither exclusive nor CPU-pinned. Fixed order, scheduling and host noise remain limitations. No universal speed ranking, no-regression guarantee or CI speed threshold follows.

## Complete paired results

UTC: 2026-10-25T18:22:38.890462+00:00 to 2026-10-25T18:24:15.169993+00:00. Load start/end: [0.60107421875, 1.60107421875, 1.33544921875] / [0.84033203125, 1.41650390625, 1.29296875]. Uncompressed raw SHA256: `411f4bbda492f117b624124999556428896a7c80d90e915765d70223609081a7`.

Rust executable SHA256: {'baseline': 'b66bcd1a737ba8f80d8fe31cad1b3683c87acc59229648de20d0416f05ed567d', 'previous': '19d2e1fdc1440b3cbdbd6813fe21e1cc79b517c48f8d36e470d0c284426d47b2', 'candidate': '2cbf459eae69a357a26ba94b0db9e40ebabda25cf67cf42b5bf98d797248dddc'}

Python executable SHA256: `fa67443527ed9647f760d807e2a38f26340757123e643c4639cf273ed15d5ea7`.

All values ns/op: median (inclusive IQR). Ratios >1 favor three-fix candidate.

| Case | Mode | Original | Two fixes | Three fixes | Pydantic | Two/three | Original/three |
|---|---|---:|---:|---:|---:|---:|---:|
| flat_valid | json | 1930.6 (73.6) | 1030.7 (37.4) | 942.9 (47.2) | 1042.8 (128.6) | 1.093 | 2.047 |
| flat_valid | value_copy | 1751.7 (399.7) | 766.3 (42.5) | 718.6 (98.9) | 3119.4 (253.5) | 1.066 | 2.438 |
| flat_unicode | json | 2140.7 (199.9) | 1132.6 (57.3) | 1029.4 (154.5) | 1390.2 (177.2) | 1.100 | 2.080 |
| flat_unicode | value_copy | 1654.4 (246.0) | 720.8 (45.3) | 679.0 (41.0) | 3190.4 (375.1) | 1.061 | 2.436 |
| nested_valid | json | 3779.9 (280.2) | 2105.4 (178.2) | 2056.0 (137.8) | 1732.9 (150.9) | 1.024 | 1.838 |
| nested_valid | value_copy | 2952.0 (136.7) | 1315.8 (206.4) | 1181.4 (104.1) | 5858.4 (151.7) | 1.114 | 2.499 |
| collection_100 | json | 205068.4 (5876.1) | 107429.0 (4332.9) | 105392.9 (6660.5) | 66641.4 (3435.7) | 1.019 | 1.946 |
| collection_100 | value_copy | 171537.3 (15454.0) | 73151.4 (4161.8) | 68548.5 (4186.7) | 224616.1 (10863.3) | 1.067 | 2.502 |
| flat_wrong_type | json | 1560.7 (94.3) | 1157.7 (15.5) | 1152.0 (150.9) | 1454.1 (168.7) | 1.005 | 1.355 |
| flat_wrong_type | value_copy | 1207.5 (99.4) | 880.1 (93.3) | 834.1 (32.8) | 3075.0 (91.6) | 1.055 | 1.448 |
| flat_bool_as_int | json | 1494.1 (136.8) | 1102.8 (152.5) | 1151.1 (216.6) | 1399.6 (264.3) | 0.958 | 1.298 |
| flat_bool_as_int | value_copy | 1224.1 (69.4) | 864.6 (153.8) | 856.1 (64.7) | 3112.2 (183.2) | 1.010 | 1.430 |
| flat_negative | json | 2155.8 (159.4) | 1299.9 (16.3) | 1134.5 (30.9) | 1414.2 (88.8) | 1.146 | 1.900 |
| flat_negative | value_copy | 1766.2 (68.8) | 856.0 (40.5) | 920.7 (123.0) | 3120.0 (163.8) | 0.930 | 1.918 |
| flat_upper_bound | json | 2220.5 (108.7) | 1363.4 (63.2) | 1208.7 (44.8) | 1365.5 (105.0) | 1.128 | 1.837 |
| flat_upper_bound | value_copy | 1941.0 (135.4) | 977.2 (72.4) | 876.9 (22.9) | 3052.3 (143.3) | 1.114 | 2.214 |
| flat_empty_name | json | 2463.0 (295.1) | 1401.0 (71.5) | 1191.8 (120.7) | 1355.2 (18.6) | 1.175 | 2.067 |
| flat_empty_name | value_copy | 1805.0 (41.0) | 923.5 (127.9) | 849.0 (12.8) | 3329.5 (377.0) | 1.088 | 2.126 |
| flat_long_name | json | 2286.1 (168.7) | 1393.2 (36.3) | 1259.1 (260.9) | 1404.6 (56.0) | 1.107 | 1.816 |
| flat_long_name | value_copy | 1881.1 (168.0) | 899.8 (26.7) | 872.6 (41.8) | 3167.1 (54.0) | 1.031 | 2.156 |
| flat_missing | json | 1361.4 (20.9) | 874.6 (39.9) | 769.4 (30.6) | 1546.4 (50.4) | 1.137 | 1.769 |
| flat_missing | value_copy | 1180.4 (103.1) | 618.9 (65.8) | 685.8 (65.8) | 2661.4 (171.0) | 0.902 | 1.721 |
| flat_null | json | 1835.1 (185.2) | 1176.2 (72.3) | 1200.4 (126.6) | 1445.4 (36.0) | 0.980 | 1.529 |
| flat_null | value_copy | 1505.0 (182.9) | 919.4 (32.1) | 869.2 (136.4) | 3275.9 (89.8) | 1.058 | 1.731 |
| flat_extra | json | 2168.7 (95.8) | 1311.7 (31.6) | 1244.4 (41.1) | 1486.1 (56.3) | 1.054 | 1.743 |
| flat_extra | value_copy | 1776.6 (104.4) | 817.5 (16.9) | 751.1 (12.4) | 3627.7 (220.3) | 1.088 | 2.365 |
| flat_multiple | json | 2364.4 (105.1) | 1976.6 (83.6) | 1811.4 (139.1) | 1663.3 (91.9) | 1.091 | 1.305 |
| flat_multiple | value_copy | 2019.4 (138.6) | 1366.6 (66.7) | 1364.9 (78.4) | 3427.6 (139.4) | 1.001 | 1.480 |
| nested_invalid | json | 3407.9 (221.2) | 2645.1 (25.4) | 2555.0 (107.1) | 2134.1 (163.8) | 1.035 | 1.334 |
| nested_invalid | value_copy | 2793.7 (141.8) | 1788.0 (167.7) | 1817.8 (135.8) | 5474.0 (458.2) | 0.984 | 1.537 |
| collection_invalid | json | 5378.5 (585.5) | 3318.5 (227.5) | 2919.5 (108.2) | 2694.4 (544.1) | 1.137 | 1.842 |
| collection_invalid | value_copy | 4360.3 (489.4) | 1985.1 (170.5) | 1751.9 (175.5) | 7006.6 (413.3) | 1.133 | 2.489 |
| malformed_json | json | 297.3 (31.3) | 292.5 (33.1) | 309.9 (82.8) | 963.8 (69.0) | 0.944 | 0.959 |
| flat_min | json | 2046.4 (116.3) | 1043.8 (87.2) | 1128.4 (128.9) | 1066.3 (24.4) | 0.925 | 1.814 |
| flat_min | value_copy | 1724.2 (69.5) | 723.4 (23.1) | 675.2 (58.6) | 3054.7 (188.0) | 1.071 | 2.554 |
| flat_max | json | 2114.1 (69.1) | 1279.1 (301.9) | 1105.9 (25.6) | 1095.6 (132.9) | 1.157 | 1.912 |
| flat_max | value_copy | 1675.6 (129.0) | 715.3 (40.0) | 693.4 (77.6) | 3025.3 (145.7) | 1.032 | 2.416 |
| flat_unicode64 | json | 2211.6 (30.8) | 1194.2 (108.7) | 1138.6 (94.3) | 2322.5 (280.4) | 1.049 | 1.942 |
| flat_unicode64 | value_copy | 1829.2 (775.9) | 655.5 (38.3) | 873.5 (439.4) | 2892.9 (94.9) | 0.750 | 2.094 |
| flat_unicode65 | json | 2331.0 (75.3) | 1445.1 (22.2) | 1404.3 (161.3) | 2546.4 (282.7) | 1.029 | 1.660 |
| flat_unicode65 | value_copy | 1858.8 (29.2) | 1087.0 (106.5) | 905.2 (42.2) | 3062.5 (111.3) | 1.201 | 2.053 |
| flat_huge_int | json | 1565.6 (24.1) | 1245.5 (62.4) | 1103.1 (38.8) | 1526.1 (19.1) | 1.129 | 1.419 |
| flat_huge_int | value_copy | 1125.8 (81.1) | 796.6 (10.4) | 691.6 (37.3) | 3284.2 (75.8) | 1.152 | 1.628 |
| flat_root_array | json | 281.6 (11.1) | 283.2 (5.8) | 272.1 (24.3) | 858.4 (41.4) | 1.041 | 1.035 |
| flat_root_array | value_copy | 146.6 (8.6) | 147.8 (14.6) | 143.6 (21.4) | 1523.4 (128.4) | 1.029 | 1.020 |
| nested_extra | json | 3178.9 (52.7) | 1976.0 (151.7) | 1859.0 (195.0) | 1814.6 (306.4) | 1.063 | 1.710 |
| nested_extra | value_copy | 2598.0 (142.8) | 1270.2 (100.9) | 1280.0 (279.5) | 5196.7 (941.1) | 0.992 | 2.030 |
| flat_nonintegral_float | json | 1530.5 (216.4) | 1092.1 (19.4) | 1097.8 (21.1) | 1442.8 (145.6) | 0.995 | 1.394 |
| flat_nonintegral_float | value_copy | 1245.1 (88.4) | 866.4 (117.6) | 786.8 (178.5) | 3104.6 (237.9) | 1.101 | 1.583 |

## Interpretation

- flat_valid: scalar incremental reduction 8.5%; direct original-to-three-fix reduction 51.2%; candidate/Pydantic median ratio 0.904.
- nested_valid: scalar incremental reduction 2.3%; direct original-to-three-fix reduction 45.6%; candidate/Pydantic median ratio 1.186.
- collection_100: scalar incremental reduction 1.9%; direct original-to-three-fix reduction 48.6%; candidate/Pydantic median ratio 1.581.

These are same-session direct ratios, not products of percentages from older runs. All earlier experiments remain immutable; shared-host and repeated-payload caveats prevent a universal ranking or no-regression claim. Rows with worse medians are retained above.

The nested and collection incremental median differences (49.4 ns and 2,036.1 ns) are smaller than their within-run IQRs (two-fix/three-fix: 178.2/137.8 ns and 4,332.9/6,660.5 ns). These data alone do **not** establish repeatable latency improvements for those workloads; the allocation reductions are directly verified. Flat candidate/Pydantic ranking also varied across earlier sessions, so this run is not a general victory claim.

Ten case/mode medians worsened relative to the two-fix baseline (all IQRs remain in the full table):
- flat_bool_as_int / json: 1,102.8→1,151.1 ns.
- flat_negative / value_copy: 856.0→920.7 ns.
- flat_missing / value_copy: 618.9→685.8 ns.
- flat_null / json: 1,176.2→1,200.4 ns.
- nested_invalid / value_copy: 1,788.0→1,817.8 ns.
- malformed_json / json: 292.5→309.9 ns (also above original 297.3 ns).
- flat_min / json: 1,043.8→1,128.4 ns.
- flat_unicode64 / value_copy: 655.5→873.5 ns, candidate IQR 439.4 ns.
- nested_extra / value_copy: 1,270.2→1,280.0 ns.
- flat_nonintegral_float / json: 1,092.1→1,097.8 ns.

The experiment does not establish whether these differences are repeatable regressions or variation; they are neither removed nor attributed to a cause without evidence.

### Earlier direct run remains part of the record

The [previous original-to-two-fix run](../combined/README.md), preserved unchanged, measured reductions of **39.7% / 32.5% / 47.4%** for flat/nested/collection100. Its original→two-fix/Pydantic medians were 2,182.4→1,315.5/1,173.9 ns; 3,610.3→2,437.1/1,900.2 ns; and 208,013.3→109,471.5/62,467.8 ns. All three two-fix medians were slower than Pydantic in that session. This new session remeasures the original and two-fix artifacts too, rather than replacing that record with the more favorable new flat result. Between-session shared-host variation remains visible even for identical binaries. The ~2% scalar nested/collection deltas are not treated as proven repeatable wins; no further optimization code is included in this round.

Independent evidence audit recomputed all 49 rows and both ratios, verified all 392 oracle outputs/checksums, three committed source snapshots, three optimized Rust artifacts and all four executable hashes including Python, identical fixtures/lockfiles/workers, gzip/raw fidelity, and allocation probes/table claims. It confirmed all ten slower incremental rows and the small-gain/IQR caveat. Audit script/log are retained alongside this report.
