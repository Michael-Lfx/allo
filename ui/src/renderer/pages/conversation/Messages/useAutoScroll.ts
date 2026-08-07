
/**
 * useAutoScroll - Auto-scroll hook for a plain scroll container
 *
 * Strategy:
 * - Track whether the user has intentionally scrolled away from the bottom.
 * - Observe content/scroller size changes and keep the list pinned to bottom
 *   only while auto-follow mode is active.
 * - Use DOM-native scrollIntoView for explicit message jumps.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { TMessage } from '@/common/chat/chatLib';

const PROGRAMMATIC_SCROLL_GUARD_MS = 150;
// Must absorb sub-pixel scroll rounding on HiDPI/fractional-DPR displays, where
// scrollTop can settle ~1-3px off an integer "bottom"; too small a threshold
// (was 4) makes auto-follow intermittently think the user scrolled away and
// stop following streaming output.
export const FOLLOW_BOTTOM_THRESHOLD_PX = 12;
/** Show the scroll-to-bottom affordance as soon as auto-follow would stop. */
export const SCROLL_BUTTON_THRESHOLD_PX = 12;

interface UseAutoScrollOptions {
  messages: TMessage[];
  itemCount: number;
  /** When true, Virtuoso `followOutput` owns tail pinning; skip ResizeObserver scroll nudges. */
  virtuosoMode?: boolean;
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
  scrollToBottom: (behavior?: ScrollBehavior) => void;
  scrollElementIntoView: (element: HTMLElement | null, options?: ScrollElementIntoViewOptions) => void;
  pauseAutoFollow: () => void;
  hideScrollButton: () => void;
  /** Virtuoso followOutput resolver — keeps tail pinned without fighting manual scroll. */
  resolveFollowOutput: (isAtBottom: boolean) => FollowOutputMode;
}

const getBottomGap = (element: HTMLElement): number => {
  return element.scrollHeight - element.clientHeight - element.scrollTop;
};

