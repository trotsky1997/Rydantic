#!/usr/bin/env python3
"""Run and verify allocation controls separately from uninstrumented rule timings."""
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
from common import artifact_from_receipt, canonical_harness, digest, load_json, request, snapshot, write_json
from record_probes import normalized_manifest, probe_snapshot

TYPES = ("i64", "i128", "u128")
CASES = ("no_bounds", "lexical_range", "lexical_strict", "decimal_equality", "mixed_bounds", "huge_bounds")
ROUTES = ("generic", "typed")


def check_build(build):
    if snapshot(build["root"]) != build["source_snapshot"] or probe_snapshot(build["manifest"]) != build["probe_snapshot"]:
        raise ValueError("runtime/probe source differs from build attestation")
    normalized = normalized_manifest(build["manifest"])
    if hashlib.sha256(json.dumps(normalized, sort_keys=True).encode()).hexdigest() != build["normalized_manifest_sha256"]:
        raise ValueError("normalized manifest hash changed")
    if normalized != build["normalized_manifest"]:
        raise ValueError("probe manifest differs from attestation")
    if digest(build["receipt"]) != build["receipt_sha256"] or digest(build["log"]) != build["log_sha256"]:
        raise ValueError("raw build evidence changed")
    for name, binary in build["artifacts"].items():
        artifact = artifact_from_receipt(build["receipt"], binary["executable"], expected_name=name)
        if digest(binary["executable"]) != binary["executable_sha256"] or artifact["profile"] != binary["profile"] or artifact["features"] != binary["features"] or binary["fresh"] is not False:
            raise ValueError("probe binary/profile differs from receipt")


def check_pair(baseline, candidate):
    if set(baseline["artifacts"]) != {"parse_allocations"} or set(candidate["artifacts"]) != {"parse_allocations", "rule_allocations", "rule_timing"}:
        raise ValueError("expected baseline parse-only and candidate three-probe build attestations")
    if baseline["normalized_manifest"] != candidate["normalized_manifest"]:
        raise ValueError("probe manifests differ beyond the runtime dependency root")
    for name in ("rustc", "cargo", "environment"):
        if baseline[name] != candidate[name]:
            raise ValueError(f"probe builds differ in {name}")
    for path in set(baseline["probe_snapshot"]) | set(candidate["probe_snapshot"]):
        if path != "Cargo.toml" and baseline["probe_snapshot"].get(path) != candidate["probe_snapshot"].get(path):
            raise ValueError(f"probe source/lock differs: {path}")
    for path in ("Cargo.lock", "benchmarks/pydantic/run.py", "benchmarks/pydantic/fixtures.json"):
        if baseline["source_snapshot"][path] != candidate["source_snapshot"][path]:
            raise ValueError(f"runtime control corpus/lock differs: {path}")
    profiles = [artifact["profile"] for build in (baseline, candidate) for artifact in build["artifacts"].values()]
    if any(profile != profiles[0] for profile in profiles):
        raise ValueError("probe build profiles differ")


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, allow_nan=False, separators=(",", ":"))


def check_allocations(rows):
    expected = {(kind, case, route) for kind in TYPES for case in CASES for route in ROUTES}
    bykey = {(row["type"], row["case"], row["route"]): row for row in rows}
    if len(rows) != 36 or set(bykey) != expected:
        raise ValueError("expected all 36 unique allocation rows")
    for row in rows:
        if any(type(row[name]) is not int or row[name] < 0 for name in ("calls", "bytes")):
            raise ValueError("invalid allocation counters")
        if canonical(row["result"]) != canonical(bykey[row["type"], row["case"], "typed"]["result"]):
            raise ValueError("full typed/generic rule errors differ")
        if set(row["result"]) not in ({"Ok"}, {"Err"}):
            raise ValueError("unexpected Context result")
    return bykey


