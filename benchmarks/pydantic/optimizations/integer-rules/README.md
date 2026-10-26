# Decoded integer numeric-rule validation

Refs #125. This is a bounded rule-dispatch optimization, not a parser rewrite,
validation shortcut or universal Rydantic/Pydantic ranking.

## Implementation and preserved behavior

Main `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` decoded a primitive integer and
then inherited generic `JsonType::validate_rules`, encoding it back to an
arbitrary-precision JSON Number. Each bound comparison parsed that value again.
The candidate uses the decoded i128 directly for lexical i128 bounds. A single
shared numeric error loop preserves all operators, diagnostics, parameters and
error order. Unsupported decimal/exponent/out-of-i128 bounds lazily create the
exact Number once and retain the exact decimal-comparison behavior; u128 values above i128::MAX keep
the old generic route. Native failures are emitted directly rather than
repeating the entire comparison loop. No float conversion or public API change.

Selected measured local source commit: `13f1e11dcff9490294a935adc89cf8776605107f`.
The initial source `4640402` used the shared public comparator even after a bound
had already failed the lexical i128 accessor. The selected implementation extracts
that comparator's unchanged exact-decimal fallback and calls it directly in this
known-fallback branch, avoiding the repeated bound parse and reparse of the known
integer. No inlining hint was retained: the isolated experiments did not establish
consistent benefit from that additional change.

The [initial full run](initial/README.md) and [all four bounded experiments](experiments/README.md)
are retained, including slower fallback/u128 observations and same-source-control
variability. These local provenance IDs do not imply those commit objects are
published. Raw source-file hashes match the final implementation independently of
the commit metadata assigned when publishing the PR.

## Measured result and important limits

The [complete final table](results.md) retains all 49 case/mode pairs and
[raw samples](results.json.gz), plus identical-source control and both rounds.
The pooled JSON collection median was **96.00 → 90.70 µs** (5.52% lower in this
run), versus Pydantic **69.14 µs**: the candidate remained **1.31× slower**.
Collection reductions were 7.26% and 0.79% by round. This supports a promising
bounded improvement, **not a precise established 5.52% effect**. Baseline/control
pooled collection medians were nearly identical, but other rows and rounds had
substantial variability; flat's second round was slower and nested's apparent
pooled gain is not reliable evidence of a stable gain. All nine slower pooled
case/mode rows are listed, including syntax/type rejection controls. Do not
attribute every noisy difference to this code change or erase the observations.

Two important adverse observations are **malformed JSON +54.83%** and **missing-field
JSON +23.43%** slower in the canonical pooled results. They are not hidden or assumed harmless. Malformed JSON fails before model
decoding, but the missing-field fixture contains `id: 42` and is missing `name`,
so it **does execute integer-rule validation**.
A [bounded longer-batch check](experiments/long-batch-controls.md) on the exact same
binaries did not reproduce the large penalties: malformed input was **+1.71%**
(367.6 → 373.9 ns), missing-field input **−4.39%**, and the flat-valid anchor
**−11.95%**. The approximately 6.3 ns malformed residual is still reported, not
claimed to be zero. This follow-up changes the confidence in a persistent large
regression, not the original samples; code-layout/scheduling sensitivity remains
an important limitation.

The longer-batch rounds still disagreed: malformed input was −1.94% then +8.32%,
and missing-field input −12.03% then +18.14%. Moderate error-path regressions
therefore remain possible; this draft is **not a blanket merge-ready or net-speedup
recommendation**. Review the complete tradeoffs rather than merging solely for
the uncertain collection median gain.

The [complete allocation/fallback controls](diagnostics.md) show warm collection
parse allocation calls falling **1,215 → 1,115**, with cumulative requested bytes
81,430 → 81,240. Counts are not RSS or timing. Final gated rule controls show the
i64 decimal-equality typed route at 262.7 versus generic 290.7 ns/op, after removal
of duplicate parsing. Upper-u128 remains the generic fallback and is **not a
claimed optimization**: final strict/huge-bound controls were 8.80%/8.97% slower,
while no-bounds was 1.82% slower; those observations and their IQR remain visible.
This tradeoff is not hidden behind common-integer gains. The original slower
fallback measurements and every local variant are retained separately.

Final diagnostics include fresh artifact receipts, source/binary before-and-after
checks, and [path-safe provenance](diagnostic-provenance.json). Initial diagnostic
provenance was only post-hoc fingerprinting and is explicitly labeled as weaker.
No instrumented executable was used for canonical timings. Pydantic's core is
also Rust and its normal repeated-string cache remains enabled. Measurements use
synthetic warmed fixtures on shared cloud hardware, not production or high-cardinality
traffic. There is no universal speedup or superiority claim.

## Correctness and verification

- Four new tests pass on unchanged main and candidate. They contain 16,920
  three-way comparisons against generic validation and a frozen numeric error
  loop, across all twelve integer types, native/fallback bounds, complete
  serialized errors, normalization/coercion, escaped nested paths, borrowing,
  aliases, validation/dump, optional values and u128::MAX.
- Independent review found no correctness blocker. A separate no-default-feature
  consumer passed 4,896 direct/BorrowType/wrapper comparisons, 506 additional
  full-width fallback combinations against a frozen baseline comparator/error
  loop, and hand-authored error/order/boundary assertions; 47 focused repository
  tests also passed.
