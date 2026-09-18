import { describe, expect, test } from 'bun:test';

import {
  claimServiceWorkerCleanupReload,
  SERVICE_WORKER_CLEANUP_RELOAD_STORAGE_KEY,
} from './serviceWorkerRecovery';

function createStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  };
}

describe('service worker cleanup recovery', () => {
  test('allows one cleanup reload per URL', () => {
    const storage = createStorage();
    const href = 'http://127.0.0.1:5173/#/conversation';

    expect(claimServiceWorkerCleanupReload(storage, href)).toBe(true);
    expect(storage.getItem(SERVICE_WORKER_CLEANUP_RELOAD_STORAGE_KEY)).toBe(href);
    expect(claimServiceWorkerCleanupReload(storage, href)).toBe(false);
    expect(claimServiceWorkerCleanupReload(storage, `${href}/next`)).toBe(true);
  });

  test('fails closed when session storage is unavailable', () => {
    expect(claimServiceWorkerCleanupReload(undefined, 'http://localhost/')).toBe(false);
    expect(
      claimServiceWorkerCleanupReload(
        {
          getItem: () => null,
          setItem: () => {
            throw new Error('storage blocked');
          },
        },
        'http://localhost/',
      ),
    ).toBe(false);
  });
});
