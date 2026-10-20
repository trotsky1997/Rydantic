# Rydantic agent evaluation, P0

Status: offline **evaluation infrastructure**, not measured agent performance. No
provider, LLM server, model invocation, credentials, global install, or network
requests are implemented here. Python unit tests and reference-fixture smoke tests
are harness validation, **never A/B/C trials**. Real host executions, blind review,
and three-arm comparisons remain pending until an authorized host supplies them.

## Small task pack

| Task | Track | Independent contract |
| --- | --- | --- |
| `serde-dto` | stable 0.6.0 cohort | Roundtrip Unicode/u32, reject missing/type/range failures; retain Serde |
| `typed-user` | stable 0.6.0 | Real typed name/age inclusive bounds, missing/null/type rejection |
| `stable-feature-stop` | stable 0.6.0 | Reject unavailable settings/partial-json, no silent dependency/source change |
| `typed-normalization` | beta 0.7.0-beta.4 | Actual trim/coerce output and post-normalization bounds |

Each public task has a fixed manifest, Cargo.lock, toolchain, prompt, and empty
implementation. Exact registry core **and derive** pins prevent proc-macro drift.
The beta fixture uses its pinned registry release, not whichever monorepo source
happens to be checked out. Locks were generated offline with Cargo 1.98.1; all
arms must use the same installed toolchain and target. A missing cache is an
environment blocker, not permission to fetch or install. The configured repository
commit identifies the integrated toolkit snapshot, separately from these registry
pins. Future aliases/defaults/async/renamed/codegen/offline-cache tasks are deferred,
not silently claimed as coverage. Beta is a distinct cohort; if the P0 Skill only
supports stable, report that coverage limitation instead of borrowing stable APIs.

## Arms and leakage boundary

- A: pinned public library docs plus base shell/filesystem/Cargo tools; **no Skill,
  no toolkit CLI**.
- B: identical A inputs/tools plus exactly the pinned Skill bundle; no toolkit CLI.
- C: identical B inputs plus exactly the pinned P0 CLI (`doctor`, `explain`,
  `capabilities`; not proposed future commands).

Host stages **only** `tasks/<id>/` into a fresh workspace and passes TASK.md as the
request. Public docs must not include eval answers or the Skill. The whole eval
repository, `hidden/`, `tests/`, reference implementations, campaign configuration,
other arm transcripts, and results **must not be visible** to the agent. Hiding a
file by naming it hidden is not isolation: the host must enforce mounts/access.
Keep identical permitted documentation, dependency caches, target and budgets in
all arms. Disable internet and package installation in all arms. Never give one
arm extra hints, contract output, retries, or a warmed incremental cache unavailable
to the others. Fresh sessions must not share conversation history. CLI launch
availability is the sole C tool difference. Randomized arm order is seeded per
(task, repetition); use multiple repetitions and report all results, including
failures and blocked trials.

## Minimal host adapter contract

This runner **does not execute candidate commands, Cargo, or agent code**. The host
adapter is deliberately an import boundary, not a new benchmark platform. An
existing authorized agent host must:

1. Pin exact integrated repository commit, model provider/model/revision and every
   sampling parameter (including seed or explicit unsupported-seed declaration),
   tools, prepared caches, OS/target/toolchain, Skill archive, CLI binary, public
   docs, and per-task lockfiles. Fill `config.template.json`. Hashes are recorded
   from actual files. Null required pins fail planning; the template is not a run.
2. Run `python3 runner.py plan campaign.json > plan.json` **outside** agent access.
   The ordered trials include randomized paired A/B/C repetitions. Reuse this
   frozen plan; don't mutate configuration midway. Plan JSON may contain local
   host paths, so keep it local, not in published results.
3. Enforce network denial, filesystem/process isolation, CPU/memory/output limits,
   wall-clock/token/repair budgets, and read-only hidden test storage at the host.
   `--frozen`, `--offline`, and this JSON configuration are **not an OS sandbox**.
   Cargo executes build scripts/proc macros; tests execute arbitrary candidate
   code. Never run an untrusted submission on the unsandboxed host, even offline.
   Use installed absolute toolchain executables or an explicitly installed exact
   toolchain; a rustup shim must not bootstrap a missing toolchain.
