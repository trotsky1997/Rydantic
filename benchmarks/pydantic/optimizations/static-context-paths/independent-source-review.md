# Independent static-path review

Result: no correctness blocker found in the two production-file changes reviewed. Performance is not approved or inferred by this review.

## Source reasoning

- The public `PathSegment` type and all serialization are unchanged. Private `ContextSegment` preserves field/index distinction and converts each stored segment back to the original owned type only while recording errors.
- `at` still accepts the same `Into<PathSegment>` inputs. Dynamic strings are owned as before; borrowed compile-time labels enter only through an explicit static API.
- The derive changes pass literal strings already known at macro expansion. Input versus output aliases are unchanged. Index, flatten, extra, and transparent newtype handling remain unchanged.
- `child` clones path and shares application data as before; `child_static` differs only in label ownership. `trial` resets paths; `absorb` appends already absolute diagnostics. Generated async children remain isolated, including cancellation.
- `Context` Debug shape remains the same. Normal-return stack restoration is preserved. Existing lack of unwind restoration is intentionally unchanged; no new panic contract is claimed.
- Native Cap'n Proto and SQL generated literal routes were not changed. They continue to use the dynamic path API; this is not a correctness issue and no performance benefit for those routes is claimed here.

## Independent executed differential

Fresh separate consumers and target directories compare unchanged baseline commit `2ce66b92b281ff5f0e391844bf1ce8cce8cde728` with the candidate. Both consume the same `review.rs`; the baseline static helper uses the original owned API and the candidate static helper uses the new static API. The dependency lock package lists are equal after omitting the distinct probe package names. These are default-feature debug-profile correctness runs, not benchmarks.

57 output records containing 168 full error objects are byte-identical (not normalized). Both output SHA256 values:

`2e866a0fe9391be184297ed284a91df1cbf9df0f8fad01a9fba40874f5130167`

Coverage includes:

- empty, Unicode, NUL, slash/tilde and numeric-string field labels;
- static/dynamic/index nesting, usize maximum index, relative locations, all stages and parameters;
- ordered multi-error `extend`, static/dynamic/None child, trial, absorb, shared typed data and Debug output;
- nested normal returns returning None and root diagnostics after restoration;
- derived input/output aliases, Vec/map paths, flatten, tuple and newtype behavior;
- known/unknown/invalid/missing tagged discriminators, untagged branch errors and extra-field maps;
- borrowed fields and missing/type/length failures;
- direct output/input encoding failures and validation errors;
- nested async children with typed context and cancellation followed by a root error.

The initial review probe incorrectly attempted a tagged tuple variant, which the baseline compiler correctly rejects. The probe was corrected to a supported named variant; `initial-invalid-probe.log` retains that test-authoring error. It is not a repository defect. Final build logs are clean and both runs passed.

## Evidence and boundaries

`review.rs`, frozen baseline/candidate output JSON, manifests, locks, build logs and `provenance.json` are retained beside this report. Provenance contains exact candidate file/diff hashes and toolchain. No candidate files were edited by this reviewer, no GitHub mutation occurred, and all review execution was in the assistant's cloud. No full-workspace CI or allocation/timing result is inferred from these focused runs. All review CPU work is stopped before the parent's timing window.
