"""Predeclared affinity/interleaving A/A gate; not a repeat-until-fast benchmark."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import resource
import statistics
import subprocess
import sys
import time

p = argparse.ArgumentParser()
p.add_argument('--root', required=True, type=Path)
p.add_argument('--baseline', required=True, type=Path)
p.add_argument('--candidate', required=True, type=Path)
p.add_argument('--control', required=True, type=Path)
p.add_argument('--pydantic-python', required=True, type=Path)
p.add_argument('--output', required=True, type=Path)
a = p.parse_args()
if a.output.exists():
    raise SystemExit('Refusing to overwrite an existing diagnostic')
sys.path.insert(0, str(a.root / 'benchmarks/pydantic'))
from run import verify, verify_engine

cases = {c['name']: c for c in json.loads((a.root / 'benchmarks/pydantic/fixtures.json').read_text())['cases']}
settings = {'nested_valid': (25000, 1000), 'flat_wrong_type': (50000, 1000), 'collection_100': (500, 50)}
commands = {'baseline': [str(a.baseline)], 'candidate': [str(a.candidate)], 'control': [str(a.control)],
            'pydantic': [str(a.pydantic_python), str(a.root / 'benchmarks/pydantic/python_worker.py')]}
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

hashes = {name: sha(Path(command[0]).resolve()) for name, command in commands.items()}
assert hashes['candidate'] == hashes['control'], 'A/A requires identical binaries'
allowed = sorted(os.sched_getaffinity(0))
if not allowed:
    raise SystemExit('No allowed CPU')
core = allowed[0]
def pin_child():
    os.sched_setaffinity(0, {core})
    if os.sched_getaffinity(0) != {core}:
        raise RuntimeError('Child affinity verification failed')

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

result = {'started': utc(), 'driver_sha256': sha(Path(__file__)), 'fixture_sha256': sha(a.root / 'benchmarks/pydantic/fixtures.json'),
          'binary_sha256': hashes, 'commands': commands, 'allowed_cpus_observed': allowed, 'child_cpu': core,
          'load_start': os.getloadavg(), 'records': [], 'gates': {},
          'design': {'cases': settings, 'mode': 'json', 'blocks_per_case': 16, 'timed_batches_per_process': 1,
                     'aa_order': 'alternating ABBA / BAAB',
                     'paired_order': 'rotate baseline,candidate,control,pydantic by block modulo 4, then append reverse',
                     'thresholds': {'aggregate_median_ratio_distance_from_one_max': 0.02,
                                    'block_ratio_iqr_max': 0.05,
                                    'blocks_with_ratio_distance_at_most_0.05_min': 14},
                     'stop': 'Any A/A case failure stops before baseline/Pydantic. No retries or exclusions. Paired conclusions require the same A/A gate during pairing.',
                     'timing': 'Primary worker samples exclude startup/setup/warmup. Whole-child wall/CPU/context-switch receipts include all process work and are diagnostic only.'}}

def run_process(stage, case_name, block, position, engine):
    iterations, warmup = settings[case_name]
    case = cases[case_name]
    request = dict(schema=case['schema'], input=case['input'], mode='json', iterations=iterations, warmup=warmup, repeats=1)
    start_utc = utc()
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start_ns = time.monotonic_ns()
    proc = subprocess.run(commands[engine], input=json.dumps(request), capture_output=True, text=True, preexec_fn=pin_child)
    wall_ns = time.monotonic_ns() - start_ns
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    record = {'stage': stage, 'case': case_name, 'block': block, 'position': position, 'engine': engine,
              'started': start_utc, 'finished': utc(), 'whole_child_wall_ns': wall_ns,
              'child_user_cpu_seconds': after.ru_utime - before.ru_utime,
              'child_system_cpu_seconds': after.ru_stime - before.ru_stime,
              'child_voluntary_context_switches': after.ru_nvcsw - before.ru_nvcsw,
              'child_involuntary_context_switches': after.ru_nivcsw - before.ru_nivcsw,
              'affinity_asserted_before_exec': [core], 'returncode': proc.returncode}
    if proc.returncode:
        record['stderr'] = proc.stderr
        result['records'].append(record)
        raise RuntimeError(f'Worker failed: {engine} {case_name}')
    output = json.loads(proc.stdout)
    verify_engine('pydantic' if engine == 'pydantic' else 'rydantic', output)
    verify(case, output, request)
    record['result'] = output
    result['records'].append(record)

def values(records):
    return [n / r['result']['iterations'] for r in records for n in r['result']['samples_ns']]

def gate(stage):
    outcomes = {}
    for name in settings:
        records = [r for r in result['records'] if r['stage'] == stage and r['case'] == name]
        medians = {e: statistics.median(values([r for r in records if r['engine'] == e])) for e in ['candidate', 'control']}
        aggregate = medians['candidate'] / medians['control']
        ratios = []
        for block in range(16):
            by_engine = {e: statistics.median(values([r for r in records if r['block'] == block and r['engine'] == e])) for e in ['candidate', 'control']}
            ratios.append(by_engine['candidate'] / by_engine['control'])
        q1, _, q3 = statistics.quantiles(ratios, n=4, method='inclusive')
        within = sum(abs(r - 1) <= 0.05 for r in ratios)
        outcomes[name] = {'medians_ns': medians, 'aggregate_candidate_control_ratio': aggregate,
                          'block_ratios': ratios, 'block_ratio_iqr': q3 - q1, 'blocks_within_5_percent': within,
                          'pass': abs(aggregate - 1) <= 0.02 and q3 - q1 <= 0.05 and within >= 14}
    return outcomes

try:
    for name in settings:
        for block in range(16):
            order = ['candidate', 'control', 'control', 'candidate'] if block % 2 == 0 else ['control', 'candidate', 'candidate', 'control']
            for position, engine in enumerate(order):
                run_process('aa', name, block, position, engine)
    result['gates']['aa'] = gate('aa')
    if all(g['pass'] for g in result['gates']['aa'].values()):
        for name in settings:
            for block in range(16):
                engines = ['baseline', 'candidate', 'control', 'pydantic']
                offset = block % 4
                forward = engines[offset:] + engines[:offset]
                for position, engine in enumerate(forward + forward[::-1]):
                    run_process('paired', name, block, position, engine)
        result['gates']['paired'] = gate('paired')
        result['outcome'] = 'paired_complete_control_stable' if all(g['pass'] for g in result['gates']['paired'].values()) else 'paired_control_unstable_no_latency_conclusion'
    else:
        result['outcome'] = 'aa_unstable_stopped_before_pairing'
finally:
    result.update(finished=utc(), load_end=os.getloadavg())
    result['sources_and_binaries_unchanged'] = (sha(Path(__file__)) == result['driver_sha256'] and hashes == {name: sha(Path(command[0]).resolve()) for name, command in commands.items()} and sha(a.root / 'benchmarks/pydantic/fixtures.json') == result['fixture_sha256'])
    a.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({'outcome': result['outcome'], 'gates': result['gates'], 'records': len(result['records'])}, indent=2))
