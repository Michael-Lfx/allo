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
    expect(text.includes('void refreshProvidersCatalogIfStale().catch')).toBe(true);
  });

  test('skips the Flowy catalog sync when autoRefreshCatalog is disabled', () => {
    const text = source();
    expect(text.includes('autoRefreshCatalog')).toBe(true);
    expect(text.includes('if (!enabled || !autoRefreshCatalog) return')).toBe(true);
  });

  test('refresh syncs Flowy catalog then replaces SWR without revalidate race', () => {
    const text = source();

    expect(text.includes('ipcBridge.cloud.syncModels.invoke()')).toBe(true);
    expect(text.includes('mutate(PROVIDERS_SWR_KEY, providers, { revalidate: false })')).toBe(true);
    expect(text.includes('clearAvailableModelsCache()')).toBe(true);
    // Must not skip remounts with a TTL — delisted models need a fresh sync on every page enter.
    expect(text.includes('PROVIDERS_AUTO_REFRESH_MIN_INTERVAL_MS')).toBe(false);
  });

  test('local catalog refresh does not sync the cloud catalog', () => {
    const text = source();
    const start = text.indexOf('export async function refreshLocalProviderCatalog');
    const end = text.indexOf('function retainLocalProviderCatalogSync');
    const local = text.slice(start, end);

    expect(start).toBeGreaterThan(-1);
    expect(local.includes('syncModels')).toBe(false);
    expect(local.includes('clearAvailableModelsCache')).toBe(false);
    expect(text.includes('onCatalogChanged')).toBe(true);
    expect(text.includes('conversation.reconnected')).toBe(true);
  });
});
