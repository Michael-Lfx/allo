/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * useAutoScroll - Auto-scroll hook for a plain scroll container
 *
 * Strategy:
 * - Track whether the user has intentionally scrolled away from the bottom.
 * - One owner: the end spacer is the overflow-anchor, and useLayoutEffect
 *   plus ResizeObserver pin the spacer to the visible bottom before paint.
 *   Pinning scrollTop to scrollHeight max is wrong when Virtuoso's height
 *   cache still includes a just-collapsed thinking window — that leaves a
 *   hole under the reply and parks the answer in the middle of the screen.
 *   Virtuoso followOutput stays off.
 *   Tool chips grow the outer disclosure; followOutput would notice the
 *   taller list while the spacer keeps it off the true bottom, then jump the
 *   last two lines back into place.
 * - Session reading position persistence:
 *   Record per-session scroll positions in `sessionScrollRegistry`.
 *   When returning to a session where the user was previously reading history,
 *   restore the exact scroll offset instead of blindly scrolling to bottom.
 * - Sending a user message always jumps to the tail, even if follow was paused.
 * - Use DOM-native scrollIntoView for explicit message jumps.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import type { VirtuosoHandle } from 'react-virtuoso';
import type { TMessage } from '@/common/chat/chatLib';
import { sessionScrollRegistry } from './sessionScrollRegistry';

const PROGRAMMATIC_SCROLL_GUARD_MS = 150;
const USER_LAYOUT_CHANGE_GUARD_MS = 600;
// Scroll events fire per-frame (user scroll + streaming follow pins). The
// registry only needs the settled position; switch/unmount paths save
// synchronously from lastScrollTopRef, so per-event writes buy nothing.
const SCROLL_SAVE_DEBOUNCE_MS = 200;
// Must absorb sub-pixel scroll rounding on HiDPI/fractional-DPR displays, where
// scrollTop can settle ~1-3px off an integer "bottom"; too small a threshold
// (was 4) makes auto-follow intermittently think the user scrolled away and
// stop following streaming output.
export const FOLLOW_BOTTOM_THRESHOLD_PX = 12;
/** Show the scroll-to-bottom affordance as soon as auto-follow would stop. */
export const SCROLL_BUTTON_THRESHOLD_PX = 12;

interface UseAutoScrollOptions<T = unknown> {
  /** Optional conversation/session ID to track and restore per-session reading positions. */
  conversationId?: string;
  /**
   * Id of the conversation the current `messages` list was applied for.
   * Restoration waits for this to match `conversationId`: the id flips one
   * commit before the async fetch swaps the list, and restoring against the
   * previous session's DOM loses the offset to clamping (whose scroll events
   * would then save garbage into the new session's snapshot).
   */
  loadedConversationId?: string | null;
  messages: TMessage[];
  /** Processed renderable items (as rendered by Virtuoso), used to find row indices accurately. */
  displayItems?: T[];
  itemCount: number;
  /** When set, jump-to-bottom uses Virtuoso so off-screen tail rows still mount. */
  virtuosoRef?: RefObject<VirtuosoHandle | null>;
  /** True while Virtuoso is mounted and owns streaming tail follow. */
  virtuosoMode?: boolean;
  layoutPinKey?: unknown;
  /** True when conversation is actively streaming/processing output. */
  isProcessing?: boolean;
}

interface ScrollElementIntoViewOptions {
  behavior?: ScrollBehavior;
  block?: ScrollLogicalPosition;
}

type FollowOutputMode = false | 'auto';

interface UseAutoScrollReturn {
  handleScrollerRef: (ref: HTMLDivElement | null) => void;
  handleContentRef: (ref: HTMLDivElement | null) => void;
  handleScroll: (e: React.UIEvent<HTMLDivElement>) => void;
  handleWheel: (e: React.WheelEvent<HTMLDivElement>) => void;
  handlePointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
  showScrollButton: boolean;
  /** True when the user intentionally left the bottom (shows "new content" label). */
  hasNewContentBelow: boolean;
  /** Number of unread messages/turns below the viewport when viewing history. */
  unreadCount: number;
  scrollToBottom: (behavior?: ScrollBehavior) => void;
  scrollElementIntoView: (element: HTMLElement | null, options?: ScrollElementIntoViewOptions) => void;
  pauseAutoFollow: () => void;
  hideScrollButton: () => void;
  /** Virtuoso followOutput resolver — always off; DOM pin owns the tail. */
  resolveFollowOutput: (isAtBottom: boolean) => FollowOutputMode;
}

