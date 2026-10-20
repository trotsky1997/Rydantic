import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createBridge } from './bridge.mjs';

async function fixture(t, runtime, options) {
  const server = createBridge(runtime, options);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  const base = `http://127.0.0.1:${server.address().port}`;
  return (body, headers = {}) => fetch(`${base}/v1/responses`, { method: 'POST', headers: { 'content-type': 'application/json', ...headers }, body: JSON.stringify(body) });
}
function runtime(auth = true) {
  const seen = [];
  return {
    seen, checkAuth: async () => auth, getModel: (_, id) => id === 'test' ? { id } : undefined,
    stream(model, _, options) { return { result: async () => {
      seen.push(await options.onPayload({ instructions: 'default' }, model));
      await options.onProviderStreamEvent({ type: 'response.output_text.delta', delta: 'hello' });
      await options.onProviderStreamEvent({ type: 'response.completed', response: { status: 'completed', output: [], usage: { input_tokens: 2, output_tokens: 3 } } });
      return { stopReason: 'stop' };
    } }; },
  };
}
test('native Responses SSE events and usage pass through without credential values', async t => {
  const model = runtime();
  const post = await fixture(t, model);
  const response = await post({ model: 'test', input: 'hello', stream: true, instructions: 'caller instructions' });
  assert.equal(response.status, 200);
  const text = await response.text();
  assert.match(text, /response.output_text.delta/);
  assert.match(text, /"input_tokens":2/);
  assert.equal(model.seen[0].instructions, 'caller instructions');
  assert.equal(model.seen[0].store, false);
  assert.equal(model.seen[0].stream, true);
});
test('nonstream requests return the native completed response', async t => {
  const post = await fixture(t, runtime());
  const response = await post({ model: 'test', input: [] });
  assert.equal((await response.json()).status, 'completed');
});
test('unknown parameters, unrecognized models and browser origins fail before inference', async t => {
  const model = runtime();
  const post = await fixture(t, model);
  assert.equal((await post({ model: 'test', input: [], max_output_tokens: 4 })).status, 400);
  assert.equal((await post({ model: 'missing', input: [] })).status, 404);
  assert.equal((await post({ model: 'test', input: [] }, { origin: 'https://example.com' })).status, 403);
  assert.equal(model.seen.length, 0);
});
test('missing OAuth authorization fails explicitly', async t => {
  const post = await fixture(t, runtime(false));
  const response = await post({ model: 'test', input: [] });
  assert.equal(response.status, 401);
  assert.equal((await response.json()).error.code, 'login_required');
});
test('upstream request timeout aborts the provider', async t => {
  const model = runtime();
  let aborted = false;
  model.stream = (_, __, options) => ({ result: () => new Promise((resolve, reject) => options.signal.addEventListener('abort', () => { aborted = true; reject(new Error('aborted')); }, { once: true })) });
  const post = await fixture(t, model, { timeoutMs: 40 });
  const response = await post({ model: 'test', input: [] });
  assert.equal(response.status, 504);
  assert.equal(aborted, true);
});
