# Independent static-path evidence audit

Result: numerical and semantic evidence checks passed. This is **not** performance/merge approval. The measured regressions remain visible and unresolved as general performance conclusions.

## Evidence checked

- Initial run: 49 canonical rows, 392 process outputs, 1,176 timed batches; all 19 slower candidate medians retained.
- Bounded longer-batch confirmation: 49 rows, 392 outputs, 1,960 timed batches; all seven slower candidate medians retained.
- Independently recomputed acceptance, canonical accepted output, normalized error paths/categories, positive samples, batch counts, consumption checksums, release profiles, pinned Pydantic/core versions, exact process order and adaptive iteration sizing.
- Independently recomputed every displayed median, inclusive Q1/Q3 and percentage in both 49-row reports; no arithmetic or omitted-row discrepancy found.
- All 416 source hashes per run matched the inspected trees. The only baseline/candidate snapshot differences are error.rs and derive lib.rs. The baseline copied regression test is untracked but byte-identical to the candidate test and is not linked into the example worker. Committed runtime/worker/manifest/lock/corpus/Python-worker blobs match the recorded commit references.

Raw result identities:
- initial: SHA256 `b83442b1ff5e9acdd593cd5f332ed100bfaa14de4090db9ec50a31fe472c28f4`
- confirmation: SHA256 `1db0b53f62f446b8b2df0556ec4c57d4b2e5fa9c0293a72b8474bb0f68bec277`

## Artifact and driver provenance

The three Cargo JSON receipts each report exactly one successful opt-level-3, non-debug-asserting benchmark executable, from the intended package root. Binary files hash to the raw result records. Candidate and control binaries are identical; the control is only a separate target/process-order observation, not an independent code-layout or source variant. The misleading early target name is correctly disclosed and is not treated as the main baseline.

The control receipt digest was corrected to its actual `c80a043a24d3fd3255b7d7d6fc322c5c0c582205116f6683412c360129f2da03`. The immutable confirmation driver is retained as `confirmation/measured-compare.py`; its SHA256 `151aaf87e3fdae338f73c226eb7749a08875b65d768590305cdd75d77db37220` matches the raw record. The current driver differs only by correcting “three” to “five” in a comment. No raw hash was rewritten.

Limitation: the initial raw record did not capture a driver self-hash. Its saved source is retained, but exact driver byte identity during that initial run cannot be established from the raw JSON alone. This does not affect the verified result structure/math or recorded runtime/input/executable identities.

## Results and interpretation

| JSON workload | Initial change | Confirmation change | Confirmation control versus candidate |
|---|---:|---:|---:|
| flat_valid | -9.8% | -7.7% | -2.5% |
| nested_valid | -6.8% | +13.8% | -13.9% |
| collection_100 | -7.6% | -4.8% | -3.4% |
| flat_wrong_type | +7.0% | +67.2% | -11.2% |

The same binary has meaningful position-to-position dispersion. In confirmation, nested candidate is +13.8% versus main while the identical control is about -2.0%; wrong-type candidate is +67.2% while control is about +48.5%. These observations support a shared-host/order confound, **not** deletion of adverse measurements or proof of no regression. Flat and collection improvements are workload/session results only. The data do not establish a universal win or justify chaining earlier optimization ratios.

Confirmation slower rows:
- nested_valid/json: +13.8%
- flat_wrong_type/json: +67.2%
- flat_negative/json: +2.1%
- flat_long_name/json: +4.9%
- nested_invalid/json: +0.1%
- malformed_json/json: +0.5%
- flat_root_array/value_copy: +0.6%

## Separate allocation audit

All 49 baseline/candidate allocation rows preserve identical acceptance and full serialized errors, and their normalized errors also match the canonical independent fixture oracle. The probe source matches both consumer sources; lockfiles match; Cargo fingerprints show baseline without and candidate with the static-path feature. Current allocation executable hashes are recorded in the audit JSON. These probes were not used for timed results.

Allocation deltas over the full corpus are only `(calls, cumulative requested bytes)` = `(0,0)`, `(3,12)`, `(5,20)`, `(7,29)`, `(301,1205)`, consistent with visited static labels. The isolated prewarmed success control removes 1,000 calls / 11,000 bytes; dynamic labels stay 1,000 / 11,000. The isolated error changes 8→7 calls and 667→656 bytes with identical full errors. Flat 14→11 / 862→850, nested 26→21 / 1770→1750, collection 1215→914 / 81430→80225 are all correct. These are cumulative requested allocations, not peak memory, RSS, or latency evidence.

## Privacy and publication boundary

The inspected raw timing records contain five unique absolute path strings each, all assistant-owned cloud paths. Allocation and build-provenance JSON have no absolute paths. Structured field inspection and credential-pattern scanning found no user-computer path, credential, token or non-synthetic user data. Raw cloud paths still require the planned public redaction; a public derivative should preserve measurements/protocol/source hashes and retain its private-original checksum. A final privacy check of that derivative has not yet been performed.

The README seen at audit start was pre-run prose with a missing control and stale timing-pending statement. Its final rewrite must distinguish both real protocols, retain all regressions, and describe the immutable measured-driver path. Publication readiness is therefore separate from this completed raw-data audit. No benchmark, build, upload or GitHub mutation was executed by this audit. Any later distinct diagnostic requires its own labeled evidence and audit; it must not replace these two immutable runs.

## Audit artifacts

`audit_evidence.py` is the independent read-only checker; `evidence-audit.json` contains every recomputed row, artifact identity, allocation delta and privacy result. Existing source/semantic review remains in `REVIEW.md`.
