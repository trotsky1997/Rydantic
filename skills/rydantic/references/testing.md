# Real verification and a bounded repair loop

1. Establish the intended contract before editing: types, keys, missing/null,
   defaults, coercion, normalization, extra fields and callback timing.
2. Add tests around the actual concrete Rust Model. Cover valid input, invalid
   input, inclusive bounds and neighbors, type errors and meaningful Unicode.
   With transforms, assert the parsed and dumped values, not just `is_ok()`.
3. Check input/output schema independently. Test local-rule refusal and explicitly
   structural-only behavior separately from typed validation. Standard schema
   agreement is not a substitute for callbacks or normalization.
4. Run the project's authorized frozen/offline Cargo checks. Compilation invokes
   proc macros/build scripts, tests invoke application code; offline is not a sandbox.
5. On failure, classify environment, compile, or contract failure. Make the smallest
   justified fix, rerun the affected test, then the relevant suite. Stop and report
   unsupported stable behavior rather than removing constraints, skipping assertions,
   switching to structural-only, or silently upgrading to beta.

## Runnable published consumer

Copy `assets/stable-0.6.0/` to a temporary directory outside the application's
workspace if desired. Its `[workspace]` isolates it even inside this repository.
It contains only registry dependencies and a lockfile; core and derive are both
exactly 0.6.0. The verification toolchain is Rust 1.98.1 (matching repository CI),
not an MSRV claim. Confirm it is already installed before using the explicit
`+1.98.1` selector; do not let rustup install it silently. With the locked
dependencies already cached:

    cargo +1.98.1 test --manifest-path assets/stable-0.6.0/Cargo.toml --frozen

Run from this skill's directory or adjust the path. Missing cached crates require
an explicit environment preparation decision; do not automatically retry online.
A different Rust/toolchain/target may need different cache artifacts even with a
lockfile. This fixture is not a universal MSRV or platform certification.

`tests/contracts.rs` exercises typed bounds/transforms, default/null/required/extra
semantics, input/output aliases/exclusion, local-rule refusal and structural-only
acceptance of typed-invalid data, format/default dynamic limits, and structured
error location/stage/code. It deliberately avoids beta-only APIs and custom
callback promises not verified against this published version.

A smoke-test suite is not an Agent behavioral benchmark. Record each command and
its actual outcome, and distinguish example tests from independent hidden task
tests. Do not claim improved Agent success rates without measured trials.

## Recorded fixture verification

On 2026-10-20, Linux x86_64, Rust 1.98.1: the command above passed all six
contract tests against the locked registry packages (`rydantic` and
`rydantic-derive` 0.6.0). `rustfmt` 1.98.1 check passed. This records one tested
configuration, not every feature combination or platform.

Skill frontmatter passed the official `skills-ref validate` implementation from
[agentskills/agentskills commit 69ef37e](https://github.com/agentskills/agentskills/tree/69ef37e9424c0a7ea9dd2293b559e43ec8176379/skills-ref).
All relative Markdown references were checked for existence. Format validation
is not a claim of measured Agent task performance.

## Independent 0.7.0 consumer

For exactly 0.7.0 core/derive, select `assets/stable-0.7.0/` instead. It retains
the six typed/dynamic contracts above and adds two tests for settings source
precedence/final validation and provisional partial-JSON versus typed parsing.
It enables `derive`, `dynamic`, `settings`, `partial-json`; it does not certify
all optional features. The package has its own registry-only lockfile/workspace.

    cargo +1.98.1 test --manifest-path assets/stable-0.7.0/Cargo.toml --frozen

Repository CI runs both consumers separately with `--locked` on Linux and macOS.
The 0.6.0 verification receipt above does not claim 0.7.0 execution. Report the
actual result for the selected version. Neither consumer is an Agent trial;
the evaluation task pins and hidden tests remain unchanged.
