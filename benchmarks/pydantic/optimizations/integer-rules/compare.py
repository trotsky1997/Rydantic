#!/usr/bin/env python3
"""Counterbalanced four-engine comparison; consume fresh build manifests, never build during timing."""
import argparse
import atexit
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import random
import subprocess
from common import (ENGINES, RUST_ENGINES, artifact_from_receipt, canonical_harness,
                    digest, engine_kind, load_json, orders, request, snapshot, write_json)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for engine in RUST_ENGINES:
        parser.add_argument(f"--{engine}-build", type=Path, required=True)
    parser.add_argument("--pydantic-python", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=10000)
    parser.add_argument("--warmup", type=int, default=1000)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--rounds", type=int, default=2)
    parser.add_argument("--seed", type=int, default=20261026)
    args = parser.parse_args()
    partial = args.output.with_suffix(args.output.suffix + ".incomplete")
    if args.output.exists() or partial.exists():
        raise ValueError("refusing to overwrite existing evidence")
    if not (1 <= args.iterations <= 10000000 and 0 <= args.warmup <= 1000000 and
            1 <= args.repeats <= 100 and 1 <= args.rounds <= 20):
        parser.error("invalid iteration/warmup/repeat/round bounds")
    builds = {engine: load_json(getattr(args, engine + "_build")) for engine in RUST_ENGINES}
    for engine, build in builds.items():
        artifact_from_receipt(build["receipt"], build["executable"])
        if digest(build["receipt"]) != build["receipt_sha256"] or digest(build["log"]) != build["log_sha256"]:
            raise ValueError(f"{engine}: build evidence changed")
        if snapshot(build["root"]) != build["source_snapshot"] or digest(build["executable"]) != build["executable_sha256"]:
            raise ValueError(f"{engine}: source/binary no longer matches build manifest")
    if builds["baseline"]["source_snapshot"] != builds["control"]["source_snapshot"]:
        raise ValueError("baseline/control are not rebuilt from identical source snapshots")
    for key in ("rustc", "cargo", "environment", "profile"):
        if any(build[key] != builds["baseline"][key] for build in builds.values()):
            raise ValueError(f"Rust builds differ in {key}")
    for path in ["Cargo.lock", "benchmarks/pydantic/fixtures.json", "benchmarks/pydantic/requirements.txt",
                 "benchmarks/pydantic/run.py", "benchmarks/pydantic/python_worker.py", "crates/rydantic/examples/pydantic_benchmark.rs"]:
        if len({build["source_snapshot"][path] for build in builds.values()}) != 1:
            raise ValueError(f"workers do not share {path}")
    root = Path(builds["candidate"]["root"])
    harness = canonical_harness(root)
    python = args.pydantic_python.resolve(strict=True)
    commands = {engine: [build["executable"]] for engine, build in builds.items()}
    commands["pydantic"] = [str(args.pydantic_python.absolute()), str(root / "benchmarks/pydantic/python_worker.py")]
    fixture_text = (root / "benchmarks/pydantic/fixtures.json").read_text()
    fixtures = json.loads(fixture_text)
    jobs = [(case, mode) for case in fixtures["cases"] for mode in case["modes"]]
    if len(jobs) != 49:
        raise ValueError("expected canonical 49 case/mode pairs")
    tools = {p.name: digest(p) for p in Path(__file__).parent.glob("*.py")}
    report = dict(format_version=1, purpose="paired integer-rule optimization workload comparison",
                  builds=builds, commands=commands, python_executable=str(python), python_executable_sha256=digest(python),
                  python_version=subprocess.check_output([str(args.pydantic_python), "-V"], text=True).strip(),
                  fixtures_text=fixture_text, fixtures_sha256=digest(root / "benchmarks/pydantic/fixtures.json"),
                  tool_sha256=tools,
                  protocol=dict(iterations=args.iterations, warmup=args.warmup, repeats_per_position=args.repeats,
                                rounds=args.rounds, engine_orders=orders(args.rounds), seed=args.seed,
                                samples_per_engine_case=args.rounds * 2 * args.repeats,
                                notes="Sequential processes; case order seeded per round; reverse engine order on alternating rounds; no baseline subtraction"),
                  platform=platform.platform(), machine=platform.machine(), cpu_count=os.cpu_count(),
                  affinity=sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
                  cpu_model=next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), None) if Path("/proc/cpuinfo").exists() else None,
                  environment={key: os.environ.get(key) for key in ("PYTHONHASHSEED", "PYTHONMALLOC")},
                  verification=[], known_differences=[], execution_order=[], measurements=[],
                  started=datetime.now(timezone.utc).isoformat(), load_start=os.getloadavg())

    completed_ok = False

    def retain_incomplete():
        if not completed_ok:
            report["incomplete"] = "Run did not reach all end-of-run invariants; not final performance evidence"
            write_json(partial, report)

    atexit.register(retain_incomplete)

    def run(engine, req, expected):
        completed = subprocess.run(commands[engine], input=json.dumps(req), capture_output=True, text=True, timeout=600, check=True)
        result = json.loads(completed.stdout)
        harness.verify(expected, result, req)
        harness.verify_engine(engine_kind(engine), result)
        return result

    # Independent fixture gate before any reported timed batch.
    for case, mode in jobs:
        req = request(case, mode, 1, 0, 1)
        for engine in ENGINES:
            result = run(engine, req, case)
            report["verification"].append(dict(case=case["name"], mode=mode, engine=engine, result=result))
    for case in fixtures.get("known_differences", []):
        for mode in case["modes"]:
            req = request(case, mode, 1, 0, 1)
            for engine in ENGINES:
                expected = dict(name=case["name"], **case["expected_by_engine"][engine_kind(engine)])
                result = run(engine, req, expected)
                report["known_differences"].append(dict(case=case["name"], mode=mode, engine=engine, result=result))
    print(f"Pre-timing oracle gate: {len(report['verification'])} comparable, {len(report['known_differences'])} known-difference results", flush=True)
    for round_index, order in enumerate(orders(args.rounds)):
        ordered_jobs = jobs.copy()
        random.Random(args.seed + round_index).shuffle(ordered_jobs)
        for case, mode in ordered_jobs:
            req = request(case, mode, args.iterations, args.warmup, args.repeats)
            for position, engine in enumerate(order):
                result = run(engine, req, case)
                row = dict(round=round_index, position=position, case=case["name"], mode=mode, engine=engine, request=req, result=result)
                report["measurements"].append(row)
                report["execution_order"].append([round_index, case["name"], mode, position, engine])
        print(f"Round {round_index + 1}/{args.rounds}: all 49 rows checked", flush=True)
    for engine, build in builds.items():
        if snapshot(build["root"]) != build["source_snapshot"] or digest(build["executable"]) != build["executable_sha256"]:
            raise RuntimeError(f"{engine}: source/binary changed while timing")
    if digest(python) != report["python_executable_sha256"] or tools != {p.name: digest(p) for p in Path(__file__).parent.glob("*.py")}:
        raise RuntimeError("Python executable or evidence tooling changed while timing")
    report.update(finished=datetime.now(timezone.utc).isoformat(), load_end=os.getloadavg())
    write_json(args.output, report)
    completed_ok = True
    print(f"Retained {len(report['measurements'])} verified process results", flush=True)


if __name__ == "__main__":
    main()
