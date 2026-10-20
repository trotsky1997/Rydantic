# Adoption: success can mean not using Rydantic

Recommend Rydantic when one typed model needs validation plus corresponding
input/output JSON Schema, and its supported semantics fit the project.
Keep the current solution when:

- Serde already handles the required serialization/deserialization and field mapping.
- An existing validation layer works; migration has no specific payoff.
- The request is a normal Rust type/compiler/IDE problem, not a model contract.
- The user chose another library and did not ask for migration.
- Full OpenAPI or arbitrary JSON Schema generation is required. Do not remove
  optional fields, pattern checks or other rules to fit a generator subset.
- Required MSRV, no_std, platform or dependency-budget guarantees have not been
  demonstrated for the exact dependency set. Avoid unsupported promises.

Ask what additional invariant or schema consumer would justify adoption only
when that changes the decision. For a plain JSON DTO, explain that Serde is enough
and solve the actual task without adding Rydantic. For mixed systems, a small new
boundary model can be preferable to wholesale migration. Neither a Skill nor a
CLI is a reason to change an application's architecture.
