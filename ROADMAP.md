# Rydantic roadmap

**Baseline: 2026-10-21.** Stabilize contracts before expanding capabilities.
Now / Next / Later express priority and dependencies, not promised dates or
release commitments. An item is complete only when its acceptance evidence is
recorded; source availability, passing tests and publication are separate states.
This file is the canonical roadmap; release records remain authoritative for
what users can install.

## Baseline, not future work

- **Core 0.7.0 stable** remains the default and Latest release. Runtime and derive
  are published together with an exact derive dependency. See the
  [stable release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0).
- **Core 0.8.0-beta.1** is a published, verified opt-in prerelease, not a stable
  promotion. Ordered callback lists, per-call context and typed pipelines are
  implemented, not a future feature wishlist. See [#72](https://github.com/trotsky1997/Rydantic/issues/72),
  [#73](https://github.com/trotsky1997/Rydantic/pull/73),
  [#79](https://github.com/trotsky1997/Rydantic/pull/79) and the
  [beta release](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0-beta.1).
  Publication documentation merged in [#81](https://github.com/trotsky1997/Rydantic/pull/81);
  its post-merge checks are tracked separately from the verified package release.
- **RydanticAI 0.6.0** is independently published. PDF inputs and the refreshed
  tutorials merged in [#75](https://github.com/trotsky1997/Rydantic/pull/75), but
  the PDF API is not in the published 0.6.0 package. The workspace AI source now
  depends on core `=0.8.0-beta.1`; this does not republish or change the old package.
- CLI and codegen remain unpublished source tools. Agent evaluation infrastructure
  exists, but no measured agent-performance improvement is established.
- An open issue is not proof of an unimplemented defect. Existing repair and
  verification links must be checked before scheduling duplicate work. This
  roadmap does not close issues or resolve review threads.

## Now: P0 — stabilize the core 0.8 contract

**Outcome:** determine whether the published beta is ready for a stable promotion
without enlarging its scope merely to fill a version milestone.

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

**Dependencies:** beta usage evidence and the layered verification below.
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

## Next: P1 — deliver PDF support as an independent AI release

**Outcome:** turn merged source into an installable, accurately documented AI
release without conflating the core and AI lifecycles.

Acceptance evidence:

- Decide and document the next AI package version and supported core dependency
  policy. A stable AI user must not be silently moved onto a core beta; current
  source alignment alone is not a release-policy decision.
- Freeze a capability/rejection matrix for the existing three protocols and nine
  frontend/upstream combinations. Cover JSON/SSE, mixed input order, tool history,
  replay identity/isolation, cancellation/deadlines and request/output limits.
- Retain PDF portability boundaries: embedded data across all nine routes;
  URLs only where supported; explicit detail controls rejected on unsupported
  routes. MIME/Base64/header validation is not full PDF parsing or a guarantee
  of upstream page/model support.
- Verify the final package with offline/loopback regressions, official SDK
  fixtures and a registry-only consumer. Match examples, dependencies and docs
  to the actual published version; retain exact-head CI and archive receipts.

**Dependencies:** the core compatibility decision, not necessarily a core stable
promotion. Live-provider tests require a separately authorized environment and
budget; fixtures are not proof of universal provider support. See
[#75](https://github.com/trotsky1997/Rydantic/pull/75) and the
[PDF contract](docs/integrations/pdf.mdx).

## Next: P1 — unblock documentation dependency maintenance

**Outcome:** find a supported remediation for
[#80](https://github.com/trotsky1997/Rydantic/issues/80), without weakening the
build or overstating security evidence.

Dependabot remediation is blocked for eight transitive package families under
Mintlify tooling. The checked `mint` 4.2.1085 candidate did not remove those
families from the audit result. Audit package/metavulnerability counts are not
unique vulnerability counts or proof of deployed-site exploitability.

**Acceptance:** identify a supported upstream dependency path, verify applicable
advisory ranges and the resolved lockfile, then pass documentation build, links,
snippets and homepage checks in a bounded change. If no supported path exists,
keep the blocker and candidate evidence explicit rather than calling an
ineffective upgrade a fix. No blind overrides, CLI downgrades or security-setting
changes. This work does not assert a vulnerability in the Rust runtime.

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
