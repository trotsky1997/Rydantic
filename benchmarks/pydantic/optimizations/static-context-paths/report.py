"""Render every canonical row and independently check the paired-run structure."""
import argparse
import gzip
import json
from pathlib import Path
import statistics

p = argparse.ArgumentParser()
p.add_argument('input', type=Path)
p.add_argument('output', type=Path)
a = p.parse_args()
raw = gzip.decompress(a.input.read_bytes()) if a.input.suffix == '.gz' else a.input.read_bytes()
data = json.loads(raw)
rows = {}
for record in data['measurements']:
    key = (record['case'], record['mode'])
    rows.setdefault(key, {}).setdefault(record['engine'], []).append(record['result'])
assert len(rows) == 49
assert len(data['measurements']) == 392
out = ['# Complete paired timings', '', 'Batch-average ns/op; median [Q1, Q3] from the recorded samples per engine. No baseline subtraction.', '',
       '| Case | Mode | Main | Static paths | Same-source control | Pydantic | Change vs main | Control vs candidate |', '|---|---|---:|---:|---:|---:|---:|---:|']
slower = []
for (case, mode), engines in rows.items():
    summary = {}
    for engine in ['baseline', 'candidate', 'control', 'pydantic']:
        processes = engines[engine]
        assert len(processes) == 2
        samples = []
        for process in processes:
            assert process['iterations'] == data['protocol']['iterations_per_row'][f'{case}/{mode}']
            assert len(process['samples_ns']) == data['protocol']['repeats_per_position']
            assert len(process['baseline_ns']) == data['protocol']['repeats_per_position']
            assert process['checksum'] == (process['iterations'] * len(process['samples_ns']) if process['accepted'] else 0)
            samples.extend(n / process['iterations'] for n in process['samples_ns'])
        q1, _, q3 = statistics.quantiles(samples, n=4, method='inclusive')
        summary[engine] = (statistics.median(samples), q1, q3)
    change = (summary['candidate'][0] / summary['baseline'][0] - 1) * 100
    if change > 0:
        slower.append(f'{case}/{mode}: {change:+.1f}%')
    values = ['{:.1f} [{:.1f}, {:.1f}]'.format(*summary[e]) for e in ['baseline', 'candidate', 'control', 'pydantic']]
    out.append(f'| {case} | {mode} | ' + ' | '.join(values) + f" | {change:+.1f}% | {(summary['control'][0] / summary['candidate'][0] - 1) * 100:+.1f}% |")
out += ['', '## All slower candidate medians', '', *(f'- {row}' for row in slower), '',
        'Shared-cloud scheduling, warmed repeated fixtures and default Pydantic string caching limit generalization. IQRs describe batch averages, not request-tail latency. Native diagnostic work differs. Secondary value_copy includes copying and is not pure validation.']
a.output.write_text('\n'.join(out) + '\n')
print(f'Checked {len(rows)} rows / {len(data["measurements"])} process outputs; slower candidate medians: {len(slower)}')
