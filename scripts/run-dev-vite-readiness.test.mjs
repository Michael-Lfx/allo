import { describe, expect, test } from 'bun:test';

import { createViteHttpReadinessProbe } from './run-dev-vite-readiness.mjs';

const response = (ok) => ({
  ok,
  text: async () => 'ready',
});

describe('Vite HTTP readiness probe', () => {
  test('requires both the app document and a representative lazy route module', async () => {
    const requested = [];
    const probe = createViteHttpReadinessProbe({
      host: '127.0.0.1',
      port: 5173,
      fetchImpl: async (url) => {
        requested.push(url);
        return response(true);
      },
    });

    expect(await probe()).toBe(true);
    expect(requested).toEqual([
      'http://127.0.0.1:5173/',
      'http://127.0.0.1:5173/src/renderer/pages/guid/index.tsx',
    ]);
  });

  test('does not report readiness when the module graph request fails', async () => {
    let requestCount = 0;
    const probe = createViteHttpReadinessProbe({
      host: '::1',
      port: 5173,
      fetchImpl: async () => {
        requestCount += 1;
        return response(requestCount === 1);
      },
    });

    expect(await probe()).toBe(false);
    expect(requestCount).toBe(2);
  });

  test('uses a bounded request timeout and handles a refused request', async () => {
    const probe = createViteHttpReadinessProbe({
      host: '127.0.0.1',
      port: 5173,
      requestTimeoutMs: 1,
      fetchImpl: (_url, { signal }) =>
        new Promise((_, reject) => {
          signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
        }),
    });

    expect(await probe()).toBe(false);
  });
});
