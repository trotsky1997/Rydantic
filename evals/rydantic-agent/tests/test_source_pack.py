"""Synthetic planning regressions only, never measured agent trials."""
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import runner

LEGACY_HASH = '915e9bec9917a95603a893ea5c33e6586d007644b9d5639fcbb23fecb55190e7'


class SourcePackTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        (self.base / 'pin').write_text('Synthetic local harness artifact, not agent evidence')
        self.config = runner.read(ROOT / 'config.template.json')
        self.config.update(campaign_id='source-harness-only', repository_commit='a' * 40,
                           host_adapter_version='synthetic', sandbox_description='no execution',
                           eval_pack=runner.SOURCE_PACK,
                           library_source_commit=runner.SOURCE_REVISION)
        self.config['model'] = dict(provider='synthetic', model='not-run', revision='fixture', parameters={})
        self.config['artifacts'] = {key: 'pin' for key in ('skill_bundle', 'cli_binary', 'public_docs')}
        self.config['locks'] = {'source-context-policy': str(
            runner.pack_root(runner.SOURCE_PACK) / 'tasks/source-context-policy/Cargo.lock')}
        self.campaign = runner.plan(self.config, self.base)

    def copy_source(self):
        local = self.base / 'repo'
        shutil.copytree(ROOT / 'packs', local / 'packs')
        task = local / 'packs' / runner.SOURCE_PACK / 'tasks/source-context-policy'
        self.config['locks'] = {'source-context-policy': str(task / 'Cargo.lock')}
        return local, task

    def test_source_is_explicit_separate_and_reproducible(self):
        self.assertEqual(self.campaign, runner.plan(self.config, self.base))
        self.assertEqual(len(self.campaign['trials']), 9)
        self.assertEqual({trial['task_id'] for trial in self.campaign['trials']}, {'source-context-policy'})
        self.assertEqual({trial['track'] for trial in self.campaign['trials']}, {runner.SOURCE_PACK})
        self.assertEqual(self.campaign['frozen']['library_source_commit'], runner.SOURCE_REVISION)
        self.assertNotEqual(self.campaign['frozen']['pack_sha256'], runner.pack_hash())
        for repetition in range(3):
            self.assertEqual({trial['arm'] for trial in self.campaign['trials']
                              if trial['repetition'] == repetition}, set('ABC'))

    def test_default_catalog_and_pack_identity_unchanged(self):
        self.assertEqual(runner.pack_hash(), LEGACY_HASH)
        del self.config['eval_pack'], self.config['library_source_commit']
        self.config['locks'] = {task['id']: 'pin' for task in runner.read(ROOT / 'tasks/catalog.json')}
        legacy = runner.plan(self.config, self.base)
        self.assertEqual(len(legacy['trials']), 36)
        self.assertEqual(legacy['frozen']['pack_sha256'], LEGACY_HASH)
        self.assertNotIn('eval_pack', legacy['frozen'])
        self.assertNotIn('library_source_commit', legacy['frozen'])
        self.assertEqual({trial['track'] for trial in legacy['trials']},
                         {'stable-0.6.0', 'beta-0.7.0-beta.4'})

    def test_unknown_null_short_or_multiple_pack_selection_rejected(self):
        for selection in (None, '', 'source-6006720', '../tasks', 'stable',
                          [runner.SOURCE_PACK, 'legacy']):
            with self.subTest(selection=selection):
                self.config['eval_pack'] = selection
                with self.assertRaisesRegex(ValueError, 'eval_pack'):
                    runner.plan(self.config, self.base)

    def test_source_pin_cannot_be_silently_used_without_opt_in(self):
        del self.config['eval_pack']
        with self.assertRaisesRegex(ValueError, 'explicit eval_pack'):
            runner.plan(self.config, self.base)

    def test_missing_or_wrong_source_revision_rejected(self):
        for revision in (None, '6006720', 'b' * 40, '0.8.0-beta.1'):
            self.config['library_source_commit'] = revision
            with self.assertRaisesRegex(ValueError, 'library_source_commit'):
                runner.plan(self.config, self.base)
        del self.config['library_source_commit']
        with self.assertRaises(ValueError):
            runner.plan(self.config, self.base)

    def test_source_requires_exact_toolchain(self):
        self.config['rust_toolchain'] = 'stable'
        with self.assertRaisesRegex(ValueError, 'toolchain'):
            runner.plan(self.config, self.base)

    def test_source_locks_cannot_mix_cohorts_or_omit_task(self):
        self.config['locks']['typed-user'] = 'pin'
        with self.assertRaisesRegex(ValueError, 'mix cohorts'):
            runner.plan(self.config, self.base)
        self.config['locks'] = {}
        with self.assertRaisesRegex(ValueError, 'omit tasks'):
            runner.plan(self.config, self.base)

    def test_host_lock_requires_exact_committed_bytes(self):
        original = Path(self.config['locks']['source-context-policy']).read_bytes()
        altered = self.base / 'altered.lock'
        altered.write_bytes(original + b'\n')
        self.config['locks']['source-context-policy'] = str(altered)
        with self.assertRaisesRegex(ValueError, 'byte-for-byte'):
            runner.plan(self.config, self.base)

    def test_core_and_derive_manifest_revisions_are_both_checked(self):
        local, task = self.copy_source()
        manifest = task / 'Cargo.toml'
        original = manifest.read_text()
        for crate in ('rydantic', 'rydantic-derive'):
            for replacement in (
                '{ git = "' + runner.SOURCE_GIT + '", rev = "' + 'b' * 40 + '" }',
                '"=0.8.0-beta.1"',
                '{ path = "../../../../crates/' + crate + '" }',
                '{ git = "' + runner.SOURCE_GIT + '", branch = "main" }',
            ):
                with self.subTest(crate=crate, replacement=replacement):
                    lines = original.splitlines()
                    manifest.write_text('\n'.join(crate + ' = ' + replacement
                                                 if line.startswith(crate + ' = ') else line
                                                 for line in lines) + '\n')
                    with patch.object(runner, 'ROOT', local), self.assertRaisesRegex(ValueError, 'exact git'):
                        runner.plan(self.config, self.base)
        manifest.write_text(original + '\n[patch.crates-io]\nrydantic = { path = "other" }\n')
        with patch.object(runner, 'ROOT', local), self.assertRaisesRegex(ValueError, 'override'):
            runner.plan(self.config, self.base)

    def test_source_lock_rejects_release_drift_and_duplicate_crates(self):
        local, task = self.copy_source()
        lock = task / 'Cargo.lock'
        original = lock.read_text()
        for contents in (
            original.replace('#' + runner.SOURCE_REVISION, '#' + 'b' * 40, 1),
            original.replace('version = "0.8.0-beta.1"', 'version = "0.7.0"', 1),
            original + '\n[[package]]\nname = "rydantic-derive"\nversion = "0.8.0-beta.1"\n',
        ):
            lock.write_text(contents)
            with patch.object(runner, 'ROOT', local), self.assertRaisesRegex(ValueError, 'exact source snapshot'):
                runner.plan(self.config, self.base)

    def test_missing_source_hidden_contract_rejected(self):
        local, _ = self.copy_source()
        (local / 'packs' / runner.SOURCE_PACK / 'hidden/source-context-policy.rs').unlink()
        with patch.object(runner, 'ROOT', local), self.assertRaisesRegex(ValueError, 'missing pinned file'):
            runner.plan(self.config, self.base)

    def test_source_catalog_cannot_include_legacy_task(self):
        local, _ = self.copy_source()
        catalog = local / 'packs' / runner.SOURCE_PACK / 'tasks/catalog.json'
        catalog.write_text((ROOT / 'tasks/catalog.json').read_text())
        with patch.object(runner, 'ROOT', local), self.assertRaisesRegex(ValueError, 'independent task'):
            runner.plan(self.config, self.base)

    def test_pack_hashes_do_not_include_other_cohort_files(self):
        local, task = self.copy_source()
        shutil.copytree(ROOT / 'tasks', local / 'tasks')
        shutil.copytree(ROOT / 'hidden', local / 'hidden')
        with patch.object(runner, 'ROOT', local):
            source_hash = runner.pack_hash(runner.SOURCE_PACK)
            legacy_hash = runner.pack_hash()
            (task / 'src/lib.rs').write_text('// changed source task input\n')
            changed_source = runner.pack_hash(runner.SOURCE_PACK)
            self.assertNotEqual(changed_source, source_hash)
            self.assertEqual(runner.pack_hash(), legacy_hash)
            (local / 'hidden/typed-user.rs').write_text('// changed legacy contract\n')
            self.assertNotEqual(runner.pack_hash(), legacy_hash)
            self.assertEqual(runner.pack_hash(runner.SOURCE_PACK), changed_source)

    def test_default_campaign_result_cannot_be_imported_as_source(self):
        # Reuse the existing synthetic record builder, without executing an agent.
        from test_runner import HarnessTests
        harness = HarnessTests()
        harness.setUp()
        self.addCleanup(harness.doCleanups)
        with self.assertRaisesRegex(ValueError, 'campaign mismatch'):
            runner.validate_result(harness.record(), self.campaign)
        result = harness.record()
        result['campaign_sha256'] = self.campaign['campaign_sha256']
        result['trial_id'] = self.campaign['trials'][0]['trial_id']
        with self.assertRaisesRegex(ValueError, 'hidden pack mismatch'):
            runner.validate_result(result, self.campaign)

    def test_source_summary_cannot_pool_with_legacy(self):
        summary = runner.summarize([], self.campaign)
        self.assertEqual(len(summary['missing_trials']), 9)
        self.assertEqual(summary['groups'], [])
        self.assertTrue(all(trial.startswith('source-context-policy.') for trial in summary['missing_trials']))


if __name__ == '__main__':
    unittest.main()
