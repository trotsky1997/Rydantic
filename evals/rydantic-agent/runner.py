#!/usr/bin/env python3
"""Offline campaign planner and host-evidence importer. Never launches an agent or Rust."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import statistics
import sys

ROOT = Path(__file__).resolve().parent
ARMS = ('A', 'B', 'C')
COUNTS = ('invalid_api', 'wrong_feature', 'invented_method')
RUBRICS = ('security', 'maintainability', 'unnecessary_complexity')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON key: ' + key)
            result[key] = value
        return result
    with Path(path).open('rb') as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    require(len(data) <= 4 * 1024 * 1024, 'JSON exceeds 4 MiB')
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda s: (_ for _ in ()).throw(ValueError(s)))


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                    allow_nan=False).encode()).hexdigest()


def file_hash(path):
    path = Path(path)
    require(path.is_file(), 'missing pinned file: ' + str(path))
    hasher = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def pack_hash():
    return digest({str(p.relative_to(ROOT)): file_hash(p)
                   for folder in ('tasks', 'hidden')
                   for p in sorted((ROOT / folder).rglob('*')) if p.is_file() and '__pycache__' not in p.parts})


def integer(value, label, minimum=0):
    require(type(value) is int and value >= minimum, label + ' must be an integer >= ' + str(minimum))


def nonempty(value, label):
    require(isinstance(value, str) and bool(value.strip()), label + ' must be nonempty')


def sha(value, label):
    require(isinstance(value, str) and len(value) == 64 and
            all(c in '0123456789abcdef' for c in value), label + ' must be sha256')


def plan(config, base):
    """Config paths are trusted host inputs, never supplied by the task/agent."""
    require(config['protocol'] == 1, 'unsupported protocol')
    require(config['network'] == 'disabled', 'P0 requires host-enforced network disabled')
    for key in ('campaign_id', 'repository_commit', 'rust_toolchain', 'target',
                'host_adapter_version', 'sandbox_description', 'base_tools'):
        nonempty(config[key], key)
    commit = config['repository_commit']
    require(len(commit) == 40 and all(c in '0123456789abcdef' for c in commit), 'full repository SHA required')
    for key in ('provider', 'model', 'revision'):
        nonempty(config['model'][key], 'model.' + key)
    require(isinstance(config['model']['parameters'], dict), 'model parameters must be explicit')
    for key in ('wall_seconds', 'input_tokens', 'output_tokens', 'repair_rounds'):
        integer(config['budgets'][key], 'budgets.' + key, 1)
    integer(config['repetitions'], 'repetitions', 1)
    integer(config['seed'], 'seed')
    require(config['repetitions'] <= 100, 'maximum 100 repetitions')
    tasks = read(ROOT / 'tasks/catalog.json')
    pins = {}
    for key in ('skill_bundle', 'cli_binary', 'public_docs'):
        pins[key] = file_hash(base / config['artifacts'][key])
    locks = {task['id']: file_hash(base / config['locks'][task['id']]) for task in tasks}
    frozen = dict(config, artifact_sha256=pins, lock_sha256=locks, pack_sha256=pack_hash())
    trials = []
    rng = random.Random(config['seed'])
    for task in tasks:
        for repetition in range(config['repetitions']):
            arms = list(ARMS)
            rng.shuffle(arms)
            for arm in arms:
                trials.append({'trial_id': f"{task['id']}.{repetition}.{arm}",
                               'task_id': task['id'], 'track': task['track'],
                               'arm': arm, 'repetition': repetition,
                               'compile_applicable': task['compile_applicable'],
                               'serde_choice_applicable': task['serde_choice_applicable'],
                               'lock_sha256': locks[task['id']]})
    campaign_hash = digest({'frozen': frozen, 'trials': trials})
    return {'protocol': 1, 'campaign_sha256': campaign_hash, 'frozen': frozen, 'trials': trials}


def evidence(refs, label):
    require(isinstance(refs, list) and len(refs) > 0, label + ' requires evidence')
    for ref in refs:
        nonempty(ref['artifact'], label + '.artifact')
        sha(ref['sha256'], label + '.sha256')


def validate_result(result, campaign):
    require(digest({'frozen': campaign['frozen'], 'trials': campaign['trials']}) == campaign['campaign_sha256'], 'corrupt campaign digest')
    require(result['protocol'] == 1, 'unsupported result protocol')
    require(result['kind'] == 'host_agent_trial', 'harness smoke/unit tests are not agent trials')
    require(result['campaign_sha256'] == campaign['campaign_sha256'], 'campaign mismatch')
    matches = [t for t in campaign['trials'] if t['trial_id'] == result['trial_id']]
    require(len(matches) == 1, 'unknown trial')
    trial = matches[0]
    require(result['status'] in ('completed', 'timeout', 'budget_exhausted', 'environment_blocked', 'agent_failed'), 'bad status')
    require(result['host_attestation'] == {
        'isolated_session': True, 'fresh_workspace': True, 'hidden_tests_withheld': True,
        'network_disabled': True, 'unchanged_pins': True, 'budgets_enforced': True,
        'no_cross_arm_history': True}, 'host isolation attestation missing')
    evidence(result['artifacts'], 'trial')
    time = result['wall_seconds']
    require(type(time) in (int, float) and math.isfinite(time) and time >= 0, 'invalid wall time')
    for key in ('input_tokens', 'output_tokens', 'repair_rounds'):
        integer(result[key], key)
    # Compile events must preserve the FIRST meaningful implementation, not first success.
    events = result['compile_attempts']
    require(isinstance(events, list), 'compile attempts must be an array')
    if not trial['compile_applicable']:
        require(not events, 'compile is not applicable for this task')
    for i, event in enumerate(events):
        require(event['attempt'] == i + 1, 'compile attempts must be contiguous')
        require(type(event['success']) is bool, 'compile success must be boolean')
        require(event['meaningful_contract_implementation'] is True,
                'empty/deleted-function implementation cannot count as first compilation')
        evidence(event['evidence'], 'compile')
        for category in COUNTS:
            findings = event['findings'][category]
            require(isinstance(findings, list), category + ' must contain classified findings')
            for finding in findings:
                nonempty(finding['symbol'], 'finding.symbol')
                nonempty(finding['reason'], 'finding.reason')
                evidence(finding['evidence'], 'finding')
    for category in COUNTS:
        require(isinstance(result['final_findings'][category], list), 'final findings must be arrays')
        for finding in result['final_findings'][category]:
            nonempty(finding['symbol'], 'finding.symbol')
            nonempty(finding['reason'], 'finding.reason')
            evidence(finding['evidence'], 'finding')
    verdict = result['hidden_contract']
    require(verdict['status'] in ('pass', 'fail', 'not_run'), 'bad contract status')
    if verdict['status'] != 'not_run':
        evidence(verdict['evidence'], 'hidden contract')
        require(verdict['pack_sha256'] == campaign['frozen']['pack_sha256'], 'hidden pack mismatch')
    if verdict['status'] == 'pass':
        require(result['status'] == 'completed', 'incomplete trial cannot claim pass')
        require(not trial['compile_applicable'] or bool(events) and events[-1]['success'],
                'pass requires a successful final compile')
    for key in RUBRICS:
        review = result['review'][key]
        require(review['score'] is None or type(review['score']) is int and 0 <= review['score'] <= 4,
                'rubric scores must be null or 0..4')
        if review['score'] is not None:
            nonempty(review['blind_reviewer_id'], 'blind reviewer')
            evidence(review['evidence'], 'rubric')
    choice = result['appropriate_serde_choice']
    require(choice is None or type(choice) is bool, 'invalid Serde choice')
    if not trial['serde_choice_applicable']:
        require(choice is None, 'Serde choice not applicable')
    elif choice is not None:
        evidence(result['serde_choice_evidence'], 'Serde choice')
    return trial


def distribution(values):
    if not values:
        return {'n': 0, 'median': None, 'p90': None}
    ordered = sorted(values)
    return {'n': len(values), 'median': statistics.median(values),
            'p90': ordered[max(0, math.ceil(.9 * len(values)) - 1)]}


def summarize(results, campaign):
    require(digest({'frozen': campaign['frozen'], 'trials': campaign['trials']}) == campaign['campaign_sha256'],
            'corrupt campaign digest')
    seen, groups = set(), {}
    for result in results:
        trial = validate_result(result, campaign)
        require(result['trial_id'] not in seen, 'duplicate trial result')
        seen.add(result['trial_id'])
        groups.setdefault((trial['track'], trial['arm']), []).append((trial, result))
    summary = []
    for (track, arm), rows in sorted(groups.items()):
        eligible = [r for t, r in rows if t['compile_applicable']]
        serde = [r for t, r in rows if t['serde_choice_applicable']]
        item = {'track': track, 'arm': arm, 'recorded_trials': len(rows),
                'planned_trials': sum(t['track'] == track and t['arm'] == arm for t in campaign['trials']),
                'first_compile': {'success': sum(bool(r['compile_attempts']) and r['compile_attempts'][0]['success'] for r in eligible),
                                  'eligible': len(eligible), 'not_run': sum(not r['compile_attempts'] for r in eligible)},
                'final_contract': {s: sum(r['hidden_contract']['status'] == s for _, r in rows)
                                   for s in ('pass', 'fail', 'not_run')},
                'status_counts': {s: sum(r['status'] == s for _, r in rows) for s in
                                  ('completed', 'timeout', 'budget_exhausted', 'environment_blocked', 'agent_failed')},
                'appropriate_serde_choice': {'correct': sum(r['appropriate_serde_choice'] is True for r in serde),
                                             'eligible': len(serde), 'unknown': sum(r['appropriate_serde_choice'] is None for r in serde)}}
        for key in ('wall_seconds', 'input_tokens', 'output_tokens', 'repair_rounds'):
            item[key] = distribution([r[key] for _, r in rows])
        item['findings'] = {key: {'occurrences': sum(len(e['findings'][key]) for _, r in rows for e in r['compile_attempts']),
                                 'affected_trials': sum(any(e['findings'][key] for e in r['compile_attempts']) for _, r in rows),
                                 'compile_attempts': sum(len(r['compile_attempts']) for _, r in rows)} for key in COUNTS}
        item['final_findings'] = {key: {'occurrences': sum(len(r['final_findings'][key]) for _, r in rows),
                                         'affected_trials': sum(bool(r['final_findings'][key]) for _, r in rows)} for key in COUNTS}
        item['rubrics'] = {key: distribution([r['review'][key]['score'] for _, r in rows if r['review'][key]['score'] is not None]) for key in RUBRICS}
        summary.append(item)
    return {'campaign_sha256': campaign['campaign_sha256'], 'missing_trials': sorted(t['trial_id'] for t in campaign['trials'] if t['trial_id'] not in seen),
            'groups': summary, 'note': 'Host-reported evidence, not independently authenticated. No causal claim; compare paired repetitions within this campaign only.'}


def verify_artifacts(value, root):
    """Verify evidence references without permitting host-path traversal."""
    root = Path(root).resolve(strict=True)
    if isinstance(value, dict):
        if 'artifact' in value and 'sha256' in value:
            relative = Path(value['artifact'])
            require(not relative.is_absolute() and '..' not in relative.parts, 'unsafe artifact path')
            path = (root / relative).resolve(strict=True)
            require(path.is_relative_to(root), 'artifact escaped evidence root')
            require(file_hash(path) == value['sha256'], 'evidence digest mismatch')
        for child in value.values():
            verify_artifacts(child, root)
    elif isinstance(value, list):
        for child in value:
            verify_artifacts(child, root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('plan'); p.add_argument('config', type=Path)
    p = sub.add_parser('summarize'); p.add_argument('plan', type=Path); p.add_argument('results', nargs='*', type=Path); p.add_argument('--evidence-root', required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'plan':
            output = plan(read(args.config), args.config.parent)
        else:
            results = [read(p) for p in args.results]
            for result in results:
                verify_artifacts(result, args.evidence_root)
            output = summarize(results, read(args.plan))
        print(json.dumps(output, indent=2, sort_keys=True, allow_nan=False))
    except (ValueError, KeyError, TypeError, OSError) as error:
        print('invalid evaluation input: ' + str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
