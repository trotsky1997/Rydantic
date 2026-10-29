# Integer rule validation: paired measurements

Refs #125.

## Protocol and provenance

UTC: 2026-10-26T04:15:31.888807+00:00 to 2026-10-26T04:19:09.440508+00:00. 10,000 operations/batch, 1,000 warmups, 3 batches/position, 2 rounds, 12 samples/engine/case. All 49 case/mode pairs are retained.

Orders by round: `[['baseline', 'control', 'candidate', 'pydantic', 'pydantic', 'candidate', 'control', 'baseline'], ['pydantic', 'candidate', 'control', 'baseline', 'baseline', 'control', 'candidate', 'pydantic']]`. Case order is independently shuffled with seeds `20261026 + round`. Processes run sequentially; warmup, validation gates and process startup are outside reported timings. Loop controls are retained, never subtracted.

Baseline is current main before this fix. Control is a fresh build of exactly the same source, not another optimization. Candidate is this integer-rule change. Identical baseline/control executable hashes are permitted and explicitly reported. Their observed differences indicate run-to-run noise, not a code difference.

Fresh build receipts verify opt-level 3, no debug info/assertions or overflow checks. Source snapshots and executable hashes are checked before and after measurement. Full source-file hashes and receipt hashes are in the raw evidence.

| Engine | Measured local source commit | Binary SHA256 |
|---|---|---|
| baseline | `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` | `bd16da874b4350af0f8548daadb396d0076c1cf682017f2821fbba4d636705ee` |
| control | `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` | `bd16da874b4350af0f8548daadb396d0076c1cf682017f2821fbba4d636705ee` |
| candidate | `13f1e11dcff9490294a935adc89cf8776605107f` | `f02238c35a39bf253647f26bb60103ba3a11637dcf6e7bd00a68a2fd1813a51b` |

Baseline/control binary equality: **True**.

Rust compiler: `rustc 1.98.1 (48a229cea 2026-09-01)`. Python: `Python 3.12.14`. Pydantic/core pins: 2.14.1/2.50.1. Python executable SHA256: `fa67443527ed9647f760d807e2a38f26340757123e643c4639cf273ed15d5ea7`.

CPU: INTEL(R) XEON(R) PLATINUM 8573C; 9 logical CPUs. Affinity: `[0, 1, 2, 3, 4, 5, 6, 7, 8]`. Shared-host load start/end: `[2.19287109375, 4.1357421875, 3.30322265625]` / `[0.92724609375, 2.46484375, 2.7978515625]`. These metadata do not establish an exclusive host or fixed CPU frequency.

## Complete results

All timings are median (inclusive IQR) **batch-average ns/op**, not individual-request latency percentiles. Reduction = 100 × (1 − candidate/baseline); positive is faster. Baseline/control compares identical source; candidate/Pydantic > 1 means candidate is slower. Ratios of medians are descriptive, not statistical significance tests.

