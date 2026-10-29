#!/usr/bin/env python3
"""Record a fresh, source-attested diagnostic-probe build; never time probes here."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib
from common import artifact_from_receipt, digest, snapshot, write_json


def probe_snapshot(manifest):
    manifest = Path(manifest)
    root = manifest.parent
    paths = set(root.glob("src/**/*.rs")) | {manifest, root / "Cargo.lock"}
    return {str(path.relative_to(root)): digest(path) for path in sorted(paths)}


def normalized_manifest(manifest):
    value = tomllib.loads(Path(manifest).read_text())
    value["dependencies"]["rydantic"]["path"] = "artifact:runtime-source"
    return value


def environment():
    keys = ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"] + [
        "CARGO_PROFILE_RELEASE_" + name for name in
        ["OPT_LEVEL", "CODEGEN_UNITS", "LTO", "DEBUG", "DEBUG_ASSERTIONS", "OVERFLOW_CHECKS", "PANIC"]]
    return {key: os.environ.get(key) for key in keys}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--manifest", type=Path, required=True)
    p.add_argument("--target-dir", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--bins", nargs="+", required=True)
    args = p.parse_args()
    root, manifest, target = args.root.resolve(), args.manifest.resolve(), args.target_dir.resolve()
    if len(set(args.bins)) != len(args.bins) or any(not name.replace("_", "").isalnum() for name in args.bins):
        p.error("unique alphanumeric/underscore binary names required")
    dependency = tomllib.loads(manifest.read_text())["dependencies"]["rydantic"]["path"]
    if (manifest.parent / dependency).resolve() != root / "crates/rydantic":
        raise ValueError("probe manifest does not point to the declared runtime root")
    receipt, log = args.output.with_suffix(".jsonl"), args.output.with_suffix(".log")
    if any(path.exists() for path in (args.output, receipt, log)):
        raise ValueError("refusing to overwrite build evidence")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    sources, probes = snapshot(root), probe_snapshot(manifest)
    command = ["cargo", "build", "--manifest-path", str(manifest), "--locked", "--offline", "--release", "--message-format=json"]
    for name in args.bins:
        command += ["--bin", name]
    started = datetime.now(timezone.utc).isoformat()
    with receipt.open("w") as out, log.open("w") as err:
        result = subprocess.run(command, cwd=root, env=dict(os.environ, CARGO_TARGET_DIR=str(target)), stdout=out, stderr=err)
    if result.returncode:
        raise RuntimeError(f"probe build failed ({result.returncode}); original logs retained")
    if sources != snapshot(root) or probes != probe_snapshot(manifest):
        raise RuntimeError("runtime/probe source changed during build")
    artifacts = {}
    for name in args.bins:
        binary = target / "release" / name
        artifact = artifact_from_receipt(receipt, binary, expected_name=name)
        artifacts[name] = dict(executable=str(binary), executable_sha256=digest(binary), profile=artifact["profile"],
                               features=artifact["features"], fresh=artifact["fresh"])
    normalized = normalized_manifest(manifest)
    write_json(args.output, dict(format_version=1, purpose="diagnostic probe build attestation", root=str(root),
                                manifest=str(manifest), target_dir=str(target), source_snapshot=sources,
                                probe_snapshot=probes, normalized_manifest=normalized,
                                normalized_manifest_sha256=hashlib.sha256(json.dumps(normalized, sort_keys=True).encode()).hexdigest(),
                                artifacts=artifacts, command=command, started=started, finished=datetime.now(timezone.utc).isoformat(),
                                receipt=str(receipt.resolve()), receipt_sha256=digest(receipt), log=str(log.resolve()), log_sha256=digest(log),
                                rustc=subprocess.check_output(["rustc", "-Vv"], text=True), cargo=subprocess.check_output(["cargo", "-V"], text=True),
                                environment=environment(), revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                                git_status=subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).strip()))
    print("Fresh diagnostic artifacts verified: " + ", ".join(args.bins), flush=True)


if __name__ == "__main__":
    main()
