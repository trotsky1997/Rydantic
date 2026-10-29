"""Shared, side-effect-free evidence helpers."""
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path

ENGINES = ("baseline", "control", "candidate", "pydantic")
RUST_ENGINES = ENGINES[:-1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def snapshot(root):
    root = Path(root)
    paths = set(root.glob("crates/**/*.rs")) | set(root.glob("crates/**/Cargo.toml"))
    paths |= {root / "Cargo.toml", root / "Cargo.lock"}
    harness = root / "benchmarks/pydantic"
    paths |= set(harness.glob("*.py")) | {harness / "fixtures.json", harness / "requirements.txt"}
    return {str(p.relative_to(root)): digest(p) for p in sorted(paths)}


def canonical_harness(root):
    path = Path(root) / "benchmarks/pydantic/run.py"
    spec = importlib.util.spec_from_file_location("canonical_benchmark", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_json(path):
    path = Path(path)
    raw = path.read_bytes()
    return json.loads(gzip.decompress(raw) if path.suffix == ".gz" else raw)


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n")


def engine_kind(engine):
    return "pydantic" if engine == "pydantic" else "rydantic"


def orders(rounds):
    # Two positions per engine in each round; reverse the first position order
    # in alternating rounds. Different engine positions remain directly visible.
    forward = list(ENGINES)
    return [(forward if i % 2 == 0 else forward[::-1]) +
            (forward[::-1] if i % 2 == 0 else forward) for i in range(rounds)]


def request(case, mode, iterations, warmup, repeats):
    return dict(schema=case["schema"], input=case["input"], mode=mode,
                iterations=iterations, warmup=warmup, repeats=repeats)


def artifact_from_receipt(receipt, binary, expected_name="pydantic_benchmark"):
    rows = [json.loads(line) for line in Path(receipt).read_text().splitlines() if line.strip()]
    if not any(row.get("reason") == "build-finished" and row.get("success") is True for row in rows):
        raise ValueError("Cargo receipt has no successful build-finished event")
    workers = [row for row in rows if row.get("reason") == "compiler-artifact" and
               row.get("target", {}).get("name") == expected_name and row.get("executable")]
    if len(workers) != 1:
        raise ValueError("expected exactly one benchmark worker artifact")
    artifact = workers[0]
    profile = artifact["profile"]
    if (str(profile.get("opt_level")) != "3" or profile.get("debug_assertions") is not False or
            profile.get("debuginfo") != 0 or profile.get("overflow_checks") is not False or artifact["fresh"] is not False):
        raise ValueError("expected a freshly compiled opt-level3, debug-free release worker")
    if Path(artifact["executable"]).resolve() != Path(binary).resolve():
        raise ValueError("Cargo receipt executable differs from requested binary")
    return artifact
