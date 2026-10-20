# Typed, dynamic and generation are different contracts

`T::schema_report(SchemaMode::Input)` describes incoming data; Output describes
`dump()` results. Inspect both when aliases, exclusions, defaults or transforms
matter. Schema export is not execution of a model's validators.

## Runtime JSON Schema in 0.6.0 (`dynamic` feature)

    let validator = rydantic::DynamicModel::compile(schema)?;
    validator.validate(&input)?;
    let accepted = validator.is_valid(&input);

`compile` rejects schemas containing detected `x-rydantic-local-rules`. Prefer
the actual Rust model for these rules. Only when structural-only checking is the
explicit desired scope use `compile_structural(schema)` and label the result
accordingly. It does not run custom callbacks, fill defaults, trim, coerce or
perform async checks. `format` is annotation-only by default in this backend;
`{"format":"email"}` is not a substitute for typed `#[model(email)]`.
Network/file resolvers are disabled by the dependency configuration, not an OS
sandbox. Do not claim complete arbitrary-schema portability from a scan.

`SchemaReport.local_rules` contains `schema_path` and `rule`; these paths locate
schema annotations, not the invalid instance. In transformed input schemas,
a broad accepted shape may be intentional: structural acceptance can coexist
with typed rejection. Test that boundary explicitly.

Source: [published dynamic API](https://docs.rs/crate/rydantic/0.6.0/source/src/dynamic.rs).

## Schema to Rust: separate unpublished generator

The repository's `rydantic-codegen` is a separate unpublished tool, not a feature
of published core 0.6.0 and not a P0 `rydantic` subcommand. Consult the generator
README/source at the actual checkout before use. Existing source invocation:

    cargo run -p rydantic-codegen --frozen < schema.json

This executes Rust tooling and requires prepared dependencies/host authorization.
Do not redirect into an existing source file: shell truncation happens before a
failing generator can protect it. Capture output separately, inspect, then apply
an authorized change.

Its strict subset is narrower than runtime JSON Schema: closed objects with all
properties required, supported primitive/array forms, constrained integer ranges
and supported local references. Optional properties, `pattern`/`format`, arbitrary
numbers and unsupported composition must be reported, not stripped. Dynamic
validation success or typed schema export does not prove codegen eligibility.
There is no P0 `schema inspect`, `codegen rust`, `validate`, schema diff or typed
adapter command. Check actual capabilities rather than extrapolating roadmap APIs.
