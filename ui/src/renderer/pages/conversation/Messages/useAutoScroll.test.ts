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
      'requestAnimationFrame(() => {\n      requestAnimationFrame(() => {'
    );
    const scrollToBottom = scrollToBottomBody();

    expect(scrollToBottom.includes('scrollToIndex')).toBe(true);
    expect(scrollToBottom.includes("align: 'end'")).toBe(true);
    expect(sendEffect.includes('lastUserId !== previousLastUserId')).toBe(true);
    expect(sendEffect.includes('if (userIntentPausedRef.current || userScrolledRef.current) return;')).toBe(false);
  });

  test('keeps follow pinned through tail growth and only drops it on an upward scroll', () => {
    expect(source.includes('if (!isAtBottom || userIntentPausedRef.current || userScrolledRef.current)')).toBe(false);
    expect(source.includes('delta < -2')).toBe(true);
  });
});

describe('useAutoScroll session reading position persistence', () => {
  test('accepts optional conversationId in UseAutoScrollOptions', () => {
    expect(source.includes('conversationId?: string')).toBe(true);
  });

  test('records scroll position in sessionScrollRegistry on handleScroll and pauseAutoFollow', () => {
    expect(source.includes('sessionScrollRegistry.save(conversationId, {')).toBe(true);
    expect(source.includes('sessionScrollRegistry')).toBe(true);
  });

  test('restores saved reading position in useLayoutEffect when user was reading history', () => {
    const restoreEffect = sliceBetween(
      '// Handle session switch, initial scroll, and reading position restoration before paint',
      '// Save on unmount'
    );
    expect(restoreEffect.includes('sessionScrollRegistry.get(conversationId)')).toBe(true);
    expect(restoreEffect.includes('if (saved && saved.userScrolled)')).toBe(true);
    expect(restoreEffect.includes('scrollerEl.scrollTop = targetScrollTop')).toBe(true);
    expect(restoreEffect.includes('isRestoringScrollRef.current = true')).toBe(true);
    expect(restoreEffect.includes('userScrolledRef.current = true')).toBe(true);
    expect(restoreEffect.includes('userIntentPausedRef.current = true')).toBe(true);
    expect(restoreEffect.includes('setShowScrollButton(false)')).toBe(true);
  });

  test('saves scroll state when switching conversation or unmounting', () => {
    expect(source.includes('previousConversationIdRef')).toBe(true);
    expect(source.includes('conversationId !== previousConversationIdRef.current')).toBe(true);
    expect(source.includes('sessionScrollRegistry.save(prevId, {')).toBe(true);
    expect(source.includes('sessionScrollRegistry.save(currentId, {')).toBe(true);
  });

  test('guards switch-time saves and restoration against stale timing', () => {
    const restoreEffect = sliceBetween(
      '// Handle session switch, initial scroll, and reading position restoration before paint',
      '// Save on unmount'
    );
    expect(source.includes('loadedConversationId?: string | null')).toBe(true);
    // Switch-time save uses the ref-tracked offset, not the (possibly
    // detached or next-session) DOM element.
    expect(restoreEffect.includes('scrollTop: lastScrollTopRef.current')).toBe(true);
    expect(restoreEffect.includes('scrollTop: scrollerEl.scrollTop')).toBe(false);
    // Restoration waits for the store to confirm list ownership.
    expect(restoreEffect.includes('loadedConversationId !== conversationId')).toBe(true);
    // The send-effect baseline is seeded when the new session's list arrives.
    expect(restoreEffect.includes('previousLastUserIdRef.current = findLastUserMessageId(messages)')).toBe(true);
    expect(restoreEffect.includes('swapBaselinePendingRef.current = false')).toBe(true);
    // Sessions never displayed (rapid A→B→C hops) are not snapshotted.
    expect(restoreEffect.includes('prevId && initialScrollDoneRef.current')).toBe(true);
  });

  test('debounces per-event registry writes and suppresses swap-triggered send jumps', () => {
    expect(source.includes('SCROLL_SAVE_DEBOUNCE_MS')).toBe(true);
    const sendEffect = sliceBetween(
      'const lastUserId = findLastUserMessageId',
      'requestAnimationFrame(() => {\n      requestAnimationFrame(() => {'
    );
    expect(
      sendEffect.includes(
        'if (!initialScrollDoneRef.current || swapBaselinePendingRef.current) return;'
      )
    ).toBe(true);
  });

  test('gates all registry write paths on visible list ownership', () => {
    const pauseBlock = sliceBetween('const pauseAutoFollow = useCallback', 'const followContentGrowth');
    expect(pauseBlock.includes('const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;')).toBe(true);
    expect(pauseBlock.includes('if (conversationId && scrollerEl && ownsVisibleList)')).toBe(true);

    const scrollBlock = sliceBetween('const handleScroll = useCallback', 'const handleWheel');
    expect(scrollBlock.includes('const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;')).toBe(true);
    expect(scrollBlock.includes('if (conversationId && ownsVisibleList)')).toBe(true);

    const bottomBlock = sliceBetween('const scrollToBottom = useCallback', 'const resolveFollowOutput');
    expect(bottomBlock.includes('const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;')).toBe(true);
    expect(bottomBlock.includes('if (conversationId && ownsVisibleList)')).toBe(true);

    const hideBlock = sliceBetween('const hideScrollButton = useCallback', 'return {');
    expect(hideBlock.includes('const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;')).toBe(true);
    expect(hideBlock.includes('if (conversationId && scrollerEl && ownsVisibleList)')).toBe(true);
  });

  test('resets reading position to bottom when user sends a new message', () => {
    const sendEffect = sliceBetween(
      'const sentNewUserMessage =',
      'requestAnimationFrame(() => {\n      requestAnimationFrame(() => {'
    );
    expect(sendEffect.includes('sessionScrollRegistry.save(conversationId')).toBe(true);
    expect(sendEffect.includes('userScrolled: false')).toBe(true);
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

    expect(growth.includes('if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current || isRestoringScrollRef.current) return;')).toBe(
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

  test('settles to bottom on stream completion only when user did not scroll away', () => {
    const finishEffect = sliceBetween(
      '// Handle stream lifecycle: when output finishes',
      'const hideScrollButton = useCallback'
    );

    expect(finishEffect.includes('wasProcessing && !isProcessing')).toBe(true);
    expect(finishEffect.includes('if (!userScrolledRef.current && !userIntentPausedRef.current)')).toBe(true);
    expect(finishEffect.includes("scrollToBottom('auto')")).toBe(true);
    expect(finishEffect.includes('setHasNewContentBelow(true)')).toBe(true);
    expect(finishEffect.includes('setShowScrollButton(true)')).toBe(true);
  });
});
