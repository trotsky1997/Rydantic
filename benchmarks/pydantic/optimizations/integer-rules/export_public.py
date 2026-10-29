#!/usr/bin/env python3
"""Create a path-redacted public derivative without overwriting private evidence."""
import argparse
import copy
import gzip
import hashlib
import json
from pathlib import Path
import re
from common import RUST_ENGINES


def export(raw):
    original = json.loads(raw)
    if "public_export" in original:
        raise ValueError("expected original, not already exported evidence")
    out = copy.deepcopy(original)
    out["python_executable"] = "artifact:python-interpreter"
    out["commands"] = {engine: ([f"artifact:{engine}-worker"] if engine != "pydantic" else
                                ["artifact:python-interpreter", "benchmarks/pydantic/python_worker.py"])
                       for engine in original["commands"]}
    out["platform"] = "Linux x86_64 shared cloud host (OS build identifier redacted)"
    for engine in RUST_ENGINES:
        build = out["builds"][engine]
        for field in ("root", "target_dir", "executable", "receipt", "log"):
            build[field] = f"artifact:{engine}-{field}"
    out["public_export"] = dict(
        private_original_sha256=hashlib.sha256(raw).hexdigest(),
        removed=["absolute source, build and executable paths", "detailed operating-system build identifier"],
        preserved=["all raw samples and outputs", "all source and executable hashes", "build receipt/log hashes and profiles",
                   "compiler versions and flags", "CPU model and affinity", "timing configuration, order, UTC intervals and load"],
        note="Artifact labels are not executable paths; use your own source roots and manifests when reproducing.")
    encoded = (json.dumps(out, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()
    if re.search(rb"/(?:workspace|home|Users|tmp|private)/", encoded):
        raise ValueError("unredacted local path remains; review environment flags or metadata before publishing")
    return encoded


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("private_original", type=Path)
    parser.add_argument("public_output", type=Path)
    args = parser.parse_args()
    if args.public_output.exists() or args.private_original.resolve() == args.public_output.resolve():
        raise ValueError("refusing to overwrite input or existing public artifact")
    raw = export(args.private_original.read_bytes())
    args.public_output.write_bytes(gzip.compress(raw, mtime=0) if args.public_output.suffix == ".gz" else raw)
    print(f"Public uncompressed JSON SHA256: {hashlib.sha256(raw).hexdigest()}")


if __name__ == "__main__":
    main()