| Case | Mode | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction | Baseline/control | Candidate/Pydantic |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| flat_valid | json | 957.6 (343.8) | 964.3 (298.9) | 833.7 (529.9) | 1162.8 (431.9) | +12.94% | 0.993 | 0.717 |
| flat_valid | value_copy | 582.3 (71.4) | 617.2 (180.6) | 652.4 (159.3) | 3773.8 (798.2) | -12.04% | 0.943 | 0.173 |
| flat_unicode | json | 887.8 (170.2) | 933.5 (152.1) | 814.8 (55.3) | 1242.4 (443.7) | +8.22% | 0.951 | 0.656 |
| flat_unicode | value_copy | 576.4 (50.5) | 615.8 (55.7) | 552.3 (94.0) | 3582.0 (507.6) | +4.19% | 0.936 | 0.154 |
| nested_valid | json | 2054.6 (1858.0) | 2137.9 (913.5) | 1929.6 (494.7) | 2162.8 (956.3) | +6.09% | 0.961 | 0.892 |
| nested_valid | value_copy | 1183.8 (139.6) | 1162.4 (134.9) | 1102.5 (67.3) | 6881.6 (271.7) | +6.87% | 1.018 | 0.160 |
| collection_100 | json | 95999.2 (4480.4) | 96005.2 (3892.4) | 90700.7 (6317.3) | 69135.2 (2130.4) | +5.52% | 1.000 | 1.312 |
| collection_100 | value_copy | 68089.5 (16608.6) | 68769.7 (6804.7) | 65795.5 (3429.6) | 261618.6 (54215.6) | +3.37% | 0.990 | 0.251 |
| flat_wrong_type | json | 1071.7 (102.7) | 1070.4 (152.4) | 988.0 (46.3) | 1572.6 (89.1) | +7.81% | 1.001 | 0.628 |
| flat_wrong_type | value_copy | 795.6 (57.6) | 860.9 (77.8) | 889.5 (103.7) | 3855.4 (550.5) | -11.81% | 0.924 | 0.231 |
| flat_bool_as_int | json | 1273.5 (469.6) | 1898.0 (1212.6) | 1001.3 (381.2) | 1521.5 (265.3) | +21.37% | 0.671 | 0.658 |
| flat_bool_as_int | value_copy | 1206.6 (927.5) | 957.6 (269.4) | 1038.7 (385.2) | 5705.9 (2264.1) | +13.91% | 1.260 | 0.182 |
| flat_negative | json | 1164.3 (181.1) | 1162.0 (290.9) | 1061.7 (46.3) | 1718.8 (440.3) | +8.82% | 1.002 | 0.618 |
| flat_negative | value_copy | 1055.8 (681.7) | 886.4 (161.1) | 843.1 (102.2) | 4169.2 (411.1) | +20.14% | 1.191 | 0.202 |
| flat_upper_bound | json | 1193.7 (77.0) | 1163.4 (55.2) | 1105.0 (115.1) | 1607.3 (272.2) | +7.43% | 1.026 | 0.687 |
| flat_upper_bound | value_copy | 1000.5 (108.8) | 957.7 (114.1) | 850.5 (52.7) | 3933.5 (276.1) | +14.99% | 1.045 | 0.216 |
| flat_empty_name | json | 1233.1 (481.9) | 1197.6 (122.3) | 1190.4 (372.5) | 1915.6 (864.9) | +3.46% | 1.030 | 0.621 |
| flat_empty_name | value_copy | 942.5 (346.7) | 1011.4 (760.4) | 846.4 (125.5) | 4581.7 (535.7) | +10.20% | 0.932 | 0.185 |
| flat_long_name | json | 1368.0 (192.6) | 1315.4 (308.5) | 1230.0 (390.2) | 1774.5 (909.3) | +10.08% | 1.040 | 0.693 |
| flat_long_name | value_copy | 959.0 (150.8) | 996.4 (1577.1) | 985.7 (5629.1) | 4212.5 (1715.8) | -2.79% | 0.962 | 0.234 |
| flat_missing | json | 859.2 (453.2) | 817.1 (265.9) | 1060.5 (802.0) | 1743.7 (394.8) | -23.43% | 1.051 | 0.608 |
| flat_missing | value_copy | 636.1 (62.0) | 681.4 (154.0) | 577.7 (124.8) | 3305.1 (541.1) | +9.19% | 0.934 | 0.175 |
| flat_null | json | 1067.4 (83.6) | 1160.1 (177.4) | 1044.8 (101.2) | 1679.1 (235.4) | +2.11% | 0.920 | 0.622 |
| flat_null | value_copy | 815.0 (53.3) | 850.6 (78.8) | 819.0 (79.1) | 3882.6 (361.4) | -0.49% | 0.958 | 0.211 |
| flat_extra | json | 1119.9 (94.9) | 1180.5 (142.8) | 1080.2 (73.0) | 1560.1 (146.8) | +3.54% | 0.949 | 0.692 |
| flat_extra | value_copy | 1096.9 (565.1) | 875.8 (422.3) | 849.6 (639.0) | 4802.8 (4802.7) | +22.55% | 1.252 | 0.177 |
| flat_multiple | json | 1839.9 (147.5) | 1857.5 (327.4) | 1656.6 (85.0) | 1868.1 (114.3) | +9.97% | 0.991 | 0.887 |
| flat_multiple | value_copy | 1582.7 (532.5) | 1538.3 (109.5) | 1404.6 (164.0) | 3828.0 (274.1) | +11.25% | 1.029 | 0.367 |
| nested_invalid | json | 2633.6 (220.0) | 2484.2 (301.9) | 2244.7 (103.8) | 2468.3 (270.9) | +14.77% | 1.060 | 0.909 |
| nested_invalid | value_copy | 1973.0 (736.0) | 2088.1 (464.8) | 1747.6 (328.3) | 7255.1 (1520.1) | +11.42% | 0.945 | 0.241 |
| collection_invalid | json | 3178.3 (796.4) | 3166.5 (1255.4) | 3049.3 (401.8) | 3125.4 (419.5) | +4.06% | 1.004 | 0.976 |
| collection_invalid | value_copy | 1809.3 (261.7) | 1833.0 (70.5) | 1896.2 (258.2) | 9515.2 (1176.5) | -4.80% | 0.987 | 0.199 |
| malformed_json | json | 334.7 (81.8) | 392.1 (154.0) | 518.2 (267.3) | 997.7 (256.1) | -54.83% | 0.853 | 0.519 |
| flat_min | json | 894.7 (133.8) | 893.2 (145.2) | 791.6 (143.1) | 1172.0 (298.0) | +11.53% | 1.002 | 0.675 |
| flat_min | value_copy | 594.8 (49.4) | 611.7 (102.3) | 552.9 (140.3) | 3752.7 (681.8) | +7.04% | 0.972 | 0.147 |
| flat_max | json | 1346.2 (430.9) | 1140.9 (455.0) | 1109.4 (376.7) | 1211.9 (179.6) | +17.59% | 1.180 | 0.915 |
| flat_max | value_copy | 622.9 (96.3) | 638.9 (44.8) | 533.5 (47.7) | 3564.7 (382.9) | +14.35% | 0.975 | 0.150 |
| flat_unicode64 | json | 1103.2 (263.4) | 1132.4 (443.3) | 1183.5 (743.6) | 2796.6 (451.1) | -7.28% | 0.974 | 0.423 |
| flat_unicode64 | value_copy | 598.3 (131.1) | 613.3 (82.3) | 570.3 (88.9) | 3985.7 (730.3) | +4.69% | 0.976 | 0.143 |
| flat_unicode65 | json | 1362.8 (112.6) | 1391.7 (183.8) | 1285.2 (193.1) | 3034.5 (286.5) | +5.69% | 0.979 | 0.424 |
| flat_unicode65 | value_copy | 1023.3 (160.8) | 996.5 (74.7) | 947.6 (123.6) | 4068.8 (562.8) | +7.39% | 1.027 | 0.233 |
| flat_huge_int | json | 1088.9 (127.2) | 1039.4 (176.4) | 1035.7 (115.3) | 1727.5 (132.2) | +4.88% | 1.048 | 0.600 |
| flat_huge_int | value_copy | 757.6 (85.9) | 766.8 (316.3) | 720.3 (74.5) | 4525.8 (433.8) | +4.93% | 0.988 | 0.159 |
| flat_root_array | json | 413.5 (202.4) | 341.2 (146.3) | 335.0 (65.4) | 1001.5 (223.5) | +18.98% | 1.212 | 0.335 |
| flat_root_array | value_copy | 172.2 (191.4) | 170.3 (50.7) | 158.4 (23.8) | 1997.6 (742.0) | +8.02% | 1.011 | 0.079 |
| nested_extra | json | 1585.2 (262.3) | 1605.8 (219.1) | 1555.7 (233.0) | 2088.2 (214.4) | +1.86% | 0.987 | 0.745 |
| nested_extra | value_copy | 1171.2 (201.7) | 1198.1 (128.6) | 1259.8 (123.5) | 6601.9 (437.2) | -7.57% | 0.978 | 0.191 |
| flat_nonintegral_float | json | 1087.7 (153.7) | 1152.2 (499.5) | 1038.4 (338.4) | 1684.4 (315.7) | +4.54% | 0.944 | 0.617 |
| flat_nonintegral_float | value_copy | 773.4 (106.1) | 773.8 (57.7) | 759.1 (106.8) | 4060.5 (363.3) | +1.85% | 0.999 | 0.187 |

