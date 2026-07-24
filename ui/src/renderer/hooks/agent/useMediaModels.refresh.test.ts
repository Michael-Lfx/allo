/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./useMediaModels.ts', import.meta.url), 'utf8');

describe('useMediaModels catalog refresh wiring', () => {
  test('revalidates on mount and shares a single SWR key', () => {
    const text = source();

    expect(text.includes('MEDIA_MODELS_SWR_KEY')).toBe(true);
    expect(text.includes('revalidateOnMount: true')).toBe(true);
    expect(text.includes('refreshMediaModelsCatalogIfStale')).toBe(true);
    expect(text.includes('void refreshMediaModelsCatalogIfStale()')).toBe(true);
  });

  test('refresh hits /api/media/models and replaces SWR without revalidate race', () => {
    const text = source();

    expect(text.includes('ipcBridge.media.listModels.invoke()')).toBe(true);
    expect(text.includes('mutate(MEDIA_MODELS_SWR_KEY, list, { revalidate: false })')).toBe(true);
    expect(text.includes('MEDIA_AUTO_REFRESH_MIN_INTERVAL_MS')).toBe(false);
  });
});
