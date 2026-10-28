# Retained instability evidence and next-method design

No third full-corpus performance repeat was performed. Both raw runs, all slower medians, and
byte-identical candidate controls are retained. Allocation counts are convincing;
these timings do not establish a clean no-regression result.

## Observed, not inferred from code changes

Initial overall UTC window: 2026-10-26 04:41:26.384314–04:42:58.787748.
One-/five-/fifteen-minute load averages: start 1.626/10.524/9.295,
end 1.179/8.058/8.530.

Confirmation overall UTC window: 04:44:15.296415–04:51:32.092852.
Load averages: start 0.379/6.280/7.870, end 1.151/2.296/5.307.
These are system load observations, not proof of an idle or dedicated CPU.

Selected **process-position** medians from the confirmation, ns/op:

| Case | Engine | First position | Second position |
|---|---|---:|---:|
| nested_valid/json | identical candidate control | 1,677.7 | 2,518.1 |
| nested_valid/json | candidate | 2,087.3 | 1,974.8 |
| flat_wrong_type/json | identical candidate control | 2,752.7 | 1,021.3 |
| flat_wrong_type/json | candidate | 1,082.4 | 2,331.0 |
| flat_wrong_type/json | Pydantic | 3,797.2 | 1,764.8 |
| collection_100/json | Pydantic | 190,419.2 | 70,143.9 |
| collection_100/json | identical candidate control | 106,859.8 | 89,041.1 |

One Pydantic wrong-type batch measured 31,858.7 ns/op, versus an ordinary
same-run position median around 1,765 ns/op. The identical candidate/control
executable SHA256 is
`1e9806eaa9fe6fa1ae381c43016134add27d239bda68b0b83fa29c7fa4beee9e`.
Differences therefore span engines and time positions; an algorithm-only causal
explanation is insufficient. This does **not** prove every slower candidate row
is harmless noise, identify a specific scheduling cause, or justify dropping rows.

## Missing telemetry and limits

Only whole-run timestamps, start/end load averages, process order and batch wall
times were recorded. No per-process timestamps, CPU-time counters, migration,
frequency, throttling or context-switch telemetry was captured. The driver did
not set affinity. A later read-only observation showed 9 CPUs, allowed affinity
0–8; this is not a historical worker-placement trace. cgroup CPU statistics and
CPU-frequency policy files were unavailable. No system settings were changed.

Longer contiguous batches alone did not eliminate instability. The confirmation
also reverses process order and retains every row, rather than selecting only
initial regressions or replacing the original run.

## Distinct predeclared diagnostic — executed once, failed and stopped

1. Pin only the benchmark's spawned child processes to one verified allowed CPU;
   record the chosen affinity in each worker. No machine-wide scheduler,
   governor, security or service changes; a shared host remains shared.
2. Use short, balanced, interleaved baseline/candidate/identical-control blocks,
   rather than several contiguous batches from each engine. Predeclare the full
   row/order/batch schedule and stopping condition before observing results.
3. Record wall-clock start/end and process CPU time, voluntary/involuntary context
   switches, binary hashes and inherited affinity around each child. Keep model
   startup/warmup outside the primary worker's timed section, and label whole-
   process CPU receipts as diagnostics because they include setup.
4. Inspect identical-control dispersion and time trends first. Retain all
   results, including any failure of the method to stabilize measurements. Do
   not tune until a favorable median appears.

The parent approved one execution with precise gates before observing results.
The diagnostic flag `sources_and_binaries_unchanged` checks its driver, fixture
and executable hashes only; it is not a fresh full runtime-source snapshot. Those
hashes were unchanged. Only spawned child affinity was set to CPU 0, asserted in
pre-exec before loading the worker; it was not continuously sampled inside timed
loops. No system settings changed. Cases used
25,000/1,000 timed/warmup operations for nested_valid, 50,000/1,000 for
flat_wrong_type, and 500/50 for collection_100, one timed batch per child. Each
case ran 16 alternating ABBA/BAAB blocks (32 samples/alias; 192 total children).

PASS required **every** case to have aggregate A/B medians within ±2%, IQR of
16 within-block ratios ≤5 percentage points, and ≥14/16 ratios within ±5%.

| Case | Aggregate A/B ratio | Block-ratio IQR | Blocks within ±5% | Gate |
|---|---:|---:|---:|---|
| nested_valid | 0.996276 | 12.340 percentage points | 4/16 | FAIL |
| flat_wrong_type | 1.010464 | 7.758 percentage points | 9/16 | FAIL |
| collection_100 | 0.978708 | 10.209 percentage points | 7/16 | FAIL |

One nested A/A block ratio was 3.882. All 192 child processes succeeded and
matched the fixture oracle, so this is a failed **measurement-stability** gate,
not a validation failure. The conditionally allowed paired phase did not run;
no core reselection, threshold relaxation, exclusions or retries occurred.

Separate whole-child wall/CPU median ratios were 1.020/1.016/1.019 (maxima
1.137/1.028/1.045 for the three cases); involuntary context-switch totals were
70/116/155. These receipts include startup/setup/warmup and are not substituted
for worker batch latency. They do not establish simple wall-only descheduling as
the sole cause; frequency, contention and migration history remain unproven.

Pinning and short interleaving did **not** resolve repeatability here. A suitable
controlled environment may be needed before interpreting the modest latency
changes. Correctness/allocation conclusions remain separate from latency.
