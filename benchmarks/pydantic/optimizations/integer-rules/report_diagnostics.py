#!/usr/bin/env python3
"""Audit and render all isolated allocation and rule-timing controls."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import statistics
from common import canonical_harness, load_json


def render(directory, root):
    allocation = [json.loads(line) for line in (directory / 'rule-allocations.jsonl').read_text().splitlines()]
    if len(allocation) != 36:
        raise ValueError('expected all 36 allocation rows')
    out = ['# Allocation and fallback diagnostics', '', 'Refs #125.', '',
           'These standalone probes are separate from the uninstrumented canonical end-to-end workers. Allocation counts use a System global allocator and count allocation/reallocation calls and cumulative requested bytes, not RSS, peak memory or time. Rule construction and Context initialization are outside those counts. Full errors are compared before rendering.', '',
           'Values are i64 = 42, i128 = i128::MIN and u128 = u128::MAX. Case names describe bound representations, not an assertion that every value passes. Generic and typed routes below both use the candidate library; this isolates the direct-integer route, not a separately compiled old-library timing comparison.', '',
           '## All isolated rule allocation controls', '',
           '| Type | Bounds | Generic calls / bytes | Typed calls / bytes | Accepted |',
           '|---|---|---:|---:|---|']
    by_pair = {}
    for row in allocation:
        key = row['type'], row['case'], row['route']
        if key in by_pair:
            raise ValueError('duplicate allocation row')
        by_pair[key] = row
    for row in allocation:
        if row['route'] != 'generic':
            continue
        other = by_pair[row['type'], row['case'], 'typed']
        if row['result'] != other['result']:
            raise ValueError('full rule errors differ')
        out.append(f"| {row['type']} | {row['case']} | {row['calls']} / {row['bytes']} | {other['calls']} / {other['bytes']} | {'Ok' in row['result']} |")
    parse = load_json(directory / 'parse-allocations.json')
    harness = canonical_harness(root)
    fixtures = json.loads((root / 'benchmarks/pydantic/fixtures.json').read_text())['cases']
    expected = {(case['name'], mode): case for case in fixtures for mode in case['modes']}
    rows = {(row['case'], row['mode'], row['engine']): row for row in parse['measurements']}
    if len(rows) != len(parse['measurements']) or len(rows) != 98:
        raise ValueError('expected all 98 unique parse allocation rows')
    out += ['', '## All parse allocation controls', '',
            'One warmup parse prepares static model rules before the measured operation; immutable input construction is excluded. Counts end when the validated parse result has been constructed, before its disposal; deallocation is not counted. These counts therefore must not be directly compared to earlier cold-first-parse allocation reports. Every one of the 98 outputs is checked against the unchanged independent fixture oracle.', '',
            '| Case | Mode | Baseline calls / bytes | Candidate calls / bytes |', '|---|---|---:|---:|']
    for (case, mode), oracle in expected.items():
        cells = []
        for engine in ['baseline', 'candidate']:
            row = rows[case, mode, engine]
            for key in ['accepted', 'output']:
                if json.dumps(row[key], sort_keys=True) != json.dumps(oracle[key], sort_keys=True):
                    raise ValueError(f'{case}: {key} mismatch')
            if harness.normalized_errors(row['errors']) != harness.normalized_errors(oracle['errors']):
                raise ValueError(f'{case}: errors mismatch')
            cells.append(f"{row['allocation']['calls']} / {row['allocation']['bytes']}")
        out.append(f'| {case} | {mode} | ' + ' | '.join(cells) + ' |')
    timed = [json.loads(line) for line in (directory / 'rule-timings.jsonl').read_text().splitlines()]
    if len(timed) != 144:
        raise ValueError('expected all 144 rule-timing rows')
    values = defaultdict(list)
    seen = set()
    for row in timed:
        key = row['type'], row['case'], row['round'], row['position']
        if key in seen:
            raise ValueError('duplicate rule timing position')
        seen.add(key)
        order = ['generic', 'typed', 'typed', 'generic'] if row['round'] == 0 else ['typed', 'generic', 'generic', 'typed']
        if row['round'] not in [0, 1] or row['route'] != order[row['position']]:
            raise ValueError('unexpected rule timing order')
        if (row['iterations'], row['warmup'], row['repeats']) != (10000, 1000, 3):
            raise ValueError('unexpected rule timing protocol')
        if len(row['samples_ns']) != 3 or any(type(v) is not int or v <= 0 for v in row['samples_ns']):
            raise ValueError('invalid rule samples')
        expected_success = 'Ok' in by_pair[row['type'], row['case'], row['route']]['result']
        if row['checksum'] != (30000 if expected_success else 0):
            raise ValueError('result consumption mismatch')
        values[row['type'], row['case'], row['route']].extend(v / 10000 for v in row['samples_ns'])
    out += ['', '## All uninstrumented rule timing controls', '',
            'Two reversed counterbalanced rounds, 1,000 warmups and three 10,000-operation batches per position give 12 samples per cell. Timing includes a fresh Context and result disposal. Median (inclusive IQR) batch-average ns/op is shown; no baseline subtraction. Fallback and upper-u128 rows are retained even when slower. This same-binary route diagnostic does not replace the full 49-case paired benchmark.', '',
            '| Type | Bounds | Generic ns/op (IQR) | Typed ns/op (IQR) | Typed change |', '|---|---|---:|---:|---:|']
    for row in allocation:
        if row['route'] != 'generic':
            continue
        cells, medians = [], []
        for route in ['generic', 'typed']:
            sample = values[row['type'], row['case'], route]
            if len(sample) != 12:
                raise ValueError('missing diagnostic samples')
            q1, _, q3 = statistics.quantiles(sample, n=4, method='inclusive')
            medians.append(statistics.median(sample))
            cells.append(f'{medians[-1]:.1f} ({q3-q1:.1f})')
        out.append(f"| {row['type']} | {row['case']} | " + ' | '.join(cells) + f' | {100*(medians[1]/medians[0]-1):+.2f}% |')
    return '\n'.join(out) + '\n'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(render(args.directory, args.root))
