import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const range = '>=22.23.5 <23 || >=24.22.3 <25';
const atLeast = (actual, minimum) => {
  const parts = actual.split('.').map(Number);
  for (let i = 0; i < minimum.length; i++) {
    if (parts[i] !== minimum[i]) return parts[i] > minimum[i];
  }
  return true;
};

function assertAuditedRuntime(node, undici) {
  assert.match(node, /^\d+\.\d+\.\d+$/, 'runtime must report a stable Node release');
  const major = Number(node.split('.')[0]);
  assert.ok(major === 22 || major === 24, `unsupported Node line: ${node}`);
  assert.ok(atLeast(node, major === 22 ? [22, 23, 5] : [24, 22, 3]), `Node below audited baseline: ${node}`);
  assert.ok(undici, 'runtime must report its built-in undici');
  assert.match(undici, /^\d+\.\d+\.\d+$/, 'runtime must report a stable built-in undici release');
  const expectedMajor = major === 22 ? 6 : 7;
  assert.equal(Number(undici.split('.')[0]), expectedMajor, 're-audit a new built-in undici major');
  // Node 22.23.5 still bundles 6.29.1; the November undici fixes target 7.x/8.x.
  assert.ok(atLeast(undici, expectedMajor === 6 ? [6, 29, 1] : [7, 30, 2]), `built-in undici below audited baseline: ${undici}`);
}

test('runtime policy accepts patched floors and rejects previous or mismatched baselines', () => {
  for (const [node, undici] of [
    ['22.23.5', '6.29.1'], ['22.24.0', '6.30.0'],
    ['24.22.3', '7.30.2'], ['24.23.0', '7.31.0'],
  ]) assert.doesNotThrow(() => assertAuditedRuntime(node, undici), `${node} / ${undici}`);
  for (const [node, undici] of [
    ['22.23.4', '6.29.1'], ['24.22.1', '7.30.1'], ['24.22.2', '7.30.2'],
    ['22.23.5', '6.29.0'], ['24.22.3', '7.30.1'],
    ['24.22.3', '8.12.4'], ['25.0.0', '7.30.2'],
    ['22.23.5-rc.1', '6.29.1'], ['24.22.3', '7.30.2-rc.1'],
    ['24.22.3', undefined],
  ]) assert.throws(() => assertAuditedRuntime(node, undici), `${node} / ${undici}`);
});

test('supported Node runtime and built-in undici meet the audited baselines', t => {
  const manifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url)));
  const lock = JSON.parse(readFileSync(new URL('./package-lock.json', import.meta.url)));
  assert.equal(manifest.engines.node, range);
  assert.equal(lock.packages[''].engines.node, range);
  const { node, undici } = process.versions;
  t.diagnostic(`Node ${node}; built-in undici ${undici}; npm override is independent`);
  assertAuditedRuntime(node, undici);
});
