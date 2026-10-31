# Rydantic CLI (P0, registry publication pending)

Thin, static, offline discovery and exact-version explanations for Coding Agents.
This independent package is prepared for crates.io, but has not been published.
It links no Rydantic validation/codegen engine and
never starts Cargo, rustup, a shell, a build script, a proc macro or project code.
It makes no network requests and writes no files. Running the executable is
read-only; building it with Cargo is a separate, explicit code-execution step.

From a prepared repository checkout:

```sh
cargo run -p rydantic-cli --frozen -- capabilities --format json --protocol-version 1
cargo run -p rydantic-cli --frozen -- doctor --workspace . --format json
cargo run -p rydantic-cli --frozen -- explain RD_LOCAL_RULES_REQUIRED --core-version 0.6.0 --format json
```

Do not run `cargo install rydantic-cli` yet: there is no verified registry release.
See [standalone distribution](DISTRIBUTION.md) for the selected installation path,
prepublication checks and remaining release gates. No global installation,
credentials or telemetry are needed to use the workspace build. `--offline` is accepted for
clarity, but all commands are always offline. It is not an OS sandbox.

## Commands and limits

- `capabilities`: reports protocol/CLI/knowledge versions, linked engines (all
  null), available commands, supported limits and explicit unsupported features.
- `doctor --workspace DIR [--package NAME] [--schema FILE]`: reads DIR/Cargo.toml,
  explicit workspace member manifests and DIR/Cargo.lock. Schema paths are
  relative to the current working directory, not DIR. Only local-rule annotation
  presence is inspected; neither schema validity nor portability is established.
- `explain CODE --core-version VERSION`: consults bundled exact 0.6.0, 0.7.0 or 0.8.0 knowledge.
- `explain --diagnostics FILE --input-format envelope|validation-errors|cargo
  --core-version VERSION`: explicitly reads CLI v1 envelopes, Rydantic error
  arrays, or Cargo NDJSON. Cargo compiler diagnostics are unknown rather than
  attributed to Rydantic using text matching. Unknown input codes are replaced
  by `unknown` so callback-controlled content is not echoed.

All commands accept `--format text|json`, `--protocol-version 1`, `--offline`.
Repeated/unknown/inapplicable options and stray arguments fail. Text mode is a
pretty-printed version of the same envelope. P0 does **not** implement `--resolve`,
`--config`, Cargo invocation, validation, codegen, schema diff or typed adapters.
It reports presence of root Cargo/toolchain configuration files without reading
or executing them. A present `rydantic.toml` produces an unsupported-config
warning; it is not silently interpreted. Explicit `--config` is an error.

Each file is bounded to 1 MiB; schema files to 64 KiB; JSON nesting to 64;
explain to 100 diagnostics; workspace member entries and direct Rydantic
records to 256 each. Files must be regular files; no stdin streams. JSON
syntax, duplicate object keys, reserved serde_json object members and trailing content are rejected before inspection.
TOML syntax errors are rejected, but this is not a complete Cargo manifest validator.

## Discovery evidence, never guessed resolution

`declared_dependencies` records direct declarations, renames, feature lists,
optional/default declarations, dependency kind and unevaluated target predicates.
`locked_dependencies` records **all** recorded Rydantic versions and source kinds.
Potential matching lock versions are only candidates, never a resolved package
association. Sources/URLs are redacted. No claim is made about lock freshness,
transitive packages, feature activation, target selection or actual Rust APIs.
`resolution`, `build`, `tests` remain `unknown`; resolved/verified feature sets are
null. `project_code_executed` is false.

