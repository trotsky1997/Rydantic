#!/usr/bin/env python3
"""Install an extracted package OR registry release, then test without source/PATH.

Archive mode is prepublication evidence, never a claim of registry publication.
A new temporary Cargo home is used by default. --cargo-home explicitly permits a
prepared dependency cache; its use is included in the receipt.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


def check_binary(binary, cwd, version):
    env = {"PATH": "", "HOME": str(cwd), "USERPROFILE": str(cwd),
           "CARGO_HOME": str(cwd / "cargo-home"), "CARGO_NET_OFFLINE": "true"}
    # Windows needs its system directory to load system DLLs, not command lookup.
    if "SystemRoot" in os.environ:
        env["SystemRoot"] = os.environ["SystemRoot"]
    def invoke(args, expected=0):
        result = subprocess.run([str(binary), *args, "--format", "json", "--protocol-version", "1"],
                                cwd=cwd, env=env, capture_output=True, text=True, check=False)
        assert result.returncode == expected, (args, result.returncode, result.stderr)
        assert result.stderr == "" and result.stdout.endswith("\n")
        assert len(result.stdout.splitlines()) == 1
        value = json.loads(result.stdout)
        assert value["schema_version"] == "rydantic.cli/v1"
        assert value["tool"] == {"name": "rydantic", "version": version}
        assert value["ok"] == (expected == 0)
        assert value["artifacts"] == []
        return value
    capabilities = invoke(["capabilities"])
    assert capabilities["data"]["knowledge_versions"] == ["0.6.0", "0.7.0", "0.8.0"]
    assert capabilities["data"]["project_code_executed"] is False
    assert capabilities["data"]["linked_engines"] == {"core": None, "codegen": None, "backend": None}
    for core in ("0.6.0", "0.7.0", "0.8.0"):
        known = invoke(["explain", "RD_LOCAL_RULES_REQUIRED", "--core-version", core])
        assert known["data"]["explanations"][0]["known"] is True
        unknown = invoke(["explain", "PRIVATE_UNKNOWN_CODE", "--core-version", core])
        assert unknown["data"]["explanations"][0]["known"] is False
        assert "PRIVATE_UNKNOWN_CODE" not in json.dumps(unknown)
    invoke(["explain", "RD_LOCAL_RULES_REQUIRED", "--core-version", "0.99.0"], 3)
    assert list(cwd.iterdir()) == [], "CLI wrote runtime files"
    return capabilities


def unpack(archive, destination, version):
    prefix = f"rydantic-cli-{version}"
    with tarfile.open(archive, "r:gz") as package:
        members = package.getmembers()
        for member in members:
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or path.parts[0] != prefix:
                raise ValueError("Unexpected package path")
            if not (member.isfile() or member.isdir()):
                raise ValueError("Package contains a link or special file")
        package.extractall(destination, members=members, filter="data")
    source = destination / prefix
    manifest = tomllib.loads((source / "Cargo.toml").read_text())
    assert manifest["package"]["name"] == "rydantic-cli"
    assert manifest["package"]["version"] == version
    assert "workspace" not in manifest and "patch" not in manifest and "replace" not in manifest
    assert (source / "Cargo.lock").is_file()
    for name in ("LICENSE", "protocol-v1.schema.json", "README.md"):
        assert (source / name).is_file(), name
    for dependency in manifest["dependencies"].values():
        if isinstance(dependency, dict):
            assert not {"path", "git", "registry", "registry-index"} & dependency.keys()
    lock = tomllib.loads((source / "Cargo.lock").read_text())
    for package in lock["package"]:
        if package["name"] != "rydantic-cli":
            assert package["source"] == "registry+https://github.com/rust-lang/crates.io-index"
            assert len(package["checksum"]) == 64
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--archive", type=Path)
    source.add_argument("--registry", action="store_true")
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--cargo-home", type=Path)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--work-parent", type=Path)
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()
    cargo = shutil.which(args.cargo)
    if cargo is None:
        parser.error("cargo executable not found")
    archive = args.archive.resolve() if args.archive else None
    with tempfile.TemporaryDirectory(prefix="cli-consumer-", dir=args.work_parent) as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        # No preexisting user Cargo configuration, credentials, path override or
        # release target reuse in the default fresh-home verification.
        for key in list(env):
            if key.startswith("CARGO_"):
                del env[key]
        env["CARGO_HOME"] = str(args.cargo_home.resolve() if args.cargo_home else root / "cargo-home")
        env["CARGO_TARGET_DIR"] = str(root / "target")
        install = root / "install"
        command = [cargo, "install", "--locked", "--root", str(install), "--bin", "rydantic"]
        if args.offline:
            command.append("--offline")
        if archive:
            source = unpack(archive, root / "source", args.version)
            # Test the shipped tests, not workspace binaries.
            subprocess.run([cargo, "test", "--manifest-path", str(source / "Cargo.toml"), "--locked",
                            *(["--offline"] if args.offline else [])], cwd=root, env=env, check=True)
            command += ["--path", str(source)]
        else:
            command += ["--registry", "crates-io", "--version", "=" + args.version, "rydantic-cli"]
        subprocess.run(command, cwd=root, env=env, check=True)
        if archive:
            shutil.rmtree(root / "source")
        shutil.rmtree(root / "target", ignore_errors=True)
        runtime = root / "empty-runtime"
        runtime.mkdir()
        capabilities = check_binary(install / "bin" / ("rydantic.exe" if os.name == "nt" else "rydantic"), runtime, args.version)
        receipt = {"mode": "extracted-package" if archive else "registry-install",
                   "version": args.version, "fresh_cargo_home": args.cargo_home is None,
                   "offline_build": args.offline, "capabilities": capabilities,
                   "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest() if archive else None,
                   "agent_abc_trials": "not-executed"}
        if args.receipt:
            args.receipt.write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
