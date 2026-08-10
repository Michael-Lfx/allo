import { describe, expect, test } from 'bun:test';

import { createRefreshRetryController } from './refreshRetry';

const flush = async (): Promise<void> => {
  await Promise.resolve();
  await Promise.resolve();
};

describe('createRefreshRetryController', () => {
  test('coalesces events and retries until the authoritative refresh succeeds', async () => {
    let calls = 0;
    const controller = createRefreshRetryController({
      delaysMs: [0],
      load: async () => {
        calls += 1;
        if (calls === 1) throw new Error('temporary');
        return true;
      },
    });
    controller.trigger();
    controller.trigger();
    await new Promise((resolve) => setTimeout(resolve, 5));
    await flush();
    expect(calls).toBe(2);
    controller.dispose();
  });

  test('dispose cancels future retries', async () => {
    let calls = 0;
    const controller = createRefreshRetryController({
      delaysMs: [20],
      load: async () => {
        calls += 1;
        throw new Error('temporary');
      },
    });
    controller.trigger();
    await flush();
    controller.dispose();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(calls).toBe(1);
  });

  test('retries a resolved refresh until it authoritatively applies', async () => {
    let calls = 0;
    const controller = createRefreshRetryController({
      delaysMs: [0],
      load: async () => {
        calls += 1;
        return calls > 1;
      },
    });
    controller.trigger();
    await new Promise((resolve) => setTimeout(resolve, 5));
    await flush();
    expect(calls).toBe(2);
    controller.dispose();
  });
});
