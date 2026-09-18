/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./runtimePatches.ts', import.meta.url), 'utf8');

describe('runtime resize observer scope', () => {
  test('does not replace the browser ResizeObserver globally', () => {
    expect(source.includes('class SafeResizeObserver')).toBe(false);
    expect(source.includes('window.ResizeObserver =')).toBe(false);
    expect(source.includes('patchResizeObserver')).toBe(false);
    expect(source.includes('patchGlobalErrorFilters();')).toBe(true);
  });
});
