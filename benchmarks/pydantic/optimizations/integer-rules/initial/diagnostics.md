# Allocation and fallback diagnostics

Refs #125.

These standalone probes are separate from the uninstrumented canonical end-to-end workers. Allocation counts use a System global allocator and count allocation/reallocation calls and cumulative requested bytes, not RSS, peak memory or time. Rule construction and Context initialization are outside those counts. Full errors are compared before rendering.

Values are i64 = 42, i128 = i128::MIN and u128 = u128::MAX. Case names describe bound representations, not an assertion that every value passes. Generic and typed routes below both use the candidate library; this isolates the direct-integer route, not a separately compiled old-library timing comparison.

## All isolated rule allocation controls

| Type | Bounds | Generic calls / bytes | Typed calls / bytes | Accepted |
|---|---|---:|---:|---|
| i64 | no_bounds | 1 / 2 | 0 / 0 | True |
| i128 | no_bounds | 1 / 40 | 0 / 0 | True |
| u128 | no_bounds | 1 / 39 | 1 / 39 | True |
| i64 | lexical_range | 1 / 2 | 0 / 0 | True |
| i128 | lexical_range | 7 / 1142 | 6 / 1102 | False |
| u128 | lexical_range | 21 / 1507 | 21 / 1507 | False |
| i64 | lexical_strict | 12 / 1777 | 11 / 1775 | False |
| i128 | lexical_strict | 7 / 1137 | 6 / 1097 | False |
| u128 | lexical_strict | 21 / 1483 | 21 / 1483 | False |
| i64 | decimal_equality | 9 / 66 | 9 / 66 | True |
| i128 | decimal_equality | 21 / 1497 | 21 / 1497 | False |
| u128 | decimal_equality | 21 / 1493 | 21 / 1493 | False |
| i64 | mixed_bounds | 30 / 3218 | 30 / 3218 | False |
| i128 | mixed_bounds | 26 / 2178 | 26 / 2178 | False |
| u128 | mixed_bounds | 40 / 2521 | 40 / 2521 | False |
| i64 | huge_bounds | 9 / 105 | 9 / 105 | True |
| i128 | huge_bounds | 15 / 431 | 15 / 431 | True |
| u128 | huge_bounds | 15 / 428 | 15 / 428 | True |

## All parse allocation controls

One warmup parse prepares static model rules before the measured operation; immutable input construction is excluded. Counts end when the validated parse result has been constructed, before its disposal; deallocation is not counted. These counts therefore must not be directly compared to earlier cold-first-parse allocation reports. Every one of the 98 outputs is checked against the unchanged independent fixture oracle.