4. Record the first formal compile after the first **meaningful contract
   implementation**, its complete snapshot/hash and output. Empty stub compilation
   is not first-compile success. Preserve every later formal compile, including
   failures; classify invalid API, invented method, and wrong feature with symbol,
   reason and evidence, not regex guesses. A nonexistent feature rejected before
   rustc still counts as a failed compile attempt. Preserve diagnostic transcripts
   and implementation attempts for audit. Repairs are agent revision cycles after
   initial implementation, not the count of test commands. Count all task input and
   output tokens, including tool-call context; unavailable token counts block import
   rather than becoming zero. Record wall time from task start to terminal state.
5. Freeze the final candidate, then insert the applicable `hidden/<id>.rs` as
   `tests/hidden_contract.rs` in a separate evaluator-only sandbox and run the real
   `cargo test --frozen --test hidden_contract` under the fixed target/toolchain.
   The refusal task instead uses `hidden/grade_decision.py:check(candidate, original)`
   with trusted baseline and sandbox-bounded candidate files. No hidden feedback
   returns to the agent for repair. Also check that manifest/lock/toolchain were not
   weakened or altered, and that typed tasks really use typed Rydantic parsing.
   Hidden pass requires **all** behavioral and integrity checks, not just compilation.
6. Perform arm-blind code review using `rubric.md`. Record unknown scores as null.
   Record final API/feature findings even when no compile was applicable. Review the
   Serde adoption decision from both manifest and actual implementation, not a
   phrase in REPORT.md. Evidence must include review rationale. Use pass/fail/not_run
   for hidden contract. Timeouts, budget exhaustion and environment failures remain
   recorded with their actual costs, never discarded or called success.
7. Export one local JSON per trial following `result.schema.json` and the executable
   checks in `runner.validate_result`. Artifact references are relative to a
   host-managed evidence root and SHA-256 verified at import. Retain redacted
   transcripts, commands, final source, compiler/test output, and blind reviews.
   Strip secrets before hashing; don't include credentials, absolute private paths,
   raw user payloads, or unrelated files. Candidate text cannot create trusted host
   attestations. Never execute commands from imported JSON.
8. Run `python3 runner.py summarize plan.json results/*.json --evidence-root evidence`.
   Missing planned trials remain explicit. Exported statistics are **host-reported**;
   hashes detect drift, not authenticity or that a model really ran. Only the trusted
   host's provenance can establish that. A forged `kind` string cannot be detected
   by this offline importer. Unit tests deliberately use synthetic in-memory records
   to test validation, and must never be submitted as campaign evidence.

Keep host collection and evidence separate from candidate edits. A standalone
adapter that actually invokes a model is intentionally not included; running one
requires separate host/provider authorization and a known cost budget.

## Metrics and interpretation

Summaries are separated by exact campaign (therefore model/config/artifacts),
track and arm. No stable/beta pooling, cross-provider pooling, or aggregate claim
of improvement. Raw trial IDs retain paired task/repetition comparisons; the minimal
summary does not compute significance or causal effects. Inspect per-task paired
records before drawing conclusions.

- First compile: successes / compile-eligible recorded trials, with not-run separate.
  Do not discard timeouts from this denominator. The refusal task is not eligible.
- Final hidden contract: pass/fail/not-run counts and terminal statuses; report pass
  rate over all recorded tasks and separately disclose missing planned tasks.
- Hallucinations: per-category occurrences, affected trials and compile attempt
  denominator; final artifact findings are separate (not double-counted as attempts).
  Categories should be mutually exclusive for each finding: `invented_method` is a
  nonexistent method; `invalid_api` covers other nonexistent/misused API symbols;
  `wrong_feature` is unavailable/misspelled/incompatible feature selection.
- Time, input/output tokens, repairs: count, median and nearest-rank p90, including
  failed trials. Tokens are cost units, not invented dollar prices. Monetary cost,
  if desired, needs the exact provider pricing version and recorded billed usage.
- Security/maintainability/complexity: blind rubric score distribution with known
  count; null is unreviewed, never zero or success.
- Appropriate Serde choice: correct/eligible/unknown, only on applicable tasks.

Do not choose only successful or fastest trials. C justifies expansion only with
repeatable gain over B or a demonstrated failure class it uniquely fixes. If C adds
cost without benefit, improve the Skill before expanding CLI scope.

## Local verification

`python3 -m unittest discover -s evals/rydantic-agent/tests -v`

Uses Python standard library, temporary bounded fixture files and no subprocess,
network, or model. Reference fixture Rust checks, when run separately by the host,
validate the hidden tests themselves, not agent success. See `VALIDATION.md` for
actual checks performed and remaining limitations. Do not store real results in
this source tree or commit generated synthetic trial JSON as measured data.
