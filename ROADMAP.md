# Rydantic roadmap

**Baseline: 2026-10-27.** Stabilize contracts before expanding capabilities.
Now / Next / Later express priority and dependencies, not promised dates or
release commitments. An item is complete only when its acceptance evidence is
recorded; source availability, passing tests and publication are separate states.
This file is the canonical roadmap; release records remain authoritative for
what users can install.

## Baseline, not future work

- **Core 0.8.0 stable** is now the default and Latest release. Runtime and derive
  are published together with an exact derive dependency. See the
  [stable release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0).
- **Core 0.8.0-beta.2** is a published, verified opt-in prerelease, not a stable
  promotion. Ordered callback lists, per-call context and typed pipelines are
  implemented, not a future feature wishlist. See [#72](https://github.com/trotsky1997/Rydantic/issues/72),
  [#73](https://github.com/trotsky1997/Rydantic/pull/73),
  [#79](https://github.com/trotsky1997/Rydantic/pull/79) and the
  [beta.2 release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0-beta.2) and
  [registry acceptance](acceptance/core-0.8/BETA2-REGISTRY.md). Beta.2 includes the
  optional bounded OpenAPI components added after beta.1.
  Earlier beta.1 publication documentation merged in [#81](https://github.com/trotsky1997/Rydantic/pull/81);
  its post-merge checks are tracked separately from the verified package release.
- **RydanticAI 0.7.0** is the latest published AI package, pinned to stable core
  `=0.8.0`. Its PDF release record includes exact-main CI, registry checksum and
  consumer receipts: [GitHub release](https://github.com/trotsky1997/Rydantic/releases/tag/rydantic-ai-v0.7.0).
- CLI and codegen remain unpublished source tools. Agent evaluation infrastructure
  exists, but no measured agent-performance improvement is established.
- An open issue is not proof of an unimplemented defect. Existing repair and
  verification links must be checked before scheduling duplicate work. This
  roadmap does not close issues or resolve review threads.

## Completed: P0 — stabilize the core 0.8 contract

**Outcome:** core 0.8.0 is published and independently verified without enlarging
the beta.2 runtime contract. Stable receipt: [stable registry acceptance](acceptance/core-0.8/STABLE-REGISTRY.md).
Exact source: `337c8b05798fa4d02eb75359ba9c9980901949df`; publication: https://github.com/trotsky1997/Rydantic/actions/runs/40203697840.

Acceptance evidence:

- Preserve and exercise callback ordering, before-chain short-circuiting,
  check-list accumulation, sync/async prerequisite gates, and the distinction
  between construction and revalidation.
- Exercise context ownership, missing/wrong-type errors, nested and untagged
  diagnostics, and repeated validation/dumping. Failed untagged alternatives
  share application data; their mutations and external effects are not rolled back.
- Maintain compile-negative and successful migration examples for the breaking
  `Context` loss of `UnwindSafe` / `RefUnwindSafe`. Never recommend a blanket
  `AssertUnwindSafe` wrapper as a compatibility fix.
- Collect reproducible beta usage feedback from concrete application integrations.
  Triage confirmed contract gaps and retain failing examples as regressions;
  do not relabel synthetic fixtures as external adoption evidence.
- Before promotion, record the compatibility decision and migration notes,
  resolve or explicitly defer nonblocking findings, and leave no known blocking
  contract failure. Verify the exact final source with platform CI, package
  checks, paired runtime/derive pins and fresh registry-only consumers.

**Progress:** [#83](https://github.com/trotsky1997/Rydantic/pull/83) adds
synthetic regressions for failed untagged alternatives: shared effects persist,
branch errors stay relative, successful fallback discards rejected-branch errors,
and context ownership is released. The tests cover JSON and Value entry points.
This is regression evidence, not external adoption or stable-promotion evidence.

**Application feedback and release sequence:** [#106](https://github.com/trotsky1997/Rydantic/pull/106)
adds a concrete maintainer-operated HTTP/context/pipeline/SQLite application,
actual OpenAPI request/response components and migration probes. The
[record](acceptance/core-0.8/VALIDATION.md) separates source from published beta.1,
independent reruns from author results, and synthetic input from outside adoption.
This is a bounded first-party interpretation of the concrete-integration gate,
not an external-adopter claim or a substitute for the platform/package gates
recorded separately in the final stable receipt: [stable registry acceptance](acceptance/core-0.8/STABLE-REGISTRY.md).
No library contract failure was confirmed by this bounded integration.

Core **0.8.0 stable** follows the independently verified beta.2 with paired-version,
metadata and documentation changes only, not new runtime behavior. Compatibility,
breaking migration notes, paired pins, exact-source CI and package/registry receipts
are recorded in [the stable receipt](acceptance/core-0.8/STABLE-REGISTRY.md) and
[#105](https://github.com/trotsky1997/Rydantic/issues/105). The synthetic first-party
integration evidence remains bounded, not external adoption.

**Acceptance basis:** beta usage evidence and the layered verification below,
with final stable evidence recorded at [stable registry acceptance](acceptance/core-0.8/STABLE-REGISTRY.md).
The existing package and synthetic-test receipts are a starting point, not a
substitute for the usage-feedback gate. See the
[context guide](docs/models/validation-context.mdx) and
[pipeline boundaries](docs/models/validator-extensions.mdx).

## Now: P0 — build layered verification, not one misleading percentage

**Outcome:** protect the actual supported contracts across the workspace.

- Retain the existing **85% line and 85% function floors** for the bounded
  default-core runtime measurement. Prioritize remaining failure-path gaps in
  `types`, `borrowed` and `object` using annotated reports and regression cases.
- Keep macro behavior and compile-pass/compile-fail diagnostics as an explicit
  layer. Exercise callback hygiene, model forms, renamed dependencies and
  consumer cfg combinations; do not imply runtime coverage instruments the
  proc-macro compiler process.
- Add separately scoped verification/measurement for settings and partial JSON,
  other optional runtime features, Cap'n Proto, SQL/Axum and AI. Reuse the existing
  platform, real-database and protocol fixtures rather than claiming these are
  currently untested or putting everything in one expensive job.
- For each added layer, record enabled features, platform/toolchain, selected
  tests, measured source universe and exclusions. Establish a reproducible
  baseline before choosing any new numerical floor. Disabled or zero-test suites
  do not count as coverage; absent branch counters are not branch coverage.
- Preserve exact-source provenance and failure logs. Separate correctness,
  coverage and performance claims; no performance improvement is inferred from
  a higher coverage percentage.

**Acceptance:** each introduced gate fails on a deliberate negative control,
passes on its intended source, and documents its limits. No whole-workspace
percentage is promised. Existing evidence: [COVERAGE.md](COVERAGE.md),
[#76](https://github.com/trotsky1997/Rydantic/pull/76) and
[#78](https://github.com/trotsky1997/Rydantic/pull/78).

## Delivered: P1 — publish PDF support as an independent AI release

**Outcome:** RydanticAI 0.7.0 is published independently from core 0.8.0 and
available from crates.io. Release record: [RydanticAI 0.7.0](https://github.com/trotsky1997/Rydantic/releases/tag/rydantic-ai-v0.7.0).

Acceptance evidence:

- Tag `rydantic-ai-v0.7.0` points to exact main commit
  `6517e6f9f59b7b5a9fb349665ab7987c886289f5`; exact-main push CI
  [passed](https://github.com/trotsky1997/Rydantic/actions/runs/40529535081)
  and the release record reports that the guarded release preflight passed.
- The protocol matrix covers JSON/SSE, mixed input order, tool history, replay
  identity/isolation, cancellation/deadlines and request/output limits across
  the three protocols and nine frontend/upstream combinations.
- PDF portability boundaries are documented and tested: embedded data across
  all nine routes, URLs only where supported, and detail controls rejected on
  unsupported routes. MIME/Base64/header validation is not full PDF parsing or
  a guarantee of upstream page/model support.
- Exact-main CI includes offline/loopback regressions and 89 strict official
  Python SDK cases; the release record reports successful preflight and package audit.
- Published archive SHA-256 matches the local package, crates.io API and sparse
  index: `686e804910edd87411483ca994930f6990a12d8faa9485205b9074e5a0f339ab`;
  the registry version is not yanked. A fresh registry-only consumer resolved
  both AI 0.7.0 and core 0.8.0 and passed its PDF prompt/API test.
- Examples, dependency pins and publication status now match the crate on
  crates.io.

**Release-process deviation:** no AI `0.7.0-beta.N` version was published in the
registry before this stable release. The PDF publication milestone is delivered,
but the beta-first acceptance gate was not completed; do not treat stable
publication or core's earlier beta acceptance as evidence of an AI beta campaign.

Retain these gates for future larger or core-breaking AI upgrades:

- Prepare and test an explicitly versioned beta first, publish it with a GitHub
  Release marked prerelease, and record acceptance before separately testing and
  promoting stable. Use versions/channels, not a long-lived beta branch.
- Before each upload: merge the matching version/lockfile preparation, require
  successful exact-main push CI, and pass release preflight plus package verification.
  After upload: verify registry/API and sparse-index checksums and run a fresh
  registry-only consumer. Postpublication acceptance cannot precede the upload.
- AI 0.6 consumers must align direct core dependencies with `=0.8.0` and review
  the [AI upgrade warning](crates/rydantic-ai/README.md#upgrading-from-ai-06).
  No compatibility layer or automatic/lossless data migration is promised.

**Dependencies:** the core compatibility decision, not necessarily a core stable
promotion. Live-provider tests require a separately authorized environment and
budget; fixtures are not proof of universal provider support. See
[#75](https://github.com/trotsky1997/Rydantic/pull/75) and the
[PDF contract](docs/integrations/pdf.mdx).

## Delivered: P1 — remove the blocked local documentation dependency chain

**Outcome:** repository-owned remediation merged in
[#84](https://github.com/trotsky1997/Rydantic/pull/84), tracked by
[#80](https://github.com/trotsky1997/Rydantic/issues/80). The root lockfile no longer
contains the local Mintlify CLI or any of the eight affected package families.
This removes those dependency paths from this repository; it does not claim that
upstream packages were patched or that a new full audit found zero vulnerabilities.

The replacement pins official MDX/GFM/frontmatter/YAML/slugification tooling and
checks the supported source subset, imports, navigation, links, anchors and assets.
Its 46 regression tests and independent review accompany the existing homepage,
stable-version and runnable-snippet checks. No blind overrides, CLI downgrade,
credential or hosting/security-setting changes were used.

**Intentional tradeoff:** `docs:dev` and local proprietary Mintlify strict-prebuild
validation are removed. The replacement does not promise renderer parity. The
existing hosted Mintlify preview is still a separate manual merge gate: inspect
an actual Ready preview and affected pages, navigation and snippets. A skipped
check alone is not approval; an explicit no-changes result can reuse verified
preview evidence only when rendered inputs are unchanged. See
[the documentation workflow and limits](docs/README.md#preview-and-validate).

**Ongoing acceptance:** retain the fail-closed source checks and their negative
controls, monitor the smaller dependency tree, and keep rendering verification
separate. Any future local-preview tooling proposal needs fresh dependency and
compatibility evidence. Historical audit counts in #80 are not unique
vulnerability counts or evidence of deployed-site exploitability; no Rust runtime
vulnerability is asserted.

## Later: P2 — measure Agent tooling value before expanding it

**Outcome:** decide whether the Skill and optional CLI improve real task outcomes.

- Run controlled, repeated A/B/C trials: pinned docs/tools; the same inputs plus
  Skill; then the same inputs plus CLI. Enforce hidden-test isolation and blind
  review with fixed version cohorts and budgets.
- Record all trials, failures, timeouts, first meaningful compile results,
  final contract outcomes, invalid API/feature usage, tokens/time and repairs.
  Do not pool stable and beta cohorts or present harness smoke tests as trials.
- Expand CLI scope only if it shows repeatable benefit beyond the Skill, or fixes
  a demonstrated failure class. Otherwise improve the Skill first. Adding new
  version knowledge must not silently rewrite frozen evaluation cohorts.

**Dependencies:** an authorized execution host/provider, explicit cost budget,
telemetry and evidence retention. **Acceptance:** an auditable campaign report
with limitations and an explicit expand / improve / defer decision, not a
predetermined success claim. See the [evaluation contract](evals/rydantic-agent/README.md)
and [current CLI boundary](crates/rydantic-cli/README.md).

## Non-goals for this roadmap

- No legacy compatibility layer or blanket restoration of panic-safety markers.
- No unsupported promise of lossless transformations, automatic side-effect
  rollback, production-data migration or cross-configuration wire compatibility.
- No extra provider protocols or generic plugin framework without a demonstrated
  contract need and separately reviewed scope.
- No general OpenAPI/Protobuf generator or full JSON Schema parity. The
  [source-only codegen tool](crates/rydantic-codegen/README.md) remains deliberately
  limited and fail-closed; publishing it is not implied here.
- No automatic CLI installation, real-provider execution, stable promotion or
  package publication merely because an item appears in this document.

## Updating progress

Link each milestone to its implementation, review, tests and release evidence.
Use precise states such as planned, implemented, verified, published or blocked.
Update this file when those facts or priorities change, without copying the full
roadmap into guides. New scope needs a concrete problem, acceptance criteria and
an explicit trade-off against the work above.
