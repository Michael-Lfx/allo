/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageList.tsx', import.meta.url), 'utf8');

describe('MessageList streaming caches', () => {
  test('reuses the settled raw prefix while message objects remain stable', () => {
    expect(source.includes('prefixProcessedListCacheRef')).toBe(true);
    expect(source.includes('hasStablePrefix(prefixProcessedListCacheRef.current.sourceList')).toBe(true);
    expect(source.includes('sourceList: list')).toBe(true);
  });

  test('replaces only the growing assistant tail in the display model', () => {
    expect(source.includes('const canReuseStreamingTail =')).toBe(true);
    expect(source.includes('hasStablePrefix(previous.processedList, processedList, processedList.length - 1)')).toBe(
      true
    );
    expect(source.includes('nextDisplayList[displayIndex] = nextTail')).toBe(true);
  });
});
