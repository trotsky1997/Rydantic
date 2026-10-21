#!/usr/bin/env python3
"""Compare default-core runtime coverage before/after the contract regression suites.

Requires matching rustup llvm-tools-preview. Does not run trybuild, optional
features, doctests, proc-macro coverage, or the other workspace crates.
"""
import argparse
import json
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    env = os.environ.copy()
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target" / "core-coverage"))
    env.update(CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
    env.setdefault("CARGO_BUILD_JOBS", "2")
    # Cargo gives encoded flags priority. Canonicalize coverage flags rather than
    # accumulating duplicates and forcing a redundant instrumented rebuild.
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
    rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.split(": ", 1)[1] for line in rustc.splitlines() if line.startswith("host:"))
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    llvm = Path(sysroot) / "lib" / "rustlib" / host / "bin"
    suffix = ".exe" if os.name == "nt" else ""
    cov, profdata = (llvm / (name + suffix) for name in ["llvm-cov", "llvm-profdata"])
    if not cov.is_file() or not profdata.is_file():
        parser.error("Install llvm-tools-preview for the active Rust toolchain first.")
    target = Path(env["CARGO_TARGET_DIR"]).resolve()
    env["CARGO_TARGET_DIR"] = str(target)
    target.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="report-", dir=target))
    (output / "toolchain.txt").write_text(rustc)
    objects = []
    for phase, tests in [("existing", EXISTING), ("regressions", REGRESSIONS)]:
        env["LLVM_PROFILE_FILE"] = str(output / phase / "%m-%p.profraw")
        command = ["cargo", "test", "-p", "rydantic", "--locked", "--no-default-features", "--features", "derive", "--message-format=json"]
        if args.offline:
            command.append("--offline")
        if phase == "existing":
            command.append("--lib")
        for test in tests:
            command += ["--test", test]
        result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / f"{phase}.log").write_text(result.stdout)
        for line in result.stdout.splitlines():
            try:
                artifact = json.loads(line)
            except json.JSONDecodeError:
                continue
            if artifact.get("reason") == "compiler-artifact" and artifact.get("executable") and artifact["profile"]["test"]:
                if artifact["executable"] not in objects:
                    objects.append(artifact["executable"])
        if result.returncode:
            raise SystemExit(f"Tests failed; inspect {output / (phase + '.log')}")
    # Both reports use the SAME object/source universe. Newly instantiated generic
    # functions are thus visible as uncovered in the existing-suite baseline.
    sources = sorted(str(path) for path in (ROOT / "crates/rydantic/src").rglob("*.rs"))
    for phase, directories in [("baseline", ["existing"]), ("expanded", ["existing", "regressions"])]:
        profiles = [str(path) for directory in directories for path in (output / directory).glob("*.profraw")]
        merged = output / f"{phase}.profdata"
        subprocess.run([str(profdata), "merge", "-sparse", *profiles, "-o", str(merged)], check=True)
        options = [objects[0], *[f"-object={obj}" for obj in objects[1:]], f"-instr-profile={merged}", *sources]
        for mode, extension in [("report", "txt"), ("export", "json"), ("show", "show.txt")]:
            with (output / f"{phase}.{extension}").open("w") as handle:
                subprocess.run([str(cov), mode, *options], stdout=handle, check=True)
        print(f"\n{phase}:\n{(output / (phase + '.txt')).read_text()}")
    print(f"Reports and exact toolchain: {output}")
    print("Scope: default core runtime sources only. Stable Rust branch coverage is unavailable, not 100%.")


if __name__ == "__main__":
    main()
