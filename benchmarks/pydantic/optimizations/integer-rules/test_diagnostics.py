"""Synthetic provenance/control tests only: no Rust build or timing execution."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from record_probes import normalized_manifest
from run_diagnostics import CASES, ROUTES, TYPES, check_allocations, check_pair, check_timings, public_build


def allocation_rows():
    return [dict(type=kind, case=case, route=route, calls=0, bytes=0, result={"Ok": None})
            for kind in TYPES for case in CASES for route in ROUTES]


def timing_rows():
    rows = []
    for kind in TYPES:
        for case in CASES:
            for round_ in (0, 1):
                order = ("generic", "typed", "typed", "generic") if round_ == 0 else ("typed", "generic", "generic", "typed")
                for position, route in enumerate(order):
                    rows.append(dict(type=kind, case=case, route=route, round=round_, position=position,
                                     iterations=10000, warmup=1000, repeats=3, samples_ns=[1, 2, 3], checksum=30000))
    return rows


def builds():
    profile = dict(opt_level="3", debuginfo=0, debug_assertions=False, overflow_checks=False)
    base = dict(normalized_manifest={"dependencies": {"rydantic": {"path": "artifact:runtime-source"}}},
                probe_snapshot={"Cargo.toml": "differentallowed", "Cargo.lock": "same", "src/bin/parse_allocations.rs": "same"},
                source_snapshot={key: "same" for key in ("Cargo.lock", "benchmarks/pydantic/run.py", "benchmarks/pydantic/fixtures.json")},
                rustc="synthetic-test-only", cargo="synthetic-test-only", environment={},
                artifacts={"parse_allocations": dict(profile=profile, executable="/workspace/private/parse")},
                root="/workspace/private/root", manifest="/workspace/private/Cargo.toml", target_dir="/workspace/private/target",
                receipt="/workspace/private/build.jsonl", log="/workspace/private/build.log")
    candidate = copy.deepcopy(base)
    for name in ("rule_allocations", "rule_timing"):
        candidate["artifacts"][name] = dict(profile=profile, executable=f"/workspace/private/{name}")
    return base, candidate


class DiagnosticTests(unittest.TestCase):
    def test_all_expected_allocation_and_timing_rows(self):
        checked = check_allocations(allocation_rows())
        check_timings(timing_rows(), checked)
        self.assertEqual(len(checked), 36)

    def test_missing_allocation_rejected(self):
        with self.assertRaises(ValueError):
            check_allocations(allocation_rows()[:-1])

    def test_full_error_difference_rejected(self):
        rows = allocation_rows()
        rows[0]["result"] = {"Err": [{"code": "synthetic"}]}
        with self.assertRaises(ValueError):
            check_allocations(rows)

    def test_wrong_timing_route_rejected(self):
        rows = timing_rows()
        rows[0]["route"] = "typed"
        with self.assertRaises(ValueError):
            check_timings(rows, check_allocations(allocation_rows()))

    def test_wrong_checksum_rejected(self):
        rows = timing_rows()
        rows[0]["checksum"] = 0
        with self.assertRaises(ValueError):
            check_timings(rows, check_allocations(allocation_rows()))

    def test_only_dependency_root_is_normalized(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "Cargo.toml"
            manifest.write_text('[dependencies]\nrydantic = {path="../private", version="0.8.0"}\nserde="1"\n')
            value = normalized_manifest(manifest)
            self.assertEqual(value["dependencies"]["rydantic"], dict(path="artifact:runtime-source", version="0.8.0"))
            self.assertEqual(value["dependencies"]["serde"], "1")

    def test_build_pair_rejects_probe_content_changes(self):
        baseline, candidate = builds()
        check_pair(baseline, candidate)
        candidate["probe_snapshot"]["src/bin/parse_allocations.rs"] = "changed"
        with self.assertRaises(ValueError):
            check_pair(baseline, candidate)

    def test_public_provenance_has_artifact_ids_not_local_paths(self):
        baseline, candidate = builds()
        exported = public_build(candidate, "candidate")
        self.assertNotIn("/workspace/", json.dumps(exported))
        self.assertEqual(exported["source_snapshot"], candidate["source_snapshot"])
        self.assertIn("--bin", exported["command"])


if __name__ == "__main__":
    unittest.main()
