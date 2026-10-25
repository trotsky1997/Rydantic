# Application acceptance record — 2026-10-25

Maintainer-operated first-party integration, Linux x86_64, Rust/Cargo 1.98.1.
All request data is synthetic. No external adopters, production deployment,
provider execution or performance claim is represented here.

## Executed checks

| Dependency selection | Command | Result |
|---|---|---|
| Published exact `=0.8.0-beta.1`, crates.io core and derive | `cargo test --locked` after registry lock resolution | 3 application/lifecycle/pipeline tests passed |
| Core+derive exact source `e9a5679990bbffda420865839c21fd951b60bd01` | `cargo test --locked --features openapi-acceptance` | 3 common + 1 OpenAPI test passed |
| Source default packaged with relative path and matching source lock | Same source command | 4 passed |
| Published beta.1 and same source, separately | `scripts/context-migration.py` with each explicit dependency selector | Each 4/4 expected outcomes |

A separate reviewer copied the final application/test sources and independently
reran both application commands: source 4/4 and registry beta.1 3/3. The reviewer
also independently reran registry beta.1 migration probes: 4/4.

The final three application files were byte-identical across registry and source
runs (OpenAPI is feature-gated, unavailable in published beta.1):

```text
e778cb023711a40327fe2d9ec0bfdf14f9008d5e64ee2037435c3a3d352917da  src/lib.rs
f447d47bb9a8c18c3806d4a3657f221c64dfcc150158335254913dc8807a164e  tests/application.rs
e8c79548dab656c9914ea7d9f3c9dbeb25f06cb43731262ce12c97f71bde7ee3  tests/openapi.rs
```

Published beta.1 resolution used crates.io, with package checksums:

- rydantic: `4fd4c86005e776675cdadc1c117a3561b07d2fd5a14650a93c23a273c2c289ca`
- rydantic-derive: `f30ab6d7eeb1430a3c35ac78c4337e237c42f52edf16c627bed1e5a2fbcdfa19`

Source files were materialized from exact Git blobs, with a reduced workspace
containing only core and derive. This is not full-workspace CI or cross-platform
evidence. The package version string alone does not identify this source:
OpenAPI was added after the identically numbered published beta.1.

## Review-driven corrections retained

The initial application had commuting pipeline stages and no observable rejecting
pipeline case. The final version uses noncommuting stages, an actual HTTP rejection
before SQL writes, and a continuation counter proving fail-fast behavior. Initial
OpenAPI projection fixtures were not the actual service DTO; the final version adds
actual Order input and OrderResponse output components and checks successful HTTP
response fields and context-aware DTO round trips. The standalone alias/exclusion
fixture is labeled separately, and its runtime-rule list must be nonempty.

The test harness initially mishandled percent-encoded schema references; its
reference resolution was corrected. This was a fixture error, not evidence of a
Rydantic bug. Switching back to source with an old registry lock correctly failed
`--locked`; the checked-in source manifest now uses the matching candidate lock.

## Remaining publication gates

No beta.2 or stable 0.8.0 registry test is claimed by this record. Run the same
harness and migration probes after each actual registry publication; record the
exact package checksums and versions. Complete final-main CI, compatibility and
version/pin/package audits separately. Stable promotion must retain accepted beta.2
runtime source, with only reviewed release metadata/version/pin changes. See #105.

Formatting and all-target Clippy with `-D warnings` also passed for the packaged source workspace.
