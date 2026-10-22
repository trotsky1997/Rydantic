#!/usr/bin/env python3
"""Run trusted source-pack harness fixtures offline. Never runs an agent/provider."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT / 'packs/source-60067205276e314653f46a75c213ec1756c7ad7c'


def installed_binary(value, name):
    path = Path(value)
    if not path.is_absolute() or not path.is_file():
        raise ValueError(name + ' must be an absolute, already-installed toolchain binary')
    output = subprocess.run([str(path), '--version'], check=True, capture_output=True, text=True).stdout
    if not output.startswith(name + ' 1.98.1 '):
        raise ValueError(name + ' must be the installed exact 1.98.1 binary')
    return str(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo', required=True)
    parser.add_argument('--rustc', required=True)
    args = parser.parse_args()
    cargo = installed_binary(args.cargo, 'cargo')
    rustc = installed_binary(args.rustc, 'rustc')
    env = dict(os.environ, CARGO_NET_OFFLINE='true', CARGO_INCREMENTAL='0',
               CARGO_PROFILE_DEV_DEBUG='0', CARGO_BUILD_JOBS='1', RUSTC=rustc)
    fixtures = [
        ('reference_solutions/source-context-policy.rs', True),
        ('reference_mutants/source-context-policy-no-hook.rs', False),
        ('reference_mutants/source-context-policy-default-context.rs', False),
    ]
    with tempfile.TemporaryDirectory(prefix='rydantic-source-harness-') as directory:
        for index, (fixture, should_pass) in enumerate(fixtures):
            candidate = Path(directory) / str(index)
            shutil.copytree(PACK / 'tasks/source-context-policy', candidate)
            shutil.copyfile(ROOT / 'tests' / fixture, candidate / 'src/lib.rs')
            (candidate / 'tests').mkdir()
            shutil.copyfile(PACK / 'hidden/source-context-policy.rs',
                            candidate / 'tests/hidden_contract.rs')
            command = [cargo, 'test', '--frozen', '--offline', '--manifest-path',
                       str(candidate / 'Cargo.toml'), '--test', 'hidden_contract']
            # Compile every control first: a compiler error is NOT a successful
            # behavioral negative control. Only the pinned trusted fixtures run.
            subprocess.run(command + ['--no-run'], cwd=candidate, env=env, check=True)
            result = subprocess.run(command, cwd=candidate, env=env,
                                    capture_output=True, text=True)
            print(f'=== Harness fixture: {fixture} ===', flush=True)
            print(result.stdout, end='', flush=True)
            print(result.stderr, end='', flush=True)
            if should_pass:
                if result.returncode != 0:
                    raise RuntimeError('reference fixture did not pass its hidden contract')
            elif result.returncode != 101 or 'test result: FAILED.' not in result.stdout:
                raise RuntimeError('negative control was not rejected by behavioral tests')
    print('Harness only: reference passed; both compiling mutants failed. No agent trials.')


if __name__ == '__main__':
    main()
