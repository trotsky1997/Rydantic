import { createServer } from 'node:http';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';
import { ModelRuntime } from '@earendil-works/pi-coding-agent';

const MAX_BODY = 1024 * 1024;
export function createBridge(runtime, { maxConcurrent = 16, timeoutMs = 120_000 } = {}) {
  let active = 0;
  const server = createServer(async (req, res) => {
    const json = (status, value) => {
      if (res.destroyed) return;
      res.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' });
      res.end(JSON.stringify(value));
    };
    const fail = (status, code, message) => json(status, { error: { code, message } });
    if (req.headers.origin) return fail(403, 'origin_not_allowed', 'Browser-origin requests are disabled.');
    const url = new URL(req.url, 'http://127.0.0.1');
    if (req.method === 'GET' && url.pathname === '/health') {
      const auth = await runtime.checkAuth('openai-codex').catch(() => undefined);
      return json(200, { provider: 'openai-codex', authenticated: Boolean(auth), active });
    }
    if (req.method === 'GET' && url.pathname === '/v1/models') {
      const models = await runtime.getAvailable('openai-codex').catch(() => []);
      return json(200, { object: 'list', data: models.map(m => ({ id: m.id, object: 'model', owned_by: 'openai-codex' })) });
    }
    if (req.method !== 'POST' || url.pathname !== '/v1/responses') return fail(404, 'not_found', 'Unknown endpoint.');
    if (!req.headers['content-type']?.startsWith('application/json')) return fail(415, 'content_type', 'Expected application/json.');
    if (active >= maxConcurrent) return fail(429, 'busy', 'Bridge concurrency limit reached.');
    active++;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    res.on('close', () => { if (!res.writableEnded) controller.abort(); });
    try {
      let size = 0;
      const chunks = [];
      for await (const chunk of req) {
        size += chunk.length;
        if (size > MAX_BODY) return fail(413, 'body_too_large', 'Request exceeds 1 MiB.');
        chunks.push(chunk);
      }
      let body;
      try { body = JSON.parse(Buffer.concat(chunks).toString()); }
      catch { return fail(400, 'invalid_json', 'Malformed JSON.'); }
      if (!body || typeof body !== 'object' || Array.isArray(body)) return fail(400, 'invalid_request', 'Expected a request object.');
      const allowed = new Set(['model', 'input', 'instructions', 'tools', 'text', 'reasoning', 'tool_choice', 'parallel_tool_calls', 'stream', 'store', 'include']);
      const unknown = Object.keys(body).filter(k => !allowed.has(k));
      if (unknown.length) return fail(400, 'unsupported_parameter', `Unsupported Codex parameter: ${unknown.join(', ')}`);
      if (body.store === true) return fail(400, 'unsupported_parameter', 'This bridge uses store=false.');
      if (body.stream !== undefined && typeof body.stream !== 'boolean') return fail(400, 'invalid_request', 'stream must be boolean.');
      if (typeof body.input !== 'string' && !Array.isArray(body.input)) return fail(400, 'invalid_request', 'input must be text or an array.');
      if (typeof body.model !== 'string') return fail(400, 'invalid_request', 'model is required.');
      const model = runtime.getModel('openai-codex', body.model);
      if (!model) return fail(404, 'unknown_model', 'Model is absent from the pi Codex catalog.');
      if (!await runtime.checkAuth('openai-codex')) return fail(401, 'login_required', 'Run npm run login in the bridge directory.');
      let final;
      const wantsStream = body.stream === true;
      const events = runtime.stream(model, { messages: [] }, {
        signal: controller.signal, transport: 'sse', maxRetries: 0, timeoutMs,
        onPayload: original => ({ ...original, ...body, model: model.id, store: false, stream: true }),
        async onProviderStreamEvent(event) {
          if (event?.response && ['response.completed', 'response.done', 'response.incomplete', 'response.failed'].includes(event.type)) final = event.response;
          if (!wantsStream) return;
          if (!res.headersSent) res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', 'x-accel-buffering': 'no' });
          if (!res.write(`event: ${event.type ?? 'message'}\ndata: ${JSON.stringify(event)}\n\n`)) await once(res, 'drain', { signal: controller.signal });
        },
      });
      const result = await events.result();
      if (result.stopReason === 'error' || result.stopReason === 'aborted' || !final) {
        const message = controller.signal.aborted ? 'Request cancelled or timed out.' : 'Codex did not complete the response.';
        if (res.headersSent) res.end(`event: error\ndata: ${JSON.stringify({ type: 'error', error: { code: 'upstream_error', message } })}\n\n`);
        else fail(controller.signal.aborted ? 504 : 502, 'upstream_error', message);
      } else if (wantsStream) res.end();
      else json(200, final);
    } catch {
      if (!res.headersSent) fail(controller.signal.aborted ? 504 : 502, 'bridge_error', 'The pi Codex request failed.');
      else if (!res.destroyed) res.end(`event: error\ndata: {"type":"error","error":{"code":"bridge_error","message":"Upstream stream interrupted."}}\n\n`);
    } finally {
      clearTimeout(timer);
      active--;
    }
  });
  server.requestTimeout = timeoutMs + 5000;
  server.headersTimeout = 15_000;
  return server;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const runtime = await ModelRuntime.create({ refreshOnCreate: false, modelsPath: null });
  const port = Number(process.env.RYDANTIC_PI_PORT ?? 8787);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid RYDANTIC_PI_PORT');
  const server = createBridge(runtime);
  server.listen(port, '127.0.0.1', () => console.log(`Rydantic pi Codex bridge listening on http://127.0.0.1:${port}/v1`));
  const stop = () => server.close(() => process.exit(0));
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
}