def check_timings(rows, allocations):
    expected = {(kind, case, round_, position) for kind in TYPES for case in CASES for round_ in (0, 1) for position in range(4)}
    seen = {(row["type"], row["case"], row["round"], row["position"]) for row in rows}
    if len(rows) != 144 or seen != expected:
        raise ValueError("expected all 144 unique timing positions")
    for row in rows:
        order = ("generic", "typed", "typed", "generic") if row["round"] == 0 else ("typed", "generic", "generic", "typed")
        if row["route"] != order[row["position"]] or (row["iterations"], row["warmup"], row["repeats"]) != (10000, 1000, 3):
            raise ValueError("changed timing order/protocol")
        if len(row["samples_ns"]) != 3 or any(type(value) is not int or value <= 0 for value in row["samples_ns"]):
            raise ValueError("invalid timing samples")
        accepted = "Ok" in allocations[row["type"], row["case"], row["route"]]["result"]
        if row["checksum"] != (30000 if accepted else 0):
            raise ValueError("rule timing checksum mismatch")


def public_build(build, engine):
    result = copy.deepcopy(build)
    for name in ("root", "manifest", "target_dir", "receipt", "log"):
        result[name] = f"artifact:{engine}-{name}"
    result["command"] = ["cargo", "build", "--manifest-path", f"artifact:{engine}-manifest", "--locked", "--offline", "--release", "--message-format=json"]
    for name, artifact in result["artifacts"].items():
        result["command"] += ["--bin", name]
        artifact["executable"] = f"artifact:{engine}-{name}"
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--baseline-build", type=Path, required=True)
    p.add_argument("--candidate-build", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    p.add_argument("--private-log-dir", type=Path, required=True)
    args = p.parse_args()
    baseline, candidate = load_json(args.baseline_build), load_json(args.candidate_build)
    check_build(baseline)
    check_build(candidate)
    check_pair(baseline, candidate)
    output = args.output_dir
    names = ("rule-allocations.jsonl", "parse-allocations.json", "rule-timings.jsonl", "diagnostic-provenance.json")
    if any((output / name).exists() for name in names) or args.private_log_dir.exists():
        raise ValueError("refusing to overwrite diagnostic evidence or private log directory")
    output.mkdir(parents=True, exist_ok=True)
    args.private_log_dir.mkdir(parents=True)
    tools_before = {path.name: digest(path) for path in Path(__file__).parent.glob("*.py")}
    start = datetime.now(timezone.utc).isoformat()
    intervals, logs = {}, []

    def run(build, name, label, req=None):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", label):
            raise ValueError("unsafe private diagnostic log label")
        command = [build["artifacts"][name]["executable"]]
        result = subprocess.run(command, input=json.dumps(req) if req else None, text=True, capture_output=True, timeout=600)
        for suffix, content in (("stdout", result.stdout), ("stderr", result.stderr)):
            path = args.private_log_dir / (label + "." + suffix)
            path.write_text(content)
            logs.append(dict(artifact=f"artifact:diagnostic-log-{label}-{suffix}", sha256=digest(path)))
        if req is not None:
            path = args.private_log_dir / (label + ".request.json")
            write_json(path, req)
            logs.append(dict(artifact=f"artifact:diagnostic-log-{label}-request", sha256=digest(path)))
        result.check_returncode()
        return result

    intervals["rule_allocations_start"] = datetime.now(timezone.utc).isoformat()
    rule_result = run(candidate, "rule_allocations", "candidate-rule-allocations")
    allocation_rows = [json.loads(line) for line in rule_result.stdout.splitlines()]
    allocation_map = check_allocations(allocation_rows)
    (output / "rule-allocations.jsonl").write_text(rule_result.stdout)
    intervals["rule_allocations_finish"] = datetime.now(timezone.utc).isoformat()

    root = Path(candidate["root"])
    harness = canonical_harness(root)
    fixtures = json.loads((root / "benchmarks/pydantic/fixtures.json").read_text())
    jobs = [(case, mode) for case in fixtures["cases"] for mode in case["modes"]]
    if len(jobs) != 49:
        raise ValueError("expected all 49 canonical case/mode pairs")
    parse_rows = []
    intervals["parse_allocations_start"] = datetime.now(timezone.utc).isoformat()
    for case, mode in jobs:
        req = request(case, mode, 1, 0, 1)
        for engine, build in (("baseline", baseline), ("candidate", candidate)):
            result = run(build, "parse_allocations", f"{engine}-{case['name']}-{mode}", req)
            parsed, counters = json.loads(result.stdout), json.loads(result.stderr)
            harness.verify(case, parsed, req)
            harness.verify_engine("rydantic", parsed)
            if set(counters) != {"calls", "bytes"} or any(type(value) is not int or value < 0 for value in counters.values()):
                raise ValueError("invalid parse allocation counters")
            parse_rows.append(dict(case=case["name"], mode=mode, engine=engine, request=req, allocation=counters,
                                   accepted=parsed["accepted"], output=parsed["output"], errors=parsed["errors"], worker_result=parsed))
    write_json(output / "parse-allocations.json", dict(purpose="allocation counts, not timing evidence", measurements=parse_rows))
    intervals["parse_allocations_finish"] = datetime.now(timezone.utc).isoformat()

    intervals["rule_timing_start"] = datetime.now(timezone.utc).isoformat()
    timed_result = run(candidate, "rule_timing", "candidate-rule-timing")
    timing_rows = [json.loads(line) for line in timed_result.stdout.splitlines()]
    check_timings(timing_rows, allocation_map)
    (output / "rule-timings.jsonl").write_text(timed_result.stdout)
    intervals["rule_timing_finish"] = datetime.now(timezone.utc).isoformat()
    check_build(baseline)
    check_build(candidate)
    if tools_before != {path.name: digest(path) for path in Path(__file__).parent.glob("*.py")}:
        raise ValueError("diagnostic tooling changed during execution")
    provenance = dict(format_version=1, purpose="build-attested final-source diagnostic evidence", started=start,
                      finished=datetime.now(timezone.utc).isoformat(), intervals=intervals,
                      builds={"baseline": public_build(baseline, "baseline"), "candidate": public_build(candidate, "candidate")},
                      private_build_manifest_sha256={"baseline": digest(args.baseline_build), "candidate": digest(args.candidate_build)},
                      tool_sha256=tools_before, log_artifacts=logs,
                      outputs={name: digest(output / name) for name in names[:-1]},
                      counts=dict(rule_allocation_rows=len(allocation_rows), parse_allocation_rows=len(parse_rows), rule_timing_rows=len(timing_rows)),
                      environment=dict(platform="Linux x86_64 shared cloud host", cpu_count=os.cpu_count(), affinity=sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None),
                      protocol=dict(parse_request=dict(iterations=1, warmup=0, repeats=1), rule_timing=dict(rounds=2, positions=4, iterations=10000, warmup=1000, repeats=3),
                                    allocation_interval="System allocation/reallocation calls and cumulative requested bytes. Parse: through validated Result construction, before dump/error normalization and result disposal; immutable input construction and one static-rule warmup excluded. Isolated rules: during validate_rules/Rules::validate, including construction of Context error entries, before ctx.result and disposal; value/rules/Context preparation excluded",
                                    timing_separation="rule_timing is a separate uninstrumented executable; parse_allocations samples are retained raw but are NOT timing evidence"),
                      privacy="Private build manifests, original Cargo receipts/logs and full execution logs stay outside the public evidence; this derivative preserves their SHA256 values and replaces local paths with artifact IDs.")
    encoded = json.dumps(provenance, ensure_ascii=False)
    if re.search(r"/(?:workspace|home|Users|tmp|private)/", encoded):
        raise ValueError("private path remains in diagnostic provenance; review metadata before publication")
    write_json(output / "diagnostic-provenance.json", provenance)
    print("PASS: 36 full-error allocation, 98 canonical parse-allocation and 144 uninstrumented rule-timing rows; source/binary/receipt invariants retained", flush=True)


if __name__ == "__main__":
    main()
