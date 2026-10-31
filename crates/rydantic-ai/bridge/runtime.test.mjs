import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const range = '>=22.23.4 <23 || >=24.22.1 <25';
const atLeast = (actual, minimum) => {
  const parts = actual.split('.').map(Number);
  for (let i = 0; i < minimum.length; i++) {
    if (parts[i] !== minimum[i]) return parts[i] > minimum[i];
  }
  return true;
};

test('supported Node runtime and built-in undici meet the audited baselines', t => {
  const manifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url)));
  const lock = JSON.parse(readFileSync(new URL('./package-lock.json', import.meta.url)));
  assert.equal(manifest.engines.node, range);
  assert.equal(lock.packages[''].engines.node, range);
  const { node, undici } = process.versions;
  t.diagnostic(`Node ${node}; built-in undici ${undici}; npm override is independent`);
  const major = Number(node.split('.')[0]);
  assert.ok(major === 22 || major === 24, `unsupported Node line: ${node}`);
  assert.ok(atLeast(node, major === 22 ? [22, 23, 4] : [24, 22, 1]), `Node below audited baseline: ${node}`);
  assert.ok(undici, 'runtime must report its built-in undici');
  const expectedMajor = major === 22 ? 6 : 7;
  assert.equal(Number(undici.split('.')[0]), expectedMajor, 're-audit a new built-in undici major');
  assert.ok(atLeast(undici, expectedMajor === 6 ? [6, 29, 1] : [7, 30, 1]), `built-in undici below audited baseline: ${undici}`);
});
