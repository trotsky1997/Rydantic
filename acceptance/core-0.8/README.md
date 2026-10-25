# First-party 0.8 application acceptance

This is a bounded synthetic order service built to exercise concrete integration
friction. It is **not third-party/external adoption**, a production workload,
performance evidence, or a substitute for the repository CI suite. It uses only
loopback HTTP and a fresh in-memory SQLite database. No provider/network service,
user data, deployment, or credentials are used by the running application.
Cargo needs registry access to obtain dependencies.

## Workflow and feedback

A real Axum listener accepts order requests. The handler selects an explicit,
request-owned tenant policy, parses a context-aware model, revalidates it, dumps
it through the same context, and runs a typed Pipeline to produce a storage
command. An SQLite transaction writes an audit record before inserting the order.
A duplicate constraint causes explicit rollback, including the prior audit write.
Programmatic HTTP clients cover success, accumulated policy failures, malformed
JSON, unknown tenant, concurrent different-tenant requests, normalization, fail-fast slug-policy rejection before database writes, and
rollback. Supplementary direct API controls cover missing/wrong context types,
changed policy rejection, Arc ownership and an observable construction callback
counter proving validate/dump do not rerun construction transforms.

Integration feedback:

* Existing high-level `rydantic::axum::Json` and SQL model collection APIs do not
  propagate this business context automatically. The application explicitly uses
  the Axum framework body/header extractors and context-aware `Model` methods,
  then maps the accepted model through a Pipeline to SQLx queries. This does **not**
  claim automatic context forwarding by `Json`/`SqlModel`.
* Pipeline execution is explicit and separate from model validation; normalization
  occurs in ordered `plain -> then -> check -> then` stages. Lowercasing must precede
  case-sensitive `order: ` prefix removal (the operations do not commute). A
  reserved slug is rejected by the check; an observable later-stage counter proves
  fail-fast skips that stage. Model callback check errors
  accumulate, while Pipeline errors are fail-fast. Neither rolls back side effects.
  The database transaction is the application's rollback boundary.
* Per-request context is an owned `Arc`, not stored in a model or an ambient global.
  A callback counter remains observable through shared context, so model validation
  must not be treated as isolation of arbitrary context mutations.
* OpenAPI remains an optional bounded schema-component feature. The application
  supplies `/orders` route metadata, registers actual `Order` input and explicit
  `{tenant, slug, order}` response DTO output components. Feature-enabled HTTP
  acceptance compares actual response properties to resolved schema properties and
  roundtrips the actual response through that DTO with context. Component references
  are URI fragments: application traversal must percent-decode the fragment before
  JSON Pointer resolution (initial fixture traversal omitted this; fixed in harness,
  not a library regression). A separate projection fixture covers aliases/output
  exclusions/nonempty runtime-rule pointers and scoped `$id` atomic rejection.
  This is not route inference or full OpenAPI document validation. Published beta.1
  does not have this feature and cannot run those checks.
* Transaction failure in this bounded fixture is a duplicate-order constraint.
  Other SQL errors and setup failures are not mapped into a production error policy.
  The handler's `unwrap` calls deliberately fail acceptance on unexpected errors;
  this fixture is not a hardened deployable service.

No library regression has been confirmed by these checks as of the recorded runs.
Do not use that statement as evidence of external beta usage or exhaustive coverage.

## Reproduction

Use Rust 1.98.1 (the recorded toolchain). This directory is an independent Cargo
workspace. The checked-in manifest and lock select the repository source. Run
`cargo test --locked --features openapi-acceptance` for that default.
Select a different exact dependency without altering application/test sources:

```sh
python3 select-dependency.py registry 0.8.0-beta.1
cargo test
python3 select-dependency.py source /absolute/path/to/crates/rydantic
cargo test --features openapi-acceptance
# After those actual registry releases exist:
python3 select-dependency.py registry 0.8.0-beta.2
cargo test --features openapi-acceptance
python3 select-dependency.py registry 0.8.0
cargo test --features openapi-acceptance
```

Do not enable `openapi-acceptance` against beta.1: its core feature does not exist.
The future version commands are **planned**, not receipts. Cargo.lock should be
saved with each recorded resolution. Switching source/registry changes the lock;
run the shown unlocked resolution once and then use `--locked` for repeat checks.
Do not pair the old registry lock with the source manifest. In each resolution, the exact core/derive registry versions and
package checksums should be audited before stable publication.

The initial source candidate was `e9a5679990bbffda420865839c21fd951b60bd01`.
Its core/derive production files were materialized from GitHub exact blobs by the
parent task; the workspace members were narrowed to those two crates to run this
application. This is reduced-workspace application acceptance, not a complete
checkout integration test or a claim that all main CI has passed.

## Panic-boundary migration probe

The 0.8 business context changes unwind auto-trait behavior. Run the independently
maintained probe with the same selected dependency:

```sh
python3 scripts/context-migration.py --version 0.8.0-beta.1 --output /tmp/rydantic-beta1-migration
python3 scripts/context-migration.py --dependency-path /absolute/path/to/crates/rydantic --output /tmp/rydantic-candidate-migration
```

It distinguishes unsafe-to-assume captured-context unwind compatibility from the
supported pattern that constructs and consumes the context inside the unwind
closure. See its exact command interface and receipts; no blanket
`AssertUnwindSafe` compatibility wrapper is introduced by this application.

## Supplementary migration probes

From this directory, run:

```sh
python3 scripts/context-migration.py --dependency-path ../../crates/rydantic --output results/source-migration
python3 scripts/context-migration.py --version 0.8.0-beta.1 --output results/beta1-migration
```

The positive case executes a safe panic boundary. Each negative case must fail
compilation with the intended diagnostic, not merely any Cargo failure. These
compile checks are migration evidence, not application adoption.

## Published beta.2 follow-up

The [beta.2 registry receipt](BETA2-REGISTRY.md) records actual published-package
application/OpenAPI acceptance, independent repeats and archive verification.
Earlier beta.1/source records above remain historical; stable 0.8.0 commands are
still planned until their own publication and acceptance receipts exist.
