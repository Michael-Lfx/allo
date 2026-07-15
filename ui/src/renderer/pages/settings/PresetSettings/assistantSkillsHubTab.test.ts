/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('PresetSettings page shell', () => {
  test('consumes highlight search params without an activeTab dependency', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes("searchParams.get('highlight')")).toBe(true);
    expect(source.includes('handleHighlightConsumed')).toBe(true);
    expect(source.includes('activeTab')).toBe(false);
    expect(source.includes('assistant-skills-hub-tabs')).toBe(false);
  });

  test('renders through HubPageShell without nested flex height chains', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('<HubPageShell')).toBe(true);
    expect(source.includes('lazyload')).toBe(false);
    expect(source.includes('flex-1 min-h-0')).toBe(true);
  });
});
