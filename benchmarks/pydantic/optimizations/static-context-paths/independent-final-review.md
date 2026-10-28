# Final public-artifact and pinned-diagnostic audit

Result: no remaining numerical, semantic, or privacy blocker found in the prepared public evidence. This is a technical evidence review, **not authorization to upload, merge, or claim a clean latency improvement**.

## Public derivative verification

All three compressed JSON artifacts were independently decompressed and compared recursively with their private originals. Exactly the five declared `commands` strings changed; every measurement, error, source/binary hash, protocol value, status string, regression and telemetry value is identical. Manifest compressed/decompressed/original checksums, byte counts, top-level field lists and record counts match. All gzip timestamps are zero.

| Artifact | Process records | Compressed SHA256 |
|---|---:|---|
| `initial/results.json.gz` | 392 | `85bc0de1247153a53a09ef50be1395bcf7a75aad9a9c7de3c926335951c22f63` |
| `confirmation/results.json.gz` | 392 | `476ee63b187b3c7198afd022bd166742e321e33c8531f2de9915648609ee9298` |
| `pinned-diagnostic.json.gz` | 192 | `ab33bbd71b892eb1b0ed26c7fa4c1452121b851d5c426afd8d2fd002bb17113c` |

Decompressed public payloads contain no absolute cloud/user paths or credential-pattern hits. Structured contents are synthetic repository fixtures, public source/binary identities and assistant-cloud metadata. A broader text scan of the artifact directory found only forbidden-pattern literals in the exporter itself, not leaked paths or credentials. The initial and confirmation raw datasets retain their independently audited values and all adverse rows.

## One-shot pinned A/A diagnostic

Independently verified 192 successful records: three predeclared cases × 16 blocks × four children. Each case follows exact alternating ABBA/BAAB order, with 32 samples for each of the two byte-identical executable aliases. Worker iterations/warmups, result consumption checksums, release identity, canonical output/error oracle, positive sample arrays, timestamps, return codes, affinity assertions and unchanged executable/driver/fixture hashes all match the recorded design.

The immutable artifact contains only A/A records and the `aa` gate. There are no paired-stage, baseline-engine or Pydantic-engine execution records. The outcome correctly reports stopping before pairing.

| Case | Aggregate candidate/control | Block-ratio IQR | Blocks within ±5% | Result |
|---|---:|---:|---:|---|
| nested_valid | 0.996276 | 12.340 percentage points | 4/16 | FAIL |
| flat_wrong_type | 1.010464 | 7.758 percentage points | 9/16 | FAIL |
| collection_100 | 0.978708 | 10.209 percentage points | 7/16 | FAIL |

Thresholds are exactly the predeclared aggregate-distance ≤0.02, ratio-IQR ≤0.05, and at least 14 of 16 blocks within ±0.05, with every case required. Nested and wrong-type pass the aggregate component but fail the dispersion/count components; collection fails all components. The case-level FAIL values are correct. One independent linear interpolation differs from Python statistics by 1.1e-16 in collection IQR; this is normal floating-point rounding and does not affect any displayed value or gate.

## Telemetry interpretation

Pre-exec sets only the spawned child affinity to the selected allowed CPU and verifies it before exec. The record does not prove continuous worker placement or historical frequency/migration state. The unchanged worker code does not alter affinity. The raw `sources_and_binaries_unchanged` flag checks driver, fixture and executable bytes; it is not a fresh complete runtime-source snapshot. The final prose now states both limits precisely.

Whole-child elapsed wall time is measured by the parent around subprocess execution and includes startup/setup/warmup/cleanup and wrapper overhead. RUSAGE_CHILDREN CPU and context-switch deltas cover that child process, not only the benchmark loop. They are not substituted for primary worker batch timings.

| Case | Median wall/CPU | Maximum wall/CPU | Involuntary switches |
|---|---:|---:|---:|
| nested_valid | 1.020 | 1.137 | 70 |
| flat_wrong_type | 1.016 | 1.028 | 116 |
| collection_100 | 1.019 | 1.045 | 155 |

All telemetry summaries and selected process-position medians in the instability analysis were independently recomputed. The cross-engine 31,858.7 ns/op Pydantic wrong-type batch and A/A 3.882 block ratio are present. These observations do not establish a single scheduling cause or exonerate every candidate slowdown.

## Documentation and retained limits

The README now describes both original protocols, all 19 initial and seven confirmation slower medians, the failed distinct diagnostic, same-binary control limitations, and exact measured-driver retention. The instability introduction now says no third **full-corpus performance repeat**, rather than denying the distinct timed A/A diagnostic. Source correctness and allocation reduction remain separate from latency and merge approval.

Retained logs support 40 passing core harnesses, 91 workspace harnesses, four derive tests, successful no-default/Clippy checks, the 98 canonical parity checks, and the explicitly corrected earlier build-target provenance. The initially missing driver self-hash remains disclosed; no retroactive self-hash claim was introduced. Existing ignored feature/database tests remain identified as such.

No benchmark, build, publication, GitHub mutation or system setting change was executed during this audit. The private audit workspace retains `audit_public_and_pinned.py` and `public-pinned-audit.json` with the independent checks and recomputed evidence. The earlier `EVIDENCE_REVIEW.md` and `evidence-audit.json` remain intact as the original two-run audit; their publication-to-do items are resolved by this addendum for the artifact candidates checked here.
