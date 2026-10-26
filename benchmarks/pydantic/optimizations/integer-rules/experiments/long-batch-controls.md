# Longer-batch regression confirmation

Refs #125. This targeted diagnostic does not replace or rewrite the complete canonical benchmark. It uses the same exact three Rust binaries and unchanged inputs; there is no Pydantic engine and no cross-library ranking in this check.

The canonical final run observed malformed JSON +54.83% and missing-field JSON +23.43% slower pooled medians. Malformed JSON fails before model decoding. The missing-field fixture is `{"id":42,"active":true}`: it is missing `name`, retains the ID, and does execute integer-rule validation. Both observations were checked rather than assumed to be noise; compiler-layout effects can still be real binary regressions.

Two reversed counterbalanced rounds use 250,000 operations/batch, 10,000 warmups and five batches/position, giving 20 samples/cell. All 36 process responses match the fixture oracle, and source/binary hashes are checked before/after. No samples are removed or baseline-subtracted. Values are median (inclusive IQR) batch-average ns/op.

UTC: 2026-10-26T04:25:37.171788+00:00 to 2026-10-26T04:26:16.149549+00:00. Shared-host load [0.2421875, 1.39453125, 2.20458984375] → [0.6171875, 1.34375, 2.15234375].

| Case | Baseline | Same-source control | Candidate | Candidate change |
|---|---:|---:|---:|---:|
| malformed_json | 367.6 (40.5) | 366.9 (36.7) | 373.9 (32.6) | +1.71% |
| flat_missing | 787.4 (62.0) | 821.0 (56.3) | 752.8 (194.7) | -4.39% |
| flat_valid | 904.6 (255.1) | 955.9 (101.2) | 796.5 (85.2) | -11.95% |

| Round | Case | Baseline | Same-source control | Candidate | Candidate change |
|---|---|---:|---:|---:|---:|
| 1 | malformed_json | 382.6 | 376.8 | 375.2 | -1.94% |
| 1 | flat_missing | 824.0 | 776.9 | 724.8 | -12.03% |
| 1 | flat_valid | 1154.0 | 970.2 | 834.0 | -27.73% |
| 2 | malformed_json | 345.2 | 346.2 | 373.9 | +8.32% |
| 2 | flat_missing | 762.8 | 828.6 | 901.2 | +18.14% |
| 2 | flat_valid | 904.6 | 933.1 | 793.0 | -12.33% |

The large canonical regressions did not reproduce at this batch size. Malformed JSON still has a +1.71% pooled residual (about 6.3 ns/op); it is not reported as zero or automatically dismissed. Missing-field and flat-valid pooled medians are lower here. These observations limit any claim of a persistent 54.83% malformed-input penalty, but do not prove that no regression can occur on other builds, scheduling conditions or workloads. The earlier slower samples remain intact, and no universal speedup follows.
