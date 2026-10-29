# Bounded dispatch/fallback experiments

Refs #125. All four local alternatives and all 18 controls are retained. These are separate same-binary generic-versus-typed rule diagnostics, not complete application benchmarks. Entire variants ran sequentially, so cross-variant absolute differences can include host drift and binary layout. Each route has 12 batch-average samples; no allocator instrumentation was linked.

The selected production change is **fallback extraction only**, without `#[inline]`. The private helper is the unchanged exact-decimal comparator; reaching it after the bound accessor failed avoids parsing that bound and the known integer again. Inlining alone did not establish a consistent benefit, so the hint was not retained. Upper-u128 remains on the generic route and no universal zero-overhead guarantee is made. The original slower observations and later disagreements remain visible.

Source patches are relative to local `4640402`; fingerprints and UTC intervals are in the accompanying provenance JSON. `initial` repeats the initial binary, rather than replacing its earlier recorded run. Entries below are typed/generic median ns/op, followed by typed percentage change (positive is slower).

| Type | Bounds | Initial repeat | Inline only | Inline + fallback | Fallback only (selected) |
|---|---|---:|---:|---:|---:|
| i64 | no_bounds | 16.2 / 35.0 (-53.86%) | 16.7 / 41.2 (-59.45%) | 16.2 / 35.1 (-53.89%) | 16.1 / 32.9 (-51.00%) |
| i128 | no_bounds | 16.2 / 49.0 (-67.02%) | 21.0 / 60.5 (-65.23%) | 16.2 / 49.4 (-67.24%) | 15.8 / 59.8 (-73.51%) |
| u128 | no_bounds | 48.6 / 49.0 (-0.88%) | 71.7 / 57.2 (+25.32%) | 48.2 / 48.0 (+0.46%) | 49.7 / 49.0 (+1.42%) |
| i64 | lexical_range | 29.1 / 60.7 (-52.06%) | 26.6 / 58.6 (-54.71%) | 29.8 / 64.6 (-53.85%) | 33.3 / 60.3 (-44.79%) |
| i128 | lexical_range | 177.9 / 414.3 (-57.07%) | 210.6 / 473.1 (-55.49%) | 175.7 / 416.3 (-57.79%) | 174.0 / 434.0 (-59.90%) |
| u128 | lexical_range | 999.5 / 1026.1 (-2.59%) | 1207.0 / 1078.8 (+11.88%) | 1054.5 / 970.4 (+8.67%) | 1028.4 / 1034.2 (-0.56%) |
| i64 | lexical_strict | 334.1 / 353.9 (-5.60%) | 369.4 / 394.3 (-6.30%) | 300.8 / 324.4 (-7.30%) | 335.3 / 356.3 (-5.88%) |
| i128 | lexical_strict | 164.0 / 390.8 (-58.05%) | 217.5 / 450.7 (-51.75%) | 186.8 / 395.1 (-52.73%) | 175.2 / 406.9 (-56.96%) |
| u128 | lexical_strict | 968.9 / 960.5 (+0.87%) | 1167.3 / 1052.7 (+10.88%) | 955.0 / 976.5 (-2.21%) | 950.2 / 988.5 (-3.88%) |
| i64 | decimal_equality | 264.4 / 250.4 (+5.59%) | 305.0 / 286.5 (+6.47%) | 263.4 / 266.1 (-1.01%) | 247.0 / 258.7 (-4.52%) |
| i128 | decimal_equality | 941.7 / 961.6 (-2.08%) | 1133.4 / 1111.9 (+1.93%) | 726.7 / 925.6 (-21.48%) | 828.2 / 999.8 (-17.17%) |
| u128 | decimal_equality | 932.7 / 938.3 (-0.60%) | 1138.5 / 1024.9 (+11.08%) | 943.4 / 890.0 (+6.00%) | 923.3 / 959.2 (-3.74%) |
| i64 | mixed_bounds | 927.2 / 900.2 (+3.00%) | 1012.1 / 1068.8 (-5.31%) | 873.0 / 907.1 (-3.76%) | 884.2 / 930.9 (-5.02%) |
| i128 | mixed_bounds | 1209.0 / 1314.0 (-7.99%) | 1321.3 / 1589.5 (-16.87%) | 904.5 / 1320.9 (-31.52%) | 946.6 / 1355.7 (-30.18%) |
| u128 | mixed_bounds | 1854.5 / 1848.0 (+0.35%) | 2109.0 / 2062.1 (+2.27%) | 1863.8 / 1800.4 (+3.52%) | 1848.0 / 1919.7 (-3.74%) |
| i64 | huge_bounds | 269.6 / 264.2 (+2.01%) | 291.9 / 294.9 (-1.02%) | 257.6 / 267.1 (-3.53%) | 263.5 / 298.2 (-11.65%) |
| i128 | huge_bounds | 789.0 / 765.4 (+3.08%) | 876.5 / 873.4 (+0.35%) | 548.5 / 757.1 (-27.56%) | 608.7 / 853.4 (-28.68%) |
| u128 | huge_bounds | 755.9 / 757.0 (-0.15%) | 855.0 / 851.9 (+0.36%) | 755.6 / 772.7 (-2.22%) | 825.8 / 775.4 (+6.49%) |
