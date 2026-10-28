"""Serial counterbalanced main/candidate/Pydantic benchmark; retain every row."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys

p = argparse.ArgumentParser()
p.add_argument('--baseline-root', required=True, type=Path)
p.add_argument('--candidate-root', required=True, type=Path)
p.add_argument('--baseline', required=True, type=Path)
p.add_argument('--candidate', required=True, type=Path)
p.add_argument('--control', required=True, type=Path)
p.add_argument('--pydantic-python', required=True, type=Path)
p.add_argument('--output', required=True, type=Path)
p.add_argument('--reference', required=True, type=Path, help='Immutable initial result used only to size longer batches')
a = p.parse_args()
sys.path.insert(0, str(a.candidate_root / 'benchmarks/pydantic'))
from run import verify, verify_engine, normalized_errors

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def snapshot(root):
    paths = set(root.glob('crates/**/*.rs')) | set(root.glob('crates/**/Cargo.toml'))
    paths |= {root / 'Cargo.toml', root / 'Cargo.lock'}
    paths |= set((root / 'benchmarks/pydantic').glob('*.py'))
    paths |= {root / 'benchmarks/pydantic/fixtures.json', root / 'benchmarks/pydantic/requirements.txt'}
    return {str(path.relative_to(root)): sha(path) for path in sorted(paths)}

def source(root):
    return {'head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
            'status': subprocess.check_output(['git', '-C', str(root), 'status', '--short'], text=True),
            'snapshot': snapshot(root)}

fixtures = json.loads((a.candidate_root / 'benchmarks/pydantic/fixtures.json').read_text())
commands = {'baseline': [str(a.baseline)], 'candidate': [str(a.candidate)], 'control': [str(a.control)],
            'pydantic': [str(a.pydantic_python), str(a.candidate_root / 'benchmarks/pydantic/python_worker.py')]}
# Reverse the entire order, including the external comparison engine. Each engine
# receives two positions, three repeated batches per position, for every case/mode.
order = ['pydantic', 'control', 'candidate', 'baseline', 'baseline', 'candidate', 'control', 'pydantic']
reference_bytes = a.reference.read_bytes()
reference_run = json.loads(reference_bytes)
iterations = {}
for record in reference_run['measurements']:
    if record['engine'] == 'baseline':
        key = (record['case'], record['mode'])
        iterations.setdefault(key, []).extend(n / record['result']['iterations'] for n in record['result']['samples_ns'])
iterations = {key: max(10000, min(1000000, round(100000000 / statistics.median(values)))) for key, values in iterations.items()}
binaries = {'baseline': a.baseline, 'candidate': a.candidate, 'control': a.control, 'python': a.pydantic_python.resolve()}
result = {'started': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
          'cargo': subprocess.check_output(['cargo', '-V'], text=True),
          'platform': platform.platform(), 'load_start': os.getloadavg(),
          'environment': {key: os.environ.get(key, '') for key in ['RUSTFLAGS', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_LTO', 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS']},
          'driver_sha256': sha(Path(__file__)),
          'reference_sha256': hashlib.sha256(reference_bytes).hexdigest(),
          'protocol': {'iterations': 'per-row: max(10000, min(1000000, round(100ms / initial baseline median)))', 'iterations_per_row': {f'{case}/{mode}': n for (case, mode), n in iterations.items()}, 'warmup': 1000, 'repeats_per_position': 5,
                       'order': order, 'instrumented': False, 'baseline_subtraction': False, 'control': 'Independent same-source candidate target build; binary-identical duplicate process-order control, not an independent source or code-layout variant.'},
          'sources': {'baseline': source(a.baseline_root), 'candidate': source(a.candidate_root)},
          'binary_sha256': {k: sha(v) for k, v in binaries.items()}, 'commands': commands, 'measurements': []}
for case in fixtures['cases']:
    for mode in case['modes']:
        request = dict(schema=case['schema'], input=case['input'], mode=mode, iterations=iterations[(case['name'], mode)], warmup=1000, repeats=5)
        reference = None
        for engine in order:
            proc = subprocess.run(commands[engine], input=json.dumps(request), capture_output=True, text=True, check=True)
            output = json.loads(proc.stdout)
            verify(case, output, request)
            verify_engine('pydantic' if engine == 'pydantic' else 'rydantic', output)
            signature = {key: output[key] for key in ['accepted', 'output']}
            signature['errors'] = normalized_errors(output['errors'])
            if reference is None:
                reference = signature
            assert signature == reference, (case['name'], mode, engine, signature, reference)
            result['measurements'].append(dict(case=case['name'], mode=mode, engine=engine, result=output))
assert sha(Path(__file__)) == result['driver_sha256'], 'driver changed'
assert source(a.baseline_root) == result['sources']['baseline'], 'baseline source changed'
assert source(a.candidate_root) == result['sources']['candidate'], 'candidate source changed'
assert {k: sha(v) for k, v in binaries.items()} == result['binary_sha256'], 'binaries changed'
result.update(finished=datetime.datetime.now(datetime.timezone.utc).isoformat(), load_end=os.getloadavg())
a.output.write_text(json.dumps(result, indent=2) + '\n')
for name in ['flat_valid', 'nested_valid', 'collection_100']:
    print(name, {engine: round(statistics.median([n / row['result']['iterations'] for row in result['measurements']
        if row['case'] == name and row['mode'] == 'json' and row['engine'] == engine for n in row['result']['samples_ns']]), 1) for engine in commands})
print('process_results', len(result['measurements']))
