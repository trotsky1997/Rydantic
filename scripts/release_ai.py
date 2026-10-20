#!/usr/bin/env python3
"""Independently versioned, fail-closed local releases for rydantic-ai."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tomllib

sys.dont_write_bytecode = True
from release import VERSION, package_checksum, registry_version, wait_for_publication

REPOSITORY = "trotsky1997/Rydantic"
CRATE = "rydantic-ai"
TAG_PREFIX = "rydantic-ai-v"


def command(root, *args):
    return subprocess.check_output(args, cwd=root, text=True).strip()


def validate_manifests(root, tag):
    version = tag.removeprefix(TAG_PREFIX)
    if not tag.startswith(TAG_PREFIX) or not VERSION.fullmatch(version):
        raise ValueError("AI tags must use rydantic-ai-vMAJOR.MINOR.PATCH")
    manifest = tomllib.loads((root / "crates/rydantic-ai/Cargo.toml").read_text())
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    if manifest["package"]["name"] != CRATE or manifest["package"]["version"] != version:
        raise ValueError("AI tag and independent package version differ")
    if "crates/rydantic-ai" not in workspace["workspace"]["members"]:
        raise ValueError("AI crate is absent from the workspace")
    return version


def validate_ci_runs(runs, sha):
    matches = [r for r in runs if r["head_sha"] == sha and r["head_branch"] == "main" and r["event"] == "push"]
    if not matches:
        raise ValueError("No exact-commit main-push CI for the AI release")
    latest = max(matches, key=lambda r: (r["id"], r.get("run_attempt", 1)))
    if latest["status"] != "completed" or latest["conclusion"] != "success":
        raise ValueError("Latest exact-commit main-push CI is not successful")


def preflight(root, tag):
    version = validate_manifests(root, tag)
    if command(root, "gh", "api", "user", "--jq", ".login") != "trotsky1997":
        raise ValueError("Use the trotsky1997 GitHub identity for this release")
    if command(root, "git", "status", "--porcelain"):
        raise ValueError("AI releases require a clean working tree")
    sha = command(root, "git", "rev-parse", "HEAD")
    if command(root, "git", "rev-parse", f"refs/tags/{tag}^{{commit}}") != sha:
        raise ValueError("AI release tag must match the checked-out commit")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], cwd=root, check=True)
    remote = command(root, "git", "-c", "credential.helper=", "-c",
                     "credential.helper=!gh auth git-credential", "ls-remote",
                     f"https://github.com/{REPOSITORY}.git", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}")
    refs = {line.split()[1]: line.split()[0] for line in remote.splitlines() if line.strip()}
    if refs.get(f"refs/tags/{tag}^{{}}", refs.get(f"refs/tags/{tag}")) != sha:
        raise ValueError("Remote AI release tag does not match the checkout")
    endpoint = f"repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={sha}&event=push&per_page=100"
    runs = json.loads(command(root, "gh", "api", endpoint))["workflow_runs"]
    validate_ci_runs(runs, sha)
    if registry_version(CRATE, version) is not None:
        raise ValueError("AI version already exists; inspect the existing publication before recovery")
    print(f"Verified {CRATE} {version} at tested main commit {sha}")
    return version, sha


def audit_package(root, version):
    metadata = json.loads(command(root, "cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"))
    path = Path(metadata["target_directory"]) / "package" / f"{CRATE}-{version}.crate"
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            parts = Path(member.name).parts
            if "node_modules" in parts or ".git" in parts or Path(member.name).name in {"auth.json", ".env", ".npmrc"} or member.name.endswith((".key", ".pem")):
                raise ValueError("Package contains installed dependencies or a credential file")


def publish(root, tag):
    version, sha = preflight(root, tag)
    subprocess.run(["cargo", "publish", "--locked", "--dry-run", "-p", CRATE], cwd=root, check=True)
    checksum = package_checksum(root, CRATE, version)
    audit_package(root, version)
    # A nonzero exit can still follow an accepted upload. Observe the outcome;
    # never send a second upload from this invocation.
    outcome = subprocess.run(["cargo", "publish", "--locked", "-p", CRATE], cwd=root)
    print(f"AI upload exit status: {outcome.returncode}; verifying registry outcome")
    wait_for_publication(CRATE, version, checksum)
    print(json.dumps({"crate": CRATE, "version": version, "sha": sha, "checksum": checksum}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["preflight", "publish"])
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.command == "preflight":
        preflight(root, args.tag)
    else:
        publish(root, args.tag)


if __name__ == "__main__":
    main()
