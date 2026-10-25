"""Synthetic local Git/Cargo transport checks; no network or credentials required."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import with_source_checkout as helper


class SourceCheckoutTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "source with spaces"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "CI fixture")
        self.git("config", "user.email", "ci@example.invalid")
        (self.repo / "Cargo.toml").write_text('[package]\nname="source-fixture"\nversion="0.1.0"\nedition="2021"\n')
        (self.repo / "src").mkdir()
        (self.repo / "src/lib.rs").write_text("pub fn value() -> u8 { 1 }\n")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.revision = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def test_environment_is_scoped_and_preserves_source_identity(self):
        before = os.environ.copy()
        config = (self.repo / ".git/config").read_bytes()
        env = helper.fetch_environment(self.repo, self.revision)
        self.assertEqual(env["GIT_CONFIG_VALUE_0"], helper.REPOSITORY)
        self.assertEqual(env["GIT_CONFIG_KEY_0"], f"url.{self.repo.as_uri()}.insteadOf")
        self.assertEqual(env["CARGO_NET_GIT_FETCH_WITH_CLI"], "true")
        self.assertEqual(dict(os.environ), before)
        self.assertEqual((self.repo / ".git/config").read_bytes(), config)

    def test_ambient_global_rewrite_and_credentials_are_isolated(self):
        home = self.root / "home"
        home.mkdir()
        (home / ".gitconfig").write_text(
            '[url "https://example.invalid/remote"]\n'
            '    insteadOf = ' + helper.REPOSITORY + '\n'
            '[credential]\n    helper = synthetic-must-not-run\n')
        with patch.dict(os.environ, {"HOME": str(home)}):
            env = helper.fetch_environment(self.repo, self.revision)
        resolved = subprocess.check_output(
            ["git", "ls-remote", "--get-url", helper.REPOSITORY], env=env, text=True).strip()
        self.assertEqual(resolved, self.repo.as_uri())
        credential = subprocess.run(["git", "config", "--get-all", "credential.helper"],
                                    env=env, capture_output=True, text=True)
        self.assertEqual(credential.returncode, 1)
        self.assertEqual(env["GIT_ALLOW_PROTOCOL"], "file")
        self.assertEqual(env["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(env["GIT_CONFIG_GLOBAL"], os.devnull)

    def test_wrong_revision_and_non_root_fail_closed(self):
        for path, revision in [(self.repo, "0" * 40), (self.repo / "src", self.revision)]:
            with self.subTest(path=path), self.assertRaises(ValueError):
                helper.fetch_environment(path, revision)

    def test_shallow_checkout_is_rejected(self):
        shallow = self.root / "shallow"
        subprocess.run(["git", "clone", "-q", "--depth", "1", self.repo.as_uri(), str(shallow)], check=True)
        with self.assertRaisesRegex(ValueError, "complete history"):
            helper.fetch_environment(shallow, self.revision)

    def test_authentication_and_transport_overrides_are_rejected(self):
        for key in ["http.https://github.com/.extraheader", "credential.helper", "url.file:///other.insteadOf",
                    "remote.origin.promisor", "extensions.partialclone"]:
            with self.subTest(key=key):
                self.git("config", key, "synthetic-not-a-secret")
                with self.assertRaises(ValueError):
                    helper.fetch_environment(self.repo, self.revision)
                self.git("config", "--unset", key)
        with patch.dict(os.environ, {"GIT_CONFIG_COUNT": "0"}):
            with self.assertRaises(ValueError):
                helper.fetch_environment(self.repo, self.revision)

    def test_cargo_fetch_preserves_git_lock_and_uses_no_network(self):
        consumer = self.root / "consumer"
        consumer.mkdir()
        (consumer / "src").mkdir()
        (consumer / "src/lib.rs").write_text("pub fn value() -> u8 { source_fixture::value() }\n")
        (consumer / "Cargo.toml").write_text(
            '[package]\nname="consumer"\nversion="0.1.0"\nedition="2021"\n'
            '[dependencies]\nsource-fixture={git="' + helper.REPOSITORY + '",rev="' + self.revision + '"}\n')
        env = helper.fetch_environment(self.repo, self.revision)
        env["CARGO_HOME"] = str(self.root / "cargo")
        # Any non-file Git transport must fail, not silently authenticate remotely.
        env["GIT_ALLOW_PROTOCOL"] = "file"
        subprocess.run(["cargo", "generate-lockfile"], cwd=consumer, env=env, check=True, capture_output=True)
        lock = (consumer / "Cargo.lock").read_bytes()
        self.assertIn(f"git+{helper.REPOSITORY}?rev={self.revision}#{self.revision}".encode(), lock)
        # Baseline negative: no rewrite must fail in a cold cache, without network.
        baseline = env.copy()
        for key in ("GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0"):
            del baseline[key]
        baseline["CARGO_HOME"] = str(self.root / "baseline-cargo")
        baseline["CARGO_NET_RETRY"] = "0"
        failure = subprocess.run(["cargo", "fetch", "--locked"], cwd=consumer,
                                 env=baseline, capture_output=True, text=True)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn("transport 'https' not allowed", failure.stderr)
        # A cold cache proves the lock is satisfied via local Git, not a warm cache.
        env["CARGO_HOME"] = str(self.root / "cold-cargo")
        subprocess.run(["cargo", "fetch", "--locked"], cwd=consumer, env=env, check=True, capture_output=True)
        self.assertEqual((consumer / "Cargo.lock").read_bytes(), lock)
        fetched = list((Path(env["CARGO_HOME"]) / "git/checkouts").glob("*/*/src/lib.rs"))
        self.assertEqual(len(fetched), 1)
        self.assertEqual(fetched[0].read_bytes(), (self.repo / "src/lib.rs").read_bytes())

    def test_workflow_keeps_checkout_before_project_execution(self):
        workflow = (Path(__file__).resolve().parents[1] / ".github/workflows/ci.yml").read_text()
        job = workflow.split("  test:\n", 1)[1].split("  compiler-paths-windows:", 1)[0]
        checkout = job.split("      - name: Check out the pinned source track\n", 1)[1]
        self.assertLess(job.index("Check out the pinned source track"), job.index("        run:"))
        for setting in ("repository: trotsky1997/Rydantic", f"ref: {helper.REVISION}",
                        "fetch-depth: 0", "persist-credentials: false"):
            self.assertIn(setting, checkout.split("      - name:", 1)[0])
        self.assertNotIn("persist-credentials: true", workflow)
        self.assertNotIn("GITHUB_ENV", job)
        self.assertIn("--locked --offline", job)
        self.assertEqual(job.count("scripts/with_source_checkout.py .ci-source-6006720 -- cargo fetch"), 2)

    def test_command_exit_code_and_environment(self):
        with patch.object(helper, "fetch_environment", return_value={"SCOPED": "1"}) as verify:
            with patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 17)) as run:
                self.assertEqual(helper.main(["source", "--", "cargo", "fetch"]), 17)
                verify.assert_called_once_with("source")
                run.assert_called_once_with(["cargo", "fetch"], env={"SCOPED": "1"}, check=False)


if __name__ == "__main__":
    unittest.main()
