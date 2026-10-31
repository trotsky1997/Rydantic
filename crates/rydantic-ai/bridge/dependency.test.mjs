import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { gzipSync } from 'node:zlib';
import { ModelRuntime } from '@earendil-works/pi-coding-agent';
import { createBridge } from './bridge.mjs';

// Resolve exactly as the SDK does, not from an unrelated global installation.
const sdkRequire = createRequire(import.meta.resolve('@earendil-works/pi-coding-agent'));
const undici = sdkRequire('undici');
const nativeFetch = globalThis.fetch;

test('SDK resolves the narrowly pinned patched undici without changing its upstream engine floor', () => {
  const manifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url)));
  const lock = JSON.parse(readFileSync(new URL('./package-lock.json', import.meta.url)));
  const installed = sdkRequire('undici/package.json');
  assert.equal(manifest.overrides['@earendil-works/pi-coding-agent@1.3.0'].undici, '8.12.2');
  assert.equal(installed.version, '8.12.2');
  assert.equal(installed.engines.node, '>=22.19.0');
  const entries = Object.entries(lock.packages).filter(([path]) => path.endsWith('/undici'));
  assert.equal(entries.length, 1);
  assert.equal(entries[0][1].version, installed.version);
  for (const name of ['Client', 'Pool', 'EnvHttpProxyAgent', 'setGlobalDispatcher', 'install']) {
    assert.equal(typeof undici[name], 'function', `SDK dispatcher requires ${name}`);
  }
});

async function listen(t, server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  return `http://127.0.0.1:${server.address().port}`;
}

// This is a fixture string, not a credential. It is sent only to loopback.
const token = `test.${Buffer.from(JSON.stringify({
  'https://api.openai.com/auth': { chatgpt_account_id: 'local-fixture' },
})).toString('base64url')}.test`;
const credentials = {
  read: async () => undefined,
  list: async () => [],
  modify: async () => { throw new Error('Fixture must not write credentials'); },
  delete: async () => { throw new Error('Fixture must not delete credentials'); },
};

for (const [label, providerFetch] of [['Node native fetch', nativeFetch], ['npm undici fetch', undici.fetch]]) {
  test(`real ModelRuntime and bridge preserve compressed Responses over ${label}`, { timeout: 10_000 }, async t => {
    const seen = [];
    let upstreamClosed;
    const response = { id: 'local-response', status: 'completed', output: [],
      usage: { input_tokens: 2, output_tokens: 3, total_tokens: 5 } };
    const upstream = await listen(t, createServer(async (req, res) => {
      // Consume the body without depending on the SDK's request-compression choice.
      for await (const _ of req) { /* drain */ }
      seen.push({ url: req.url, authorization: req.headers.authorization });
      if (seen.length === 3) {
        res.writeHead(503, { 'content-type': 'application/json' });
        return res.end(JSON.stringify({ error: { message: 'local failure' } }));
      }
      if (seen.length === 4) {
        upstreamClosed = once(res, 'close');
        return; // The bridge must cancel this stalled provider request.
      }
      res.writeHead(200, { 'content-type': 'text/event-stream', 'content-encoding': 'gzip' });
      res.end(gzipSync(`event: response.completed\ndata: ${JSON.stringify({ type: 'response.completed', response })}\n\n`));
    }));
    const fetchBeforeCreate = globalThis.fetch;
    const runtime = await ModelRuntime.create({ credentials, modelsPath: null,
      refreshOnCreate: false, allowModelNetwork: false });
    assert.equal(globalThis.fetch, fetchBeforeCreate, 'SDK create must preserve native transport choice');
    runtime.registerProvider('openai-codex', { baseUrl: upstream, apiKey: token });
    const model = runtime.getModels('openai-codex')[0];
    assert.ok(model, 'SDK static catalog must contain a Codex model');
    assert.equal(model.baseUrl, upstream);
    const bridge = await listen(t, createBridge(runtime, { timeoutMs: 2000 }));
    const priorFetch = globalThis.fetch;
    globalThis.fetch = (url, options) => {
      assert.equal(new URL(url).origin, upstream, 'provider requests must remain local');
      return providerFetch(url, options);
    };
    t.after(() => { globalThis.fetch = priorFetch; });
    for (const stream of [false, true]) {
      const actual = await nativeFetch(`${bridge}/v1/responses`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: model.id, input: 'local fixture', stream }),
      });
      assert.equal(actual.status, 200);
      const body = await actual.text();
      assert.ok(!body.includes(token));
      if (stream) {
        assert.match(body, /event: response.completed/);
        assert.match(body, /"input_tokens":2/);
      } else assert.deepEqual(JSON.parse(body), response);
    }
    for (const status of [502, 504]) {
      const actual = await nativeFetch(`${bridge}/v1/responses`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: model.id, input: 'local failure fixture' }),
      });
      assert.equal(actual.status, status);
      assert.equal((await actual.json()).error.code, 'upstream_error');
    }
    assert.ok(upstreamClosed, 'stalled request must reach the fixture');
    await upstreamClosed;
    assert.equal(seen.length, 4);
    for (const request of seen) {
      assert.equal(request.url, '/codex/responses');
      assert.equal(request.authorization, `Bearer ${token}`);
    }
  });
}
