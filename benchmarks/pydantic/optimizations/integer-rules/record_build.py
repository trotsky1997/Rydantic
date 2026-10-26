#!/usr/bin/env python3
"""Build a frozen worker, retaining source, receipt and binary provenance."""
import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import subprocess
from common import artifact_from_receipt, digest, snapshot, write_json


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--target-dir", type=Path, required=True)
    output = p.add_mutually_exclusive_group(required=True)
    output.add_argument("--output", type=Path, help="Build manifest; .jsonl/.log siblings retain raw Cargo output")
    output.add_argument("--output-dir", type=Path, help="Write LABEL.json plus raw .jsonl/.log receipts here")
    p.add_argument("--label", default="build", help="Filename stem when using --output-dir")
    args = p.parse_args()
    if args.output_dir:
        if not args.label or Path(args.label).name != args.label:
            p.error("label must be a filename stem, not a path")
        args.output = args.output_dir / (args.label + ".json")
    root, target = args.root.resolve(), args.target_dir.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    receipt, log = args.output.with_suffix(".jsonl"), args.output.with_suffix(".log")
    if any(path.exists() for path in (args.output, receipt, log)):
        raise ValueError("refusing to overwrite an existing build evidence file")
    sources = snapshot(root)
    started = datetime.now(timezone.utc).isoformat()
    command = ["cargo", "build", "--locked", "--offline", "--release", "-p", "rydantic",
               "--example", "pydantic_benchmark", "--message-format=json"]
    env = dict(os.environ, CARGO_TARGET_DIR=str(target))
    with receipt.open("w") as out, log.open("w") as err:
        completed = subprocess.run(command, cwd=root, env=env, stdout=out, stderr=err, check=False)
    if completed.returncode:
        raise RuntimeError(f"build failed with {completed.returncode}; raw logs retained")
    if snapshot(root) != sources:
        raise RuntimeError("source changed while building; manifest not created")
    binary = target / "release/examples/pydantic_benchmark"
    artifact = artifact_from_receipt(receipt, binary)
    data = dict(format_version=1, root=str(root), target_dir=str(target), executable=str(binary),
                executable_sha256=digest(binary), source_snapshot=sources,
                command=command, started=started, finished=datetime.now(timezone.utc).isoformat(),
                profile=artifact["profile"], features=artifact["features"], fresh=artifact["fresh"],
                receipt=str(receipt.resolve()), receipt_sha256=digest(receipt),
                log=str(log.resolve()), log_sha256=digest(log),
                rustc=subprocess.check_output(["rustc", "-Vv"], text=True),
                cargo=subprocess.check_output(["cargo", "-V"], text=True),
                environment={key: env.get(key) for key in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"] +
                             ["CARGO_PROFILE_RELEASE_" + name for name in ["OPT_LEVEL", "CODEGEN_UNITS", "LTO", "DEBUG", "DEBUG_ASSERTIONS", "OVERFLOW_CHECKS", "PANIC"]]},
                revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                git_status=subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).strip())
    write_json(args.output, data)
    print(f"Fresh worker verified: {binary.name}; SHA256 {data['executable_sha256']}", flush=True)


if __name__ == "__main__":
    main()
