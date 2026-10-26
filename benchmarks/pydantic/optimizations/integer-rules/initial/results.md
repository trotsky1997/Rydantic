# Integer rule validation: paired measurements

Refs #125.

## Protocol and provenance

UTC: 2026-10-26T03:55:33.972995+00:00 to 2026-10-26T03:59:33.273966+00:00. 10,000 operations/batch, 1,000 warmups, 3 batches/position, 2 rounds, 12 samples/engine/case. All 49 case/mode pairs are retained.

Orders by round: `[['baseline', 'control', 'candidate', 'pydantic', 'pydantic', 'candidate', 'control', 'baseline'], ['pydantic', 'candidate', 'control', 'baseline', 'baseline', 'control', 'candidate', 'pydantic']]`. Case order is independently shuffled with seeds `20261026 + round`. Processes run sequentially; warmup, validation gates and process startup are outside reported timings. Loop controls are retained, never subtracted.

Baseline is current main before this fix. Control is a fresh build of exactly the same source, not another optimization. Candidate is this integer-rule change. Identical baseline/control executable hashes are permitted and explicitly reported. Their observed differences indicate run-to-run noise, not a code difference.

Fresh build receipts verify opt-level 3, no debug info/assertions or overflow checks. Source snapshots and executable hashes are checked before and after measurement. Full source-file hashes and receipt hashes are in the raw evidence.

| Engine | Measured local source commit | Binary SHA256 |
|---|---|---|
| baseline | `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` | `bd16da874b4350af0f8548daadb396d0076c1cf682017f2821fbba4d636705ee` |
| control | `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` | `bd16da874b4350af0f8548daadb396d0076c1cf682017f2821fbba4d636705ee` |
| candidate | `4640402e2a8caab81f9c4062fb4670cd6334addd` | `8ce068639a1cc0a7fc3b966381c14e87ebeebaba7b0e12dd7c109a6449854718` |

Baseline/control binary equality: **True**.

Rust compiler: `rustc 1.98.1 (48a229cea 2026-09-01)`. Python: `Python 3.12.14`. Pydantic/core pins: 2.14.1/2.50.1. Python executable SHA256: `fa67443527ed9647f760d807e2a38f26340757123e643c4639cf273ed15d5ea7`.

CPU: INTEL(R) XEON(R) PLATINUM 8573C; 9 logical CPUs. Affinity: `[0, 1, 2, 3, 4, 5, 6, 7, 8]`. Shared-host load start/end: `[1.447265625, 4.7216796875, 4.00341796875]` / `[1.0673828125, 2.74365234375, 3.3505859375]`. These metadata do not establish an exclusive host or fixed CPU frequency.

## Complete results

All timings are median (inclusive IQR) **batch-average ns/op**, not individual-request latency percentiles. Reduction = 100 × (1 − candidate/baseline); positive is faster. Baseline/control compares identical source; candidate/Pydantic > 1 means candidate is slower. Ratios of medians are descriptive, not statistical significance tests.