const getBottomGap = (element: HTMLElement): number => {
  return element.scrollHeight - element.clientHeight - element.scrollTop;
};

const getMaxScrollTop = (element: HTMLElement): number => {
  return Math.max(0, element.scrollHeight - element.clientHeight);
};

const findLastUserMessageId = (messages: TMessage[]): string | undefined => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.position === 'right') {
      return messages[index].id;
    }
  }
  return undefined;
};

const getUserMessagesCount = (messages: TMessage[]): number => {
  let count = 0;
  for (let index = 0; index < messages.length; index += 1) {
    if (messages[index]?.position === 'right') {
      count += 1;
    }
  }
  return count;
};

const countAssistantMessagesAfter = (messages: TMessage[], lastReadId?: string): number => {
  if (!lastReadId) return 0;
  const targetIndex = messages.findIndex((m) => m.id === lastReadId);
  if (targetIndex < 0) return 0;
  let count = 0;
  for (let i = targetIndex + 1; i < messages.length; i += 1) {
    if (messages[i]?.position !== 'right') {
      count += 1;
    }
  }
  return count;
};

export function useAutoScroll({
  conversationId,
  loadedConversationId,
  messages,
  displayItems,
  itemCount,
  virtuosoRef,
  virtuosoMode: _virtuosoMode = false,
  layoutPinKey,
  isProcessing = false,
}: UseAutoScrollOptions): UseAutoScrollReturn {
  const [scrollerEl, setScrollerEl] = useState<HTMLDivElement | null>(null);
  const [contentEl, setContentEl] = useState<HTMLDivElement | null>(null);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [hasNewContentBelow, setHasNewContentBelow] = useState(false);
  const [unreadCount, setUnreadCount] = useState(0);
  const lastReadMessageIdRef = useRef<string | undefined>(messages[messages.length - 1]?.id);

  const displayItemsRef = useRef(displayItems);
  displayItemsRef.current = displayItems;

  const userScrolledRef = useRef(false);
  const userIntentPausedRef = useRef(false);
  const showScrollButtonRef = useRef(false);
  const hasNewContentBelowRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const lastProgrammaticScrollTimeRef = useRef(0);
  const initialScrollDoneRef = useRef(false);
  const isRestoringScrollRef = useRef(false);
  const targetRestoringScrollTopRef = useRef<number | null>(null);
  const userInputActiveRef = useRef(false);
  const resizeAutoFollowBlockedUntilRef = useRef(0);
  const previousLastUserIdRef = useRef<string | undefined>(findLastUserMessageId(messages));
  const previousUserMessageCountRef = useRef<number>(getUserMessagesCount(messages));
  const previousConversationIdRef = useRef<string | undefined>(conversationId);
  const scrollSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const restoreTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Set on session switch or initial mount, consumed by the send-effect: list
  // changes before initial scroll restoration completes are initial fetch or A→B
  // swap — not a newly sent user message.
  const swapBaselinePendingRef = useRef(true);
  const virtuosoRefLatest = useRef(virtuosoRef);
  virtuosoRefLatest.current = virtuosoRef;

  const markProgrammaticScroll = useCallback(() => {
    lastProgrammaticScrollTimeRef.current = Date.now();
  }, []);

  const updateBottomState = useCallback((element: HTMLDivElement) => {
    if (isRestoringScrollRef.current || targetRestoringScrollTopRef.current !== null) {
      return false;
    }
    const bottomGap = getBottomGap(element);
    const withinButtonThreshold = bottomGap <= SCROLL_BUTTON_THRESHOLD_PX;
    const pinnedToBottom = bottomGap <= FOLLOW_BOTTOM_THRESHOLD_PX;
    const leftTheBottom = userScrolledRef.current || userIntentPausedRef.current;
    const nextShowButton = leftTheBottom && !withinButtonThreshold;
    const nextHasNew = (hasNewContentBelowRef.current || isProcessing === true) && userScrolledRef.current && !withinButtonThreshold;

    if (nextShowButton !== showScrollButtonRef.current) {
      showScrollButtonRef.current = nextShowButton;
      setShowScrollButton(nextShowButton);
    }
    if (nextHasNew !== hasNewContentBelowRef.current) {
      hasNewContentBelowRef.current = nextHasNew;
      setHasNewContentBelow(nextHasNew);
    }

    if (pinnedToBottom && Date.now() >= resizeAutoFollowBlockedUntilRef.current) {
      userScrolledRef.current = false;
      userIntentPausedRef.current = false;
      userInputActiveRef.current = false;
      lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
      setUnreadCount(0);
      if (hasNewContentBelowRef.current) {
        hasNewContentBelowRef.current = false;
        setHasNewContentBelow(false);
      }
      lastProgrammaticScrollTimeRef.current = Date.now() - (PROGRAMMATIC_SCROLL_GUARD_MS - 50);
    }

    return pinnedToBottom;
  }, [isProcessing, messages]);

  const pauseAutoFollow = useCallback(() => {
    userIntentPausedRef.current = true;
    userScrolledRef.current = true;
    if (!lastReadMessageIdRef.current) {
      lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
    }
    const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;
    if (conversationId && scrollerEl && ownsVisibleList) {
      const existing = sessionScrollRegistry.get(conversationId);
      sessionScrollRegistry.save(conversationId, {
        scrollTop: scrollerEl.scrollTop,
        userScrolled: true,
        lastReadMessageId: lastReadMessageIdRef.current,
        unreadCount: unreadCount,
        targetMessageId: existing?.targetMessageId,
        turnIndex: existing?.turnIndex,
      });
    }
    if (!scrollerEl || getBottomGap(scrollerEl) <= SCROLL_BUTTON_THRESHOLD_PX) {
      return;
    }
    const nextHasNew = hasNewContentBelowRef.current || isProcessing === true;
    showScrollButtonRef.current = true;
    hasNewContentBelowRef.current = nextHasNew;
    setShowScrollButton(true);
    setHasNewContentBelow(nextHasNew);
    const newCount = countAssistantMessagesAfter(messages, lastReadMessageIdRef.current);
    setUnreadCount(nextHasNew ? Math.max(1, newCount) : newCount);
  }, [conversationId, isProcessing, loadedConversationId, messages, scrollerEl, unreadCount]);

  const followContentGrowth = useCallback(() => {
    if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current || isRestoringScrollRef.current) return;
    if (Date.now() < resizeAutoFollowBlockedUntilRef.current) return;

    const spacer = scrollerEl.querySelector('.message-list-end-spacer');
    if (spacer instanceof HTMLElement) {
      const paddingBottom = Number.parseFloat(getComputedStyle(scrollerEl).paddingBottom) || 0;
      const pinLine = scrollerEl.getBoundingClientRect().bottom - paddingBottom;
      // The top of the spacer is the bottom edge of actual message content.
      // Only scroll when message content grows beyond the visible viewport bottom.
      const delta = spacer.getBoundingClientRect().top - pinLine;
      if (delta <= 1) return;
      markProgrammaticScroll();
      scrollerEl.scrollTop += delta;
      return;
    }

    if (!isProcessing) {
      const maxTop = getMaxScrollTop(scrollerEl);
      if (Math.abs(scrollerEl.scrollTop - maxTop) < 1) return;
      markProgrammaticScroll();
      scrollerEl.scrollTop = maxTop;
    }
  }, [isProcessing, markProgrammaticScroll, scrollerEl]);

  const scrollToBottom = useCallback(
    (behavior: ScrollBehavior = 'smooth') => {
      if (itemCount <= 0) return;

      targetRestoringScrollTopRef.current = null;
      isRestoringScrollRef.current = false;
      markProgrammaticScroll();
      userScrolledRef.current = false;
      userIntentPausedRef.current = false;
      userInputActiveRef.current = false;
      showScrollButtonRef.current = false;
      hasNewContentBelowRef.current = false;
      setShowScrollButton(false);
      setHasNewContentBelow(false);
      lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
      setUnreadCount(0);

      const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;
      if (conversationId && ownsVisibleList) {
        sessionScrollRegistry.save(conversationId, {
          scrollTop: scrollerEl ? getMaxScrollTop(scrollerEl) : 0,
          userScrolled: false,
          lastReadMessageId: messages[messages.length - 1]?.id,
          unreadCount: 0,
        });
      }

      const lastIndex = itemCount - 1;
      const virtuoso = virtuosoRefLatest.current?.current;
      if (virtuoso && lastIndex >= 0) {
        virtuoso.scrollToIndex({
          index: lastIndex,
          align: 'end',
          behavior: 'auto',
        });
        if (!scrollerEl) return;
        requestAnimationFrame(() => {
          scrollerEl.scrollTo({
            top: getMaxScrollTop(scrollerEl),
            behavior,
          });
          requestAnimationFrame(() => {
            followContentGrowth();
          });
        });
        return;
      }

      if (!scrollerEl) return;
      scrollerEl.scrollTo({
        top: getMaxScrollTop(scrollerEl),
        behavior,
      });
    },
    [conversationId, followContentGrowth, itemCount, loadedConversationId, markProgrammaticScroll, scrollerEl]
  );

  const resolveFollowOutput = useCallback((_isAtBottom: boolean): FollowOutputMode => {
    return false;
  }, []);

  const handleScrollerRef = useCallback((ref: HTMLDivElement | null) => {
    setScrollerEl(ref);
  }, []);

  const handleContentRef = useCallback((ref: HTMLDivElement | null) => {
    setContentEl(ref);
  }, []);

  const scrollElementIntoView = useCallback(
    (element: HTMLElement | null, options?: ScrollElementIntoViewOptions) => {
      if (!element) return;

      targetRestoringScrollTopRef.current = null;
      isRestoringScrollRef.current = false;
      pauseAutoFollow();
      markProgrammaticScroll();
      element.scrollIntoView({
        behavior: options?.behavior ?? 'smooth',
        block: options?.block ?? 'start',
        inline: 'nearest',
      });
    },
    [markProgrammaticScroll, pauseAutoFollow]
  );

  const handleScroll = useCallback(
    (e: React.UIEvent<HTMLDivElement>) => {
      const target = e.currentTarget;
      const currentScrollTop = target.scrollTop;

      if (isRestoringScrollRef.current || targetRestoringScrollTopRef.current !== null) {
        lastScrollTopRef.current = currentScrollTop;
        return;
      }

      if (userInputActiveRef.current) {
        targetRestoringScrollTopRef.current = null;
      }

      const timeSinceGuard = Date.now() - lastProgrammaticScrollTimeRef.current;
      const delta = currentScrollTop - lastScrollTopRef.current;
      const bottomGap = getBottomGap(target);
      const pinnedToBottom = bottomGap <= FOLLOW_BOTTOM_THRESHOLD_PX;

      // Only an upward move with active user input counts as leaving the tail.
      // Programmatic scroll and internal Virtuoso measurement reflows must never
      // be mistaken for user intent.
      if (
        !pinnedToBottom &&
        delta < -2 &&
        userInputActiveRef.current &&
        timeSinceGuard >= PROGRAMMATIC_SCROLL_GUARD_MS
      ) {
        userScrolledRef.current = true;
        userIntentPausedRef.current = true;
      }

      if (pinnedToBottom) {
        userInputActiveRef.current = false;
      } else if (Math.abs(delta) > 2) {
        userInputActiveRef.current = false;
      }

      lastScrollTopRef.current = currentScrollTop;
      updateBottomState(target);

      const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;
      if (conversationId && ownsVisibleList) {
        // Debounced: per-frame events (incl. streaming follow pins) would
        // otherwise churn the registry. The timer is cleared on switch and
        // unmount, where lastScrollTopRef is saved synchronously instead.
        if (scrollSaveTimerRef.current) clearTimeout(scrollSaveTimerRef.current);
        scrollSaveTimerRef.current = setTimeout(() => {
          scrollSaveTimerRef.current = null;
          const existing = sessionScrollRegistry.get(conversationId);
          sessionScrollRegistry.save(conversationId, {
            scrollTop: lastScrollTopRef.current,
            userScrolled: userScrolledRef.current,
            lastReadMessageId: lastReadMessageIdRef.current,
            unreadCount: unreadCount,
            targetMessageId: existing?.targetMessageId,
            turnIndex: existing?.turnIndex,
          });
        }, SCROLL_SAVE_DEBOUNCE_MS);
      }
    },
    [conversationId, loadedConversationId, updateBottomState]
  );

  const handleWheel = useCallback((e: React.WheelEvent<HTMLDivElement>) => {
    if (Math.abs(e.deltaY) > 0 || Math.abs(e.deltaX) > 0) {
      userInputActiveRef.current = true;
      targetRestoringScrollTopRef.current = null;
      isRestoringScrollRef.current = false;
      if (e.deltaY < 0) {
        // User wheeled up to view history
        userScrolledRef.current = true;
        userIntentPausedRef.current = true;
      }
    }
  }, []);

  const handlePointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      userInputActiveRef.current = true;
      targetRestoringScrollTopRef.current = null;
      isRestoringScrollRef.current = false;
      const target = event.target;
      if (target instanceof Element && target.closest('[aria-expanded]')) {
        if (!target.closest('[data-live-window="true"]')) {
          resizeAutoFollowBlockedUntilRef.current = Date.now() + USER_LAYOUT_CHANGE_GUARD_MS;
          pauseAutoFollow();
        }
      }
    },
    [pauseAutoFollow]
  );

  useLayoutEffect(() => {
    followContentGrowth();
  }, [followContentGrowth, layoutPinKey]);

  useEffect(() => {
    if (!scrollerEl || !contentEl) return;

    let frameId: number | null = null;
    let disposed = false;

    const flushResizeWork = () => {
      frameId = null;
      if (disposed) return;

      if (targetRestoringScrollTopRef.current !== null && scrollerEl) {
        const target = targetRestoringScrollTopRef.current;
        scrollerEl.scrollTop = target;
        lastScrollTopRef.current = target;
        const virtuoso = virtuosoRefLatest.current?.current;
        if (virtuoso) {
          virtuoso.scrollTo({ top: target, behavior: 'auto' });
        }
        if (getMaxScrollTop(scrollerEl) >= target) {
          targetRestoringScrollTopRef.current = null;
          isRestoringScrollRef.current = false;
        }
      }

      if (Date.now() < resizeAutoFollowBlockedUntilRef.current) {
        updateBottomState(scrollerEl);
        return;
      }
      followContentGrowth();
      updateBottomState(scrollerEl);
    };

    const scheduleResizeWork = () => {
      if (disposed) return;
      if (frameId !== null) window.cancelAnimationFrame(frameId);
      frameId = window.requestAnimationFrame(flushResizeWork);
    };

    const observer = new ResizeObserver(scheduleResizeWork);

    observer.observe(scrollerEl);
    observer.observe(contentEl);

    return () => {
      disposed = true;
      if (frameId !== null) window.cancelAnimationFrame(frameId);
      observer.disconnect();
    };
  }, [contentEl, followContentGrowth, scrollerEl, updateBottomState]);

  // Handle session switch, initial scroll, and reading position restoration before paint
  useLayoutEffect(() => {
    // Switch bookkeeping touches no DOM: the previous session's offset comes
    // from lastScrollTopRef (tracked live in handleScroll). Reading scrollerEl
    // here would race the switch — the element may already be detached (the
    // skeleton/empty early-return unmounts it) or belong to the next session.
    if (conversationId !== previousConversationIdRef.current) {
      const prevId = previousConversationIdRef.current;
      // Skip sessions that were never displayed (a rapid A→B→C hop): their
      if (prevId && initialScrollDoneRef.current) {
        const existing = sessionScrollRegistry.get(prevId);
        sessionScrollRegistry.save(prevId, {
          scrollTop: lastScrollTopRef.current,
          userScrolled: userScrolledRef.current,
          lastReadMessageId: lastReadMessageIdRef.current,
          unreadCount: unreadCount,
          targetMessageId: existing?.targetMessageId,
          turnIndex: existing?.turnIndex,
        });
      }
      if (scrollSaveTimerRef.current) {
        clearTimeout(scrollSaveTimerRef.current);
        scrollSaveTimerRef.current = null;
      }
      if (restoreTimerRef.current) {
        clearTimeout(restoreTimerRef.current);
        restoreTimerRef.current = null;
      }
      previousConversationIdRef.current = conversationId;
      initialScrollDoneRef.current = false;
      swapBaselinePendingRef.current = true;
      targetRestoringScrollTopRef.current = null;
    }

    if (!scrollerEl || initialScrollDoneRef.current || itemCount === 0) return;
    // The provider's list can still belong to the previous session in this
    // commit (the fetch resolves async). Restore only once the store confirms
    // the list was applied for THIS conversation. Consumers without a
    // conversationId keep the legacy ungated behavior.
    if (conversationId && loadedConversationId !== conversationId) return;

    initialScrollDoneRef.current = true;
    // The list belongs to this conversation now: seed the send-effect baseline
    // so the A→B list swap is not misread as a newly sent user message (which
    // would yank a restored view back to the bottom).
    previousLastUserIdRef.current = findLastUserMessageId(messages);
    previousUserMessageCountRef.current = getUserMessagesCount(messages);
    swapBaselinePendingRef.current = false;
    const saved = conversationId ? sessionScrollRegistry.get(conversationId) : undefined;

    if (saved && saved.userScrolled) {
      userScrolledRef.current = true;
      userIntentPausedRef.current = true;
      isRestoringScrollRef.current = true;
      markProgrammaticScroll();

      lastReadMessageIdRef.current = saved.lastReadMessageId ?? messages[messages.length - 1]?.id;
      const targetScrollTop = saved.scrollTop;
      targetRestoringScrollTopRef.current = targetScrollTop;
      scrollerEl.scrollTop = targetScrollTop;
      lastScrollTopRef.current = targetScrollTop;

      const virtuoso = virtuosoRefLatest.current?.current;
      if (virtuoso) {
        const items = displayItemsRef.current as Array<{ id?: string; message?: { id?: string }; sourceMessageIds?: string[] } | undefined> | undefined;
        let targetRowIndex = -1;
        const targetMessageId = saved.targetMessageId;
        if (items && targetMessageId) {
          targetRowIndex = items.findIndex((item) => {
            if (!item) return false;
            if (item.id === targetMessageId) return true;
            if (item.message?.id === targetMessageId) return true;
            if (Array.isArray(item.sourceMessageIds) && item.sourceMessageIds.includes(targetMessageId)) return true;
            return false;
          });
        }
        if (targetRowIndex >= 0) {
          virtuoso.scrollToIndex({
            index: targetRowIndex,
            align: 'start',
            behavior: 'auto',
          });
        } else {
          virtuoso.scrollTo({ top: targetScrollTop, behavior: 'auto' });
        }
      }

      showScrollButtonRef.current = true;
      setShowScrollButton(true);
      const nextHasNew = isProcessing === true || (saved.unreadCount ?? 0) > 0;
      hasNewContentBelowRef.current = nextHasNew;
      setHasNewContentBelow(nextHasNew);
      const restoredUnread = countAssistantMessagesAfter(messages, saved.lastReadMessageId);
      setUnreadCount(nextHasNew ? Math.max(1, saved.unreadCount ?? restoredUnread) : restoredUnread);

      // Re-apply and verify post-layout to ensure Virtuoso virtualization measurements
      // have settled and cannot clamp the restored position or falsely trip auto-follow.
      const applyRestoration = () => {
        if (!scrollerEl) return;
        const currentTarget = targetRestoringScrollTopRef.current ?? targetScrollTop;
        scrollerEl.scrollTop = currentTarget;
        lastScrollTopRef.current = currentTarget;
        if (virtuoso) {
          virtuoso.scrollTo({ top: currentTarget, behavior: 'auto' });
        }
      };

      requestAnimationFrame(() => {
        applyRestoration();
        requestAnimationFrame(() => {
          applyRestoration();
          if (restoreTimerRef.current) {
            clearTimeout(restoreTimerRef.current);
          }
          restoreTimerRef.current = setTimeout(() => {
            restoreTimerRef.current = null;
            if (targetRestoringScrollTopRef.current !== null) {
              applyRestoration();
              if (scrollerEl && getMaxScrollTop(scrollerEl) >= (targetRestoringScrollTopRef.current ?? 0)) {
                targetRestoringScrollTopRef.current = null;
                isRestoringScrollRef.current = false;
              }
              if (scrollerEl) {
                updateBottomState(scrollerEl);
              }
            }
          }, 150);
        });
      });
      return;
    }

    // Reset button and state flags when session is at bottom
    userScrolledRef.current = false;
    userIntentPausedRef.current = false;
    userInputActiveRef.current = false;
    showScrollButtonRef.current = false;
    hasNewContentBelowRef.current = false;
    setShowScrollButton(false);
    setHasNewContentBelow(false);
    lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
    setUnreadCount(0);

    requestAnimationFrame(() => {
      scrollToBottom('auto');
      lastScrollTopRef.current = scrollerEl.scrollTop;
    });
  }, [conversationId, isProcessing, itemCount, loadedConversationId, markProgrammaticScroll, messages, scrollerEl, scrollToBottom, updateBottomState]);

  // Save on unmount
  useEffect(() => {
    return () => {
      if (scrollSaveTimerRef.current) {
        clearTimeout(scrollSaveTimerRef.current);
        scrollSaveTimerRef.current = null;
      }
      if (restoreTimerRef.current) {
        clearTimeout(restoreTimerRef.current);
        restoreTimerRef.current = null;
      }
      const currentId = previousConversationIdRef.current;
      if (currentId && initialScrollDoneRef.current) {
        // lastScrollTopRef, not scrollerEl.scrollTop: passive cleanups run
        // after the element detaches, where scrollTop reads back as 0. The
        // initialScrollDone guard skips sessions that were never displayed.
        const existing = sessionScrollRegistry.get(currentId);
        sessionScrollRegistry.save(currentId, {
          scrollTop: lastScrollTopRef.current,
          userScrolled: userScrolledRef.current,
          lastReadMessageId: lastReadMessageIdRef.current,
          unreadCount: unreadCount,
          targetMessageId: existing?.targetMessageId,
          turnIndex: existing?.turnIndex,
        });
      }
    };
  }, []);

  const dockToLatestUserMessage = useCallback(() => {
    const lastUserId = findLastUserMessageId(messages);
    if (!lastUserId) return;

    const items = (displayItemsRef.current as Array<{ position?: string; id?: string; sourceMessageIds?: string[] } | undefined>) ?? messages;
    const targetIndex = items.findLastIndex((item) => {
      if (item && typeof item === 'object') {
        if ('position' in item && item.position === 'right') return true;
        if ('id' in item && item.id === lastUserId) return true;
        if ('sourceMessageIds' in item && Array.isArray(item.sourceMessageIds) && item.sourceMessageIds.includes(lastUserId)) return true;
      }
      return false;
    });

    const virtuoso = virtuosoRefLatest.current?.current;
    if (virtuoso && targetIndex >= 0) {
      markProgrammaticScroll();
      resizeAutoFollowBlockedUntilRef.current = Date.now() + 400;
      virtuoso.scrollToIndex({
        index: targetIndex,
        align: 'start',
        behavior: 'auto',
      });
    } else {
      scrollToBottom('auto');
    }
  }, [markProgrammaticScroll, messages, scrollToBottom]);

  useEffect(() => {
    const lastUserId = findLastUserMessageId(messages);
    const previousLastUserId = previousLastUserIdRef.current;
    previousLastUserIdRef.current = lastUserId;

    const currentUserMessageCount = getUserMessagesCount(messages);
    const previousUserMessageCount = previousUserMessageCountRef.current;
    previousUserMessageCountRef.current = currentUserMessageCount;

    // Jump on a new user send. Load-older prepends older rows but leaves the
    // newest user message id unchanged, so it must not yank the viewport.
    // While a session swap is pending or initial scroll restoration hasn't
    // completed, list changes are the initial load, A→B swap, or
    // stale-session streaming — never a send. The restore branch consumes the
    // flag when it seeds the baseline above.
    if (!initialScrollDoneRef.current || swapBaselinePendingRef.current) return;
    const sentNewUserMessage =
      currentUserMessageCount > previousUserMessageCount &&
      lastUserId !== undefined &&
      lastUserId !== previousLastUserId;
    if (!sentNewUserMessage) return;

    userScrolledRef.current = false;
    userIntentPausedRef.current = false;
    userInputActiveRef.current = false;
    showScrollButtonRef.current = false;
    hasNewContentBelowRef.current = false;
    setShowScrollButton(false);
    setHasNewContentBelow(false);
    lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
    setUnreadCount(0);

    if (conversationId) {
      sessionScrollRegistry.save(conversationId, {
        scrollTop: scrollerEl ? getMaxScrollTop(scrollerEl) : 0,
        userScrolled: false,
        lastReadMessageId: messages[messages.length - 1]?.id,
        unreadCount: 0,
      });
    }

    resizeAutoFollowBlockedUntilRef.current = Date.now() + 400;

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        dockToLatestUserMessage();
      });
    });
  }, [conversationId, dockToLatestUserMessage, messages, scrollerEl]);

  // Handle stream lifecycle: when output finishes, cleanly settle to the bottom ONLY
  // if the user did not scroll away; if the user is viewing history, strictly preserve
  // their reading position and highlight unread content below.
  const previousIsProcessingRef = useRef(isProcessing);
  useEffect(() => {
    const wasProcessing = previousIsProcessingRef.current;
    previousIsProcessingRef.current = isProcessing;

    if (!initialScrollDoneRef.current || swapBaselinePendingRef.current) return;

    if (userScrolledRef.current || userIntentPausedRef.current) {
      const newCount = countAssistantMessagesAfter(messages, lastReadMessageIdRef.current);
      const effectiveCount = (hasNewContentBelowRef.current || isProcessing) ? Math.max(1, newCount) : newCount;
      setUnreadCount(effectiveCount);
    }

    if (!wasProcessing && isProcessing) {
      if (userScrolledRef.current || userIntentPausedRef.current) {
        showScrollButtonRef.current = true;
        hasNewContentBelowRef.current = true;
        setShowScrollButton(true);
        setHasNewContentBelow(true);
      } else {
        resizeAutoFollowBlockedUntilRef.current = Date.now() + 400;
        requestAnimationFrame(() => {
          dockToLatestUserMessage();
        });
      }
    } else if (wasProcessing && !isProcessing) {
      if (!userScrolledRef.current && !userIntentPausedRef.current) {
        // User stayed at bottom: settle to show full response and actions
        userScrolledRef.current = false;
        userIntentPausedRef.current = false;
        showScrollButtonRef.current = false;
        hasNewContentBelowRef.current = false;
        setShowScrollButton(false);
        setHasNewContentBelow(false);
        lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
        setUnreadCount(0);
        requestAnimationFrame(() => {
          scrollToBottom('auto');
          requestAnimationFrame(() => {
            followContentGrowth();
          });
        });
      } else {
        // User is viewing history: strictly protect position and show unread badge
        showScrollButtonRef.current = true;
        hasNewContentBelowRef.current = true;
        setShowScrollButton(true);
        setHasNewContentBelow(true);
        const newCount = countAssistantMessagesAfter(messages, lastReadMessageIdRef.current);
        setUnreadCount(Math.max(1, newCount));
      }
    }
  }, [dockToLatestUserMessage, followContentGrowth, isProcessing, messages, scrollToBottom]);

  const hideScrollButton = useCallback(() => {
    userScrolledRef.current = false;
    userIntentPausedRef.current = false;
    showScrollButtonRef.current = false;
    hasNewContentBelowRef.current = false;
    setShowScrollButton(false);
    setHasNewContentBelow(false);
    lastReadMessageIdRef.current = messages[messages.length - 1]?.id;
    setUnreadCount(0);

    const ownsVisibleList = !conversationId || !loadedConversationId || loadedConversationId === conversationId;
    if (conversationId && scrollerEl && ownsVisibleList) {
      sessionScrollRegistry.save(conversationId, {
        scrollTop: scrollerEl.scrollTop,
        userScrolled: false,
        lastReadMessageId: messages[messages.length - 1]?.id,
        unreadCount: 0,
      });
    }
  }, [conversationId, loadedConversationId, messages, scrollerEl]);

  return {
    handleScrollerRef,
    handleContentRef,
    handleScroll,
    handleWheel,
    handlePointerDown,
    showScrollButton,
    hasNewContentBelow,
    unreadCount,
    scrollToBottom,
    scrollElementIntoView,
    pauseAutoFollow,
    hideScrollButton,
    resolveFollowOutput,
  };
}
