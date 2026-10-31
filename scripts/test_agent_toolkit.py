"""Local packaging/compatibility checks, not an Agent trial or official spec validator."""
import json
from pathlib import Path
import re
import shutil
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "skills/rydantic"
CLI = ROOT / "crates/rydantic-cli"


class ToolkitPackagingTests(unittest.TestCase):
    def test_skill_metadata_and_local_references(self):
        text = (SKILL / "SKILL.md").read_text()
        self.assertTrue(text.startswith("---\n"))
        metadata = text.split("---", 2)[1]
        for field in ("name: rydantic", "description:", "license: MIT", "compatibility:"):
            self.assertIn(field, metadata)
        self.assertNotIn("allowed-tools:", metadata)
        for path in SKILL.rglob("*.md"):
            for target in re.findall(r"\]\(([^)]+)\)", path.read_text()):
                if "://" in target or target.startswith("#"):
                    continue
                resolved = (path.parent / target.split("#", 1)[0]).resolve()
                self.assertTrue(resolved.is_relative_to(ROOT), (path, target))
                self.assertTrue(resolved.exists(), (path, target))

    def check_source_track(self, skill):
        compat = json.loads((skill / "compatibility.json").read_text())
        self.assertEqual(len(compat["source_tracks"]), 1)
        track = compat["source_tracks"][0]
        revision = "60067205276e314653f46a75c213ec1756c7ad7c"
        repository = "https://github.com/trotsky1997/Rydantic"
        self.assertEqual(track["revision"], revision)
        self.assertEqual(track["version"], "0.8.0-beta.1")
        self.assertEqual(track["derive_version"], "0.8.0-beta.1")
        self.assertEqual(track["repository"], repository)
        self.assertEqual(track["distribution"], "unpublished-workspace-snapshot")
        self.assertEqual(track["source"], f"git+{repository}?rev={revision}#{revision}")
        self.assertEqual(track["consumer_features"], ["derive"])
        self.assertEqual(track["consumer_toolchain"], "1.98.1")
        for key in ("reference", "consumer_manifest", "consumer_lock"):
            self.assertTrue((skill / track[key]).is_file())
        manifest = tomllib.loads((skill / track["consumer_manifest"]).read_text())
        self.assertEqual(manifest["workspace"], {})
        toolchain = tomllib.loads((skill / track["consumer_manifest"]).with_name("rust-toolchain.toml").read_text())
        self.assertEqual(toolchain["toolchain"]["channel"], track["consumer_toolchain"])
        self.assertFalse({"patch", "replace"} & manifest.keys())
        for name in ("rydantic", "rydantic-derive"):
            self.assertEqual(manifest["dependencies"][name], {
                "git": repository, "rev": revision, "version": "=0.8.0-beta.1"})
        lock = tomllib.loads((skill / track["consumer_lock"]).read_text())
        for name in ("rydantic", "rydantic-derive"):
            packages = [p for p in lock["package"] if p["name"] == name]
            self.assertEqual(len(packages), 1)
            self.assertEqual(packages[0]["version"], "0.8.0-beta.1")
            self.assertEqual(packages[0]["source"], track["source"])
            self.assertNotIn("checksum", packages[0])
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("cargo test --manifest-path skills/rydantic/" + track["consumer_manifest"] + " --locked", workflow)
        self.assertEqual(len(compat["source_recipes"]), 1)
        recipe = compat["source_recipes"][0]
        self.assertEqual(recipe["track"], track["id"])
        self.assertEqual(recipe["manifest"], track["consumer_manifest"])
        self.assertTrue(recipe["requires_execution_authorization"])
        self.assertEqual(recipe["command"], ["cargo", "+1.98.1", "test", "--manifest-path", track["consumer_manifest"], "--frozen"])

    def test_exact_source_track_matches_consumer(self):
        self.check_source_track(SKILL)

    def test_source_track_rejects_wrong_revision_and_registry_substitution(self):
        with tempfile.TemporaryDirectory() as temp:
            copied = Path(temp) / "skill"
            shutil.copytree(SKILL, copied, ignore=shutil.ignore_patterns("target"))
            manifest_path = copied / "assets/source-6006720/Cargo.toml"
            original = manifest_path.read_text()
            for altered in (
                original.replace("60067205276e314653f46a75c213ec1756c7ad7c", "0" * 40),
                original.replace('git = "https://github.com/trotsky1997/Rydantic", ', ''),
                original + '\n[patch.crates-io]\nrydantic = { path = "/tmp/override" }\n',
            ):
                with self.subTest(manifest=altered):
                    manifest_path.write_text(altered)
                    with self.assertRaises(AssertionError):
                        self.check_source_track(copied)
            manifest_path.write_text(original)
            lock_path = copied / "assets/source-6006720/Cargo.lock"
            lock_path.write_text(lock_path.read_text().replace(
                'git+https://github.com/trotsky1997/Rydantic?rev=60067205276e314653f46a75c213ec1756c7ad7c#60067205276e314653f46a75c213ec1756c7ad7c',
                'registry+https://github.com/rust-lang/crates.io-index'))
            with self.assertRaises(AssertionError):
                self.check_source_track(copied)

    def test_versioned_compatibility_matches_actual_artifacts(self):
        compat = json.loads((SKILL / "compatibility.json").read_text())
        crate = tomllib.loads((CLI / "Cargo.toml").read_text())
        knowledge = {v: json.loads((CLI / f"knowledge/stable-{v}.json").read_text()) for v in ("0.6.0", "0.7.0", "0.8.0")}
        schema = json.loads((CLI / "protocol-v1.schema.json").read_text())
        self.assertEqual(compat["cli"]["version"], crate["package"]["version"])
        self.assertFalse(crate["package"]["publish"])
        self.assertEqual(compat["cli"]["protocol"], schema["properties"]["schema_version"]["const"])
        for version, bundle in knowledge.items():
            self.assertEqual(bundle["core_version"], version)
            self.assertEqual(compat["cli"]["knowledge_version"], bundle["knowledge_version"])
        self.assertEqual(compat["cli"]["commands"], ["capabilities", "doctor", "explain"])
        self.assertEqual(compat["beta_tracks"], [])
        self.assertEqual(compat["cli"]["core_versions"], list(knowledge))
        tracks = compat["core_tracks"]
        self.assertEqual([t["version"] for t in tracks], ["0.6.0", "0.7.0", "0.8.0"])
        for track in tracks:
            with self.subTest(version=track["version"]):
                if track["version"] in compat["cli"]["core_versions"]:
                    self.assertEqual(set(track["available_features"]), set(knowledge[track["version"]]["features"]) - {"default"})
                    self.assertEqual(track["default_features"], knowledge[track["version"]]["default_features"])
                for key in ("reference", "consumer_manifest", "consumer_lock"):
                    self.assertTrue((SKILL / track[key]).is_file())
                manifest = tomllib.loads((SKILL / track["consumer_manifest"]).read_text())
                self.assertEqual(manifest["dependencies"]["rydantic"]["version"], "=" + track["version"])
                self.assertEqual(manifest["dependencies"]["rydantic-derive"], "=" + track["derive_version"])
                self.assertEqual(set(manifest["dependencies"]["rydantic"]["features"]) | set(track["default_features"]), set(track["consumer_features"]))
                self.assertTrue(set(track["consumer_features"]) <= set(track["available_features"]))
                self.assertEqual(manifest["workspace"], {})
                self.assertFalse({"patch", "replace"} & manifest.keys())
                self.assertFalse({"path", "git", "registry"} & manifest["dependencies"]["rydantic"].keys())
                workflow = (ROOT / ".github/workflows/ci.yml").read_text()
                self.assertIn("cargo test --manifest-path skills/rydantic/" + track["consumer_manifest"] + " --locked", workflow)
                lock = tomllib.loads((SKILL / track["consumer_lock"]).read_text())
                for name, version in (("rydantic", track["version"]), ("rydantic-derive", track["derive_version"])):
                    packages = [p for p in lock["package"] if p["name"] == name]
                    self.assertEqual(len(packages), 1)
                    self.assertEqual(packages[0]["version"], version)
                    self.assertEqual(packages[0]["source"], track["source"])
                    self.assertRegex(packages[0]["checksum"], r"^[a-f0-9]{64}$")
        for recipe in compat["recipes"]:
            self.assertTrue((SKILL / recipe["manifest"]).is_file())
            self.assertTrue(recipe["requires_execution_authorization"])
            track = next(t for t in tracks if t["version"] == recipe["core_version"])
            self.assertEqual(recipe["manifest"], track["consumer_manifest"])
            self.assertIn(track["consumer_manifest"], recipe["command"])
            self.assertIn("--frozen", recipe["command"])
        self.assertEqual({r["core_version"] for r in compat["recipes"]}, {t["version"] for t in tracks})


if __name__ == "__main__":
    unittest.main()
