"""Read-only CLI gate regression tests; no registry requests or uploads."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

import release_cli as cli


class CliReleaseTests(unittest.TestCase):
    def manifests(self, root, dependency='serde.workspace = true', extra=''):
        (root / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.8.0"\n[workspace.dependencies]\nserde = "1"\n')
        path = root / 'crates/rydantic-cli'
        path.mkdir(parents=True)
        (path / 'Cargo.toml').write_text(
            '[package]\nname = "rydantic-cli"\nversion = "0.1.0"\npublish = ["crates-io"]\n'
            '[[bin]]\nname = "rydantic"\npath = "src/main.rs"\n[dependencies]\n' + dependency + '\n' + extra)

    def test_independent_version_and_inheritance(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifests(root)
            before = (root / 'Cargo.toml').read_bytes()
            self.assertEqual(cli.validate_manifests(root, 'cli-v0.1.0'), '0.1.0')
            self.assertEqual((root / 'Cargo.toml').read_bytes(), before)

    def test_malformed_tags(self):
        for tag in ('v0.1.0', 'main', 'cli-v01.1.0', 'cli-v0.1.0\n', 'cli-v0.1.0;echo x', 'cli-v0.1.0-rc.1'):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                cli.validate_manifests(Path('/unused'), tag)

    def test_manifest_rejections(self):
        for replacement in ('version = "0.2.0"', 'version.workspace = true', 'publish = false', 'publish = true', 'publish = ["other"]', 'name = "other"'):
            with self.subTest(replacement=replacement), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.manifests(root)
                path = root / 'crates/rydantic-cli/Cargo.toml'
                key = replacement.split(' = ')[0].split('.')[0]
                old = {'version': 'version = "0.1.0"', 'publish': 'publish = ["crates-io"]', 'name': 'name = "rydantic-cli"'}[key]
                path.write_text(path.read_text().replace(old, replacement))
                with self.assertRaises(ValueError):
                    cli.validate_manifests(root, 'cli-v0.1.0')

    def test_source_overrides_rejected_everywhere(self):
        dependencies = ['dep = {version = "1", path = "../dep"}', 'dep = {version = "1", git = "https://example.test"}',
                        'dep = {version = "1", registry = "private"}', 'dep = {version = "1", registry-index = "https://example.test"}',
                        'dep = {}', 'dep.workspace = true']
        for dependency in dependencies:
            for table in ('dependencies', 'build-dependencies', 'dev-dependencies', 'target.\'cfg(unix)\'.dependencies'):
                with self.subTest(dependency=dependency, table=table), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    self.manifests(root, '', f'[{table}]\n{dependency}' if table != 'dependencies' else dependency)
                    with self.assertRaises(ValueError):
                        cli.validate_manifests(root, 'cli-v0.1.0')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifests(root)
            path = root / 'Cargo.toml'
            path.write_text(path.read_text().replace('serde = "1"', 'serde = {version = "1", path = "dep"}'))
            with self.assertRaises(ValueError):
                cli.validate_manifests(root, 'cli-v0.1.0')

    def test_registry_only_404_means_missing_not_permission(self):
        for status in (403, 404, 429, 500):
            error = urllib.error.HTTPError('url', status, 'error', {}, None)
            with self.subTest(status=status), patch.object(cli.release, 'get_json', side_effect=error):
                exception = ValueError if status == 404 else urllib.error.HTTPError
                with self.assertRaises(exception):
                    cli.require_registry_owner()

    def test_owner_must_match_user(self):
        for users in ([], [{'login': 'other', 'kind': 'user'}], [{'login': cli.OWNER, 'kind': 'team'}]):
            with patch.object(cli.release, 'get_json', side_effect=[{'crate': {'id': cli.CRATE}}, {'users': users}]), self.assertRaises(ValueError):
                cli.require_registry_owner()
        with patch.object(cli.release, 'get_json', side_effect=[{'crate': {'id': cli.CRATE}}, {'users': [{'login': cli.OWNER, 'kind': 'user'}]}]):
            cli.require_registry_owner()

    def run_preflight(self, directory, registry=None):
        env = {'GITHUB_REPOSITORY': cli.release.REPOSITORY, 'GITHUB_REF': 'refs/heads/main', 'GITHUB_SHA': 'tooling', 'GH_TOKEN': 'test'}
        with patch.dict(os.environ, env), patch.object(cli, 'validate_manifests', return_value='0.1.0'), patch.object(cli, 'git', return_value='source'), patch.object(cli.subprocess, 'run') as git, patch.object(cli.release, 'require_successful_ci') as ci, patch.object(cli, 'require_cli_package_ci') as package_ci, patch.object(cli.release, 'registry_version', return_value=registry), patch.object(cli, 'require_registry_owner') as owner:
            result = cli.preflight(Path(directory), 'cli-v0.1.0')
            self.assertEqual([c.args[0] for c in ci.call_args_list], ['source', 'tooling'])
            self.assertEqual([c.args[0] for c in package_ci.call_args_list], ['source', 'tooling'])
            git.assert_called_once_with(['git', 'merge-base', '--is-ancestor', 'source', 'origin/main'], cwd=Path(directory), check=True)
            owner.assert_called_once_with()
            self.assertFalse(result['uploaded'])
            self.assertFalse(result['publication_authorized'])

    def test_preflight_does_not_publish(self):
        with tempfile.TemporaryDirectory() as directory:
            self.run_preflight(directory)

    def test_already_published_rejected(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaisesRegex(ValueError, 'already published'):
            self.run_preflight(directory, {'num': '0.1.0'})

    def test_checkout_must_match_tag(self):
        env = {'GITHUB_REPOSITORY': cli.release.REPOSITORY, 'GITHUB_REF': 'refs/heads/main'}
        with patch.dict(os.environ, env), patch.object(cli, 'validate_manifests', return_value='0.1.0'), patch.object(cli, 'git', side_effect=['head', 'tag']), patch.object(cli.release, 'require_successful_ci') as ci, self.assertRaisesRegex(ValueError, 'does not match'):
            cli.preflight(Path('/unused'), 'cli-v0.1.0')
        ci.assert_not_called()

    def test_latest_source_ci_must_pass(self):
        good = dict(id=1, head_sha='source', head_branch='main', status='completed', conclusion='success', html_url='https://example.test/1')
        for runs in ([], [dict(good, head_sha='old')], [good, dict(good, id=2, conclusion='failure')], [dict(good, status='in_progress')]):
            with self.subTest(runs=runs), patch.object(cli.release, 'get_json', return_value={'workflow_runs': runs}), self.assertRaises(ValueError):
                cli.release.require_successful_ci('source', 'test')

    def test_git_uses_explicit_root(self):
        with patch.object(cli.subprocess, 'check_output', return_value='sha\n') as run:
            self.assertEqual(cli.git(Path('/separate'), 'rev-parse', 'HEAD'), 'sha')
            run.assert_called_once_with(['git', 'rev-parse', 'HEAD'], cwd=Path('/separate'), text=True)

    def test_package_ci_latest_exact_push(self):
        good = dict(id=1, run_attempt=1, head_sha='source', head_branch='main', event='push', status='completed', conclusion='success', html_url='https://example.test/1')
        for runs in ([], [dict(good, head_sha='old')], [dict(good, head_branch='branch')], [dict(good, event='pull_request')], [good, dict(good, id=2, conclusion='failure')], [good, dict(good, run_attempt=2, status='in_progress')], [dict(good, conclusion='skipped')]):
            with self.subTest(runs=runs), patch.object(cli.release, 'get_json', return_value={'workflow_runs': runs}), self.assertRaises(ValueError):
                cli.require_cli_package_ci('source', 'test')
        with patch.object(cli.release, 'get_json', return_value={'workflow_runs': [good]}) as request:
            cli.require_cli_package_ci('source', 'test')
            self.assertIn('/cli-package.yml/runs?head_sha=source&event=push', request.call_args.args[0])

    def test_dispatch_fail_closed(self):
        for repo, ref in (('fork/Rydantic', 'refs/heads/main'), (cli.release.REPOSITORY, 'refs/heads/feature')):
            with patch.dict(os.environ, {'GITHUB_REPOSITORY': repo, 'GITHUB_REF': ref}), self.assertRaises(ValueError):
                cli.preflight(Path('/unused'), 'cli-v0.1.0')


if __name__ == '__main__':
    unittest.main()
