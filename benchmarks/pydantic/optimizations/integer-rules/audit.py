#!/usr/bin/env python3
"""Audit every raw observation, provenance invariant, schedule and optional report."""
import argparse
import hashlib
import json
from pathlib import Path
import random
from common import (ENGINES, RUST_ENGINES, artifact_from_receipt, canonical_harness, digest,
                    engine_kind, load_json, orders, request, snapshot)


def audit_data(data, root):
    assert "incomplete" not in data, "unfinished timing is not final evidence"
    harness = canonical_harness(root)
    assert digest(Path(root) / "benchmarks/pydantic/run.py") == data["builds"]["candidate"]["source_snapshot"]["benchmarks/pydantic/run.py"], "canonical oracle changed"
    builds = data["builds"]
    assert set(builds) == set(RUST_ENGINES)
    assert builds["baseline"]["source_snapshot"] == builds["control"]["source_snapshot"], "control source differs"
    for build in builds.values():
        assert build["fresh"] is False
        assert build["profile"]["opt_level"] == "3"
        assert build["profile"]["debuginfo"] == 0
        assert build["profile"]["debug_assertions"] is False
        assert build["profile"]["overflow_checks"] is False
        for key in ("rustc", "cargo", "environment", "profile"):
            assert build[key] == builds["baseline"][key], key
        assert len(build["executable_sha256"]) == 64
        assert len(build["receipt_sha256"]) == 64
    for path in ("Cargo.lock", "benchmarks/pydantic/run.py", "benchmarks/pydantic/fixtures.json",
                 "benchmarks/pydantic/requirements.txt", "benchmarks/pydantic/python_worker.py",
                 "crates/rydantic/examples/pydantic_benchmark.rs"):
        assert len({b["source_snapshot"][path] for b in builds.values()}) == 1, path
    fixture_hash = hashlib.sha256(data["fixtures_text"].encode()).hexdigest()
    assert fixture_hash == data["fixtures_sha256"]
    assert fixture_hash == builds["candidate"]["source_snapshot"]["benchmarks/pydantic/fixtures.json"]
    fixtures = json.loads(data["fixtures_text"])
    jobs = [(c, m) for c in fixtures["cases"] for m in c["modes"]]
    assert len(jobs) == 49
    byname = {case["name"]: case for case in fixtures["cases"]}
    expected_gate = [(case["name"], mode, engine) for case, mode in jobs for engine in ENGINES]
    assert [(r["case"], r["mode"], r["engine"]) for r in data["verification"]] == expected_gate
    for row in data["verification"]:
        case = byname[row["case"]]
        req = request(case, row["mode"], 1, 0, 1)
        harness.verify(case, row["result"], req)
        harness.verify_engine(engine_kind(row["engine"]), row["result"])
    difference_jobs = [(c, m, e) for c in fixtures["known_differences"] for m in c["modes"] for e in ENGINES]
    assert [(r["case"], r["mode"], r["engine"]) for r in data["known_differences"]] == [(c["name"], m, e) for c, m, e in difference_jobs]
    for row, (case, mode, engine) in zip(data["known_differences"], difference_jobs):
        expected = dict(name=case["name"], **case["expected_by_engine"][engine_kind(engine)])
        harness.verify(expected, row["result"], request(case, mode, 1, 0, 1))
        harness.verify_engine(engine_kind(engine), row["result"])
    protocol = data["protocol"]
    assert protocol["engine_orders"] == orders(protocol["rounds"])
    assert protocol["samples_per_engine_case"] == protocol["rounds"] * 2 * protocol["repeats_per_position"]
    expected_order = []
    for round_index, order in enumerate(orders(protocol["rounds"])):
        round_jobs = jobs.copy()
        random.Random(protocol["seed"] + round_index).shuffle(round_jobs)
        expected_order.extend([round_index, case["name"], mode, position, engine]
                              for case, mode in round_jobs for position, engine in enumerate(order))
    assert data["execution_order"] == expected_order
    assert len(data["measurements"]) == len(expected_order) == 49 * 8 * protocol["rounds"]
    for row, expected in zip(data["measurements"], expected_order):
        assert [row["round"], row["case"], row["mode"], row["position"], row["engine"]] == expected
        case = byname[row["case"]]
        req = request(case, row["mode"], protocol["iterations"], protocol["warmup"], protocol["repeats_per_position"])
        assert row["request"] == req
        harness.verify(case, row["result"], req)
        harness.verify_engine(engine_kind(row["engine"]), row["result"])
    return len(data["measurements"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[4])
    parser.add_argument("--check-local", action="store_true", help="Private originals only: verify sources, binaries and raw build receipts on disk")
    parser.add_argument("--report", type=Path, help="Verify report is exactly report.py output")
    parser.add_argument("--private-original", type=Path, help="Verify public export is exactly the documented redaction of this private original")
    args = parser.parse_args()
    data = load_json(args.results)
    count = audit_data(data, args.root)
    if args.check_local:
        assert "public_export" not in data
        for build in data["builds"].values():
            assert snapshot(build["root"]) == build["source_snapshot"]
            assert digest(build["executable"]) == build["executable_sha256"]
            assert digest(build["receipt"]) == build["receipt_sha256"]
            assert digest(build["log"]) == build["log_sha256"]
            artifact_from_receipt(build["receipt"], build["executable"])
        assert digest(data["python_executable"]) == data["python_executable_sha256"]
    if args.private_original:
        from export_public import export
        assert data == json.loads(export(args.private_original.read_bytes()))
    if args.report:
        from report import render
        assert args.report.read_text() == render(data)
    print(f"PASS: {count} timed process results; all 49 rows, fixture outputs, error paths/categories, gate controls, counterbalanced order and build invariants verified")


if __name__ == "__main__":
    main()