export function useAutoScroll({ messages, itemCount, virtuosoMode = false }: UseAutoScrollOptions): UseAutoScrollReturn {
  const [scrollerEl, setScrollerEl] = useState<HTMLDivElement | null>(null);
  const [contentEl, setContentEl] = useState<HTMLDivElement | null>(null);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [hasNewContentBelow, setHasNewContentBelow] = useState(false);

  const userScrolledRef = useRef(false);
  const userIntentPausedRef = useRef(false);
  const showScrollButtonRef = useRef(false);
  const hasNewContentBelowRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const lastContentScrollHeightRef = useRef(0);
  const previousListLengthRef = useRef(messages.length);
  const previousLastIdRef = useRef<string | undefined>(messages[messages.length - 1]?.id);
  const lastProgrammaticScrollTimeRef = useRef(0);
  const initialScrollDoneRef = useRef(false);
  const pendingAutoFollowFrameRef = useRef<number | null>(null);
  const userInputActiveRef = useRef(false);
  const virtuosoModeRef = useRef(virtuosoMode);
  virtuosoModeRef.current = virtuosoMode;

  const markProgrammaticScroll = useCallback(() => {
    lastProgrammaticScrollTimeRef.current = Date.now();
  }, []);

  const updateBottomState = useCallback((element: HTMLDivElement) => {
    const bottomGap = getBottomGap(element);
    const withinButtonThreshold = bottomGap <= SCROLL_BUTTON_THRESHOLD_PX;
    const pinnedToBottom = bottomGap <= FOLLOW_BOTTOM_THRESHOLD_PX;
    const nextShowButton = !withinButtonThreshold;
    const nextHasNew = userScrolledRef.current && !withinButtonThreshold;

    if (nextShowButton !== showScrollButtonRef.current) {
      showScrollButtonRef.current = nextShowButton;
      setShowScrollButton(nextShowButton);
    }
    if (nextHasNew !== hasNewContentBelowRef.current) {
      hasNewContentBelowRef.current = nextHasNew;
      setHasNewContentBelow(nextHasNew);
    }

    if (pinnedToBottom) {
      userScrolledRef.current = false;
      userIntentPausedRef.current = false;
      userInputActiveRef.current = false;
      if (hasNewContentBelowRef.current) {
        hasNewContentBelowRef.current = false;
        setHasNewContentBelow(false);
      }
      lastProgrammaticScrollTimeRef.current = Date.now() - (PROGRAMMATIC_SCROLL_GUARD_MS - 50);
    }

    return pinnedToBottom;
  }, []);

  const pauseAutoFollow = useCallback(() => {
    userIntentPausedRef.current = true;
    userScrolledRef.current = true;
    if (!scrollerEl || getBottomGap(scrollerEl) <= SCROLL_BUTTON_THRESHOLD_PX) {
      return;
    }
    showScrollButtonRef.current = true;
    hasNewContentBelowRef.current = true;
    setShowScrollButton(true);
    setHasNewContentBelow(true);
  }, [scrollerEl]);

  const followContentGrowth = useCallback(() => {
    if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current) return;

    const prevHeight = lastContentScrollHeightRef.current;
    const newHeight = scrollerEl.scrollHeight;
    if (prevHeight === 0) {
      lastContentScrollHeightRef.current = newHeight;
      return;
    }

    const delta = newHeight - prevHeight;
    lastContentScrollHeightRef.current = newHeight;
    if (delta <= 0) return;

    const gap = getBottomGap(scrollerEl);
    if (gap > FOLLOW_BOTTOM_THRESHOLD_PX + delta) return;

    markProgrammaticScroll();
    scrollerEl.scrollTop += delta;
  }, [markProgrammaticScroll, scrollerEl]);

  const scrollToBottom = useCallback(
    (behavior: ScrollBehavior = 'smooth') => {
      if (itemCount <= 0 || !scrollerEl) return;

      markProgrammaticScroll();
      scrollerEl.scrollTo({
        top: scrollerEl.scrollHeight - scrollerEl.clientHeight,
        behavior,
      });
      lastContentScrollHeightRef.current = scrollerEl.scrollHeight;
      userScrolledRef.current = false;
      userIntentPausedRef.current = false;
      showScrollButtonRef.current = false;
      hasNewContentBelowRef.current = false;
      setShowScrollButton(false);
      setHasNewContentBelow(false);
    },
    [itemCount, markProgrammaticScroll, scrollerEl]
  );

  const scheduleAutoFollow = useCallback(() => {
    if (virtuosoModeRef.current) return;
    if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current) return;

    if (pendingAutoFollowFrameRef.current !== null) {
      cancelAnimationFrame(pendingAutoFollowFrameRef.current);
    }

    pendingAutoFollowFrameRef.current = requestAnimationFrame(() => {
      pendingAutoFollowFrameRef.current = null;
      if (!scrollerEl || userScrolledRef.current || userIntentPausedRef.current) return;

      followContentGrowth();
    });
  }, [followContentGrowth, scrollerEl]);

  const resolveFollowOutput = useCallback((isAtBottom: boolean): FollowOutputMode => {
    if (!isAtBottom || userIntentPausedRef.current || userScrolledRef.current) {
      return false;
    }
    return 'auto';
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
      const timeSinceGuard = Date.now() - lastProgrammaticScrollTimeRef.current;
      const delta = currentScrollTop - lastScrollTopRef.current;
      const bottomGap = getBottomGap(target);
      const pinnedToBottom = bottomGap <= FOLLOW_BOTTOM_THRESHOLD_PX;

      if (
        !pinnedToBottom &&
        Math.abs(delta) > 2 &&
        (userInputActiveRef.current || timeSinceGuard >= PROGRAMMATIC_SCROLL_GUARD_MS)
      ) {
        userScrolledRef.current = true;
      }

      if (pinnedToBottom) {
        userInputActiveRef.current = false;
      } else if (Math.abs(delta) > 2) {
        userInputActiveRef.current = false;
      }

      lastScrollTopRef.current = currentScrollTop;
      updateBottomState(target);
    },
    [updateBottomState]
  );

  const handleWheel = useCallback((e: React.WheelEvent<HTMLDivElement>) => {
    if (Math.abs(e.deltaY) > 0 || Math.abs(e.deltaX) > 0) {
      userInputActiveRef.current = true;
    }
  }, []);

  const handlePointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      userInputActiveRef.current = true;
      const target = event.target;
      if (target instanceof Element && target.closest('[aria-expanded]')) {
        pauseAutoFollow();
      }
    },
    [pauseAutoFollow]
  );

  useEffect(() => {
    if (!scrollerEl || !contentEl) return;

    const observer = new ResizeObserver(() => {
      scheduleAutoFollow();
      updateBottomState(scrollerEl);
    });

    observer.observe(scrollerEl);
    observer.observe(contentEl);

    return () => observer.disconnect();
  }, [contentEl, scheduleAutoFollow, scrollerEl, updateBottomState]);

  useEffect(() => {
    if (!scrollerEl || initialScrollDoneRef.current || itemCount === 0) return;

    initialScrollDoneRef.current = true;
    requestAnimationFrame(() => {
      scrollToBottom('auto');
      lastScrollTopRef.current = scrollerEl.scrollTop;
    });
  }, [itemCount, scrollerEl, scrollToBottom]);

  useEffect(() => {
    const currentListLength = messages.length;
    const previousLength = previousListLengthRef.current;
    const lastMessage = messages[messages.length - 1];
    const previousLastId = previousLastIdRef.current;
    previousListLengthRef.current = currentListLength;
    previousLastIdRef.current = lastMessage?.id;

    // Only auto-jump on a genuine NEW BOTTOM message: the list grew AND the last
    // (newest) message changed. A scroll-up "load older" prepend also grows the
    // length but leaves the last message unchanged — it must NOT yank the
    // viewport to the bottom (that would fight the prepend scroll-anchor).
    const grewAtBottom = currentListLength > previousLength && lastMessage?.id !== previousLastId;
    if (!grewAtBottom) return;

    if (lastMessage?.position !== 'right') return;
    if (userIntentPausedRef.current || userScrolledRef.current) return;

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        scrollToBottom('auto');
      });
    });
  }, [messages, scrollToBottom]);

  useEffect(() => {
    return () => {
      if (pendingAutoFollowFrameRef.current !== null) {
        cancelAnimationFrame(pendingAutoFollowFrameRef.current);
      }
    };
  }, []);

  const hideScrollButton = useCallback(() => {
    userScrolledRef.current = false;
    userIntentPausedRef.current = false;
    showScrollButtonRef.current = false;
    hasNewContentBelowRef.current = false;
    setShowScrollButton(false);
    setHasNewContentBelow(false);
  }, []);

  return {
    handleScrollerRef,
    handleContentRef,
    handleScroll,
    handleWheel,
    handlePointerDown,
    showScrollButton,
    hasNewContentBelow,
    scrollToBottom,
    scrollElementIntoView,
    pauseAutoFollow,
    hideScrollButton,
    resolveFollowOutput,
  };
}
