/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./useMediaModels.ts', import.meta.url), 'utf8');

describe('useMediaModels catalog refresh wiring', () => {
  test('fetches on mount without a shared client cache', () => {
    const text = source();

    expect(text.includes('ipcBridge.media.listModels.invoke()')).toBe(true);
    expect(text.includes('void revalidate()')).toBe(true);
    // No SWR / shared mutate cache for the model list.
    expect(text.includes('useSWR')).toBe(false);
    expect(text.includes('MEDIA_MODELS_SWR_KEY')).toBe(false);
    expect(text.includes('refreshMediaModelsCatalogIfStale')).toBe(false);
  });

  test('refresh always hits /api/media/models', () => {
    const text = source();

    expect(text.includes('export async function refreshMediaModelsCatalog')).toBe(true);
    expect(text.includes('return fetchMediaModels()')).toBe(true);
    expect(text.includes('mutate(')).toBe(false);
  });
});
