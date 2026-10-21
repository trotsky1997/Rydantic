#!/usr/bin/env python3
"""Measure default-core runtime coverage with matching rustup LLVM tools.

Discover integration suites automatically, excluding the compile/trybuild harness.
Optional features, doctests, examples and other workspace crates remain excluded.
Use --compare-regressions for the historical PR #76 comparison instead.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EXISTING = [
    "models", "features", "raw_input", "integer_exponents", "schema",
    "schema_paths", "async_stream", "callback_hygiene", "validator_execution",
]
REGRESSIONS = ["core_contracts", "stream_failures", "borrowed_contracts", "collection_contracts"]


def instrumented_environment(environ):
    env = dict(environ)
    env.update(CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
    env.setdefault("CARGO_BUILD_JOBS", "2")
    flags = (env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
             if "CARGO_ENCODED_RUSTFLAGS" in env else env.get("RUSTFLAGS", "").split())
    normalized = []
    index = 0
    while index < len(flags):
        flag = flags[index]
        if flag == "-C" and index + 1 < len(flags) and flags[index + 1].split("=", 1)[0] == "instrument-coverage":
            index += 2
        elif flag.split("=", 1)[0] == "-Cinstrument-coverage":
            index += 1
        else:
            if flag:
                normalized.append(flag)
            index += 1
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(normalized + ["-C", "instrument-coverage"])
    env.pop("RUSTFLAGS", None)
    return env


def discover_tests(root):
    test_root = root / "crates/rydantic/tests"
    names = {path.stem for path in test_root.glob("*.rs")}
    names.update(path.parent.name for path in test_root.glob("*/main.rs"))
    return sorted(names - {"compile"})


def collect_objects(output):
    objects = []
    for line in output.splitlines():
        try:
            artifact = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (isinstance(artifact, dict) and artifact.get("reason") == "compiler-artifact"
                and isinstance(artifact.get("executable"), str) and artifact["executable"]
                and isinstance(artifact.get("profile"), dict) and artifact["profile"].get("test") is True):
            if artifact["executable"] not in objects:
                objects.append(artifact["executable"])
    return objects


def summarize_coverage(export, source_root):
    """Use validated per-file production counters, never dependency/global totals."""
    try:
        return _summarize_coverage(export, source_root)
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError("Malformed LLVM coverage export") from error


def _summarize_coverage(export, source_root):
    source_root = source_root.resolve()
    summary = {name: {"count": 0, "covered": 0} for name in ["lines", "functions", "regions", "branches"]}
    files = []
    for datum in export["data"]:
        for entry in datum["files"]:
            path = Path(entry["filename"]).resolve()
            try:
                relative = path.relative_to(source_root).as_posix()
            except ValueError as error:
                raise ValueError(f"Coverage contains non-core source: {path}") from error
            if path.suffix != ".rs" or relative in files:
                raise ValueError(f"Invalid or duplicate coverage source: {path}")
            files.append(relative)
            for name, total in summary.items():
                metric = entry["summary"][name]
                count, covered = metric["count"], metric["covered"]
                if (type(count) is not int or type(covered) is not int
                        or count < 0 or not 0 <= covered <= count):
                    raise ValueError(f"Invalid {name} counters for {relative}")
                total["count"] += count
                total["covered"] += covered
    if not files or not summary["lines"]["count"] or not summary["functions"]["count"]:
        raise ValueError("Coverage is empty: executable core lines and functions are required")
    for metric in summary.values():
        metric["percent"] = 100 * metric["covered"] / metric["count"] if metric["count"] else None
    summary["files"] = sorted(files)
    return summary


def check_thresholds(summary, min_line, min_function):
    for name, minimum in [("lines", min_line), ("functions", min_function)]:
        if minimum is None:
            continue
        if not math.isfinite(minimum) or not 0 <= minimum <= 100:
            raise ValueError("Coverage thresholds must be finite percentages between 0 and 100")
        metric = summary[name]
        if not metric["count"] or 100 * metric["covered"] < minimum * metric["count"]:
            raise ValueError(f"{name} coverage {metric['covered']}/{metric['count']} is below {minimum}%")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--compare-regressions", action="store_true")
    parser.add_argument("--output-dir", type=Path, help="New, non-existing directory for reports")
    parser.add_argument("--min-line-coverage", type=float)
    parser.add_argument("--min-function-coverage", type=float)
    args = parser.parse_args()
    for value in [args.min_line_coverage, args.min_function_coverage]:
        if value is not None and (not math.isfinite(value) or not 0 <= value <= 100):
            parser.error("Coverage thresholds must be finite percentages between 0 and 100")
    env = instrumented_environment(os.environ)
    target = Path(env.get("CARGO_TARGET_DIR", ROOT / "target/core-coverage")).resolve()
    env["CARGO_TARGET_DIR"] = str(target)
    rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.split(": ", 1)[1] for line in rustc.splitlines() if line.startswith("host:"))
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    llvm = Path(sysroot) / "lib/rustlib" / host / "bin"
    suffix = ".exe" if os.name == "nt" else ""
    cov, profdata = (llvm / (name + suffix) for name in ["llvm-cov", "llvm-profdata"])
    if not cov.is_file() or not profdata.is_file():
        parser.error("Install llvm-tools-preview for the active Rust toolchain first.")
    # Capture source provenance before our own output can make a clean tree dirty.
    source_revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    source_dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    if args.output_dir:
        output = args.output_dir.resolve()
        output.mkdir(parents=True, exist_ok=False)  # Never mix stale profiles into a run.
    else:
        target.mkdir(parents=True, exist_ok=True)
        output = Path(tempfile.mkdtemp(prefix="report-", dir=target))
    (output / "toolchain.txt").write_text(rustc)
    phases = [("existing", EXISTING), ("regressions", REGRESSIONS)] if args.compare_regressions else [("current", discover_tests(ROOT))]
    if not all(tests for _, tests in phases):
        raise ValueError("No runtime test suites discovered")
    manifest = {
        "scope": "default-core-runtime", "features": ["derive"],
        "excluded_harnesses": ["compile"], "phases": dict(phases),
        "toolchain": rustc, "source_revision": source_revision,
        "source_dirty": source_dirty,
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "rust_flags": env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"),
        "commands": [],
        "minimum_lines": args.min_line_coverage, "minimum_functions": args.min_function_coverage,
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    objects = []
    for phase, tests in phases:
        env["LLVM_PROFILE_FILE"] = str(output / phase / "%m-%p.profraw")
        command = ["cargo", "test", "-p", "rydantic", "--locked", "--no-default-features", "--features", "derive", "--message-format=json"]
        if args.offline:
            command.append("--offline")
        if phase != "regressions":
            command.append("--lib")
        for test in tests:
            command += ["--test", test]
        manifest["commands"].append(command)
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / f"{phase}.log").write_text(result.stdout)
        for executable in collect_objects(result.stdout):
            if executable not in objects:
                objects.append(executable)
        if result.returncode:
            raise SystemExit(f"Tests failed; inspect {output / (phase + '.log')}")
    if not objects:
        raise ValueError("Cargo produced no test executable artifacts")
    source_root = ROOT / "crates/rydantic/src"
    sources = sorted(str(path) for path in source_root.rglob("*.rs"))
    reports = [("baseline", ["existing"]), ("expanded", ["existing", "regressions"])] if args.compare_regressions else [("current", ["current"])]
    summaries = {}
    for phase, directories in reports:
        profiles = [str(path) for directory in directories for path in (output / directory).glob("*.profraw")]
        if not profiles:
            raise ValueError(f"No instrumented profiles for {phase}")
        merged = output / f"{phase}.profdata"
        subprocess.run([str(profdata), "merge", "-sparse", *profiles, "-o", str(merged)], check=True)
        # Historical comparisons use exactly the same object/source universe.
        options = [objects[0], *[f"-object={obj}" for obj in objects[1:]], f"-instr-profile={merged}", *sources]
        for mode, extension in [("report", "txt"), ("export", "json"), ("show", "show.txt")]:
            with (output / f"{phase}.{extension}").open("w") as handle:
                subprocess.run([str(cov), mode, *options], stdout=handle, check=True)
        summaries[phase] = summarize_coverage(json.loads((output / f"{phase}.json").read_text()), source_root)
        summaries[phase]["unreported_sources"] = sorted(set(Path(p).relative_to(source_root).as_posix() for p in sources) - set(summaries[phase]["files"]))
        print(f"\n{phase}:\n{(output / (phase + '.txt')).read_text()}")
    (output / "summary.json").write_text(json.dumps(summaries, indent=2) + "\n")
    print(f"Reports and exact toolchain: {output}")
    print("Scope: emitted default-core runtime sources only; zero branch counters mean not measured.")
    check_thresholds(summaries[reports[-1][0]], args.min_line_coverage, args.min_function_coverage)


if __name__ == "__main__":
    main()