| Case | Mode | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction | Baseline/control | Candidate/Pydantic |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| flat_valid | json | 853.0 (23.3) | 922.2 (99.7) | 779.2 (91.2) | 1096.1 (91.2) | +8.65% | 0.925 | 0.711 |
| flat_valid | value_copy | 580.6 (56.5) | 583.4 (54.6) | 531.6 (50.4) | 3808.8 (1187.2) | +8.43% | 0.995 | 0.140 |
| flat_unicode | json | 936.7 (113.7) | 931.3 (86.3) | 793.2 (109.3) | 1152.6 (81.0) | +15.32% | 1.006 | 0.688 |
| flat_unicode | value_copy | 570.2 (94.2) | 585.3 (111.6) | 579.2 (84.5) | 3710.6 (456.6) | -1.58% | 0.974 | 0.156 |
| nested_valid | json | 1819.6 (207.8) | 1724.1 (111.7) | 1825.0 (183.7) | 2029.4 (296.9) | -0.30% | 1.055 | 0.899 |
| nested_valid | value_copy | 1197.1 (107.9) | 1216.3 (163.6) | 1239.4 (281.2) | 7738.0 (1317.4) | -3.54% | 0.984 | 0.160 |
| collection_100 | json | 98173.2 (19513.1) | 96954.3 (6222.1) | 90364.3 (16635.1) | 72753.9 (13449.5) | +7.95% | 1.013 | 1.242 |
| collection_100 | value_copy | 72526.7 (9340.6) | 70757.8 (16543.4) | 64909.0 (100425.4) | 240047.1 (23224.5) | +10.50% | 1.025 | 0.270 |
| flat_wrong_type | json | 1129.9 (273.7) | 1082.1 (99.6) | 995.5 (52.8) | 1613.3 (257.3) | +11.90% | 1.044 | 0.617 |
| flat_wrong_type | value_copy | 892.6 (244.7) | 871.2 (185.2) | 830.8 (101.4) | 4078.2 (413.5) | +6.92% | 1.025 | 0.204 |
| flat_bool_as_int | json | 996.9 (203.8) | 1003.0 (115.9) | 1016.2 (115.8) | 1645.7 (216.9) | -1.93% | 0.994 | 0.617 |
| flat_bool_as_int | value_copy | 814.6 (69.3) | 837.5 (79.1) | 856.2 (92.4) | 4033.3 (344.9) | -5.10% | 0.973 | 0.212 |
| flat_negative | json | 1196.8 (263.5) | 1107.0 (78.0) | 1073.6 (153.2) | 1471.0 (258.5) | +10.30% | 1.081 | 0.730 |
| flat_negative | value_copy | 879.3 (64.9) | 847.4 (80.0) | 833.8 (63.1) | 3808.9 (138.4) | +5.18% | 1.038 | 0.219 |
| flat_upper_bound | json | 1226.2 (210.3) | 1173.2 (210.0) | 1096.5 (132.7) | 1612.6 (306.7) | +10.57% | 1.045 | 0.680 |
| flat_upper_bound | value_copy | 1107.6 (368.5) | 986.4 (344.3) | 822.7 (280.1) | 4167.4 (579.6) | +25.73% | 1.123 | 0.197 |
| flat_empty_name | json | 1133.9 (153.8) | 1222.7 (229.1) | 1057.9 (96.9) | 1528.6 (589.6) | +6.70% | 0.927 | 0.692 |
| flat_empty_name | value_copy | 909.3 (131.6) | 908.7 (206.8) | 887.4 (136.2) | 3848.1 (412.9) | +2.41% | 1.001 | 0.231 |
| flat_long_name | json | 1218.4 (73.0) | 1253.2 (70.5) | 1131.9 (80.2) | 1646.7 (136.8) | +7.10% | 0.972 | 0.687 |
| flat_long_name | value_copy | 940.5 (92.8) | 903.8 (40.1) | 857.8 (59.8) | 4084.5 (481.1) | +8.79% | 1.041 | 0.210 |
| flat_missing | json | 741.2 (51.7) | 790.0 (142.6) | 694.7 (151.6) | 1676.3 (223.1) | +6.27% | 0.938 | 0.414 |
| flat_missing | value_copy | 542.7 (44.6) | 565.4 (46.2) | 523.0 (47.8) | 3318.6 (329.0) | +3.63% | 0.960 | 0.158 |
| flat_null | json | 1113.4 (407.2) | 1243.8 (232.3) | 1097.0 (332.1) | 1528.5 (196.8) | +1.47% | 0.895 | 0.718 |
| flat_null | value_copy | 843.4 (118.8) | 820.4 (70.7) | 829.4 (145.0) | 4175.9 (762.2) | +1.65% | 1.028 | 0.199 |
| flat_extra | json | 1114.2 (124.3) | 1119.3 (203.5) | 1017.6 (80.7) | 1772.8 (249.8) | +8.67% | 0.995 | 0.574 |
| flat_extra | value_copy | 827.0 (147.5) | 924.1 (607.3) | 768.4 (192.6) | 4472.9 (621.7) | +7.09% | 0.895 | 0.172 |
| flat_multiple | json | 1861.2 (241.4) | 1918.6 (677.7) | 1965.5 (658.5) | 1880.7 (133.0) | -5.60% | 0.970 | 1.045 |
| flat_multiple | value_copy | 1399.9 (200.6) | 1720.5 (1219.7) | 1562.6 (465.5) | 4071.4 (263.3) | -11.62% | 0.814 | 0.384 |
| nested_invalid | json | 2547.7 (450.5) | 2345.5 (157.9) | 2264.2 (252.2) | 2560.2 (521.7) | +11.13% | 1.086 | 0.884 |
| nested_invalid | value_copy | 1760.8 (114.5) | 1735.0 (229.4) | 1656.6 (114.0) | 6773.0 (409.3) | +5.91% | 1.015 | 0.245 |
| collection_invalid | json | 2997.0 (208.9) | 2633.8 (148.5) | 2493.5 (152.1) | 2957.6 (125.1) | +16.80% | 1.138 | 0.843 |
| collection_invalid | value_copy | 1910.7 (374.1) | 1890.3 (501.8) | 1818.2 (631.8) | 9085.2 (2183.0) | +4.84% | 1.011 | 0.200 |
| malformed_json | json | 359.2 (58.8) | 329.2 (38.0) | 352.0 (28.9) | 976.6 (82.7) | +2.00% | 1.091 | 0.360 |
| flat_min | json | 912.8 (175.1) | 811.7 (70.3) | 762.1 (81.4) | 1191.0 (119.9) | +16.51% | 1.125 | 0.640 |
| flat_min | value_copy | 567.5 (33.1) | 632.3 (91.7) | 587.8 (97.2) | 3646.2 (202.0) | -3.57% | 0.898 | 0.161 |
| flat_max | json | 1024.5 (240.8) | 935.2 (74.5) | 856.3 (60.0) | 1115.2 (63.9) | +16.42% | 1.096 | 0.768 |
| flat_max | value_copy | 638.2 (164.2) | 635.7 (42.2) | 578.8 (56.4) | 3574.2 (531.0) | +9.30% | 1.004 | 0.162 |
| flat_unicode64 | json | 1023.3 (115.1) | 1071.3 (116.5) | 864.8 (87.1) | 2483.7 (114.7) | +15.49% | 0.955 | 0.348 |
| flat_unicode64 | value_copy | 633.1 (106.2) | 613.4 (67.4) | 546.6 (80.6) | 3784.4 (682.7) | +13.66% | 1.032 | 0.144 |
| flat_unicode65 | json | 1325.7 (156.9) | 1317.7 (120.6) | 1262.9 (426.2) | 2994.2 (1309.4) | +4.74% | 1.006 | 0.422 |
| flat_unicode65 | value_copy | 1107.2 (288.7) | 993.2 (262.9) | 874.3 (81.7) | 4268.4 (419.2) | +21.03% | 1.115 | 0.205 |
| flat_huge_int | json | 1151.8 (243.3) | 1095.7 (151.4) | 1024.9 (176.2) | 1748.0 (244.1) | +11.02% | 1.051 | 0.586 |
| flat_huge_int | value_copy | 715.6 (163.2) | 716.6 (101.1) | 723.6 (242.7) | 4567.4 (522.9) | -1.11% | 0.999 | 0.158 |
| flat_root_array | json | 350.3 (193.8) | 303.6 (19.8) | 303.3 (14.9) | 921.8 (101.1) | +13.43% | 1.154 | 0.329 |
| flat_root_array | value_copy | 142.9 (15.0) | 172.9 (87.8) | 148.4 (22.4) | 1774.2 (102.3) | -3.86% | 0.826 | 0.084 |
| nested_extra | json | 1605.6 (200.3) | 1618.4 (127.1) | 1499.1 (89.2) | 2007.4 (414.8) | +6.63% | 0.992 | 0.747 |
| nested_extra | value_copy | 1224.0 (109.1) | 1266.4 (192.5) | 1114.9 (95.7) | 7432.4 (669.2) | +8.91% | 0.967 | 0.150 |
| flat_nonintegral_float | json | 1008.1 (194.6) | 1015.1 (50.2) | 987.7 (126.2) | 1465.8 (182.4) | +2.02% | 0.993 | 0.674 |
| flat_nonintegral_float | value_copy | 765.5 (97.9) | 736.4 (65.7) | 819.4 (49.0) | 3997.9 (430.7) | -7.04% | 1.040 | 0.205 |

