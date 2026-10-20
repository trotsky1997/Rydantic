# Diagnose without weakening the contract

For stable typed errors, `ValidationErrors::errors()` returns errors with `loc`,
`code`, `message`, `stage`, `params`; `to_json()` returns the serialized **array**,
not a CLI envelope. A `ValidationError::pointer()` escapes `~` as `~0` and `/` as
`~1`. Array indices and object keys remain distinct in `loc`. `stage` is parse,
validate or serialize. Dynamic errors use `json_schema` and may carry
`params.schema_path`, which is not the instance pointer.

- `missing`: inspect requiredness, aliases and actual missing keys; do not add a
  default solely to silence a fixture that should reject missing input.
- `greater_than_equal` / `less_than_equal`: test exact bounds and neighbors.
- `too_short` / `too_long`: inspect post-normalization text and Unicode length.
- `extra_forbidden`: identify an unexpected field or wrong alias; ignoring extras
  is a deliberate contract change, not the default fix.
- `email`: use typed email validation; standard schema format may only annotate.
- `custom`: inspect the actual callback. Its message/params can contain secrets;
  structured output does not automatically make custom diagnostics safe to share.
- Unknown codes: keep them unknown. Do not invent an API or explanation.

For compiler errors, use spans and compiler diagnostics to find missing imports,
feature gates, unsupported attributes or callback type mismatch. Keep compilation
failures separate from test rejections and from offline dependency/toolchain
failures. Confirm an exact-version API before suggesting a replacement method.

An available P0 CLI can explain known errors without running a model:

    rydantic explain greater_than_equal --core-version 0.6.0 --format json --protocol-version 1

For saved errors, explicitly select their actual input format with
`explain --diagnostics FILE --input-format validation-errors --core-version 0.6.0`.
Consult that CLI's help for envelope/Cargo formats. A successful explanation is
not successful validation, and the CLI cannot infer arbitrary business logic.
