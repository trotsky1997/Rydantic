import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { resolveModelScopeWithDiagnostics } from '@earendil-works/pi-coding-agent';

const sdkRequire = createRequire(import.meta.resolve('@earendil-works/pi-coding-agent'));
const { minimatch } = sdkRequire('minimatch');

test('SDK resolves the narrowly scoped patched minimatch and unchanged pi versions', () => {
  const manifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url)));
  const lock = JSON.parse(readFileSync(new URL('./package-lock.json', import.meta.url)));
  assert.equal(manifest.dependencies['@earendil-works/pi-coding-agent'], '1.3.0');
  assert.equal(manifest.dependencies['@earendil-works/pi-ai'], '1.3.0');
  assert.equal(manifest.overrides['@earendil-works/pi-coding-agent@1.3.0'].minimatch, '11.0.1');
  assert.equal(sdkRequire('minimatch/package.json').version, '11.0.1');
  const entries = Object.entries(lock.packages).filter(([path]) => path.endsWith('/minimatch'));
  assert.equal(entries.length, 1);
  assert.equal(entries[0][1].version, '11.0.1');
  assert.equal(entries[0][1].resolved, 'https://registry.npmjs.org/minimatch/-/minimatch-11.0.1.tgz');
});

test('real SDK model glob selection preserves ordering, deduplication and diagnostics', async () => {
  const models = [
    { provider: 'local', id: 'Alpha-1', name: 'Alpha' },
    { provider: 'local', id: 'Alpha-2', name: 'Alpha 2' },
    { provider: 'other', id: 'Beta-1', name: 'Beta' },
  ];
  // Synthetic catalog: no credentials, model loading, network or filesystem access.
  const runtime = { getAvailable: async () => models };
  const result = await resolveModelScopeWithDiagnostics(
    ['LOCAL/alpha-?:high', 'Alpha-[12]:low', 'other/*', 'missing-*'], runtime);
  assert.deepEqual(result.scopedModels, [
    { model: models[0], thinkingLevel: 'high' },
    { model: models[1], thinkingLevel: 'high' },
    { model: models[2], thinkingLevel: undefined },
  ]);
  assert.deepEqual(result.diagnostics, [{ type: 'warning', code: 'no-match',
    message: 'No models match pattern "missing-*"', pattern: 'missing-*' }]);
});

test('SDK-local minimatch retains representative package path matching', () => {
  // These are the named-function/default-options calls used by pi package-manager,
  // not a claim to exercise package installation or every v10 glob semantic.
  for (const [path, pattern, expected] of [
    ['skills/demo/SKILL.md', 'skills/**/SKILL.md', true],
    ['extensions/demo.ts', '**/*.{ts,js}', true],
    ['extensions/demo.txt', '**/*.{ts,js}', false],
    ['.hidden/SKILL.md', '**/SKILL.md', false],
    ['demo.ts', '!*.js', true],
    ['demo.js', '!*.js', false],
  ]) assert.equal(minimatch(path, pattern), expected, `${path} / ${pattern}`);
});

test('level-2 glob preprocessing respects a small expansion bound', () => {
  // Short fixture + small cap, isolated with a generous startup wall-time limit.
  // This is an expansion-bound check, not a speed benchmark. Never run an
  // unbounded DoS payload. This fails on 10.2.6 (12 rows) and passes on 11.0.1 (8).
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { createRequire } from 'node:module';
    const require = createRequire(${JSON.stringify(import.meta.resolve('@earendil-works/pi-coding-agent'))});
    const { Minimatch } = require('minimatch');
    const match = new Minimatch('**/../x/'.repeat(6) + 'target',
      { optimizationLevel: 2, braceExpandMax: 8 });
    if (match.globParts.length > 8) process.exit(1);
    console.log(match.globParts.length);
  `], { encoding: 'utf8', timeout: 10_000, maxBuffer: 1024 });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), '8');
});
