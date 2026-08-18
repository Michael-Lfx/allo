
/**
 * useAutoScroll - Auto-scroll hook for a plain scroll container
 *
 * Strategy:
 * - Track whether the user has intentionally scrolled away from the bottom.
 * - One owner: ResizeObserver pins scrollTop to the true content bottom
 *   (including the 64px end spacer). Virtuoso followOutput stays off.
 *   Tool chips grow the outer disclosure; followOutput would notice the
 *   taller list while the spacer keeps it off the true bottom, then jump the
 *   last two lines back into place.
 * - Sending a user message always jumps to the tail, even if follow was paused.
 * - Use DOM-native scrollIntoView for explicit message jumps.
 */
import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import type { VirtuosoHandle } from 'react-virtuoso';
import type { TMessage } from '@/common/chat/chatLib';

const PROGRAMMATIC_SCROLL_GUARD_MS = 150;
const USER_LAYOUT_CHANGE_GUARD_MS = 600;
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
  /** When set, jump-to-bottom uses Virtuoso so off-screen tail rows still mount. */
  virtuosoRef?: RefObject<VirtuosoHandle | null>;
  /** True while Virtuoso is mounted and owns streaming tail follow. */
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

export function useAutoScroll({
  messages,
  itemCount,
  virtuosoRef,
  virtuosoMode: _virtuosoMode = false,
}: UseAutoScrollOptions): UseAutoScrollReturn {
  const [scrollerEl, setScrollerEl] = useState<HTMLDivElement | null>(null);
  const [contentEl, setContentEl] = useState<HTMLDivElement | null>(null);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [hasNewContentBelow, setHasNewContentBelow] = useState(false);

  const userScrolledRef = useRef(false);
  const userIntentPausedRef = useRef(false);
  const showScrollButtonRef = useRef(false);
  const hasNewContentBelowRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const lastProgrammaticScrollTimeRef = useRef(0);
  const initialScrollDoneRef = useRef(false);
  const userInputActiveRef = useRef(false);
  const resizeAutoFollowBlockedUntilRef = useRef(0);
  const previousLastUserIdRef = useRef<string | undefined>(findLastUserMessageId(messages));
  const virtuosoRefLatest = useRef(virtuosoRef);
  virtuosoRefLatest.current = virtuosoRef;

  const markProgrammaticScroll = useCallback(() => {
    lastProgrammaticScrollTimeRef.current = Date.now();
  }, []);

  const updateBottomState = useCallback((element: HTMLDivElement) => {
    const bottomGap = getBottomGap(element);
    const withinButtonThreshold = bottomGap <= SCROLL_BUTTON_THRESHOLD_PX;
    const pinnedToBottom = bottomGap <= FOLLOW_BOTTOM_THRESHOLD_PX;
    const leftTheBottom = userScrolledRef.current || userIntentPausedRef.current;
    const nextShowButton = leftTheBottom && !withinButtonThreshold;
    const nextHasNew = userScrolledRef.current && !withinButtonThreshold;

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
    if (Date.now() < resizeAutoFollowBlockedUntilRef.current) return;

    const maxTop = getMaxScrollTop(scrollerEl);
    if (Math.abs(scrollerEl.scrollTop - maxTop) < 1) return;
    markProgrammaticScroll();
    scrollerEl.scrollTop = maxTop;
  }, [markProgrammaticScroll, scrollerEl]);

  const scrollToBottom = useCallback(
    (behavior: ScrollBehavior = 'smooth') => {
      if (itemCount <= 0) return;

      markProgrammaticScroll();
      userScrolledRef.current = false;
      userIntentPausedRef.current = false;
      userInputActiveRef.current = false;
      showScrollButtonRef.current = false;
      hasNewContentBelowRef.current = false;
      setShowScrollButton(false);
      setHasNewContentBelow(false);

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
        });
        return;
      }

      if (!scrollerEl) return;
      scrollerEl.scrollTo({
        top: getMaxScrollTop(scrollerEl),
        behavior,
      });
    },
    [itemCount, markProgrammaticScroll, scrollerEl]
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

      // Only an upward move counts as leaving the tail. Follow/content-growth
      // scrolls down; treating those as user intent is why streaming sometimes
      // stops pinning (especially after a click, which sets userInputActive).
      if (
        !pinnedToBottom &&
        delta < -2 &&
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
        if (!target.closest('[data-live-window="true"]')) {
          resizeAutoFollowBlockedUntilRef.current = Date.now() + USER_LAYOUT_CHANGE_GUARD_MS;
          pauseAutoFollow();
        }
      }
    },
    [pauseAutoFollow]
  );

  useEffect(() => {
    if (!scrollerEl || !contentEl) return;

    const observer = new ResizeObserver(() => {
      if (Date.now() < resizeAutoFollowBlockedUntilRef.current) {
        updateBottomState(scrollerEl);
        return;
      }
      followContentGrowth();
      updateBottomState(scrollerEl);
    });

    observer.observe(scrollerEl);
    observer.observe(contentEl);

    return () => observer.disconnect();
  }, [contentEl, followContentGrowth, scrollerEl, updateBottomState]);

  useEffect(() => {
    if (!scrollerEl || initialScrollDoneRef.current || itemCount === 0) return;

    initialScrollDoneRef.current = true;
    requestAnimationFrame(() => {
      scrollToBottom('auto');
      lastScrollTopRef.current = scrollerEl.scrollTop;
    });
  }, [itemCount, scrollerEl, scrollToBottom]);

  useEffect(() => {
    const lastUserId = findLastUserMessageId(messages);
    const previousLastUserId = previousLastUserIdRef.current;
    previousLastUserIdRef.current = lastUserId;

    // Jump on a new user send. Load-older prepends older rows but leaves the
    // newest user message id unchanged, so it must not yank the viewport.
    const sentNewUserMessage = lastUserId !== undefined && lastUserId !== previousLastUserId;
    if (!sentNewUserMessage) return;

    userScrolledRef.current = false;
    userIntentPausedRef.current = false;
    userInputActiveRef.current = false;

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        scrollToBottom('auto');
      });
    });
  }, [messages, scrollToBottom]);

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