| Case | Mode | Baseline calls / bytes | Candidate calls / bytes |
|---|---|---:|---:|
| flat_valid | json | 14 / 862 | 13 / 860 |
| flat_valid | value_copy | 15 / 1702 | 14 / 1700 |
| flat_unicode | json | 14 / 876 | 13 / 874 |
| flat_unicode | value_copy | 15 / 1716 | 14 / 1714 |
| nested_valid | json | 26 / 1770 | 25 / 1768 |
| nested_valid | value_copy | 27 / 3394 | 26 / 3392 |
| collection_100 | json | 1215 / 81430 | 1115 / 81240 |
| collection_100 | value_copy | 1309 / 162238 | 1209 / 162048 |
| flat_wrong_type | json | 22 / 1995 | 22 / 1995 |
| flat_wrong_type | value_copy | 24 / 2851 | 24 / 2851 |
| flat_bool_as_int | json | 21 / 1994 | 21 / 1994 |
| flat_bool_as_int | value_copy | 23 / 2850 | 23 / 2850 |
| flat_negative | json | 22 / 1990 | 21 / 1988 |
| flat_negative | value_copy | 23 / 2830 | 22 / 2828 |
| flat_upper_bound | json | 22 / 2012 | 21 / 2002 |
| flat_upper_bound | value_copy | 23 / 2852 | 22 / 2842 |
| flat_empty_name | json | 22 / 1986 | 21 / 1984 |
| flat_empty_name | value_copy | 23 / 2826 | 22 / 2824 |
| flat_long_name | json | 24 / 2117 | 23 / 2115 |
| flat_long_name | value_copy | 25 / 2957 | 24 / 2955 |
| flat_missing | json | 16 / 1320 | 15 / 1318 |
| flat_missing | value_copy | 17 / 2160 | 16 / 2158 |
| flat_null | json | 22 / 2006 | 21 / 2004 |
| flat_null | value_copy | 23 / 2846 | 22 / 2844 |
| flat_extra | json | 20 / 1344 | 19 / 1342 |
| flat_extra | value_copy | 22 / 2185 | 21 / 2183 |
| flat_multiple | json | 39 / 3440 | 38 / 3438 |
| flat_multiple | value_copy | 40 / 4280 | 39 / 4278 |
| nested_invalid | json | 48 / 3673 | 47 / 3671 |
| nested_invalid | value_copy | 47 / 5265 | 46 / 5263 |
| collection_invalid | json | 43 / 3587 | 41 / 3583 |
| collection_invalid | value_copy | 44 / 6043 | 42 / 6039 |
| malformed_json | json | 10 / 875 | 10 / 875 |
| flat_min | json | 14 / 860 | 13 / 859 |
| flat_min | value_copy | 15 / 1700 | 14 / 1699 |
| flat_max | json | 14 / 878 | 13 / 868 |
| flat_max | value_copy | 15 / 1718 | 14 / 1708 |
| flat_unicode64 | json | 14 / 1368 | 13 / 1366 |
| flat_unicode64 | value_copy | 15 / 2208 | 14 / 2206 |
| flat_unicode65 | json | 24 / 2507 | 23 / 2505 |
| flat_unicode65 | value_copy | 25 / 3347 | 24 / 3345 |
| flat_huge_int | json | 19 / 1412 | 19 / 1412 |
| flat_huge_int | value_copy | 19 / 2220 | 19 / 2220 |
| flat_root_array | json | 8 / 696 | 8 / 696 |
| flat_root_array | value_copy | 4 / 528 | 4 / 528 |
| nested_extra | json | 27 / 2024 | 26 / 2022 |
| nested_extra | value_copy | 30 / 3713 | 29 / 3711 |
| flat_nonintegral_float | json | 20 / 1385 | 20 / 1385 |
| flat_nonintegral_float | value_copy | 21 / 2213 | 21 / 2213 |

## All uninstrumented rule timing controls

Two reversed counterbalanced rounds, 1,000 warmups and three 10,000-operation batches per position give 12 samples per cell. Timing includes a fresh Context and result disposal. Median (inclusive IQR) batch-average ns/op is shown; no baseline subtraction. Fallback and upper-u128 rows are retained even when slower. This same-binary route diagnostic does not replace the full 49-case paired benchmark.

| Type | Bounds | Generic ns/op (IQR) | Typed ns/op (IQR) | Typed change |
|---|---|---:|---:|---:|
| i64 | no_bounds | 35.0 (5.6) | 17.3 (3.4) | -50.62% |
| i128 | no_bounds | 55.4 (14.1) | 16.2 (2.1) | -70.78% |
| u128 | no_bounds | 51.8 (6.1) | 57.4 (7.6) | +10.80% |
| i64 | lexical_range | 64.5 (10.0) | 29.3 (7.2) | -54.55% |
| i128 | lexical_range | 440.8 (48.4) | 183.8 (21.6) | -58.30% |
| u128 | lexical_range | 1076.7 (48.2) | 1141.9 (109.3) | +6.05% |
| i64 | lexical_strict | 373.0 (44.8) | 333.5 (32.1) | -10.61% |
| i128 | lexical_strict | 416.1 (35.7) | 171.8 (31.6) | -58.71% |
| u128 | lexical_strict | 1053.3 (110.8) | 1077.5 (108.0) | +2.29% |
| i64 | decimal_equality | 287.6 (46.4) | 338.6 (85.6) | +17.73% |
| i128 | decimal_equality | 1049.9 (113.6) | 1038.9 (66.7) | -1.05% |
| u128 | decimal_equality | 1068.3 (68.3) | 1043.7 (101.5) | -2.30% |
| i64 | mixed_bounds | 1100.3 (249.7) | 998.1 (62.9) | -9.29% |
| i128 | mixed_bounds | 1413.6 (138.0) | 1342.6 (190.9) | -5.02% |
| u128 | mixed_bounds | 1981.7 (140.9) | 1976.9 (158.9) | -0.24% |
| i64 | huge_bounds | 283.7 (68.3) | 285.3 (36.9) | +0.57% |
| i128 | huge_bounds | 851.2 (92.2) | 835.8 (107.7) | -1.81% |
| u128 | huge_bounds | 786.2 (45.3) | 843.8 (181.5) | +7.33% |
