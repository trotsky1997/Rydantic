"""Offline independent-version and single-upload regression tests."""
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import release_ai


class ReleaseAI(unittest.TestCase):
    def fixture(self, root):
        (root / "crates/rydantic-ai").mkdir(parents=True)
        (root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/rydantic-ai"]\n[workspace.package]\nversion="0.7.0-beta.3"\n')
        (root / "crates/rydantic-ai/Cargo.toml").write_text('[package]\nname="rydantic-ai"\nversion="0.1.1"\n')

    def test_independent_version_ignores_core_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            self.assertEqual(release_ai.validate_manifests(root, "rydantic-ai-v0.1.1"), "0.1.1")
            for tag in ["v0.7.0-beta.3", "rydantic-ai-v0.1.0", "rydantic-ai-v01.1.1", "rydantic-ai-v0.1.1;bad"]:
                with self.assertRaises(ValueError):
                    release_ai.validate_manifests(root, tag)

    def test_latest_ci_must_succeed_for_exact_main_push(self):
        success = dict(id=1, head_sha="sha", head_branch="main", event="push", status="completed", conclusion="success")
        release_ai.validate_ci_runs([success], "sha")
        for changes in [dict(id=2, conclusion="failure"), dict(id=2, status="in_progress", conclusion=None)]:
            with self.assertRaises(ValueError):
                release_ai.validate_ci_runs([success, success | changes], "sha")
        with self.assertRaises(ValueError):
            release_ai.validate_ci_runs([success | {"head_sha": "other"}], "sha")
        with self.assertRaises(ValueError):
            release_ai.validate_ci_runs([success | {"event": "pull_request"}], "sha")

    def test_single_upload_and_observation_after_nonzero_exit(self):
        with patch.object(release_ai, "preflight", return_value=("0.1.1", "sha")), patch.object(release_ai, "package_checksum", return_value="checksum"), patch.object(release_ai, "audit_package"), patch.object(release_ai, "wait_for_publication") as observe, patch.object(release_ai.subprocess, "run", side_effect=[subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 1)]) as run:
            release_ai.publish(Path("."), "rydantic-ai-v0.1.1")
            self.assertEqual(run.call_count, 2)
            self.assertIn("--dry-run", run.call_args_list[0].args[0])
            self.assertNotIn("--dry-run", run.call_args_list[1].args[0])
            self.assertEqual(run.call_args_list[1].args[0][-1], "rydantic-ai")
            observe.assert_called_once_with("rydantic-ai", "0.1.1", "checksum")

    def test_package_audit_rejects_installed_dependencies_and_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target/package"
            target.mkdir(parents=True)
            for filename in ["src/lib.rs", "bridge/node_modules/sdk/README.md", "bridge/auth.json", "bridge/.env", "gateway.key"]:
                archive = target / "rydantic-ai-0.1.1.crate"
                with tarfile.open(archive, "w:gz") as tar:
                    info = tarfile.TarInfo(f"rydantic-ai-0.1.1/{filename}")
                    info.size = 1
                    tar.addfile(info, io.BytesIO(b"x"))
                with patch.object(release_ai, "command", return_value=json.dumps({"target_directory": str(root / "target")})):
                    if filename == "src/lib.rs":
                        release_ai.audit_package(root, "0.1.1")
                    else:
                        with self.assertRaises(ValueError):
                            release_ai.audit_package(root, "0.1.1")


if __name__ == "__main__":
    unittest.main()
