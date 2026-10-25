import json
import unittest
import subprocess
import tempfile
from pathlib import Path
from unittest.mock import patch
import optional_runtime
from optional_runtime import executable, test_names, verify_result, LAYERS


class GateTests(unittest.TestCase):
    def test_inventory(self):
        self.assertEqual(test_names("a: test\nb: test\n\n2 tests, 0 benchmarks\n"), ["a", "b"])

    def test_disabled_suite_is_rejected(self):
        with self.assertRaises(ValueError):
            test_names("0 tests, 0 benchmarks\n")

    def test_duplicate_inventory_is_rejected(self):
        with self.assertRaises(ValueError):
            test_names("a: test\na: test\n2 tests, 0 benchmarks")

    def test_success(self):
        verify_result("test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s", 2)

    def test_skips_counts_missing_and_failures_rejected(self):
        for text in ["", "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;", "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;", "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;", "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                verify_result(text, 2)

    def test_select_exact_artifact(self):
        def artifact(name, path):
            return json.dumps(dict(reason="compiler-artifact", target=dict(name=name, kind=["test"]), profile=dict(test=True), executable=path))
        self.assertEqual(executable("noise\n" + artifact("other", "/other") + "\n" + artifact("settings", "/selected"), "settings"), "/selected")
        for output in ["", artifact("other", "/other"), artifact("settings", "/a") + "\n" + artifact("settings", "/b")]:
            with self.assertRaises(ValueError):
                executable(output, "settings")

    def test_inventory_totals_and_unexpected_lines(self):
        for output in ["a: test\n0 tests, 0 benchmarks", "a: test", "unexpected\n0 tests, 0 benchmarks", "a: test\na: test\n0 tests, 0 benchmarks"]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                test_names(output, allow_empty=True)
        self.assertEqual(test_names("0 tests, 0 benchmarks\n", allow_empty=True), [])

    def test_contradictory_execution_summaries(self):
        with self.assertRaises(ValueError):
            verify_result("test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\ntest result: FAILED. broken", 2)

    def test_runner_records_success_and_failed_control(self):
        for selected, disabled, succeeds, fail_build in [("jsonrpc", "0 tests, 0 benchmarks\n", True, False), ("settings", "0 tests, 0 benchmarks\n", True, False), ("partial-json", "0 tests, 0 benchmarks\n", True, False), ("settings", "oops\n0 tests, 0 benchmarks\n", False, False), ("settings", "", False, True)]:
            suite = LAYERS[selected]["test"]
            with self.subTest(disabled=disabled), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / "Cargo.lock").write_text("fixture")
                for name in ["scripts/optional_runtime.py", f"crates/rydantic/tests/{suite}.rs", f"crates/rydantic/src/{suite}.rs"]:
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("fixture")
                report = root / "report"
                artifact = json.dumps(dict(reason="compiler-artifact", target=dict(name=suite, kind=["test"]), profile=dict(test=True), executable="/fixture/settings"))
                outputs = [artifact, "one: test\n\n1 test, 0 benchmarks\n", "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n", artifact, disabled]
                def run(command, **kwargs):
                    return subprocess.CompletedProcess(command, 1 if fail_build else 0, outputs.pop(0))
                def provenance(command, **kwargs):
                    self.assertFalse(report.exists(), "Capture provenance before creating reports")
                    return "abc\n" if command == ["git", "rev-parse", "HEAD"] else ""
                with patch.object(optional_runtime, "ROOT", root), patch("sys.argv", ["gate", "--layer", selected, "--output-dir", str(report)]), patch.object(optional_runtime.subprocess, "run", side_effect=run), patch.object(optional_runtime.subprocess, "check_output", side_effect=provenance):
                    if succeeds:
                        optional_runtime.main()
                    else:
                        with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                            optional_runtime.main()
                manifest = json.loads((report / "manifest.json").read_text())
                self.assertEqual(manifest["status"], "passed" if succeeds else "failed")
                if fail_build:
                    self.assertTrue((report / "enabled-build.log").is_file())
                    continue
                self.assertEqual(manifest["test_count"], 1)
                self.assertEqual(manifest["test_binary"], "/fixture/settings")
                self.assertFalse(manifest["source_dirty"])
                self.assertEqual(len(manifest["commands"]), 5)
                disabled_command = manifest["commands"][3]
                if selected in ("settings", "jsonrpc"):
                    self.assertEqual(disabled_command[-2:], ["--features", "derive"])
                else:
                    self.assertNotIn("--features", disabled_command)
                self.assertTrue((report / "disabled-list.log").is_file())

    def test_nested_settings_harnesses(self):
        child = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 0.00s\n"
        parent = "test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        verify_result(child * 3 + parent, 18)
        for output in [parent + parent, parent + child, child * 3, child.replace("17 filtered", "18 filtered") + parent, child.replace("0 failed", "1 failed") + parent, child.replace("0 ignored", "1 ignored") + parent]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                verify_result(output, 18)

    def test_minimal_features(self):
        self.assertEqual(LAYERS["jsonrpc"]["features"], ["derive", "jsonrpc"])
        self.assertEqual(LAYERS["settings"]["features"], ["derive", "settings"])
        self.assertEqual(LAYERS["partial-json"]["features"], ["partial-json"])


if __name__ == "__main__":
    unittest.main()
