# Rydantic 0.2.0 validation record

Date: 2026-10-19 (Asia/Shanghai). Local environment: macOS, rustc 1.98.1, Cargo 1.98.1.

This record documents the 0.2.0 release. Later releases add coverage described in the README and their release notes.

## Automated checks

- `cargo test --workspace --all-features --locked`: passed. The 92 functional tests cover 27 original model behaviors, 11 original schema tests, 5 dynamic schema tests, 19 additional model combinations, 11 asynchronous/streaming tests, 3 date/Decimal tests, 2 schema-path tests, 3 raw JSON input tests, 10 native Cap'n Proto tests, and 1 procedural-macro field-mapping check. One additional official CLI interoperability test requires the system compiler; it was explicitly run locally and passed.
- There is also 1 documentation test and 1 compile-test harness. The harness checks 11 failure cases: escaping lifetimes, contradictory lengths, duplicate aliases, missing extra-field storage, invalid extra-field storage types, invalid regexes, mutation of read-only wrappers, conflicting tag/untagged attributes, excluded tuple fields, unknown attributes, and incompatible field types and rules.
- The original schema boundary matrix checks 1,400 inputs; the independent `jsonschema` validator checks 103 original successful outputs. Additional cases cover tuples/untagged enums, borrowed recursive and generic references, optional flattening, flattened defaults, key collisions, and date/Decimal input and output schemas. Path regressions verify that rule locations containing spaces, Chinese characters, percent signs, and JSON Pointer escapes can be resolved within schemas. Generated `$ref` values continue to use URI-fragment encoding.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo check -p rydantic --no-default-features --lib --locked`: passed.
- `cargo check -p rydantic --no-default-features --features chrono,decimal,capnp --lib --locked`: passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked`: passed.
- `cargo fmt --all -- --check`: passed.

## Behavioral evidence

- Borrowing tests directly check that `&str` addresses fall inside the input buffer. They verify that trim preserves references, ordinary Cow strings remain borrowed, and escape decoding or case conversion produces owned values. Coverage also includes multiple lifetimes, recursion, generics, flattening, extra fields, defaults, and Serde wrappers.
- Integer tests cover `i128::MIN`/`u128::MAX`, mathematical integers, overflow, and extremely long exponents. Decimal tests cover exact thresholds, 96-bit coefficients, 28-digit scale, scientific notation, and rejection of precision loss.
- Date tests cover leap days, invalid dates/times, time-zone conversion, inputs exceeding nanosecond precision, and directly constructed Rust values with out-of-range years or second-level time-zone offsets. Invalid typed defaults return structured errors.
- Asynchronous tests await futures that actually suspend on the Tokio runtime and verify Send with `tokio::spawn`. They check rejection by parent models' synchronous entry points, independent field-error collection, cross-field callbacks, skipping asynchronous checks after synchronous failures, and Context path restoration when a future is cancelled.
- Streaming tests use a reader that returns one byte per read, verifying that the first record or array callback occurs before EOF. They cover early termination by callbacks, termination after JSON-sequence syntax errors, recovery on the next NDJSON line, array error indices, and retention of borrowed array values.
- Raw-input tests define a custom field type that panics if `decode(Value)` or `encode` is called. JSON, slice, Reader, Serde, streaming, and asynchronous parsing pass through nested models, Vec, maps, arrays, Option, collection constraints, and typed defaults. Separate Box<RawValue> cases verify preservation of nested source text, whitespace, and `1e+100` notation.
- Cap'n Proto was verified with the official `capnp 0.27.2` runtime, `capnpc 0.27.0` generator, and system `capnp 1.5.0` compiler. Tests cover standard/packed messages, official Readers/Builders, nested structs, List/Text/Data, fixed arrays, optional unions, native enums, data unions/groups, maps, date/Decimal text, exact u128 text, integer-width conversions, asynchronous validation, error paths, malformed/truncated/trailing messages, traversal limits, and unaligned slices.
- The official CLI decodes Rydantic's Person message and checks Text/Enum fields. Rydantic reads CLI-encoded standard and packed `u64::MAX` messages. Running `cargo test -p rydantic-capnp-fixture --test native -- --include-ignored` passed all 11 tests locally. Additional checks cover the 29-bit List length limit and rejection of duplicate native field mappings by the procedural macro.

## Executed examples

- `borrowed`: parses a borrowed name, a flattened address, and extra Cow fields. It checks borrowed ordinary strings and owned escaped strings, then emits complete JSON.
- `streaming`: processes valid, invalid, and valid array records in sequence. NDJSON continues delivering valid records after a malformed line.
- `asynchronous`: awaits validation and serializes `AsyncValidated`, checking that the taken branch returns a structured error.
- `dates_decimal --features chrono,decimal`: preserves `12345678901234567890.123456789` and converts a `+08:00` time to UTC.
- `rydantic-renamed-fixture`: uses `rd` as its dependency name; the procedural macro works through re-exported dependencies and emits `{"text":"hello"}`.
- `rydantic-capnp-fixture`: encodes the complete example model as a 432-byte official native message and compares every field after parsing. Its output is `Native Cap'n Proto: 432 bytes, Alice`.

## CI and scope

The [CI workflow](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml) uses the same toolchain on Ubuntu and macOS for full tests, Clippy, formatting, feature checks, documentation builds, tool-argument examples, and additional examples. For 0.2.0, the Cap'n Proto fixture includes committed official bindings that ordinary builds can reuse; Ubuntu also installs the official CLI for interoperability tests. The results above describe local validation. Remote run statuses are available at the workflow link.

Version 0.2 covers the eight follow-up areas originally listed in the README. Borrowed fields reference the raw input buffer, readers use synchronous I/O, and sequence/array streaming entry points retain the current record. Schema metadata exposes local rules for dates, Decimal, and custom functions. See the README for entry points and composition semantics.
