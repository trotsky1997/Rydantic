# rydantic-codegen (unpublished developer tool)

Generate Rust models from a **deliberately limited JSON Schema 2020-12 subset**.
This is not an OpenAPI/Protobuf generator or a drop-in replacement for
`datamodel-code-generator`. The crate has `publish = false` and is not included
in the current beta publication.

```sh
cargo run -p rydantic-codegen --locked < schema.json > models.rs
```

The CLI reads only stdin and writes only stdout. It never resolves a URL or
opens a referenced file. It accepts no command-line arguments, rejects duplicate
JSON object keys, and emits no source until the complete schema passes checks.
Shell redirection can still truncate an existing destination before the CLI
runs; generate into a new temporary file and replace your destination only after
success. The library API is `generate(&serde_json::Value) -> Result<String, Error>`.

Generated source has a public `RootModel` and requires `rydantic` with `derive`
enabled. Place it in its own module. Every generated model derives
`rydantic::Model`; validation happens via the ordinary `Model::parse_*` APIs.
Public fields have deterministic `field_0`, `field_1`, ... names and explicit
wire-name aliases. Types and variants use positional names too. This avoids Rust
keywords, normalization collisions, or schema-supplied executable code, but is
not a hand-designed ergonomic domain API. All schema strings used as literals
are escaped Rust strings. Ordering follows sorted schema property/definition
keys and enum array order.

## Accepted subset

- `$schema` may be absent (2020-12 is assumed), or exactly
  `https://json-schema.org/draft/2020-12/schema`.
- Root-level `$defs`, with direct `#/$defs/name` references. JSON Pointer `~0`
  and `~1` escaping is supported. Targets are expanded, not fetched. Every
  definition, including unused definitions, is checked. Recursive references,
  percent-encoded references, and reference assertion siblings are rejected.
- `type: object`, explicit `properties`, explicit `required` containing **every**
  property, and explicit `additionalProperties: false`. Nested objects work.
- `type: array` with a homogeneous supported `items` schema, and optional
  `minItems` / `maxItems`.
- `type: string`, optionally `minLength` / `maxLength`, measured in Unicode scalar
  values by Rydantic. String enums are supported instead of length constraints.
- `type: boolean` and `type: null`.
- A two-element type array consisting of one supported type and `null`, except
  nullable enums. Nullable required properties accept null but reject absence.
- Bounded `type: integer` with explicit i64-representable integer `minimum` and
  `maximum`. Neither coercion from strings nor floating-point storage is used.
- String `title` and `description` annotations are accepted but do not affect
  generated identifiers or validation.

Schema lengths must be nonnegative integer-form JSON numbers no larger than
`u32::MAX`. Schema length/integer bounds with decimal or exponent notation
(e.g. `1.0` or `1e0`) are rejected, even if mathematically integral. This is a
schema-subset restriction, not a restriction on integer data notation.
Contradictory length/numeric bounds are rejected rather than generating an
uninhabitable Rust model. Input is limited to 64 KiB of JSON (library checks its
compact serialization), expanded schemas to 256 nodes and 32 nesting levels, JSON traversal to 8192
values and 128 levels, and enums to 64 variants (to compile under Rust’s default macro recursion limit).

## Deliberate rejections and semantics

Property and definition names `$serde_json::private::Number` and
`$serde_json::private::RawValue` are unsupported because they overlap parser
reserved member names; CLI parsing may reject them before schema validation.

Unknown keywords fail closed with a schema JSON Pointer; the root pointer is the
empty string (`#` in display). This includes optional properties, defaults,
`format`, `pattern`, compositions/conditionals, numeric `multipleOf`, arbitrary
`number`, unbounded `integer`, tuple arrays, additional-property schemas, remote
or file references, and code-generation extensions. No assertion is silently
ignored. Optional-but-nonnullable properties are not mapped to `Option<T>`:
absence and explicit null are different JSON Schema cases.

Typed dumping is **not byte- or lexical-preserving**: integer `1.0` / `1e0`
normalize to `1`; object order is not an API guarantee. Generated structs also
expose public fields, so direct Rust construction is not proof of validation.

## Verification

Tests regenerate a checked-in fixture and compile that exact emitted Rust code,
then compare source `DynamicModel` and generated `Model` acceptance on synthetic
nested/missing/null/extra-property/enum/length/numeric boundary cases. Tests cover
Rust keyword/collision names, escaping, local refs, unsupported-keyword paths,
duplicate schema keys, and resource limits. The fixture is intentionally not a
claim of complete JSON Schema or universal arbitrary-precision validator parity.
In particular, the underlying jsonschema 0.33 validator can round long decimals
near integers during integer assertions; a separate exact expected-outcome
corpus checks integer boundaries and fractional rejection rather than treating
DynamicModel as an arbitrary-precision oracle.

```sh
cargo test -p rydantic-codegen --locked
cargo clippy -p rydantic-codegen --all-targets --locked -- -D warnings
```
