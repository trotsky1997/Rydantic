#!/usr/bin/env python3
"""Fail-closed release preflight and single-attempt crates.io publishing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import tomllib
import urllib.error
import urllib.request

REPOSITORY = "trotsky1997/Rydantic"
CRATES = ("rydantic-derive", "rydantic")
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-beta\.(0|[1-9][0-9]*))?")


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def get_json(url, token=None):
    headers = {"User-Agent": "Rydantic-release (https://github.com/trotsky1997/Rydantic)"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
        headers["Accept"] = "application/vnd.github+json"
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def registry_version(crate, version):
    try:
        return get_json(f"https://crates.io/api/v1/crates/{crate}/{version}")["version"]
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise  # Rate limits, outages and authentication failures are NOT absence.


def validate_manifests(root, tag):
    if not tag.startswith("v") or not VERSION.fullmatch(tag[1:]):
        raise ValueError("Tag must be vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-beta.N")
    version = tag[1:]
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    if workspace["workspace"]["package"]["version"] != version:
        raise ValueError("Tag and workspace version differ")
    for crate in CRATES:
        manifest = tomllib.loads((root / "crates" / crate / "Cargo.toml").read_text())
        if manifest["package"]["name"] != crate or manifest["package"]["version"] != {"workspace": True}:
            raise ValueError(f"Unexpected package identity/version: {crate}")
    runtime = tomllib.loads((root / "crates/rydantic/Cargo.toml").read_text())
    if runtime["dependencies"]["rydantic-derive"]["version"] != f"={version}":
        raise ValueError("Runtime must pin rydantic-derive to the exact release version")
    return version


def require_successful_ci(sha, token):
    # The latest CI push run for this exact commit must have succeeded, not an older rerun.
    url = f"https://api.github.com/repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={sha}&event=push&per_page=100"
    runs = get_json(url, token)["workflow_runs"]
    matching = [r for r in runs if r["head_sha"] == sha and r["head_branch"] == "main"]
    if not matching:
        raise ValueError("No main-push CI run for the release commit")
    latest = max(matching, key=lambda r: (r["id"], r.get("run_attempt", 1)))
    if latest["status"] != "completed" or latest["conclusion"] != "success":
        raise ValueError(f"Release CI is not successful: {latest['html_url']}")
    print(f"Verified CI: {latest['html_url']}")


def preflight(root, tag):
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY or os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Dispatch this workflow on the original repository's main branch only")
    version = validate_manifests(root, tag)
    sha = run("git", "rev-parse", "HEAD")
    if run("git", "rev-parse", f"refs/tags/{tag}^{{commit}}") != sha:
        raise ValueError("Checkout does not match the selected tag")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    require_successful_ci(sha, os.environ["GH_TOKEN"])
    for crate in CRATES:
        if registry_version(crate, version) is not None:
            raise ValueError(f"{crate} {version} already exists; do not republish. Inspect any partial release before recovery.")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"sha={sha}\nversion={version}\n")


def wait_for_publication(crate, version, checksum, attempts=30):
    # Observation retries only. cargo publish is never retried here.
    for attempt in range(attempts):
        info = registry_version(crate, version)
        if info is not None:
            if info["checksum"] != checksum or info.get("yanked"):
                raise ValueError(f"{crate} {version}: published checksum/state differs from this package")
            # Cargo's sparse index must also see the new dependency before building runtime.
            with urllib.request.urlopen(urllib.request.Request(
                f"https://index.crates.io/ry/da/{crate}",
                headers={"User-Agent": "Rydantic-release"}), timeout=30) as response:
                records = [json.loads(line) for line in response if line.strip()]
            if any(r["vers"] == version and r["cksum"] == checksum and not r["yanked"] for r in records):
                print(f"Verified https://crates.io/crates/{crate}/{version}")
                return
        if attempt + 1 < attempts:
            time.sleep(10)
    raise RuntimeError(f"Publication outcome not confirmed for {crate} {version}; inspect registry before any retry")


def publish(root, tag, crate):
    version = validate_manifests(root, tag)
    if registry_version(crate, version) is not None:
        raise ValueError(f"{crate} {version} already exists; refusing a duplicate upload")
    subprocess.run(["cargo", "publish", "--locked", "--dry-run", "-p", crate], check=True)
    archive = root / "target/package" / f"{crate}-{version}.crate"
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    # Even an error may mean the server accepted the upload. Observe, never blindly resend.
    outcome = subprocess.run(["cargo", "publish", "--locked", "-p", crate])
    print(f"cargo publish exit status: {outcome.returncode}; verifying registry outcome")
    wait_for_publication(crate, version, checksum)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["preflight", "publish"])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--crate", choices=CRATES)
    args = parser.parse_args()
    root = Path.cwd()
    if args.command == "preflight":
        preflight(root, args.tag)
    else:
        if args.crate is None:
            parser.error("publish requires --crate")
        publish(root, args.tag, args.crate)


if __name__ == "__main__":
    main()
