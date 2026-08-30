/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./runtimePatches.ts', import.meta.url), 'utf8');

describe('SafeResizeObserver lifecycle', () => {
  test('cancels queued callbacks when an observer disconnects', () => {
    const observerSource = source.slice(
      source.indexOf('class SafeResizeObserver'),
      source.indexOf('window.ResizeObserver = SafeResizeObserver')
    );

    expect(observerSource.includes('private scheduledFrame')).toBe(true);
    expect(observerSource.includes('private callbackGeneration')).toBe(true);
    expect(observerSource.includes('if (generation !== this.callbackGeneration) return;')).toBe(true);
    expect(observerSource.includes('disconnect()')).toBe(true);
    expect(observerSource.includes('cancelAnimationFrame(this.scheduledFrame)')).toBe(true);
    expect(observerSource.includes('super.disconnect()')).toBe(true);
  });
});
