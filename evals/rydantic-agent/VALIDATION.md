# Validation record

2026-10-20, local P0 development. This is harness validation, **not an Agent trial**.

Executed:

- Python standard-library unit tests: 28 passed. Covers seeded balanced pairing,
  pin drift, missing locks/config, offline requirement, missing-result disclosure,
  first-compile failure preservation, timeout denominators, duplicate rejection,
  refusal to import explicit smoke kinds, required host isolation attestations,
  campaign digest integrity, empty-implementation rejection, incomplete-pass
  rejection, unknown review scores, stable/beta separation, invalid metric types,
  artifact digest/path checks, duplicate JSON keys and the refusal behavior checker.
- All four task locks regenerated successfully offline with installed Cargo 1.98.1;
  lock format v4. No download or Rust build performed by lock generation.
- Three reference-fixture Rust contract tests passed on exact rustc/Cargo 1.98.1:
  `serde-dto` with serde 1.0.229 / serde_json 1.0.154; `typed-user` with registry
  core+derive 0.6.0; `typed-normalization` with registry core+derive 0.7.0-beta.4.
  Used `cargo test --frozen --test hidden_contract`, debug=0, incremental=0,
  one build job and the existing shared target cache. Each isolated project copied
  its public pack, replaced src/lib.rs with the matching host-only
  tests/reference_solutions/<task>.rs and inserted hidden/<task>.rs as the test.
- Three deliberately broken reference mutants compiled but were rejected by these
  same hidden tests: Serde decode ignored input; stable model removed constraints;
  beta model removed normalization/coercion/constraints. All returned test failure
  (exit 101), demonstrating the checks are behavioral rather than compile-only.
  Mutants existed only in disposable staging projects; committed references remain
  correct. No hidden feedback was sent to an agent. These six Rust executions are
  **harness checks, not six agent trials**.

Pending:

- An authorized host implementation of the documented adapter contract, verified
  OS sandbox/budget enforcement, artifact provenance and leakage controls.
- Actual A/no-Skill, B/Skill, C/Skill+CLI executions and blinded review. No real trial
  result files exist here; no first-compile/final-pass improvement is claimed.
- The P0 Skill/CLI only establish stable 0.6.0 knowledge; beta task support must be
  reported separately and conservatively rather than inferred from stable docs.

No provider services, LLM calls, installs, publication or telemetry were performed.

PR review regression, 2026-10-20:

- Reproduced all four budget bypasses before the fix: completed wall=11 with cap=10,
  input=101/cap=100, output=51/cap=50, and repairs=3/cap=2 were accepted.
- Importer now rejects each over-budget completed record, accepts exact-boundary
  completion, and preserves noncompleted overruns/costs in summary denominators
  with explicit per-metric overrun counts. Noncompleted hidden passes remain invalid.
- Negative tests also retain all four failure/blocker terminal states and reject
  missing token telemetry. All 28 harness unit tests pass; no Agent trials run.


Source-pack extension, 2026-10-22 (harness checks only):

- 43 Python standard-library unit tests passed: the original 28 plus 15 source-pack
  regressions. Coverage includes explicit full pack selection, exact source and
  toolchain requirements, both core/derive manifest pins, lock drift/duplicates,
  host-lock byte identity, missing hidden contract, catalog/lock cohort separation,
  unchanged legacy hash, independent pack hashing, and cross-campaign import refusal.
- Legacy task/hidden bytes and default catalog are unchanged. The legacy pack hash
  remains `915e9bec9917a95603a893ea5c33e6586d007644b9d5639fcbb23fecb55190e7`.
- The new source task lock was generated with installed Cargo 1.98.1; both Rydantic
  packages resolve to git revision `60067205276e314653f46a75c213ec1756c7ad7c`.
- Installed Rust/Cargo 1.98.1 ran `tests/smoke_source_context.py` against the pinned
  git lock with `--frozen --offline`: reference 4/4 tests passed. Both mutants
  compiled first; the no-hook mutant failed 3 behavioral tests and the fallback-
  context mutant failed 2. These were trusted reference/negative-control harness
  executions, not agent trials. The script copies evaluator-only artifacts into
  temporary candidates; no hidden feedback is supplied to an agent.
- Actual source-cohort A/B/C trials, sandboxed authorized-host execution, blind
  review and performance comparisons remain pending. No provider/model execution
  or performance improvement is claimed. Published beta and the P0 CLI's stable
  capability boundary remain distinct from this exact source snapshot.
- Source-trial isolation remains unimplemented: an ordinary Cargo git checkout and
  its bare objects expose older Skill/evaluator files. A host must hide those paths
  and objects or audit minimal dependency materialization before any agent trial;
  otherwise record environment_blocked. The smoke did not validate isolation.
