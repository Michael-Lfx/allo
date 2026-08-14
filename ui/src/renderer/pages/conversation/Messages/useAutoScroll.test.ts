/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useAutoScroll thresholds and pause semantics', () => {
  test('aligns scroll button threshold with auto-follow threshold', () => {
    const source = readSource(new URL('./useAutoScroll.ts', import.meta.url));

    expect(source.includes('export const FOLLOW_BOTTOM_THRESHOLD_PX = 12')).toBe(true);
    expect(source.includes('export const SCROLL_BUTTON_THRESHOLD_PX = 12')).toBe(true);
    expect(source.includes('AT_BOTTOM_THRESHOLD_PX = 100')).toBe(false);
  });

  test('pauses on explicit jumps and layout-changing pointer interactions only', () => {
    const source = readSource(new URL('./useAutoScroll.ts', import.meta.url));

    expect(source.includes('userIntentPausedRef')).toBe(true);
    expect(source.includes('pauseAutoFollow')).toBe(true);
    expect(source.includes('scrollElementIntoView')).toBe(true);
    expect(source.includes('pauseAutoFollow();')).toBe(true);
    expect(source.includes("target.closest('[aria-expanded]')")).toBe(true);
    expect(source.includes('const handlePointerDown = useCallback(() =>')).toBe(false);
    expect(source.includes('getBottomGap(scrollerEl) <= SCROLL_BUTTON_THRESHOLD_PX')).toBe(true);
    expect(source.includes('userIntentPausedRef.current || userScrolledRef.current')).toBe(true);
  });

  test('uses incremental growth scroll and Virtuoso followOutput in virtuoso mode', () => {
    const source = readSource(new URL('./useAutoScroll.ts', import.meta.url));

    expect(source.includes('virtuosoMode')).toBe(true);
    expect(source.includes('followContentGrowth')).toBe(true);
    expect(source.includes('scrollTop +=')).toBe(true);
    expect(source.includes('resolveFollowOutput')).toBe(true);
    expect(source.includes('virtuosoModeRef.current')).toBe(true);
  });

  test('keeps follow pinned through tail growth and only drops it on an upward scroll', () => {
    const source = readSource(new URL('./useAutoScroll.ts', import.meta.url));

    expect(source.includes('if (!isAtBottom || userIntentPausedRef.current || userScrolledRef.current)')).toBe(false);
    expect(source.includes('if (userIntentPausedRef.current || userScrolledRef.current)')).toBe(true);
    expect(source.includes('delta < -2')).toBe(true);
  });
});
