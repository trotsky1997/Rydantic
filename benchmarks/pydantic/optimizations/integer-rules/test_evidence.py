"""Small synthetic tests: no Rust builds, worker processes or timing claims."""
import copy
import hashlib
import json
from pathlib import Path
import random
import tempfile
import unittest

from audit import audit_data
from common import ENGINES, artifact_from_receipt, orders, request, snapshot
from export_public import export
from report import render

ROOT = Path(__file__).resolve().parents[4]


def synthetic():
    sources = snapshot(ROOT)
    fixture_text = (ROOT / "benchmarks/pydantic/fixtures.json").read_text()
    fixtures = json.loads(fixture_text)
    profile = dict(opt_level="3", debuginfo=0, debug_assertions=False, overflow_checks=False)
    builds = {engine: dict(source_snapshot=dict(sources), fresh=False, profile=dict(profile),
                           executable_sha256="a" * 64, receipt_sha256="b" * 64,
                           rustc="rustc synthetic-test-only", cargo="cargo synthetic-test-only", environment={},
                           revision="synthetic-test-only", root="/workspace/private/root", target_dir="/workspace/private/target",
                           executable="/workspace/private/worker", receipt="/workspace/private/build.jsonl", log="/workspace/private/build.log")
              for engine in ENGINES[:-1]}
    data = dict(builds=builds, fixtures_text=fixture_text, fixtures_sha256=hashlib.sha256(fixture_text.encode()).hexdigest(),
                protocol=dict(iterations=2, warmup=0, repeats_per_position=3, rounds=2,
                              engine_orders=orders(2), seed=123, samples_per_engine_case=12),
                verification=[], known_differences=[], measurements=[], execution_order=[],
                commands={e: ["/workspace/private/worker"] for e in ENGINES}, python_executable="/usr/bin/python-test",
                python_executable_sha256="c" * 64, platform="private kernel identity", python_version="synthetic-test-only",
                cpu_model="synthetic-test-only", cpu_count=1, affinity=[0], load_start=[0, 0, 0], load_end=[0, 0, 0],
                started="synthetic-test-only", finished="synthetic-test-only")

    def result(case, engine, iterations=1, repeats=1):
        expected = case.get("expected_by_engine", {}).get("pydantic" if engine == "pydantic" else "rydantic", case)
        return dict(accepted=expected["accepted"], output=expected["output"], errors=expected["errors"],
                    iterations=iterations, checksum=iterations * repeats if expected["accepted"] else 0,
                    samples_ns=[100] * repeats, baseline_ns=[1] * repeats,
                    engine=dict(name="rydantic", profile="release") if engine != "pydantic" else
                    dict(name="pydantic", pydantic="2.14.1", pydantic_core="2.50.1"))

    jobs = [(case, mode) for case in fixtures["cases"] for mode in case["modes"]]
    for case, mode in jobs:
        for engine in ENGINES:
            data["verification"].append(dict(case=case["name"], mode=mode, engine=engine, result=result(case, engine)))
    for case in fixtures["known_differences"]:
        for mode in case["modes"]:
            for engine in ENGINES:
                data["known_differences"].append(dict(case=case["name"], mode=mode, engine=engine, result=result(case, engine)))
    for round_index, order in enumerate(orders(2)):
        shuffled = jobs.copy()
        random.Random(123 + round_index).shuffle(shuffled)
        for case, mode in shuffled:
            for position, engine in enumerate(order):
                data["measurements"].append(dict(round=round_index, case=case["name"], mode=mode, position=position,
                                                 engine=engine, request=request(case, mode, 2, 0, 3), result=result(case, engine, 2, 3)))
                data["execution_order"].append([round_index, case["name"], mode, position, engine])
    return data


class EvidenceTests(unittest.TestCase):
    def test_full_synthetic_schedule_and_semantics(self):
        self.assertEqual(audit_data(synthetic(), ROOT), 784)

    def test_incomplete_run_is_rejected(self):
        data = synthetic()
        data["incomplete"] = "synthetic interrupted run"
        with self.assertRaises(AssertionError):
            audit_data(data, ROOT)

    def test_changed_schedule_is_rejected(self):
        data = synthetic()
        order = data["protocol"]["engine_orders"][0]
        order[0], order[1] = order[1], order[0]
        with self.assertRaises(AssertionError):
            audit_data(data, ROOT)

    def test_missing_row_is_rejected(self):
        data = synthetic()
        data["measurements"].pop()
        with self.assertRaises(AssertionError):
            audit_data(data, ROOT)

    def test_boolean_integer_output_confusion_is_rejected(self):
        data = synthetic()
        row = next(row for row in data["measurements"] if row["case"] == "flat_valid")
        row["result"]["output"] = dict(row["result"]["output"], id=True)
        with self.assertRaises(ValueError):
            audit_data(data, ROOT)

    def test_control_source_mismatch_is_rejected(self):
        data = synthetic()
        data["builds"]["control"]["source_snapshot"]["Cargo.lock"] = "d" * 64
        with self.assertRaises(AssertionError):
            audit_data(data, ROOT)

    def test_export_preserves_observations_and_removes_paths(self):
        data = synthetic()
        raw = json.dumps(data).encode()
        public = json.loads(export(raw))
        self.assertEqual(public["measurements"], data["measurements"])
        self.assertEqual(public["verification"], data["verification"])
        self.assertEqual(public["builds"]["baseline"]["source_snapshot"], data["builds"]["baseline"]["source_snapshot"])
        self.assertEqual(public["public_export"]["private_original_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertNotIn("/workspace/", json.dumps(public))
        self.assertEqual(audit_data(public, ROOT), 784)

    def test_export_fails_closed_for_unredacted_flags(self):
        data = synthetic()
        data["builds"]["candidate"]["environment"]["RUSTFLAGS"] = "--remap-path-prefix=/home/private/name=/src"
        with self.assertRaises(ValueError):
            export(json.dumps(data).encode())

    def test_report_retains_all_canonical_rows(self):
        data = synthetic()
        report = render(data)
        for case in json.loads(data["fixtures_text"])["cases"]:
            for mode in case["modes"]:
                self.assertEqual(report.count(f"| {case['name']} | {mode} |"), 1)
        self.assertIn("Baseline/control binary equality: **True**", report)

    def test_receipt_must_be_successful_fresh_release_for_exact_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary, receipt = root / "worker", root / "receipt.jsonl"
            artifact = dict(reason="compiler-artifact", target=dict(name="pydantic_benchmark"), executable=str(binary),
                            profile=dict(opt_level="3", debuginfo=0, debug_assertions=False, overflow_checks=False), fresh=False)
            def write():
                receipt.write_text(json.dumps(artifact) + "\n" + json.dumps(dict(reason="build-finished", success=True)) + "\n")
            write()
            self.assertFalse(artifact_from_receipt(receipt, binary)["fresh"])
            artifact["fresh"] = True
            write()
            with self.assertRaises(ValueError):
                artifact_from_receipt(receipt, binary)


if __name__ == "__main__":
    unittest.main()
