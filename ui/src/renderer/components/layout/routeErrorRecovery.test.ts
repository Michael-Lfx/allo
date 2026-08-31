import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import {
  claimDynamicImportReload,
  DYNAMIC_IMPORT_RETRY_STORAGE_KEY,
  DYNAMIC_IMPORT_RETRY_WINDOW_MS,
  isDynamicImportFailure,
} from './routeErrorRecovery';

function createStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  };
}

describe('dynamic import route recovery', () => {
  test('recognizes browser module-load failures but not ordinary route errors', () => {
    expect(isDynamicImportFailure(new TypeError('Failed to fetch dynamically imported module'))).toBe(true);
    expect(isDynamicImportFailure(new Error('Importing a module script failed'))).toBe(true);
    expect(isDynamicImportFailure(new Error('route component crashed'))).toBe(false);
  });

  test('allows one reload per URL within the retry window', () => {
    const storage = createStorage();
    const href = 'http://127.0.0.1:5173/#/guid';

    expect(claimDynamicImportReload(storage, href, 1_000)).toBe(true);
    expect(storage.getItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY)).toContain(href);
    expect(claimDynamicImportReload(storage, href, 1_001)).toBe(false);
    expect(claimDynamicImportReload(storage, href, 1_000 + DYNAMIC_IMPORT_RETRY_WINDOW_MS + 1)).toBe(true);
  });

  test('does not block recovery when storage is unavailable', () => {
    expect(claimDynamicImportReload(undefined, 'http://localhost/#/guid', 100)).toBe(true);
  });

  test('uses the same bounded recovery gate in the nested settings boundary', () => {
    const source = readFileSync(new URL('./SettingsSiderErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(source).toContain('claimDynamicImportReload');
    expect(source).toContain('dynamicImportReloadExhausted');
  });
});
