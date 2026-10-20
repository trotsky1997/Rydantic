import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def manifests(self, root, version="0.7.0-beta.1", dependency="=0.7.0-beta.1"):
        (root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
        for crate in release.CRATES:
            directory = root / "crates" / crate
            directory.mkdir(parents=True)
            content = f'[package]\nname = "{crate}"\nversion.workspace = true\n'
            if crate == "rydantic":
                content += f'[dependencies]\nrydantic-derive = {{version = "{dependency}"}}\n'
            (directory / "Cargo.toml").write_text(content)

    def test_valid_beta(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifests(root)
            self.assertEqual(release.validate_manifests(root, "v0.7.0-beta.1"), "0.7.0-beta.1")

    def test_tag_rejection(self):
        for tag in ["main", "v0.7.0;echo pwn", "v0.7.0-beta.01", "v00.7.0", "v0.7.0-rc.1", "v0.7.0\n"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.validate_manifests(Path("/unused"), tag)

    def test_exact_dependency_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifests(root, dependency="0.7.0-beta.1")
            with self.assertRaisesRegex(ValueError, "exact"):
                release.validate_manifests(root, "v0.7.0-beta.1")

    def test_version_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifests(root)
            with self.assertRaisesRegex(ValueError, "differ"):
                release.validate_manifests(root, "v0.7.0")

    def test_registry_absence_only_404(self):
        for status in [403, 404, 429, 500]:
            error = urllib.error.HTTPError("url", status, "error", {}, None)
            with patch.object(release, "get_json", side_effect=error):
                if status == 404:
                    self.assertIsNone(release.registry_version("rydantic", "0.7.0"))
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        release.registry_version("rydantic", "0.7.0")

    def test_latest_ci_must_pass(self):
        good = dict(id=1, head_sha="sha", head_branch="main", status="completed", conclusion="success", html_url="https://example.test/1")
        bad = dict(good, id=2, conclusion="failure")
        for runs in [[], [bad], [good, bad], [dict(good, status="in_progress")]]:
            with patch.object(release, "get_json", return_value={"workflow_runs": runs}), self.assertRaises(ValueError):
                release.require_successful_ci("sha", "token")
        with patch.object(release, "get_json", return_value={"workflow_runs": [good]}):
            release.require_successful_ci("sha", "token")

    def test_duplicate_never_uploads(self):
        with patch.object(release, "validate_manifests", return_value="0.7.0"), patch.object(release, "registry_version", return_value={}), patch.object(release.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "duplicate"):
                release.publish(Path("."), "v0.7.0", "rydantic")
            run.assert_not_called()

    def test_failed_upload_is_observed_not_retried(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "target/package/rydantic-0.7.0.crate"
            archive.parent.mkdir(parents=True)
            archive.write_bytes(b"package")
            with patch.object(release, "validate_manifests", return_value="0.7.0"), patch.object(release, "registry_version", return_value=None), patch.object(release.subprocess, "run") as run, patch.object(release, "wait_for_publication") as observe:
                run.return_value.returncode = 1
                release.publish(root, "v0.7.0", "rydantic")
                self.assertEqual(run.call_count, 2)  # One dry run, one upload.
                self.assertIn("--dry-run", run.call_args_list[0].args[0])
                self.assertNotIn("--dry-run", run.call_args_list[1].args[0])
                observe.assert_called_once()

    def test_publication_checks_checksum_and_yank(self):
        for info in [{"checksum": "wrong"}, {"checksum": "ok", "yanked": True}]:
            with patch.object(release, "registry_version", return_value=info), self.assertRaises(ValueError):
                release.wait_for_publication("rydantic", "0.7.0", "ok", attempts=1)

    def test_unconfirmed_does_not_upload(self):
        with patch.object(release, "registry_version", return_value=None), patch.object(release.subprocess, "run") as run, self.assertRaisesRegex(RuntimeError, "before any retry"):
            release.wait_for_publication("rydantic", "0.7.0", "ok", attempts=1)
        run.assert_not_called()

    def test_api_and_index_confirmation(self):
        response = io.BytesIO(json.dumps({"vers": "0.7.0", "cksum": "ok", "yanked": False}).encode())
        with patch.object(release, "registry_version", return_value={"checksum": "ok", "yanked": False}), patch.object(release.urllib.request, "urlopen", return_value=response):
            release.wait_for_publication("rydantic", "0.7.0", "ok", attempts=1)


if __name__ == "__main__":
    unittest.main()
