#!/usr/bin/env python3
"""Collect exact allocation counts; this driver has no timing measurements.

Arguments are baseline root, candidate root, baseline binary, candidate binary,
output JSON. Each case/mode gets a fresh process for genuinely cold rule caches.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def snapshot(root):
    paths = ['Cargo.toml', 'Cargo.lock']
    paths += [str(p.relative_to(root)) for crate in ['rydantic', 'rydantic-derive']
              for p in (root / 'crates' / crate).rglob('*')
              if p.is_file() and (p.suffix == '.rs' or p.name == 'Cargo.toml')]
    return {p: sha(root / p) for p in sorted(paths)}


def cases(root):
    rows = json.loads((root / 'benchmarks/pydantic/fixtures.json').read_text())['cases']
    extras = []
    def add(name, schema, text, modes=('json', 'value_copy')):
        extras.append(dict(name=name, schema=schema, input=text, modes=modes))
    add('empty', 'empty', '{}')
    add('empty_unknown', 'empty', '{"z":0,"a":1,"z":2}')
    add('escaped_known', 'flat', r'{"i\u0064":42,"na\u006de":"Ada","act\u0069ve":true}')
    add('duplicate_known', 'flat', '{"id":"invalid","id":42,"name":null,"name":"Ada","active":true}')
    add('duplicate_unknown', 'flat', '{"id":42,"name":"Ada","active":true,' + ','.join('"unknown":0' for _ in range(1000)) + '}')
    add('ignore_empty', 'ignore', '{"id":42}')
    add('ignore_many', 'ignore', '{"id":42,' + ','.join(f'"unknown{i}":{{"nested":[1,2,3]}}' for i in range(1000)) + '}')
    add('flatten_fallback', 'flatten', '{"id":42,"name":"Ada","active":true}')
    add('extra_fallback', 'extra', '{"id":42,"z":[1,2,3],"a":true}')
    add('tagged_fallback', 'tagged', '{"kind":"Item","id":42}')
    add('wide_sparse', 'wide', '{}')
    add('wide_one', 'wide', '{"f079":42}')
    add('wide_dense', 'wide', '{' + ','.join(f'"f{i:03}":{i}' for i in range(80)) + '}')
    add('wide_reverse', 'wide', '{' + ','.join(f'"f{i:03}":{i}' for i in reversed(range(80))) + '}')
    return rows, extras


def main():
    baseline, candidate, old_binary, new_binary, output = map(Path, sys.argv[1:])
    roots = {'baseline': baseline, 'candidate': candidate}
    binaries = {'baseline': old_binary, 'candidate': new_binary}
    sources = {label: snapshot(root) for label, root in roots.items()}
    hashes = {label: sha(binary) for label, binary in binaries.items()}
    canonical, controls = cases(candidate)
    rows = []
    for group, inputs in [('canonical', canonical), ('control', controls)]:
        for case in inputs:
            for mode in case['modes']:
                request = {'schema': case['schema'], 'input': case['input'], 'mode': mode}
                responses = {}
                for label, binary in binaries.items():
                    result = subprocess.run([str(binary)], input=json.dumps(request), text=True,
                                            check=True, capture_output=True)
                    if result.stderr:
                        raise RuntimeError(result.stderr)
                    responses[label] = json.loads(result.stdout)
                assert responses['baseline']['result']['outcome'] == responses['candidate']['result']['outcome'], case['name']
                for result in responses.values():
                    warm = result['result']['warm']
                    assert len(warm) == 5 and all(item == warm[0] for item in warm)
                rows.append(dict(group=group, case=case['name'], request=request, responses=responses))
    assert sources == {label: snapshot(root) for label, root in roots.items()}
    assert hashes == {label: sha(binary) for label, binary in binaries.items()}
    report = dict(format_version=1, kind='allocation_only_no_timing',
                  baseline_commit=subprocess.check_output(['git', '-C', str(baseline), 'rev-parse', 'HEAD'], text=True).strip(),
                  candidate_base=subprocess.check_output(['git', '-C', str(candidate), 'merge-base', 'HEAD', '2ce66b92'], text=True).strip(),
                  sources=sources, binary_sha256=hashes,
                  probe_sha256=sha(Path(__file__).with_name('allocation_probe.rs.txt')),
                  driver_sha256=sha(__file__), fixture_sha256=sha(candidate / 'benchmarks/pydantic/fixtures.json'),
                  compiler=subprocess.check_output(['rustc', '--version', '--verbose'], text=True).strip(),
                  compiler_binary_sha256=sha(subprocess.check_output(['rustup', 'which', 'rustc'], text=True).strip()),
                  probe_lock_sha256=sha(Path(__file__).with_name('allocation-probe.Cargo.lock')),
                  scope='Synthetic public repository fixtures and generated controls; allocation/reallocation calls and cumulative requested bytes, not live/peak memory or RSS.',
                  cold='First parse in a fresh process. Input preparation and output/error serialization outside counters.',
                  warm='Five identical measured parses after one additional unmeasured prewarm; result disposal outside counters.',
                  rows=rows)
    Path(output).write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(f'{len(rows)} case/mode rows; full outcomes equal; all warmed counters stable')


if __name__ == '__main__':
    main()
