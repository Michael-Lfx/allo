/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('AssistantSettings hub tab sync', () => {
  test('syncs active tab from URL without depending on activeTab state', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('}, [searchParams]);')).toBe(true);
    expect(source.includes('}, [activeTab, searchParams]);')).toBe(false);
    expect(source.includes('if (nextTab !== activeTab)')).toBe(false);
  });

  test('keeps tab nav height stable and avoids flex height chains in tab panes', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('lazyload')).toBe(false);
    expect(source.includes('flex-1 min-h-0')).toBe(false);
    expect(source.includes('h-full')).toBe(false);
    expect(source.includes('[&_.arco-tabs-nav]:!min-h-40px')).toBe(true);
  });
});
