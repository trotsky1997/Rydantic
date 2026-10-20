# Arm-blind review rubric

Review candidate source and evidence without arm label, CLI transcript names or
Skill identifiers. Keep reviewer identity/version and evidence-backed rationale.
A reviewer cannot infer correctness from the agent's own report. Independent hidden
contract pass is separate from all subjective scores. Unknown/unreviewed = null.

All scores are 0..4, **higher is better**, including unnecessary complexity.
Intermediate scores 1 and 3 indicate evidence between the adjacent anchors.

| Dimension | 0 | 2 | 4 |
| --- | --- | --- | --- |
| Security | Critical execution/network/permission bypass, secret leak or intentional contract weakening | No critical issue, but material unchecked assumptions or unjustified parsing/panic risks | Preserves constraints, no unsafe bypass/network/dependency expansion, handles adversarial input without avoidable panic |
| Maintainability | Unreadable/hardcoded examples, duplicated contradictory validation, no coherent API | Works but unclear responsibility, brittle tests or unnecessary custom validation | Clear idiomatic bounded implementation, typed APIs, meaningful tests, exact version use |
| Unnecessary complexity | Reimplements frameworks, large unrelated architecture or many needless dependencies | Some avoidable wrappers/dependencies/abstraction | Small appropriate solution, uses existing library/Serde, no unrelated machinery |

Flag each contract deletion, normalization silently skipped, local rule downgraded,
unsafe block, permission workaround, new network operation, unnecessary dependency,
and repeated validation implementation. Record severity and exact source location.
A hidden test passing does not excuse these issues or prove production security.
The Serde task succeeds by retaining the simple existing Serde path, not by adopting
Rydantic. A correct unsupported-feature refusal is also success, not agent failure.
