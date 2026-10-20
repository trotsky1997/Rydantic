"""Synthetic harness fixtures, NOT agent evaluation results."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import runner


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        (self.base / 'pin').write_text('unit test placeholder, not a released artifact')
        self.config = dict(protocol=1, campaign_id='unit-test-only', repository_commit='a'*40,
            rust_toolchain='1.98.1', target='x86_64-unknown-linux-gnu', host_adapter_version='unit-test',
            sandbox_description='unit test does not execute agents', base_tools='unit-test', network='disabled',
            model=dict(provider='synthetic', model='no-model-called', revision='fixture', parameters={}),
            budgets=dict(wall_seconds=10, input_tokens=100, output_tokens=50, repair_rounds=2),
            repetitions=2, seed=11, artifacts={k:'pin' for k in ('skill_bundle','cli_binary','public_docs')},
            locks={t['id']:'pin' for t in runner.read(ROOT/'tasks/catalog.json')})
        self.campaign = runner.plan(self.config, self.base)
        self.ref = {'artifact':'pin', 'sha256':runner.file_hash(self.base/'pin')}

    def record(self, trial=None):
        trial = trial or self.campaign['trials'][0]
        event = dict(attempt=1, success=True, meaningful_contract_implementation=True,
                     evidence=[self.ref], findings={k:[] for k in runner.COUNTS})
        return dict(protocol=1, kind='host_agent_trial', campaign_sha256=self.campaign['campaign_sha256'],
            trial_id=trial['trial_id'], status='completed',
            host_attestation={k:True for k in ('isolated_session','fresh_workspace','hidden_tests_withheld','network_disabled','unchanged_pins','budgets_enforced','no_cross_arm_history')},
            artifacts=[self.ref], wall_seconds=2.5, input_tokens=20, output_tokens=10, repair_rounds=0,
            compile_attempts=[event] if trial['compile_applicable'] else [],
            final_findings={k:[] for k in runner.COUNTS},
            hidden_contract=dict(status='pass', evidence=[self.ref], pack_sha256=self.campaign['frozen']['pack_sha256']),
            review={k:dict(score=None) for k in runner.RUBRICS},
            appropriate_serde_choice=True if trial['serde_choice_applicable'] else None,
            serde_choice_evidence=[self.ref])

    def test_reproducible_balanced_pairs(self):
        self.assertEqual(self.campaign, runner.plan(self.config,self.base))
        self.assertEqual(len(self.campaign['trials']),24)
        for task in runner.read(ROOT/'tasks/catalog.json'):
            for rep in range(2):
                self.assertEqual({t['arm'] for t in self.campaign['trials'] if t['task_id']==task['id'] and t['repetition']==rep},set('ABC'))

    def test_pins_change_campaign_identity(self):
        (self.base/'pin').write_text('changed')
        self.assertNotEqual(self.campaign['campaign_sha256'],runner.plan(self.config,self.base)['campaign_sha256'])

    def test_unconfigured_template_fails(self):
        with self.assertRaises(ValueError): runner.plan(runner.read(ROOT/'config.template.json'),ROOT)

    def test_online_config_rejected(self):
        self.config['network']='enabled'
        with self.assertRaises(ValueError): runner.plan(self.config,self.base)

    def test_missing_lock_rejected(self):
        self.config['locks']['serde-dto']='absent'
        with self.assertRaises(ValueError): runner.plan(self.config,self.base)

    def test_empty_results_are_not_success(self):
        summary=runner.summarize([],self.campaign)
        self.assertEqual(len(summary['missing_trials']),24)
        self.assertEqual(summary['groups'],[])

    def test_compile_failure_remains_first(self):
        r=self.record(); r['compile_attempts'][0]['success']=False
        r['compile_attempts'].append(dict(r['compile_attempts'][0],attempt=2,success=True))
        summary=runner.summarize([r],self.campaign)['groups'][0]
        self.assertEqual(summary['first_compile']['success'],0)
        self.assertEqual(summary['final_contract']['pass'],1)

    def test_timeout_retained_in_denominator(self):
        r=self.record(); r.update(status='timeout',compile_attempts=[])
        r['hidden_contract']={'status':'not_run'}
        summary=runner.summarize([r],self.campaign)['groups'][0]
        self.assertEqual(summary['first_compile'],dict(success=0,eligible=1,not_run=1))
        self.assertEqual(summary['status_counts']['timeout'],1)

    def test_duplicate_results_rejected(self):
        r=self.record()
        with self.assertRaises(ValueError): runner.summarize([r,r],self.campaign)

    def test_smoke_cannot_be_imported(self):
        r=self.record(); r['kind']='harness_smoke'
        with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_false_host_attestation_rejected(self):
        r=self.record(); r['host_attestation']['hidden_tests_withheld']=False
        with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_corrupt_campaign_rejected(self):
        campaign=copy.deepcopy(self.campaign); campaign['frozen']['seed']=999
        with self.assertRaises(ValueError): runner.validate_result(self.record(),campaign)

    def test_trial_metadata_tampering_rejected(self):
        for key,value in [('arm','Z'),('track','wrong-track'),('compile_applicable',False)]:
            campaign=copy.deepcopy(self.campaign)
            campaign['trials'][0][key]=value
            with self.assertRaises(ValueError): runner.validate_result(self.record(),campaign)

    def test_all_compiles_failed_cannot_claim_pass(self):
        r=self.record(); r['compile_attempts'][0]['success']=False
        with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_empty_implementation_compile_rejected(self):
        r=self.record(); r['compile_attempts'][0]['meaningful_contract_implementation']=False
        with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_incomplete_pass_rejected(self):
        r=self.record(); r['status']='agent_failed'
        with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_unknown_metrics_not_zero_scores(self):
        summary=runner.summarize([self.record()],self.campaign)['groups'][0]
        self.assertEqual(summary['rubrics']['security'],dict(n=0,median=None,p90=None))

    def test_tracks_are_separate(self):
        records=[self.record(t) for t in self.campaign['trials']]
        groups=runner.summarize(records,self.campaign)['groups']
        self.assertEqual(len(groups),6)
        self.assertEqual({g['track'] for g in groups},{'stable-0.6.0','beta-0.7.0-beta.4'})

    def test_nonfinite_time_and_bool_counts_rejected(self):
        for key,value in [('wall_seconds',float('nan')),('input_tokens',True),('output_tokens',-1)]:
            r=self.record(); r[key]=value
            with self.assertRaises(ValueError): runner.validate_result(r,self.campaign)

    def test_artifact_verification_and_traversal(self):
        runner.verify_artifacts(self.record(),self.base)
        with self.assertRaises(ValueError): runner.verify_artifacts(dict(self.ref,artifact='../pin'),self.base)
        with self.assertRaises(ValueError): runner.verify_artifacts(dict(self.ref,sha256='0'*64),self.base)

    def test_duplicate_json_keys_rejected(self):
        p=self.base/'duplicate.json'; p.write_text('{"a":1,"a":2}')
        with self.assertRaises(ValueError): runner.read(p)

    def test_empty_results_still_validate_campaign(self):
        campaign=copy.deepcopy(self.campaign); campaign['trials'].clear()
        with self.assertRaises(ValueError): runner.summarize([],campaign)

    def test_hidden_decision_rejects_unsafe_files(self):
        spec=importlib.util.spec_from_file_location('grader',ROOT/'hidden/grade_decision.py')
        grader=importlib.util.module_from_spec(spec); spec.loader.exec_module(grader)
        p=self.base/'large'; p.write_bytes(b'x'*8193)
        with self.assertRaises(ValueError): grader.bounded_read(self.base,'large',8192)
        (self.base/'escape').symlink_to(ROOT/'runner.py')
        with self.assertRaises(ValueError): grader.bounded_read(self.base,'escape')
        with self.assertRaises(ValueError): json.loads('{"a":1,"a":2}',object_pairs_hook=grader.unique)
        self.assertFalse(grader.check(self.base,ROOT/'tasks/stable-feature-stop'))

    def test_hidden_decision_behavior(self):
        spec=importlib.util.spec_from_file_location('grader',ROOT/'hidden/grade_decision.py')
        grader=importlib.util.module_from_spec(spec); spec.loader.exec_module(grader)
        original=ROOT/'tasks/stable-feature-stop'
        (self.base/'src').mkdir()
        for name in ('Cargo.toml','src/lib.rs','Cargo.lock','rust-toolchain.toml'):
            (self.base/name).write_bytes((original/name).read_bytes())
        decision=dict(status='unsupported',requested_features=['settings','partial-json'],selected_core_version='0.6.0',upgrade_performed=False,reason='unsupported in pinned version')
        (self.base/'decision.json').write_text(json.dumps(decision))
        self.assertTrue(grader.check(self.base,original))
        (self.base/'Cargo.toml').write_text('silently upgraded')
        self.assertFalse(grader.check(self.base,original))

if __name__=='__main__': unittest.main()
