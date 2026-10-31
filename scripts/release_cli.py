#!/usr/bin/env python3
"""Read-only CLI release gates. This module never uploads or obtains credentials.

The independent cli-v* tag namespace does not release the Rust libraries. A
missing registry name is not evidence that this repository owns it: first
publication/owner setup remains a separate explicitly authorized operation.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib
import urllib.error

import release

CRATE = "rydantic-cli"
OWNER = "trotsky1997"


def validate_dependency(spec):
    if isinstance(spec, str):
        if not spec:
            raise ValueError("Dependency needs a registry version")
        return
    if not isinstance(spec, dict):
        raise ValueError("Invalid dependency declaration")
    if spec.get("workspace") is True:
        raise ValueError("Unresolved inherited dependency")
    if any(key in spec for key in ("path", "git", "registry", "registry-index")):
        raise ValueError("CLI dependencies must use the default crates.io registry")
    if not isinstance(spec.get("version"), str) or not spec["version"]:
        raise ValueError("Dependency needs a registry version")


def validate_manifests(root, tag):
    if not tag.startswith("cli-v") or not release.VERSION.fullmatch(tag[5:]):
        raise ValueError("Tag must be cli-vMAJOR.MINOR.PATCH or cli-vMAJOR.MINOR.PATCH-beta.N")
    version = tag[5:]
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    manifest = tomllib.loads((root / "crates" / CRATE / "Cargo.toml").read_text())
    package = manifest["package"]
    if package.get("name") != CRATE or package.get("version") != version:
        raise ValueError("CLI package identity/version and tag differ")
    if package.get("publish") != ["crates-io"]:
        raise ValueError("CLI must publish only to crates-io")
    if manifest.get("bin") != [{"name": "rydantic", "path": "src/main.rs"}]:
        raise ValueError("Unexpected CLI executable")
    for source in (workspace, manifest):
        if "patch" in source or "replace" in source:
            raise ValueError("Release cannot use patch/replace source overrides")
    tables = [manifest]
    tables.extend(manifest.get("target", {}).values())
    for table in tables:
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            for name, spec in table.get(kind, {}).items():
                if isinstance(spec, dict) and spec.get("workspace") is True:
                    if any(key in spec for key in ("path", "git", "registry", "registry-index", "version")):
                        raise ValueError("Ambiguous inherited dependency")
                    spec = workspace.get("workspace", {}).get("dependencies", {}).get(name)
                validate_dependency(spec)
    return version


def require_registry_owner():
    try:
        info = release.get_json(f"https://crates.io/api/v1/crates/{CRATE}")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            raise ValueError("First CLI publication requires separately authorized name/owner setup; no upload attempted") from error
        raise
    if info.get("crate", {}).get("id") != CRATE:
        raise ValueError("Unexpected registry package identity")
    owners = release.get_json(f"https://crates.io/api/v1/crates/{CRATE}/owners")
    if not any(owner.get("login") == OWNER and owner.get("kind") == "user" for owner in owners.get("users", [])):
        raise ValueError("Expected CLI registry owner not verified")


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def require_cli_package_ci(sha, token):
    url = (f"https://api.github.com/repos/{release.REPOSITORY}/actions/workflows/"
           f"cli-package.yml/runs?head_sha={sha}&event=push&per_page=100")
    runs = release.get_json(url, token)["workflow_runs"]
    matching = [run for run in runs if run["head_sha"] == sha and run["head_branch"] == "main"
                and run.get("event") == "push"]
    if not matching:
        raise ValueError("No main-push CLI package acceptance for release commit")
    latest = max(matching, key=lambda run: (run["id"], run.get("run_attempt", 1)))
    if latest["status"] != "completed" or latest["conclusion"] != "success":
        raise ValueError(f"CLI package acceptance is not successful: {latest['html_url']}")
    print(f"Verified CLI package acceptance: {latest['html_url']}")


def preflight(root, tag):
    if os.environ.get("GITHUB_REPOSITORY") != release.REPOSITORY or os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Dispatch on original repository main only")
    version = validate_manifests(root, tag)
    sha = git(root, "rev-parse", "HEAD")
    if git(root, "rev-parse", f"refs/tags/{tag}^{{commit}}") != sha:
        raise ValueError("Checkout does not match CLI tag")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], cwd=root, check=True)
    release.require_successful_ci(sha, os.environ["GH_TOKEN"])
    require_cli_package_ci(sha, os.environ["GH_TOKEN"])
    if os.environ["GITHUB_SHA"] != sha:
        release.require_successful_ci(os.environ["GITHUB_SHA"], os.environ["GH_TOKEN"])
        require_cli_package_ci(os.environ["GITHUB_SHA"], os.environ["GH_TOKEN"])
    if release.registry_version(CRATE, version) is not None:
        raise ValueError("CLI version already published; inspect registry, never blindly republish")
    require_registry_owner()
    result = {"crate": CRATE, "version": version, "sha": sha, "uploaded": False,
              "publication_authorized": False}
    print(json.dumps(result, sort_keys=True))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check-manifest", "preflight"])
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    if args.command == "check-manifest":
        print(validate_manifests(Path.cwd(), args.tag))
    else:
        preflight(Path.cwd(), args.tag)


if __name__ == "__main__":
    main()
