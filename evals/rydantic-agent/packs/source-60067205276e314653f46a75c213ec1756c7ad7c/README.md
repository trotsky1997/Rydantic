# Opt-in source context-policy pack

This is **harness readiness only**, not an agent trial, measured performance, or
published-release support claim. It is independent of the unchanged default P0
stable 0.6.0 / beta 0.7.0-beta.4 pack.

## Exact scope

- Pack ID / track: `source-60067205276e314653f46a75c213ec1756c7ad7c`.
- Library source: `https://github.com/trotsky1997/Rydantic`, full revision
  `60067205276e314653f46a75c213ec1756c7ad7c`. Both core and derive use this exact
  git revision in Cargo.toml and Cargo.lock; neither registry dependencies nor a
  nearby working tree substitute for it.
- The source reports package version 0.8.0-beta.1, but that **does not identify this
  snapshot**. The published 0.8.0-beta.1 release predates later source work, including
  OpenAPI changes. These results must not be relabeled as registry beta or stable
  results, even where an individual API overlaps.
- Task: `source-context-policy`, using the owned derived `Model` per-call context
  API. The independent hidden contract checks inclusive successful policy bounds,
  rejection and typed input validation, missing/wrong context errors and paths,
  newly supplied policy and mutated-value revalidation, and context non-retention.
  This does not cover all validator extension, async, BorrowModel, integration or
  OpenAPI behavior.
- Toolchain: already installed Rust/Cargo 1.98.1. Lock format v4. Host preparation
  may provision the exact cache before trials; candidates and fixture smoke runs
  remain offline. A missing cached git revision is an environment blocker, not
  permission to switch source or download during a trial.

## Explicit planning and no mixing

Use Python 3.11+ and start from `../../config.source.template.json`. Fill all
required host/model/artifact fields and keep its two source selectors:

```json
{
  "eval_pack": "source-60067205276e314653f46a75c213ec1756c7ad7c",
  "library_source_commit": "60067205276e314653f46a75c213ec1756c7ad7c"
}
```

Run from the repository root, keeping the filled configuration beside the template
so its relative paths resolve correctly:

```sh
python3 evals/rydantic-agent/runner.py plan evals/rydantic-agent/campaign.source.json > plan.source.json
```

Planning selects only this task, with balanced A/B/C repetitions. Unknown, null,
shortened or multiple pack selectors fail closed. Missing/wrong source revisions,
core/derive drift, source overrides, mismatched toolchain, missing hidden contract,
mixed task lock maps and host lock bytes different from the committed source task
lock are rejected. The config's `repository_commit` pins the integrated toolkit
being evaluated separately from `library_source_commit`; neither may float.

Omitting `eval_pack` preserves default catalog membership, task/hidden bytes,
legacy `pack_hash()` and default campaign serialization. The source tree is not
included in that legacy hash. Its own task and hidden tree are hashed separately;
campaign hashes and exact track labels keep imports and summaries separate.
Do not pool or compare as if source/stable/beta cohorts were interchangeable.

The existing host adapter, isolation, evidence and budget requirements still apply.
The runner does not execute commands or agents, and hashes do not authenticate a
model execution or prove that docs/Skill archives contain the correct source.
The host must verify that provenance before planning.

## Arms and leakage

A receives only the public library docs for the exact source revision and common
base tools. B also receives the explicitly source-aware Skill bundle. C receives
exactly B's inputs plus the pinned P0 CLI. That CLI's capability information remains
stable 0.6.0 knowledge; **C is not claimed to have source-aware CLI support**.
Record this boundary in reports instead of interpreting absent CLI knowledge as
an unavailable source API or silently swapping CLI versions.

Stage **only** `tasks/source-context-policy/` into the candidate workspace.
`hidden/`, `tests/reference_solutions/`, `tests/reference_mutants/`, smoke scripts,
other cohort files, campaign plans and results remain host-only. The new pack's
README and runner/configuration are also evaluator instructions, not extra hints
for an arm. Physical isolation is the host's responsibility, not a folder name.

### Git cache leakage is a host blocker

An ordinary Cargo git cache is **not safe to expose to a candidate**: the pinned
repository checkout also contains the older `skills/rydantic/` bundle and
`evals/rydantic-agent/hidden/` and `tests/` trees. Cargo's bare git object database
can recover those files even if visible checkout directories were removed. This
would invalidate Arm A's no-Skill boundary and can expose evaluator materials.
The host must exclude those checkout paths **and the bare git object databases**
from candidate access, or provide an audited minimal dependency materialization
that preserves the exact source provenance and pins without exposing repository
history or withheld files. Include other caches, symlinks and process-visible
paths in that audit. If the host cannot enforce this boundary, record
`environment_blocked`; do not run or count a trial. This PR adds no such host or
sandbox implementation. Local smoke success proves behavior only, not isolation.

## Trusted harness smoke, not agent performance

After the exact toolchain and task-lock dependencies are prepared, run:

```sh
python3 evals/rydantic-agent/tests/smoke_source_context.py \
  --cargo "$(rustup which --toolchain 1.98.1 cargo)" \
  --rustc "$(rustup which --toolchain 1.98.1 rustc)"
```

The script requires absolute, already-installed binaries at that exact version;
it does not bootstrap a toolchain. It copies the public task into three temporary
workspaces, installs trusted reference/mutant source and the hidden contract there,
and invokes Cargo with `--frozen --offline`. Each fixture must compile first. The
reference must pass all four behavioral tests; both deliberately broken fixtures
must compile but fail behavioral tests. One mutant removes the policy hook, the
other substitutes a default when context is missing or the wrong type. A compile
failure or missing cache is a smoke failure, not evidence that a mutant was caught.
No agent sees hidden feedback, and no smoke result can be called an A/B/C trial.
These trusted fixtures are not a sandbox for untrusted candidate submissions.
