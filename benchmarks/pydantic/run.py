#!/usr/bin/env python3
"""Verify fixture parity first, then run workers serially and save raw samples."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess
import sys
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def command_output(command):
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True)
    return result.stdout.strip()


def normalized_errors(errors):
    if not isinstance(errors, list):
        raise ValueError("errors must be a list")
    return sorted(({"path": e["path"], "category": e["category"]} for e in errors),
                  key=lambda e: (e["path"], e["category"]))


def verify(case, result, request):
    if type(result.get("accepted")) is not bool or result["accepted"] != case["accepted"]:
        raise ValueError(f"{case['name']}: acceptance mismatch")
    # Python equality equates 1/True/1.0; canonical JSON comparison deliberately does not.
    canonical = lambda value: json.dumps(value, sort_keys=True, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
    if canonical(result.get("output")) != canonical(case["output"]):
        raise ValueError(f"{case['name']}: normalized output mismatch")
    if normalized_errors(result["errors"]) != normalized_errors(case["errors"]):
        raise ValueError(f"{case['name']}: normalized error mismatch: {result['errors']!r}")
    if result.get("iterations") != request["iterations"]:
        raise ValueError("iteration count mismatch")
    expected = request["iterations"] * request["repeats"] if case["accepted"] else 0
    if result.get("checksum") != expected:
        raise ValueError("result consumption checksum mismatch")
    for key in ("samples_ns", "baseline_ns"):
        values = result[key]
        minimum = 1 if key == "samples_ns" else 0
        if len(values) != request["repeats"] or any(type(v) is not int or v < minimum for v in values):
            raise ValueError(f"invalid {key}")


def verify_engine(name, result):
    metadata = result["engine"]
    if name == "rydantic" and (metadata.get("name") != "rydantic" or metadata.get("profile") != "release"):
        raise ValueError("Rydantic must use the release worker")
    if name == "pydantic" and (metadata.get("pydantic") != "2.14.1" or metadata.get("pydantic_core") != "2.50.1"):
        raise ValueError("Pydantic/core differ from pinned benchmark versions")


def source_snapshot():
    paths = set(ROOT.glob("crates/**/*.rs")) | set(ROOT.glob("crates/**/Cargo.toml"))
    paths |= {ROOT / "Cargo.toml", ROOT / "Cargo.lock"}
    paths |= set(HERE.glob("*.py")) | {HERE / "fixtures.json", HERE / "requirements.txt"}
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(paths)}


def worker(command, request):
    process = subprocess.run(command, input=json.dumps(request), capture_output=True,
                             text=True, timeout=600, check=True)
    return json.loads(process.stdout)


def summarize(result):
    values = [value / result["iterations"] for value in result["samples_ns"]]
    q1, _, q3 = statistics.quantiles(values, n=4, method="inclusive") if len(values) > 1 else [values[0]] * 3
    median = statistics.median(values)
    return {"median_ns_per_operation": median, "iqr_ns_per_operation": q3 - q1,
            "min_ns_per_operation": min(values), "max_ns_per_operation": max(values),
            "operations_per_second_from_median": 1e9 / median,
            "baseline_median_ns_per_iteration": statistics.median(result["baseline_ns"]) / result["iterations"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE / "results" / "latest.json")
    parser.add_argument("--iterations", type=int, default=10_000)
    parser.add_argument("--warmup", type=int, default=1_000)
    parser.add_argument("--repeats", type=int, default=9)
    parser.add_argument("--seed", type=int, default=20261025)
    parser.add_argument("--smoke", action="store_true", help="Correctness/runner checks only; not performance evidence")
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    if args.smoke:
        args.iterations, args.warmup, args.repeats = 20, 5, 3
    if not (1 <= args.iterations <= 10_000_000 and 0 <= args.warmup <= 1_000_000 and 1 <= args.repeats <= 100):
        parser.error("iterations 1..10000000, warmup 0..1000000, repeats 1..100 required")
    sources_before = source_snapshot()
    build_command = ["cargo", "build", "--release", "--locked", "-p", "rydantic", "--example", "pydantic_benchmark", "--message-format=json"]
    build = subprocess.run(build_command, cwd=ROOT, capture_output=True, text=True, check=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.strip()]
    artifacts = [item for item in artifacts if item.get("reason") == "compiler-artifact"
                 and item.get("target", {}).get("name") == "pydantic_benchmark" and item.get("executable")]
    if len(artifacts) != 1:
        raise RuntimeError("cargo must report exactly one benchmark executable")
    artifact = artifacts[0]
    if str(artifact["profile"].get("opt_level")) != "3" or artifact["profile"].get("debug_assertions"):
        raise RuntimeError("benchmark requires opt-level 3 release artifact without debug assertions")
    executable = Path(artifact["executable"]).resolve(strict=True)
    if source_snapshot() != sources_before:
        raise RuntimeError("benchmark source changed during build; retry from a stable checkout")
    commands = {"rydantic": [str(executable)], "pydantic": [sys.executable, str(HERE / "python_worker.py")]}
    corpus = json.loads((HERE / "fixtures.json").read_text())
    cases = corpus["cases"]
    jobs = [(case, mode) for case in cases for mode in case["modes"]]
    def request(case, mode, iterations, warmup, repeats):
        return {"schema": case["schema"], "input": case["input"], "mode": mode,
                "iterations": iterations, "warmup": warmup, "repeats": repeats}
    # Fail closed: every engine/mode must match the independent expected corpus.
    verified = []
    for case, mode in jobs:
        req = request(case, mode, 1, 0, 1)
        for engine, command in commands.items():
            result = worker(command, req)
            verify_engine(engine, result)
            verify(case, result, req)
            verified.append({"case": case["name"], "mode": mode, "engine": engine})
    differences_verified = []
    for case in corpus.get("known_differences", []):
        for mode in case["modes"]:
            req = request(case, mode, 1, 0, 1)
            for engine, command in commands.items():
                result = worker(command, req)
                verify_engine(engine, result)
                expected = {"name": case["name"], **case["expected_by_engine"][engine]}
                verify(expected, result, req)
                differences_verified.append({"case": case["name"], "mode": mode, "engine": engine, "expected": expected})
    print(f"Semantic parity passed: {len(verified)} engine/case/mode checks", flush=True)
    if args.verify_only:
        if source_snapshot() != sources_before:
            raise RuntimeError("benchmark source changed during verification")
        return
    rng = random.Random(args.seed)
    rng.shuffle(jobs)
    measurements, execution_order = [], []
    for case, mode in jobs:
        engines = list(commands)
        rng.shuffle(engines)
        for engine in engines:
            req = request(case, mode, args.iterations, args.warmup, args.repeats)
            result = worker(commands[engine], req)
            verify_engine(engine, result)
            verify(case, result, req)
            execution_order.append([case["name"], mode, engine])
            measurements.append({"case": case["name"], "mode": mode, "engine": engine,
                                 "input_utf8_bytes": len(case["input"].encode()),
                                 "input_utf8_sha256": hashlib.sha256(case["input"].encode()).hexdigest(),
                                 "accepted": case["accepted"], "request": req,
                                 "raw": result, "summary": summarize(result)})
    if source_snapshot() != sources_before:
        raise RuntimeError("benchmark source changed during run; refusing to save measurements")
    tracked = command_output(["git", "ls-files", "-z"]).split("\0")
    source_files = {path: digest(ROOT / path) for path in tracked if path and (ROOT / path).is_file()}
    cpuinfo = Path("/proc/cpuinfo")
    report = {"format_version": 1, "purpose": "smoke_only_not_performance_evidence" if args.smoke else "local_microbenchmark",
              "timestamp_utc": datetime.now(timezone.utc).isoformat(), "seed": args.seed,
              "revision": command_output(["git", "rev-parse", "HEAD"]),
              "git_status": command_output(["git", "status", "--porcelain"]),
              "tracked_source_sha256": source_files,
              "build_provenance": {"command": build_command, "stdout": build.stdout, "stderr": build.stderr, "sources_sha256": sources_before, "artifact_profile": artifact["profile"], "executable": str(executable)},
              "harness_sha256": {p.name: digest(p) for p in HERE.glob("*.py")},
              "rust_worker_source_sha256": digest(ROOT / "crates/rydantic/examples/pydantic_benchmark.rs"),
              "cargo_lock_sha256": digest(ROOT / "Cargo.lock"),
              "fixtures_sha256": digest(HERE / "fixtures.json"), "requirements_sha256": digest(HERE / "requirements.txt"),
              "rust_executable_sha256": digest(executable),
              "rustc": command_output(["rustc", "-Vv"]), "cargo": command_output(["cargo", "-V"]),
              "python": sys.version, "platform": platform.platform(), "machine": platform.machine(),
              "cpu_count": os.cpu_count(), "cpuinfo": cpuinfo.read_text() if cpuinfo.exists() else None,
              "affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
              "environment": {key: os.environ.get(key) for key in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "PYTHONHASHSEED", "PYTHONMALLOC"] + ["CARGO_PROFILE_RELEASE_" + name for name in ["OPT_LEVEL", "CODEGEN_UNITS", "LTO", "DEBUG_ASSERTIONS", "OVERFLOW_CHECKS", "PANIC"]]},
              "load_average_at_end": os.getloadavg() if hasattr(os, "getloadavg") else None,
              "verification": verified, "known_differences_verified_not_timed": differences_verified, "execution_order": execution_order, "measurements": measurements,
              "limitations": ["Microbenchmarks, not application/network latency or universal rankings.",
                              "Samples are batch averages, not individual request percentile latency.",
                              "value_copy includes Rust Value clone / Python deepcopy, not pure validation.",
                              "No baseline subtraction. Language loop and allocator overhead remain included.",
                              "Shared host contention and frequency are not controlled by this runner."]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False) + "\n")
    for row in measurements:
        print(f"{row['case']:24} {row['mode']:10} {row['engine']:8} {row['summary']['median_ns_per_operation']:12.1f} ns/op")
    print(f"Raw evidence: {args.output}")


if __name__ == "__main__":
    main()
