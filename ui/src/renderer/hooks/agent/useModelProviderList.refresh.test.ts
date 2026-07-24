/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./useModelProviderList.ts', import.meta.url), 'utf8');

describe('useModelProviderList catalog refresh wiring', () => {
  test('auto-refreshes shared providers cache when model-selector consumers mount', () => {
    const text = source();

    expect(text.includes('providersAutoRefreshPromise')).toBe(true);
    expect(text.includes('refreshProvidersCatalogIfStale')).toBe(true);
    expect(text.includes('useEffect')).toBe(true);
    expect(text.includes('void refreshProvidersCatalogIfStale()')).toBe(true);
  });

  test('refresh syncs Flowy catalog then replaces SWR without revalidate race', () => {
    const text = source();

    expect(text.includes('ipcBridge.cloud.syncModels.invoke()')).toBe(true);
    expect(text.includes('mutate(PROVIDERS_SWR_KEY, providers, { revalidate: false })')).toBe(true);
    expect(text.includes('clearAvailableModelsCache()')).toBe(true);
    // Must not skip remounts with a TTL — delisted models need a fresh sync on every page enter.
    expect(text.includes('PROVIDERS_AUTO_REFRESH_MIN_INTERVAL_MS')).toBe(false);
  });
});
