# Rydantic / Pydantic benchmark

A reproducible **workload comparison**, not a claim that either library is universally
faster. Rust release builds and Pydantic's compiled Rust core run in separate,
sequential processes on the same machine. The runner checks semantics against an
independent fixture oracle before recording timings. See issue #115.

## What is comparable?

The declared models use strict settings, forbid extra fields, and have no coercion, normalization,
defaults, aliases or custom callbacks. `Flat` has an integer ID in `[0, 1000000000]`,
a 1–64 Unicode-code-point name and a boolean. `Nested` contains a Flat and string
list; `Collection` contains a list of Flats. Fixtures cover success, nested/list
inputs and type, missing, extra, bounds, length and syntax failures. Accepted
outputs must match the fixture exactly; rejected inputs must match normalized
error categories **and paths**, not merely throw an exception. Native error
messages and error object formats intentionally remain different.

- **`json` (primary):** the same UTF-8 JSON text enters `Model::parse_json` and
  `BaseModel.model_validate_json`. Includes JSON parsing, validation, model/error
  construction and disposal. Does not include printing/rendering errors.
- **`value_copy` (secondary):** a decoded immutable JSON tree is prepared once;
  each operation copies it then validates. Rydantic consumes `Value::clone()`;
  Python uses `copy.deepcopy()` followed by `model_validate()`. This is **copy +
  validation**, not pure validation: Python can reuse immutable string/int
  objects, while Rust ownership and allocation costs differ. Do not use this
  secondary metric as evidence for parser performance.
- Serialization is intentionally **not** compared: Rydantic `dump` revalidates;
  Pydantic `model_dump` does not provide that same contract. Canonical output
  generation is only part of the untimed correctness check.

The gate also verifies known differences without timing them: Rydantic accepts
an integral decimal JSON number (`42.0`) as integer `42`, whereas Pydantic strict
mode rejects it. The comparable corpus uses integer-encoded valid IDs and a
nonintegral decimal rejection case; this distinction must not be hidden.

This is a limited JSON-compatible schema subset. It does not establish general
library equivalence. Duplicate keys, nonfinite numbers, general arbitrary-precision integer semantics, custom types/callbacks, async work, schema generation, database/RPC
adapters, startup, model compilation and memory use are not performance claims
of this suite. Different invalid-input diagnostic machinery remains real work;
error-heavy results are not comparable to success throughput. Native rejection costs
are not identical diagnostic work: Rydantic does not retain raw input, while
Pydantic may. Error normalization/rendering is outside the timed section.
The same JSON text is a Python Unicode string versus a Rust UTF-8 string; input
I/O and host string decoding are outside timing, and UTF-8 fixture bytes are hashed.

## Reproduce

From the repository root, with Rust 1.98.1 and CPython 3.12 available:

```sh
python3 -m venv .venv-benchmark
.venv-benchmark/bin/python -m pip install -r benchmarks/pydantic/requirements.txt
.venv-benchmark/bin/python benchmarks/pydantic/run.py --verify-only
.venv-benchmark/bin/python benchmarks/pydantic/run.py --output /tmp/benchmark.json
```

Exact Python dependencies are pinned. Cargo uses the checked-in lockfile. The
harness records runtime/toolchain, source revision and dirty state, input/source
and binary hashes, platform and CPU details, run order/seed, settings and raw
sample totals. Preserve the JSON output when sharing a report. A dirty checkout
is not identified by its base commit alone. Do not edit source after building:
rebuild the worker before every run.

Use an idle machine, no concurrent builds/tests, consistent power settings and
CPU affinity where available. Disable unrelated services yourself only where
authorized. Cloud virtualization, scheduling, thermal state and other tenants
cannot be fully controlled; record that limitation. The runner serializes engine
processes and varies their order, but does not guarantee a dedicated host.

## Reading results

Imports, process startup, class/schema setup, fixture loading, correctness checks
and report formatting are outside timing. Warmup precedes repeated timed batches.
Rust uses `std::hint::black_box` and consumes results; Python consumes results too.
Both dispose successful objects within the loop. Loop baselines are reported as
context, **not subtracted**: their bookkeeping is not identical to the full work.
Python cyclic garbage collection remains enabled and is recorded.
The runner rebuilds the Rust worker in release mode before checking correctness,
records the build command/output, and rejects source changes during the run.

Each sample is total batch wall time divided by operations. Median and dispersion
summarize those **batch-average** ns/op; these are not individual-request p95/p99
latency. Throughput is the reciprocal of median ns/op, not network concurrency or
end-to-end service throughput. Do not average unlike fixtures into a single
winner. Short smoke runs prove wiring only; use larger iteration counts/repeats
and inspect variability for performance analysis. Fixture size and rejected-input
position matter. No timing threshold gates CI, and no baseline result implies a
regression without controlled repeated measurements.

## References

- [Pydantic strict mode](https://pydantic.dev/docs/validation/latest/concepts/strict_mode/)
- [Pydantic performance guidance](https://pydantic.dev/docs/validation/latest/concepts/performance/)
- [Pydantic 2.14.1 on PyPI](https://pypi.org/project/pydantic/2.14.1/)
- [Rust `black_box`](https://doc.rust-lang.org/std/hint/fn.black_box.html)
