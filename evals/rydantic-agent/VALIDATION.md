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
