import { createInterface } from 'node:readline/promises';
import { ModelRuntime } from '@earendil-works/pi-coding-agent';

const models = await ModelRuntime.create({ refreshOnCreate: false, modelsPath: null });
const input = createInterface({ input: process.stdin, output: process.stdout });
const controller = new AbortController();
process.once('SIGINT', () => controller.abort());
try {
  await models.login('openai-codex', 'oauth', {
    signal: controller.signal,
    notify(event) {
      if (event.type === 'auth_url') console.log(`LOGIN_URL=${event.url}`);
      else if (event.type === 'device_code') console.log(`Open ${event.verificationUri}; code ${event.userCode}`);
      else if (event.type === 'progress' || event.type === 'info') console.log(event.message);
    },
    async prompt(prompt) {
      if (prompt.type === 'select') return 'browser';
      return input.question(`${prompt.message}\n`, { signal: prompt.signal ?? controller.signal });
    },
  });
  console.log('LOGIN_SUCCESS: openai-codex credentials saved by pi.');
} catch {
  console.error('LOGIN_FAILED: authorization was not completed. Retry login or check network access.');
  process.exitCode = 1;
} finally {
  input.close();
}