Rows with a slower candidate pooled median (including small/noisy differences): `flat_unicode/value_copy` (-1.58%), `nested_valid/json` (-0.30%), `nested_valid/value_copy` (-3.54%), `flat_bool_as_int/json` (-1.93%), `flat_bool_as_int/value_copy` (-5.10%), `flat_multiple/json` (-5.60%), `flat_multiple/value_copy` (-11.62%), `flat_min/value_copy` (-3.57%), `flat_huge_int/value_copy` (-1.11%), `flat_root_array/value_copy` (-3.86%), `flat_nonintegral_float/value_copy` (-7.04%).

## Selected per-round results

Per-round medians retain disagreement rather than replacing it with only a pooled result.

| Round | Case (JSON) | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction |
|---|---|---:|---:|---:|---:|---:|
| 1 | flat_valid | 853.3 | 993.8 | 779.2 | 1154.3 | +8.68% |
| 1 | nested_valid | 1842.5 | 1740.8 | 1770.1 | 1984.0 | +3.93% |
| 1 | collection_100 | 108298.3 | 96609.6 | 95786.0 | 73800.6 | +11.55% |
| 1 | flat_negative | 1196.8 | 1098.6 | 1068.1 | 1602.8 | +10.76% |
| 1 | flat_upper_bound | 1180.8 | 1134.5 | 1192.3 | 1568.2 | -0.98% |
| 2 | flat_valid | 836.7 | 896.7 | 782.3 | 1081.4 | +6.50% |
| 2 | nested_valid | 1819.6 | 1709.9 | 1910.0 | 2156.7 | -4.97% |
| 2 | collection_100 | 98173.2 | 102872.0 | 88891.9 | 69159.8 | +9.45% |
| 2 | flat_negative | 1210.0 | 1120.5 | 1075.5 | 1458.0 | +11.12% |
| 2 | flat_upper_bound | 1300.3 | 1315.5 | 1045.1 | 1612.6 | +19.63% |

## Interpretation limits

- Primary JSON includes parsing, validation, model/error construction and disposal. Secondary `value_copy` includes Rust Value clone versus Python deepcopy and is not pure validation or parser cost.
- Invalid-input diagnostic work differs between libraries. Native formatting and full diagnostic equivalence are established by regression tests, not this normalized category/path timing oracle.
- Workers repeatedly process fixed warmed payloads; Pydantic's default string cache remains enabled. This does not establish cold or high-cardinality performance.
- Pydantic core is also Rust. No language-wide ranking, universal speedup, CPU-profile attribution, production claim or CI performance threshold follows.
- The same-source control detects some timing/build confounds but cannot remove host scheduling, frequency, allocator placement or compiler-layout sensitivity between different programs.
- Allocation instrumentation, if reported separately, is not used in these timed binaries and is not interchangeable with wall-clock evidence.
