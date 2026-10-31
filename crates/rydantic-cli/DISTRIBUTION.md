# Independent CLI distribution

## Status and selected route

The selected first distribution is the **`rydantic-cli` source package on
crates.io**, installing the executable **`rydantic`**. CLI version `0.1.0` is
independent of the Core/derive library versions. No library version is changed.
This implementation prepares packaging and acceptance; it does **not** mean a
registry release, GitHub Release or actual Agent A/B/C trial has occurred.

| Route | Requirements | Trade-off |
| --- | --- | --- |
| crates.io (selected) | Rust/Cargo and native linker; builds only this package and its registry dependencies | One self-contained package and lockfile; no repository clone or Core build after publication |
| GitHub Releases prebuilt binaries (deferred) | Verified artifacts per OS/architecture and platform runtime baseline | Avoids local Rust builds, but adds target matrix, archive/checksum and signing/provenance maintenance |

CLI dependencies are serde, serde_json, toml and semver; no Core, derive, workspace
path dependency or validation engine is linked. Its three knowledge bundles are
embedded, not opened from a checkout at runtime. The packaged protocol schema,
license, tests and lockfile travel with the source. Rust `1.98.1` is the tested
build toolchain; no lower MSRV is claimed. Linux, macOS and Windows package CI
is configured; only completed checks for the actual commit establish platform
verification, not the presence of a matrix entry.

## Installation after an explicitly verified registry release

**Not an available release announcement:** execute the following only after
maintainers publish and independently verify `rydantic-cli 0.1.0` and record the
receipt. A missing package, unrelated owner or checksum mismatch is a stop, not
a reason to install a similarly named package.

```sh
cargo +1.98.1 install rydantic-cli --version '=0.1.0' --locked --registry crates-io --bin rydantic
rydantic capabilities --format json --protocol-version 1
```

The default executable location is `$CARGO_HOME/bin` (usually `~/.cargo/bin`);
Windows normally uses `%USERPROFILE%\.cargo\bin`. Alternatively select an empty
installation directory with `--root DIR`, then run `DIR/bin/rydantic` (or
`rydantic.exe`). Do not use `--force` to overwrite an unrelated `rydantic` binary.
Building/installing executes compiler and dependency build code and requires
explicit installation authorization. The installed CLI itself remains offline,
read-only and does not execute project code. `--locked` uses shipped dependency
versions; it does not promise future security updates or a hermetic build.

Expected capabilities include `schema_version: "rydantic.cli/v1"`, tool version
`0.1.0`, knowledge versions `0.6.0`, `0.7.0`, `0.8.0`, and null linked engines.
Unknown versions remain unsupported/conservative. This is not proof of resolved
features or successful validation in a consumer's application.

## Prepublication acceptance

From the reviewed checkout, with Python 3.12+ and the tested Rust toolchain:

```sh
cargo package -p rydantic-cli --locked
python3 scripts/check_cli_distribution.py \
  --archive target/package/rydantic-cli-0.1.0.crate \
  --receipt cli-package-receipt.json
```

The checker extracts only the `.crate`, validates normalized metadata and
registry-only lock entries, tests the **shipped** contracts, and installs that
extracted source using `cargo install --path ... --locked` into a new root. By
default it uses a fresh Cargo home and target directory. It then deletes the
extracted source and target and runs the installed executable from an empty
working directory with empty PATH and isolated HOME/CARGO_HOME. It checks all
three knowledge tracks, unknown-code redaction and unknown-version rejection.
The receipt includes the exact package SHA-256 and records that Agent trials
were not executed. This is **not a registry install** and does not establish
registry ownership, propagation or release availability. `--cargo-home DIR
--offline` explicitly uses a prepared dependency cache and is recorded as such;
it must not be described as a fresh download.

`.github/workflows/cli-package.yml` builds and tests packages on Linux/macOS/
Windows with read-only repository permission. Its CI artifacts are build evidence,
not an official GitHub Release or a promise of general binary compatibility.
Existing library publication and safety workflows are unchanged.

## Publication gates (pending)

Use independent tag `cli-v0.1.0`, not a library `v0.8.0` tag. Before any tag or
upload: review/merge the implementation, verify current main CI **and** CLI package
CI, inspect exact package contents/checksum and release notes, and recheck
crates.io name availability/ownership and version absence. A 404 means absence
at that moment, not a reservation; network/auth errors are not absence.

`scripts/release_cli.py check-manifest --tag cli-v0.1.0` checks local metadata.
Its `preflight` command is read-only and fails closed on missing ownership, an
existing version or unsuccessful main CI. The first publication needs an
explicitly authorized owner action and suitable crates.io authentication. The
existing Core/derive trusted-publisher registration does **not** automatically
authorize a new CLI package/workflow. No credentials, new trusted publisher,
permission expansion or automated upload is installed by this change.

After authorized publication, run from a fresh verification environment:

```sh
python3 scripts/check_cli_distribution.py --registry --version 0.1.0 \
  --receipt cli-registry-receipt.json
```

This uses crates.io by name/version, not `--path` or `--git`, and repeats the
isolated runtime contract checks. Confirm owner, published archive checksum and
non-yanked registry/index entry separately before announcing availability.
Do not retry an uncertain upload; establish registry state first. Record actual
registry acceptance in Issue #91. Real isolated Agent A/B/C efficacy evaluation
remains independent and unfinished even after successful distribution.