The bundled feature sets are grounded in the published 0.6.0, 0.7.0 and 0.8.0 packages.
An exact `=0.6.0`, `=0.7.0` or `=0.8.0` registry declaration is checked against its own release's feature set
(unless a patch/replace/alternative registry or observed Cargo config makes that inference unknown).
A range such as `0.6` is not upgraded to exact knowledge merely because a possibly
stale lockfile contains a supported version. Each dependency declaration selects its own
knowledge; mixed-version workspaces do not share one feature set. Path/git dependencies, unknown versions and patched
sources receive unknown knowledge. Requested invalid stable features fail with 3.
This check describes the declared release, not proof of the code Cargo would load.
0.7.0 includes `settings`, `partial-json`, `phone`, `country` and `currency`;
0.6.0 does not. Neither 0.6.0 nor 0.7.0 includes `openapi`; 0.8.0 does.
0.8.0 context/pipeline APIs need no `async` or `pipeline` feature. Its bundled
context/async-guard explanations are static guidance, not callback execution.
An exact source revision or beta version does not select stable knowledge merely
because its version looks similar. `supported_lock_candidate` indicates only a
recorded candidate version with bundled knowledge, never verified provenance.
The common diagnostic explanations are version-selected, not a complete catalog
of every error code in these releases. Unknown codes remain redacted/unknown.

Full workspace discovery remains incomplete in P0; `explicit_manifest_reads_complete` only describes the explicit reads. Workspace globs and external/parent member paths (including escaping symlinks) are not expanded. Exclusions
are not evaluated, and their presence marks discovery incomplete; explicit
listed manifests may still be inspected. Auto-members via path dependencies,
ancestor workspaces and inherited external-workspace dependencies are not resolved.
Use an explicit workspace root and package. Unknown package selection fails with
5; incomplete discovery and absent lockfiles are warnings, not fake verification.

## Protocol v1

`protocol-v1.schema.json` defines the envelope. JSON mode emits exactly one object
and newline on stdout, including controlled errors. No banners or stderr parsing
are required. Diagnostic codes, severity, stage and next-action kinds are machine
fields; human messages are not stable matching keys. No timestamps or absolute
input paths are included. Consumers must handle unknown additive fields/codes.
OS termination and stdout failure cannot guarantee a complete envelope.

Exit meanings: 0 success; 1 check failed (reserved); 2 invalid invocation/input;
3 unsupported protocol/version/feature; 4 read/output I/O; 5 package/environment
unavailable; 6 execution failure (reserved); 7 resource limit; 8 policy conflict
(reserved); 70 internal error. Unknown diagnostic codes under a supported version
return success with `known: false`; an unsupported core version returns 3 and
no speculative explanation. Warnings do not turn static discovery into failure.

Schema text, diagnostic messages, params, locations, Cargo rendered diagnostics,
config contents and unknown codes are not echoed. Manifest package/alias names,
version requirements and requested feature names are discovery data and **are**
reported: do not treat discovery output as globally secret-free. Errors use
fixed messages and never quote parser input or raw OS paths.

## Test

```sh
cargo test -p rydantic-cli --locked --offline
cargo clippy -p rydantic-cli --all-targets --locked --offline -- -D warnings
```

The process tests disable PATH (no implicit Cargo/rustup), exercise protocol,
unknown options/versions/codes, rename/inheritance/target facts, multiple/stale
locks, exact-version feature rejection, explicit formats, duplicate/invalid and
bounded input, deterministic output, privacy markers and no generated project
files. They use synthetic temporary projects, not application code. Linux is
tested locally; cross-platform execution must be reported separately.


### Static configuration and inherited defaults

Doctor checks for Cargo configuration at the invocation directory and its ancestors,
the inspected workspace root and its ancestors, and `CARGO_HOME` (or the platform
home's `.cargo` directory). It observes legacy `config` and `config.toml` presence
only: it does not read values, resolve precedence, or disclose config contents.
Any observed configuration makes source-dependent feature knowledge unknown;
unavailable home/current-directory information or inspection errors also fail
closed. `cargo_config_present` is null when presence cannot be established.
A child package's config is relevant when invoked from that child, not merely
because it is listed as a workspace member. Inspection of the explicit target
root is conservative when it differs from the invocation directory.

For inherited dependencies, a member's `default-features = false` conflicting
with enabled workspace defaults is ignored on editions before 2024. Edition
2024 rejects the combination through Cargo 1.98 and overrides it from 1.99.
Doctor does not execute Cargo or infer the installed version from a toolchain
filename. For edition 2024 or an unknown edition, it therefore emits
`RD_DEFAULT_FEATURES_UNKNOWN` and null `declared_default_features` for this
combination. Workspace-inherited package editions are inspected explicitly.
These declaration facts remain separate from resolved and verified features.
