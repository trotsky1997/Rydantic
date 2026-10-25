"""Run a CI fetch against verified Git objects, never an authenticated Cargo fetch.

This is trusted CI infrastructure, not an isolation boundary for real Agent trials.
The caller supplies a separate full-history checkout made by actions/checkout with
persist-credentials: false. Manifests and lockfiles keep their original Git source.
"""
import os
from pathlib import Path
import subprocess
import sys

REVISION = "60067205276e314653f46a75c213ec1756c7ad7c"
REPOSITORY = "https://github.com/trotsky1997/Rydantic"


def fetch_environment(checkout, revision=REVISION):
    checkout = Path(checkout).resolve(strict=True)
    env = os.environ.copy()
    # Do not silently combine transport overrides or inherited Git credentials.
    if any(key.startswith("GIT_CONFIG_") for key in env):
        raise ValueError("inherited GIT_CONFIG_* overrides are not supported")
    env.update({
        "GIT_NO_REPLACE_OBJECTS": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
        # Registry downloads are unaffected; this helper permits only local Git.
        "GIT_ALLOW_PROTOCOL": "file",
    })

    def git(*args):
        return subprocess.check_output(
            ["git", "-C", str(checkout), *args], env=env, text=True
        ).strip()

    if git("rev-parse", "--show-toplevel") != str(checkout):
        raise ValueError("expected a standalone checkout root")
    if git("rev-parse", "HEAD") != revision:
        raise ValueError("source checkout HEAD does not match the pinned revision")
    if git("rev-parse", "--is-shallow-repository") != "false":
        raise ValueError("source checkout must include complete history")
    config = git("config", "--local", "--name-only", "--list").lower().splitlines()
    if any(key.startswith("credential.") or key.endswith(".extraheader")
           or key.startswith(("url.", "include.", "includeif."))
           or key.endswith((".promisor", ".partialclonefilter"))
           or key == "extensions.partialclone" for key in config):
        raise ValueError("source checkout retains authentication or URL overrides")
    env.update({
        "CARGO_NET_GIT_FETCH_WITH_CLI": "true",
        "GIT_CONFIG_COUNT": "1",
        "GIT_CONFIG_KEY_0": f"url.{checkout.as_uri()}.insteadOf",
        "GIT_CONFIG_VALUE_0": REPOSITORY,
        "GIT_TERMINAL_PROMPT": "0",
    })
    return env


def main(argv):
    if len(argv) < 3 or argv[1] != "--":
        raise ValueError("usage: with_source_checkout.py CHECKOUT -- COMMAND [ARG ...]")
    env = fetch_environment(argv[0])
    return subprocess.run(argv[2:], env=env, check=False).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
