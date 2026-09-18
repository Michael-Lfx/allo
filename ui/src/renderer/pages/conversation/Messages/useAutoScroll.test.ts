/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

const source = readSource(new URL('./useAutoScroll.ts', import.meta.url));

const sliceBetween = (start: string, end: string): string => {
  const startIndex = source.indexOf(start);
  const endIndex = source.indexOf(end);
  expect(startIndex).toBeGreaterThan(-1);
  expect(endIndex).toBeGreaterThan(startIndex);
  return source.slice(startIndex, endIndex);
};

const resizeObserverEffect = (): string => sliceBetween('let frameId: number | null = null;', 'observer.observe(scrollerEl);');

const scrollToBottomBody = (): string =>
  sliceBetween('const scrollToBottom', 'const resolveFollowOutput');

describe('useAutoScroll thresholds and pause semantics', () => {
  test('aligns scroll button threshold with auto-follow threshold', () => {
    expect(source.includes('export const FOLLOW_BOTTOM_THRESHOLD_PX = 12')).toBe(true);
    expect(source.includes('export const SCROLL_BUTTON_THRESHOLD_PX = 12')).toBe(true);
    expect(source.includes('AT_BOTTOM_THRESHOLD_PX = 100')).toBe(false);
  });

  test('pauses on explicit jumps and layout-changing pointer interactions only', () => {
    expect(source.includes('userIntentPausedRef')).toBe(true);
    expect(source.includes('pauseAutoFollow')).toBe(true);
    expect(source.includes('scrollElementIntoView')).toBe(true);
    expect(source.includes('pauseAutoFollow();')).toBe(true);
    expect(source.includes("target.closest('[aria-expanded]')")).toBe(true);
    expect(source.includes("target.closest('[data-live-window=\"true\"]')")).toBe(true);
    expect(source.includes('const handlePointerDown = useCallback(() =>')).toBe(false);
    expect(source.includes('getBottomGap(scrollerEl) <= SCROLL_BUTTON_THRESHOLD_PX')).toBe(true);
    expect(source.includes('SCROLL_BUTTON_THRESHOLD_PX + LIST_END_SPACER_PX')).toBe(false);
    expect(source.includes('userScrolledRef.current')).toBe(true);
  });

  test('jumps to the last item when a new user message is sent even if follow was paused', () => {
    const sendEffect = sliceBetween(
      'const lastUserId = findLastUserMessageId',
      '}, [messages, scrollToBottom]'
    );
    const scrollToBottom = scrollToBottomBody();

    expect(scrollToBottom.includes('scrollToIndex')).toBe(true);
    expect(scrollToBottom.includes("align: 'end'")).toBe(true);
    expect(sendEffect.includes('lastUserId !== previousLastUserId')).toBe(true);
    expect(sendEffect.includes('if (userIntentPausedRef.current || userScrolledRef.current) return;')).toBe(false);
    expect(sendEffect.includes("scrollToBottom('auto')")).toBe(true);
  });

  test('keeps follow pinned through tail growth and only drops it on an upward scroll', () => {
    expect(source.includes('if (!isAtBottom || userIntentPausedRef.current || userScrolledRef.current)')).toBe(false);
    expect(source.includes('delta < -2')).toBe(true);
  });
});