- Final-source full workspace `--all-features --locked --offline`: 528 tests
  passed, nine ignored, across 91 result blocks including doctests. An explicit
  subsequent official Cap'n Proto CLI interoperability test passed. The remaining
  eight ignored tests require dedicated live databases or provider configuration;
  none was run and no credentials/live service were used.
- Default core/derive tests: 191 passed across 43 result blocks. Final workspace
  all-target/all-feature Clippy with warnings denied, no-default library check,
  formatting and diff checks passed. Eighteen evidence-tool tests passed; synthetic
  tooling tests are not performance observations.
- Initial all-target Clippy attempts exposed a missing local capnp executable and
  then a test-only Regex-in-loop warning. The official 1.5.0 compiler was restored
  in dot's own cloud and the test was corrected; final checks passed. Earlier
  failed logs remain retained and are not relabeled as successful checks.

All work uses synthetic data in dot's own cloud, never the user's computer. No
release, deployment, credential change, issue-state change or review-thread
resolution is included. Platform CI for the published head remains a separate
integration gate; local Linux checks are not macOS CI evidence.

## Reproduction

Use the pinned Rust/Python versions and Python dependencies described in
`../../README.md`. Keep source trees stable and builds/tests idle during timing.
The baseline and rebuilt control must use exactly the same source snapshot; use
separate target directories to retain both artifacts. A fresh worker compilation
is required, while unchanged dependency artifacts may be reused. Never relabel a
cached worker as a fresh build. If reusing a target, clean just the Rydantic
release artifacts first (`CARGO_TARGET_DIR=... cargo clean -p rydantic --release`).
The build wrapper will reject `fresh: true` worker receipts.

```sh
python3 record_build.py --root BASELINE_ROOT --target-dir BASELINE_TARGET \
  --output-dir PRIVATE_EVIDENCE --label baseline
python3 record_build.py --root BASELINE_ROOT --target-dir CONTROL_TARGET \
  --output-dir PRIVATE_EVIDENCE --label control
python3 record_build.py --root CANDIDATE_ROOT --target-dir CANDIDATE_TARGET \
  --output-dir PRIVATE_EVIDENCE --label candidate
python3 compare.py --baseline-build PRIVATE_EVIDENCE/baseline.json \
  --control-build PRIVATE_EVIDENCE/control.json \
  --candidate-build PRIVATE_EVIDENCE/candidate.json \
  --pydantic-python PYDANTIC_VENV/bin/python --output PRIVATE_EVIDENCE/results.json
python3 audit.py PRIVATE_EVIDENCE/results.json --check-local
python3 export_public.py PRIVATE_EVIDENCE/results.json results.json.gz
python3 report.py results.json.gz --output results.md
python3 audit.py results.json.gz --report results.md \
  --private-original PRIVATE_EVIDENCE/results.json
python3 -m unittest discover -s . -p 'test_*.py' -v
```

The default protocol is 10,000 operations per batch, 1,000 warmups, three batches
per process position and two independently case-shuffled rounds. Each round
runs a counterbalanced eight-position order; alternating rounds reverse the
initial engine order. That gives 12 samples per engine/case/mode and 784 checked
timed process results over all 49 canonical case/mode pairs. Before timing, all
four engine artifacts pass the independent fixture oracle and known-difference
controls; original canonical two-engine known-difference checks are duplicated
for both identical-source controls, not silently omitted. Full native errors are
covered by separate Rust regression tests, not the normalized timing oracle.

`record_build.py` saves private source/binary hashes, source revision and status,
compiler/profile/environment and original Cargo JSONL/log checksums. Build-time
and pre/post-run source snapshots must match. `compare.py` verifies equal source
for baseline/control, equal compiler/profile/env for all three Rust builds, and
equal corpus/lockfile/canonical workers across builds. Identical baseline/control
binary hashes are valid. No instrumented allocator is linked into these timing
workers.

`export_public.py` produces a clearly marked derivative, retaining the private
original checksum and exact observations/hashes while replacing local paths with
artifact IDs and removing the OS build identifier. It fails closed if a local
path remains in other metadata, such as compiler flags. Keep private originals;
never edit samples or overwrite the source evidence. Interrupted runs retain an
explicitly marked `.incomplete` artifact when normal Python shutdown can run;
these cannot pass the final audit. `audit.py` rechecks every
observation, expected output/category/path, engine pin, schedule, profile and
source-control invariant, and optionally checks local files, export identity
and exact table reproduction.

Results are batch-average ns/op, not request percentiles. Baseline loops are
retained and not subtracted. Pydantic's default repeated-string caching remains
enabled; its core is Rust. `value_copy` includes different copy implementations,
and rejection diagnostic work is not identical. The same-source rebuilt control
reveals some build/timing confounds but cannot eliminate shared-host or compiler
layout sensitivity between different programs. Keep all slower/noisy rows.

## Retained receipts

[Path-redacted Cargo and verification receipts](verification-receipts.tar.gz)
include a manifest with each original/private and redacted/public SHA256. They
preserve diagnostics, results, build profiles and source/binary hashes while
replacing own-cloud path prefixes with artifact IDs. The archive includes the
initial failed Clippy attempts as well as final successful checks. Private
originals were retained for independent audit; the redacted files are explicitly
derivatives, not claimed byte-identical originals.

One trailing space in `run_diagnostics.py` was removed after measurement during
final staged diff checking. This cosmetic-only change does not alter the worker
code or executed logic. The exact measured runner is retained as
`measured-run_diagnostics.py` inside the receipt archive, matching the recorded
tool hash; the final formatted runner passes the same 18 synthetic tests.
