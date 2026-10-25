#!/usr/bin/env python3
"""Run explicitly scoped optional-runtime suites; reject empty/ignored suites.

Correctness gate only: no instrumentation or numerical coverage claim.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LAYERS = {
    "jsonrpc": {"features": ["derive", "jsonrpc"], "test": "jsonrpc", "source": "jsonrpc.rs"},
    "settings": {"features": ["derive", "settings"], "test": "settings", "source": "settings.rs"},
    "partial-json": {"features": ["partial-json"], "test": "partial_json", "source": "partial_json.rs"},
}


def executable(output, suite):
    found = []
    for line in output.splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (isinstance(item, dict) and item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == suite
                and item.get("target", {}).get("kind") == ["test"]
                and item.get("profile", {}).get("test") is True
                and item.get("executable")):
            found.append(item["executable"])
    if len(set(found)) != 1:
        raise ValueError("Expected exactly one selected integration-test executable")
    return found[0]


def test_names(output, allow_empty=False):
    lines = [line for line in output.splitlines() if line]
    if not lines or re.fullmatch(r"\d+ tests?, 0 benchmarks?", lines[-1]) is None:
        raise ValueError("Missing or malformed libtest inventory total")
    entries = lines[:-1]
    if any(not line.endswith(": test") or not line.removesuffix(": test") for line in entries):
        raise ValueError("Unexpected libtest inventory entry")
    names = [line.removesuffix(": test") for line in entries]
    if int(lines[-1].split()[0]) != len(names) or len(names) != len(set(names)):
        raise ValueError("Inconsistent or duplicate runtime test inventory")
    if not names and not allow_empty:
        raise ValueError("Empty runtime test inventory")
    return names


def verify_result(output, count):
    lines = [line for line in output.splitlines() if line.startswith("test result:")]
    pattern = rf"test result: ok\. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in .+"
    # Settings intentionally spawns isolated child harnesses; their summaries
    # precede the final parent result. Children may filter tests, but cannot fail
    # or ignore them. The parent must still execute the complete inventory.
    child_pattern = r"test result: ok\. ([1-9]\d*) passed; 0 failed; 0 ignored; 0 measured; ([1-9]\d*) filtered out; finished in .+"
    children = [re.fullmatch(child_pattern, line) for line in lines[:-1]]
    if (not lines or re.fullmatch(pattern, lines[-1]) is None or count <= 0
            or any(match is None or sum(map(int, match.groups())) != count for match in children)):
        raise ValueError("Selected tests did not all execute successfully without skips")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--layer", choices=LAYERS, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    layer = LAYERS[args.layer]
    output = args.output_dir.resolve()
    env = dict(os.environ, CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
    env.setdefault("CARGO_BUILD_JOBS", "2")
    manifest = {
        "scope": "optional-runtime-correctness", "layer": args.layer,
        "features": layer["features"], "suite": layer["test"],
        "source_under_test": "crates/rydantic/src/" + layer["source"],
        "measured_source_universe": None,
        "exclusions": "No coverage counters, macros, other optional features, workspace crates, or external adoption claim.",
        "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()),
        "toolchain": subprocess.check_output(["rustc", "-vV"], cwd=ROOT, env=env, text=True),
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "rust_flags": {key: env.get(key) for key in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]},
        "source_hashes": {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in [
            "scripts/optional_runtime.py", f"crates/rydantic/tests/{layer['test']}.rs",
            f"crates/rydantic/src/{layer['source']}"]},
        "commands": [], "status": "running",
    }

    output.mkdir(parents=True, exist_ok=False)

    def save():
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

    def run(command, log):
        manifest["commands"].append(command)
        save()
        try:
            result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        except OSError as error:
            (output / log).write_text(str(error) + "\n")
            raise
        (output / log).write_text(result.stdout)
        result.check_returncode()
        return result.stdout

    def build(features, phase):
        command = ["cargo", "test", "-p", "rydantic", "--locked", "--no-default-features", "--test", layer["test"], "--no-run", "--message-format=json"]
        if features:
            command += ["--features", ",".join(features)]
        if args.offline:
            command.append("--offline")
        return executable(run(command, phase + "-build.log"), layer["test"])

    try:
        binary = build(layer["features"], "enabled")
        names = test_names(run([binary, "--list"], "enabled-list.log"))
        manifest["tests"] = names
        manifest["test_count"] = len(names)
        manifest["test_binary"] = binary
        verify_result(run([binary, "--test-threads=1", "--color=never"], "enabled-run.log"), len(names))
        # Same suite and supporting features, but deliberately omit its optional feature.
        disabled = [feature for feature in layer["features"] if feature != args.layer]
        binary = build(disabled, "disabled")
        listing = run([binary, "--list"], "disabled-list.log")
        if test_names(listing, allow_empty=True):
            raise ValueError("Disabled-feature negative control unexpectedly discovered tests")
        try:
            test_names(listing)
        except ValueError:
            pass  # Validated empty inventory is rejected by the enabled-suite guard.
        else:
            raise ValueError("Empty-suite guard failed its negative control")
        manifest["negative_control"] = "disabled optional feature: zero-test inventory rejected"
        manifest["status"] = "passed"
        print(f"{args.layer}: {len(names)} tests passed; disabled-feature empty-suite guard verified")
    except Exception:
        manifest["status"] = "failed"
        raise
    finally:
        save()


if __name__ == "__main__":
    main()
