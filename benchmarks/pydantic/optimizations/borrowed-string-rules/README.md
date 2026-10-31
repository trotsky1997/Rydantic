dot · AI 助手

# Borrowed string rules without temporary owned strings

Refs #134. This is a six-line specialization on top of repaired PR #131 at
`cb116f17599caa4697e07a0cefa68395f1c4c56a`, not a replacement for the earlier
integer/path/slot changes and not a claim to close the Pydantic latency gap.

`BorrowType<&str>` and `Cow<str>` previously inherited generic rule validation:
encode to `Value::String`, then invoke string rules. Both now forward directly to
the same shared `Rules::validate_string` routine already used by owned `String`.
Normalization, decoding, output serialization, callbacks and wrapper forwarding
are unchanged. Full error content/order and Unicode scalar length semantics remain.

## Allocation-only result

The unchanged standalone consumer measures warmed rule validation only, using
preconstructed input, rules and owned String controls. All 16 before/after rows
preserve complete serialized errors/display or success, and every five measured
samples match exactly. It does not measure parsing, normalization or elapsed time.

| Case | Current #131 calls / requested bytes | This change |
| --- | ---: | ---: |
| 1,000 valid six-byte ASCII string rules, `&str` | 1,000 / 6,000 | 0 / 0 |
| Same, `Cow::Borrowed` | 1,000 / 6,000 | 0 / 0 |
| Same, `Cow::Owned` | 1,000 / 6,000 | 0 / 0 |
| 1,000 valid six-byte Unicode string rules, each borrowed type | 1,000 / 6,000 | 0 / 0 |
| One invalid one-byte length check, each borrowed type | 9 / 1,103 | 8 / 1,102 |
| Preconstructed `String` success control | 0 / 0 | 0 / 0 |
| Empty string control | 0 / 0 | 0 / 0 |

The `empty_zero_min` case uses an explicit zero minimum as an empty-string clone
control. Requested bytes
are cumulative allocator requests, not live/peak memory or RSS. Output disposal
and formatting are outside counters. `black_box` input/result consumption plus
full outcomes prevent treating the calls as unused results.

**Canonical collection100 uses owned String and is unaffected.** All 77 canonical
and synthetic case/mode responses from the whole-#131 allocation probe retain
identical outputs and cold/warm counters after this change. This separate borrowed
string improvement must not be presented as reducing collection100's 211 calls.
No timing campaign or A/A retry occurred; prior shared-host stability failures
remain a reason not to claim latency superiority.

## Verification

- Eight string-rule tests pass against both unchanged `ad183a40` and the fixed
  runtime. Three new tests cover complete prior errors, static/dynamic nested
  paths, aliases, Option/Box/None forwarding, Unicode min/max, trim, borrowed
  pointers, escaped owned Cow strings, and callback order/error gating.
- Final post-repair default core and derive tests: **213 passed, 0 ignored**, 45 successful
  suite summaries, including compile tests and documentation tests. The eight
  root-validation regressions from #137 are retained unchanged and pass.
- Independent review reran string rules plus borrowed contracts: **16 passed**;
  independently replayed the targeted allocation binary and compared all results.
- Core/derive all-target default-feature Clippy with warnings denied, no-default
  library check, formatting and diff checks pass.
- This local pass is not a full workspace/all-features pass; exact-head remote CI
  is a separate gate. An initial diagnostic calling a private method did not
  compile; the public existing String trait control replaced it. Initial test
  Clippy rejected redundant Box<&str>; the model now uses Box<Option<&str>>
  without suppression. Both failures are retained in local logs, not counted as
  passing runs.

## Reproduce

Use the manifest template for two standalone packages, replacing REPOSITORY_ROOT
with exact #131 and candidate source checkouts. Copy `allocation_probe.rs.txt` to
`src/main.rs`, and `allocation-probe.Cargo.lock` to `Cargo.lock`. The historical
package name in the template/lock does not change the probe's rules-only scope.
With Rust 1.98.1 / LLVM 22.1.8 on x86_64-unknown-linux-gnu:

```sh
cargo fetch --locked
cargo build --release --locked --offline
./target/release/fixed-model-slots-allocation-probe > output.json
```

Both post-repair builds use identical probe/lock/compiler identities. Before
publication, the repaired #131 baseline and candidate were freshly rebuilt; all
77 canonical/control responses match their complete earlier #131 outcomes and
counters, and all 16 targeted rows retain identical outcomes between builds. Each output contains
16 rows with five samples; compare all outcomes and all unchanged String controls,
not just allocation improvements. The probe's name `owned_string_rule_control`
means the already-existing String implementation, not a modified-library bypass.

[Full before/after samples](results.json.gz) include synthetic inputs and complete
outcomes. [Manifest](manifest.json) records exact base, changed runtime/test source,
compiler, probe, lock, executable and compressed/decompressed artifact hashes.
The candidate source is bound by its exact changed-file hashes; publication commit
identity is separately verified against the audited tree. All execution used dot's
own cloud. Artifacts contain no private absolute paths, credentials, production
inputs or user-computer data. Gzip timestamp is zero with no filename/comment.
