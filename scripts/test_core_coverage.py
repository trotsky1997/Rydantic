"""Fast coverage-tool contracts; no Rust build, LLVM, network or providers needed."""

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "core_coverage", Path(__file__).with_name("core_coverage.py")
)
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


class InstrumentedEnvironmentTests(unittest.TestCase):
    def test_defaults_and_caller_environment_are_preserved(self):
        original = {"HOME": "/home/test", "CUSTOM_FLAG": "keep"}
        result = coverage.instrumented_environment(original)
        self.assertEqual(original, {"HOME": "/home/test", "CUSTOM_FLAG": "keep"})
        self.assertEqual(result["CUSTOM_FLAG"], "keep")
        self.assertEqual(result["HOME"], "/home/test")
        self.assertEqual(result["CARGO_INCREMENTAL"], "0")
        self.assertEqual(result["CARGO_PROFILE_DEV_DEBUG"], "0")
        self.assertEqual(result["CARGO_PROFILE_TEST_DEBUG"], "0")
        self.assertEqual(result["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(
            result["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"),
            ["-C", "instrument-coverage"],
        )

    def test_explicit_jobs_and_target_survive_but_debug_is_disabled(self):
        result = coverage.instrumented_environment(
            {
                "CARGO_BUILD_JOBS": "1",
                "CARGO_TARGET_DIR": "/tmp/custom-target",
                "CARGO_INCREMENTAL": "1",
                "CARGO_PROFILE_DEV_DEBUG": "2",
                "CARGO_PROFILE_TEST_DEBUG": "2",
            }
        )
        self.assertEqual(result["CARGO_BUILD_JOBS"], "1")
        self.assertEqual(result["CARGO_TARGET_DIR"], "/tmp/custom-target")
        self.assertEqual(result["CARGO_INCREMENTAL"], "0")
        self.assertEqual(result["CARGO_PROFILE_DEV_DEBUG"], "0")
        self.assertEqual(result["CARGO_PROFILE_TEST_DEBUG"], "0")

    def test_encoded_flags_take_priority_and_keep_single_argument_boundaries(self):
        flags = ["--cfg", 'label="two words"', "-C", "opt-level=1"]
        result = coverage.instrumented_environment(
            {
                "CARGO_ENCODED_RUSTFLAGS": "\x1f".join(flags),
                "RUSTFLAGS": "--cfg must_not_leak",
            }
        )
        self.assertEqual(
            result["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"),
            flags + ["-C", "instrument-coverage"],
        )
        self.assertNotIn("RUSTFLAGS", result)

    def test_empty_encoded_flags_still_override_plain_flags(self):
        result = coverage.instrumented_environment(
            {"CARGO_ENCODED_RUSTFLAGS": "", "RUSTFLAGS": "--cfg ignored"}
        )
        self.assertEqual(
            result["CARGO_ENCODED_RUSTFLAGS"], "-C\x1finstrument-coverage"
        )
        self.assertNotIn("RUSTFLAGS", result)

    def test_plain_flags_are_retained_and_coverage_is_canonicalized(self):
        result = coverage.instrumented_environment(
            {
                "RUSTFLAGS": "--cfg custom -C instrument-coverage=off "
                "-Cinstrument-coverage=yes -C opt-level=1"
            }
        )
        self.assertEqual(
            result["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"),
            ["--cfg", "custom", "-C", "opt-level=1", "-C", "instrument-coverage"],
        )
        self.assertNotIn("RUSTFLAGS", result)

    def test_encoded_coverage_forms_are_normalized_idempotently(self):
        for option in [
            ["-C", "instrument-coverage"],
            ["-C", "instrument-coverage=yes"],
            ["-C", "instrument-coverage=off"],
            ["-Cinstrument-coverage"],
            ["-Cinstrument-coverage=on"],
            ["-Cinstrument-coverage=no"],
        ]:
            with self.subTest(option=option):
                source = {
                    "CARGO_ENCODED_RUSTFLAGS": "\x1f".join(
                        ["--cfg", "kept"] + option + option
                    )
                }
                once = coverage.instrumented_environment(source)
                twice = coverage.instrumented_environment(once)
                self.assertEqual(once, twice)
                self.assertEqual(
                    once["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"),
                    ["--cfg", "kept", "-C", "instrument-coverage"],
                )


class DiscoveryTests(unittest.TestCase):
    def test_discovery_is_sorted_automatic_and_excludes_trybuild_driver(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tests = root / "crates/rydantic/tests"
            tests.mkdir(parents=True)
            for name in ["typed_pipeline", "models", "compile", "future_contract"]:
                (tests / f"{name}.rs").write_text("#[test] fn works() {}\n")
            (tests / "README.md").write_text("Not a Cargo test target.\n")
            (tests / "ui").mkdir()
            (tests / "ui/not_a_runtime_target.rs").write_text("invalid Rust\n")
            self.assertEqual(
                coverage.discover_tests(root),
                ["future_contract", "models", "typed_pipeline"],
            )

    def test_feature_gated_targets_are_not_silently_filtered_by_text(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tests = root / "crates/rydantic/tests"
            tests.mkdir(parents=True)
            (tests / "optional.rs").write_text('#![cfg(feature = "optional")]\n')
            self.assertEqual(coverage.discover_tests(root), ["optional"])

    def test_repository_discovery_includes_context_and_pipeline_suites(self):
        suites = coverage.discover_tests(Path(__file__).resolve().parents[1])
        self.assertEqual(suites, sorted(set(suites)))
        self.assertNotIn("compile", suites)
        for name in [
            "call_context",
            "callback_context_derive",
            "callback_lists",
            "typed_pipeline",
            "core_contracts",
            "stream_failures",
            "borrowed_contracts",
            "collection_contracts",
        ]:
            with self.subTest(name=name):
                self.assertIn(name, suites)


class ObjectCollectionTests(unittest.TestCase):
    @staticmethod
    def artifact(executable, **updates):
        value = {
            "reason": "compiler-artifact",
            "executable": executable,
            "profile": {"test": True},
        }
        value.update(updates)
        return json.dumps(value)

    def test_only_test_executables_are_collected_and_deduplicated(self):
        log = "\n".join(
            [
                "Compiling rydantic v0.7.0",
                self.artifact("/target/models"),
                self.artifact("/target/lib"),
                self.artifact("/target/models"),
                self.artifact("/target/build", profile={"test": False}),
                self.artifact("/target/other", reason="build-script-executed"),
                self.artifact(None),
                self.artifact(""),
                "test result: ok. 12 passed; 0 failed",
            ]
        )
        self.assertEqual(
            coverage.collect_objects(log), ["/target/models", "/target/lib"]
        )

    def test_valid_json_that_is_not_an_artifact_does_not_crash(self):
        log = "\n".join(
            [
                "null",
                "[]",
                '"a JSON string printed by a test"',
                "42",
                "{}",
                self.artifact("/target/missing-profile", profile={}),
                self.artifact("/target/null-profile", profile=None),
                self.artifact(["not", "a", "path"]),
            ]
        )
        self.assertEqual(coverage.collect_objects(log), [])

    def test_empty_log_has_no_objects(self):
        self.assertEqual(coverage.collect_objects(""), [])


class CoverageSummaryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name) / "crates/rydantic/src"
        self.root.mkdir(parents=True)

    def file(self, name="lib.rs", lines=(10, 8), functions=(5, 4),
             regions=(20, 15), branches=(0, 0)):
        return {
            "filename": str(self.root / name),
            "summary": {
                metric: {"count": values[0], "covered": values[1], "percent": 99.99}
                for metric, values in [
                    ("lines", lines),
                    ("functions", functions),
                    ("regions", regions),
                    ("branches", branches),
                ]
            },
        }

    @staticmethod
    def export(*files):
        return {
            "type": "llvm.coverage.json.export",
            "version": "2.0.1",
            "data": [{"files": list(files), "totals": {"not": "trusted"}}],
        }

    def summarize(self, *files):
        return coverage.summarize_coverage(self.export(*files), self.root)

    def test_aggregate_counts_recompute_percentages_and_relative_file_names(self):
        summary = self.summarize(
            self.file(),
            self.file("schema/mod.rs", lines=(2, 1), functions=(1, 0), regions=(4, 2)),
        )
        self.assertEqual(set(summary["files"]), {"lib.rs", "schema/mod.rs"})
        for name, count, covered in [
            ("lines", 12, 9), ("functions", 6, 4), ("regions", 24, 17)
        ]:
            with self.subTest(metric=name):
                self.assertEqual(summary[name]["count"], count)
                self.assertEqual(summary[name]["covered"], covered)
                self.assertAlmostEqual(summary[name]["percent"], 100 * covered / count)
        self.assertEqual(
            summary["branches"], {"count": 0, "covered": 0, "percent": None}
        )

    def test_nonzero_branch_counts_are_preserved_not_relabelled(self):
        summary = self.summarize(self.file(branches=(8, 3)))
        self.assertEqual(
            summary["branches"], {"count": 8, "covered": 3, "percent": 37.5}
        )

    def test_rejects_outside_source_files_including_prefix_siblings(self):
        for filename in [
            self.root.parent / "tests/models.rs",
            self.root.parent / "src_extra/lib.rs",
            self.root / "../tests/models.rs",
        ]:
            with self.subTest(filename=str(filename)):
                row = self.file()
                row["filename"] = str(filename)
                with self.assertRaises(ValueError):
                    self.summarize(row)

    def test_rejects_duplicate_source_files(self):
        with self.assertRaises(ValueError):
            self.summarize(self.file(), self.file())

    def test_rejects_non_rust_files_even_inside_the_source_root(self):
        with self.assertRaises(ValueError):
            self.summarize(self.file("generated.txt"))

    def test_rejects_normalized_duplicate_source_files(self):
        duplicate = self.file()
        duplicate["filename"] = str(self.root) + "/./lib.rs"
        with self.assertRaises(ValueError):
            self.summarize(self.file(), duplicate)

    def test_rejects_invalid_counts_in_every_metric(self):
        for metric in ["lines", "functions", "regions", "branches"]:
            for field in ["count", "covered"]:
                for invalid in [-1, 1.5, "3", None, True, False]:
                    with self.subTest(metric=metric, field=field, invalid=invalid):
                        row = self.file()
                        row["summary"][metric][field] = invalid
                        with self.assertRaises(ValueError):
                            self.summarize(row)

    def test_rejects_covered_counts_greater_than_totals(self):
        for metric in ["lines", "functions", "regions", "branches"]:
            with self.subTest(metric=metric):
                row = self.file()
                row["summary"][metric].update(count=2, covered=3)
                with self.assertRaises(ValueError):
                    self.summarize(row)

    def test_rejects_missing_metrics_and_counts(self):
        for metric in ["lines", "functions", "regions", "branches"]:
            for missing in ["metric", "count", "covered"]:
                with self.subTest(metric=metric, missing=missing):
                    row = self.file()
                    if missing == "metric":
                        del row["summary"][metric]
                    else:
                        del row["summary"][metric][missing]
                    with self.assertRaises(ValueError):
                        self.summarize(row)

    def test_rejects_empty_line_or_function_denominators(self):
        for updates in [{"lines": (0, 0)}, {"functions": (0, 0)}]:
            with self.subTest(updates=updates), self.assertRaises(ValueError):
                self.summarize(self.file(**updates))

    def test_zero_denominator_in_one_file_is_allowed_when_aggregate_is_nonzero(self):
        summary = self.summarize(
            self.file(),
            self.file("unused.rs", lines=(0, 0), functions=(0, 0), regions=(0, 0)),
        )
        self.assertEqual(summary["lines"]["count"], 10)
        self.assertEqual(summary["functions"]["count"], 5)

    def test_rejects_missing_or_empty_reports(self):
        for report in [{}, {"data": []}, {"data": [{}]}, self.export()]:
            with self.subTest(report=report), self.assertRaises(ValueError):
                coverage.summarize_coverage(report, self.root)


class ThresholdTests(unittest.TestCase):
    @staticmethod
    def summary(line=(100, 88), function=(100, 89)):
        return {
            "lines": {"count": line[0], "covered": line[1], "percent": 100.0},
            "functions": {
                "count": function[0], "covered": function[1], "percent": 100.0
            },
        }

    def test_exact_threshold_is_inclusive(self):
        summary = self.summary()
        original = copy.deepcopy(summary)
        self.assertIsNone(coverage.check_thresholds(summary, 88, 89))
        self.assertEqual(summary, original)

    def test_either_failed_floor_rejects_the_report(self):
        for floors in [(88.01, 89), (88, 89.01)]:
            with self.subTest(floors=floors), self.assertRaises(ValueError):
                coverage.check_thresholds(self.summary(), *floors)

    def test_gate_uses_exact_count_ratio_not_exported_or_rounded_percent(self):
        for summary, floors in [
            (self.summary(line=(10000, 8799)), (88, 0)),
            (self.summary(function=(10000, 8799)), (0, 88)),
            (self.summary(line=(3, 2)), (66.67, 0)),
        ]:
            with self.subTest(summary=summary), self.assertRaises(ValueError):
                coverage.check_thresholds(summary, *floors)

    def test_zero_threshold_can_be_used_for_an_observation_only_run(self):
        self.assertIsNone(
            coverage.check_thresholds(self.summary(line=(10, 0), function=(10, 0)), 0, 0)
        )

    def test_omitted_thresholds_are_optional_independently(self):
        for floors in [(None, None), (88, None), (None, 89)]:
            with self.subTest(floors=floors):
                self.assertIsNone(coverage.check_thresholds(self.summary(), *floors))

    def test_invalid_thresholds_cannot_disable_a_gate(self):
        for invalid in [-0.01, 100.01, float("nan"), float("inf"), -float("inf")]:
            for floors in [(invalid, None), (None, invalid)]:
                with self.subTest(floors=floors), self.assertRaises(ValueError):
                    coverage.check_thresholds(self.summary(), *floors)

class RunnerTests(unittest.TestCase):
    """Exercise orchestration/failure artifacts with synthetic Cargo/LLVM commands."""
    def setUp(self):
        from unittest.mock import patch
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "crates/rydantic/tests").mkdir(parents=True)
        (self.root / "crates/rydantic/tests/example.rs").write_text("// fixture")
        (self.root / "crates/rydantic/src").mkdir()
        self.source = self.root / "crates/rydantic/src/lib.rs"
        self.source.write_text("// fixture")
        (self.root / "Cargo.lock").write_text("# synthetic lockfile")
        self.llvm = self.root / "lib/rustlib/test-host/bin"
        self.llvm.mkdir(parents=True)
        suffix = ".exe" if coverage.os.name == "nt" else ""
        for command in ["llvm-cov", "llvm-profdata"]:
            (self.llvm / (command + suffix)).touch()
        self.output = self.root / "report"
        self.calls = []
        self.source_dirty = True
        self.cargo_failure = False
        self.emit_profiles = True
        self.emit_objects = True
        self.patches = [
            patch.object(coverage, "ROOT", self.root),
            patch.dict(coverage.os.environ, {}, clear=True),
            patch.object(coverage.subprocess, "check_output", side_effect=self.check_output),
            patch.object(coverage.subprocess, "run", side_effect=self.run_command),
        ]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def check_output(self, command, **_):
        if command == ["rustc", "-vV"]:
            return "rustc fixture\nhost: test-host\n"
        if command == ["rustc", "--print", "sysroot"]:
            return str(self.root)
        if command == ["git", "rev-parse", "HEAD"]:
            return "fixture-commit\n"
        if command == ["git", "status", "--porcelain"]:
            return " M scripts/core_coverage.py\n" if self.source_dirty else ("?? report/\n" if self.output.exists() else "")
        self.fail(f"Unexpected command: {command}")

    def run_command(self, command, **options):
        self.calls.append(command)
        if command[0] == "cargo":
            if self.cargo_failure:
                return coverage.subprocess.CompletedProcess(command, 1, stdout="synthetic test failure\n")
            if self.emit_profiles:
                profile = Path(options["env"]["LLVM_PROFILE_FILE"].replace("%m-%p", "fixture"))
                profile.parent.mkdir(parents=True)
                profile.touch()
            artifact = ObjectCollectionTests.artifact(str(self.root / "test-binary"))
            return coverage.subprocess.CompletedProcess(command, 0, stdout=artifact if self.emit_objects else "no artifacts")
        if Path(command[0]).name.startswith("llvm-cov"):
            if command[1] == "export":
                metrics = {name: {"count": count, "covered": covered} for name, count, covered in [
                    ("lines", 100, 88), ("functions", 100, 89), ("regions", 100, 87), ("branches", 0, 0)
                ]}
                json.dump({"data": [{"files": [{"filename": str(self.source), "summary": metrics}]}]}, options["stdout"])
            else:
                options["stdout"].write("synthetic coverage\n")
        return coverage.subprocess.CompletedProcess(command, 0)

    def invoke(self, *arguments):
        from contextlib import redirect_stdout
        from io import StringIO
        from unittest.mock import patch
        with patch("sys.argv", ["core_coverage.py", "--output-dir", str(self.output), *arguments]), redirect_stdout(StringIO()):
            coverage.main()

    def test_success_records_provenance_commands_and_scoped_summary(self):
        self.invoke("--min-line-coverage", "85", "--min-function-coverage", "85")
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(manifest["source_revision"], "fixture-commit")
        self.assertTrue(manifest["source_dirty"])
        self.assertEqual(len(manifest["cargo_lock_sha256"]), 64)
        self.assertEqual(manifest["phases"], {"current": ["example"]})
        self.assertIn("--no-default-features", manifest["commands"][0])
        self.assertEqual(manifest["rust_flags"], ["-C", "instrument-coverage"])
        summary = json.loads((self.output / "summary.json").read_text())["current"]
        self.assertEqual(summary["files"], ["lib.rs"])
        self.assertIsNone(summary["branches"]["percent"])

    def test_clean_source_is_captured_before_unignored_reports_are_created(self):
        self.source_dirty = False
        self.invoke()
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertFalse(manifest["source_dirty"])
        self.assertTrue((self.output / "toolchain.txt").is_file())

    def test_threshold_failure_preserves_report_and_logs(self):
        with self.assertRaises(ValueError):
            self.invoke("--min-line-coverage", "90")
        self.assertTrue((self.output / "summary.json").is_file())
        self.assertTrue((self.output / "current.log").is_file())
        self.assertTrue((self.output / "current.txt").is_file())

    def test_failed_tests_preserve_logs_and_never_invoke_llvm(self):
        self.cargo_failure = True
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertEqual((self.output / "current.log").read_text(), "synthetic test failure\n")
        self.assertFalse((self.output / "summary.json").exists())
        self.assertEqual(len(self.calls), 1)

    def test_existing_output_directory_is_rejected_before_cargo(self):
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_text("not stale data to overwrite")
        with self.assertRaises(FileExistsError):
            self.invoke()
        self.assertEqual(self.calls, [])
        self.assertEqual(sentinel.read_text(), "not stale data to overwrite")

    def test_missing_profiles_or_objects_never_produce_passing_summary(self):
        for absent in ["emit_profiles", "emit_objects"]:
            with self.subTest(absent=absent):
                self.output = self.root / absent
                self.emit_profiles = self.emit_objects = True
                setattr(self, absent, False)
                with self.assertRaises(ValueError):
                    self.invoke()
                self.assertFalse((self.output / "summary.json").exists())


class DirectoryDiscoveryTests(unittest.TestCase):
    def test_directory_targets_join_top_level_targets_without_ui_cases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tests = root / "crates/rydantic/tests"
            for relative in ["simple.rs", "nested/main.rs", "ui/fail.rs", "compile/main.rs"]:
                path = tests / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("// fixture")
            self.assertEqual(coverage.discover_tests(root), ["nested", "simple"])


if __name__ == "__main__":
    unittest.main()
