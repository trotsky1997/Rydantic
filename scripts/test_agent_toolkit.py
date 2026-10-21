"""Local packaging/compatibility checks, not an Agent trial or official spec validator."""
import json
from pathlib import Path
import re
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

    def test_versioned_compatibility_matches_actual_artifacts(self):
        compat = json.loads((SKILL / "compatibility.json").read_text())
        crate = tomllib.loads((CLI / "Cargo.toml").read_text())
        knowledge = json.loads((CLI / "knowledge/stable-0.6.0.json").read_text())
        schema = json.loads((CLI / "protocol-v1.schema.json").read_text())
        self.assertEqual(compat["cli"]["version"], crate["package"]["version"])
        self.assertFalse(crate["package"]["publish"])
        self.assertEqual(compat["cli"]["protocol"], schema["properties"]["schema_version"]["const"])
        self.assertEqual(compat["cli"]["knowledge_version"], knowledge["knowledge_version"])
        self.assertEqual(compat["cli"]["commands"], ["capabilities", "doctor", "explain"])
        self.assertEqual(compat["beta_tracks"], [])
        self.assertEqual(compat["cli"]["core_versions"], [knowledge["core_version"]])
        tracks = compat["core_tracks"]
        self.assertEqual([t["version"] for t in tracks], ["0.6.0", "0.7.0"])
        for track in tracks:
            with self.subTest(version=track["version"]):
                if track["version"] in compat["cli"]["core_versions"]:
                    self.assertEqual(set(track["available_features"]), set(knowledge["features"]) - {"default"})
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