Rows with a slower candidate pooled median (including small/noisy differences): `flat_valid/value_copy` (-12.04%), `flat_wrong_type/value_copy` (-11.81%), `flat_long_name/value_copy` (-2.79%), `flat_missing/json` (-23.43%), `flat_null/value_copy` (-0.49%), `collection_invalid/value_copy` (-4.80%), `malformed_json/json` (-54.83%), `flat_unicode64/json` (-7.28%), `nested_extra/value_copy` (-7.57%).

## Selected per-round results

Per-round medians retain disagreement rather than replacing it with only a pooled result.

| Round | Case (JSON) | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction |
|---|---|---:|---:|---:|---:|---:|
| 1 | flat_valid | 853.4 | 831.6 | 792.1 | 1106.0 | +7.19% |
| 1 | nested_valid | 1780.6 | 1843.5 | 1813.9 | 1868.3 | -1.87% |
| 1 | collection_100 | 97233.5 | 96500.1 | 90174.0 | 67803.3 | +7.26% |
| 1 | flat_negative | 1154.6 | 1113.6 | 1070.5 | 1556.5 | +7.28% |
| 1 | flat_upper_bound | 1238.1 | 1163.8 | 1129.2 | 1607.3 | +8.80% |
| 2 | flat_valid | 1219.6 | 1163.2 | 1338.8 | 1575.0 | -9.77% |
| 2 | nested_valid | 4133.5 | 2471.7 | 2376.8 | 2892.4 | +42.50% |
| 2 | collection_100 | 93271.3 | 93215.9 | 92530.2 | 69358.3 | +0.79% |
| 2 | flat_negative | 1224.9 | 1273.6 | 1048.5 | 2064.0 | +14.41% |
| 2 | flat_upper_bound | 1183.3 | 1163.4 | 1066.2 | 1645.6 | +9.89% |

## Interpretation limits

- Primary JSON includes parsing, validation, model/error construction and disposal. Secondary `value_copy` includes Rust Value clone versus Python deepcopy and is not pure validation or parser cost.
- Invalid-input diagnostic work differs between libraries. Native formatting and full diagnostic equivalence are established by regression tests, not this normalized category/path timing oracle.
- Workers repeatedly process fixed warmed payloads; Pydantic's default string cache remains enabled. This does not establish cold or high-cardinality performance.
- Pydantic core is also Rust. No language-wide ranking, universal speedup, CPU-profile attribution, production claim or CI performance threshold follows.
- The same-source control detects some timing/build confounds but cannot remove host scheduling, frequency, allocator placement or compiler-layout sensitivity between different programs.
- Allocation instrumentation, if reported separately, is not used in these timed binaries and is not interchangeable with wall-clock evidence.