describe('useAutoScroll scroll ownership', () => {
  test('takes an explicit virtuosoMode flag alongside the virtuoso handle', () => {
    expect(source.includes('virtuosoMode?: boolean')).toBe(true);
    expect(source.includes('virtuosoRef?: RefObject<VirtuosoHandle | null>')).toBe(true);
    expect(source.includes('virtuosoMode')).toBe(true);
  });

  test('keeps Virtuoso followOutput off so in-item tool growth does not bounce', () => {
    const followOutput = sliceBetween('const resolveFollowOutput', 'const handleScrollerRef');

    expect(followOutput.includes("return 'auto'")).toBe(false);
    expect(followOutput.includes('return false')).toBe(true);
  });

  test('coalesces resize follow work into one cancellable animation frame', () => {
    const observer = resizeObserverEffect();
    const growth = sliceBetween('const followContentGrowth', 'const scrollToBottom');

    expect(observer.includes('followContentGrowth()')).toBe(true);
    expect(observer.includes('if (virtuosoMode) return')).toBe(false);
    expect(growth.includes('if (virtuosoMode) return')).toBe(false);
    expect(growth.includes("querySelector('.message-list-end-spacer')")).toBe(true);
    expect(growth.includes('scrollerEl.scrollTop += delta')).toBe(true);
    expect(observer.includes('window.requestAnimationFrame(flushResizeWork)')).toBe(true);
    expect(observer.includes('window.cancelAnimationFrame(frameId)')).toBe(true);
    expect(observer.includes('if (disposed) return')).toBe(true);
    expect(observer.includes('scheduleAutoFollow()')).toBe(false);
  });

  test('pins scrollTop in useLayoutEffect so wrap and follow share one frame', () => {
    expect(source.includes('layoutPinKey')).toBe(true);
    expect(source.includes('[followContentGrowth, layoutPinKey]')).toBe(true);
    const layoutPin = source.slice(source.indexOf('layoutPinKey?: unknown'));
    expect(layoutPin.includes('useLayoutEffect(() => {')).toBe(true);
    expect(layoutPin.includes('followContentGrowth();')).toBe(true);
  });

  test('pins follow to the end spacer so a thinking collapse does not leave the reply in the middle', () => {
    const growth = sliceBetween('const followContentGrowth', 'const scrollToBottom');

    expect(growth.includes("querySelector('.message-list-end-spacer')")).toBe(true);
    expect(growth.includes('getBoundingClientRect()')).toBe(true);
    expect(growth.includes('paddingBottom')).toBe(true);
    expect(growth.includes('scrollerEl.scrollTop += delta')).toBe(true);
    expect(growth.includes('getMaxScrollTop(scrollerEl)')).toBe(true);
    expect(growth.includes('scrollerEl.scrollTop = maxTop')).toBe(true);
    expect(growth.includes('userScrolledRef.current')).toBe(true);
    expect(source.includes('scrollHeight - element.clientHeight - LIST_END_SPACER_PX')).toBe(false);
    expect(source.includes('FOLLOW_BOTTOM_THRESHOLD_PX + LIST_END_SPACER_PX')).toBe(false);
    expect(source.includes('pinFollowAnchorToViewportBottom')).toBe(false);
    expect(source.includes('data-scroll-follow-anchor')).toBe(false);
  });

  test('does not pull the viewport back after the user scrolls up', () => {
    const growth = sliceBetween('const followContentGrowth', 'const scrollToBottom');

    expect(growth.includes('if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current) return;')).toBe(
      true
    );
    expect(source.includes('delta < -2')).toBe(true);
  });

  test('uses one scrollToIndex path and one layout correction for an explicit jump to bottom', () => {
    const scrollToBottom = scrollToBottomBody();
    const scrollToIndexCount = scrollToBottom.split('scrollToIndex').length - 1;
    const nestedRaf = /requestAnimationFrame\(\(\) =>\s*\{\s*requestAnimationFrame/.test(scrollToBottom);

    expect(scrollToIndexCount).toBe(1);
    expect(scrollToBottom.includes("align: 'end'")).toBe(true);
    expect(scrollToBottom.includes('getMaxScrollTop')).toBe(true);
    expect(nestedRaf).toBe(false);
    expect(scrollToBottom.includes('requestAnimationFrame')).toBe(true);
  });

  test('does not flicker the scroll button on per-frame follow gaps', () => {
    const updateBottom = sliceBetween('const updateBottomState', 'const pauseAutoFollow');

    expect(updateBottom.includes('userScrolledRef.current')).toBe(true);
    expect(updateBottom.includes('withinButtonThreshold')).toBe(true);
    expect(updateBottom.includes('nextShowButton')).toBe(true);
    expect(source.includes('FOLLOW_BOTTOM_THRESHOLD_PX = 12')).toBe(true);
  });
});
