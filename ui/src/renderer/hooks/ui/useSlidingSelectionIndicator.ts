import { useCallback, useLayoutEffect, useState, type RefObject } from 'react';

export type SlidingSelectionIndicatorState = {
  top: number;
  left: number;
  width: number;
  height: number;
  visible: boolean;
};

export type SlidingSelectionIndicatorApi = SlidingSelectionIndicatorState & {
  /**
   * Imperatively move the indicator to a specific entry at click time
   * (urgent priority), decoupled from the route-driven `data-active` commit.
   * Lets the CSS transform transition start on the compositor thread before
   * the heavy route subtree mounts and blocks the main thread.
   */
  measureElement: (el: HTMLElement | null) => void;
};

type UseSlidingSelectionIndicatorOptions = {
  containerRef: RefObject<HTMLElement | null>;
  activeSelector: string;
  /**
   * A compact description of route/menu state. Consumers update it when an
   * active entry can change without a DOM mutation (for example collapse).
   */
  revision?: string | number;
};

const hiddenIndicator: SlidingSelectionIndicatorState = {
  top: 0,
  left: 0,
  width: 0,
  height: 0,
  visible: false,
};

const isSameIndicator = (
  previous: SlidingSelectionIndicatorState,
  next: SlidingSelectionIndicatorState
): boolean =>
  previous.top === next.top &&
  previous.left === next.left &&
  previous.width === next.width &&
  previous.height === next.height &&
  previous.visible === next.visible;

/**
 * Shared measurement core: an entry's rect relative to its local container.
 * Returns null when there is no entry to measure; returns a zero-size state
 * (visible: false) for an element that exists but is hidden.
 */
const measureRect = (
  container: HTMLElement,
  el: HTMLElement | null
): SlidingSelectionIndicatorState | null => {
  if (!el) return null;
  const containerRect = container.getBoundingClientRect();
  const entryRect = el.getBoundingClientRect();
  return {
    top: entryRect.top - containerRect.top + container.scrollTop,
    left: entryRect.left - containerRect.left + container.scrollLeft,
    width: entryRect.width,
    height: entryRect.height,
    visible: entryRect.width > 0 && entryRect.height > 0,
  };
};

/**
 * Measures an active navigation entry relative to its local container.
 *
 * The indicator intentionally never crosses container boundaries: a primary
 * sider and the settings sider each own their own instance. This prevents a
 * route change from creating a transient bar in an unrelated surface.
 */
export function useSlidingSelectionIndicator({
  containerRef,
  activeSelector,
  revision,
}: UseSlidingSelectionIndicatorOptions): SlidingSelectionIndicatorApi {
  const [indicator, setIndicator] = useState<SlidingSelectionIndicatorState>(hiddenIndicator);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const update = () => {
      const next = measureRect(container, container.querySelector<HTMLElement>(activeSelector));
      if (!next) {
        setIndicator((previous) => (isSameIndicator(previous, hiddenIndicator) ? previous : hiddenIndicator));
        return;
      }

      setIndicator((previous) => (isSameIndicator(previous, next) ? previous : next));
    };

    const resizeObserver = new ResizeObserver(update);
    resizeObserver.observe(container);

    const activeEntry = container.querySelector<HTMLElement>(activeSelector);
    if (activeEntry) resizeObserver.observe(activeEntry);

    const mutationObserver = new MutationObserver(update);
    mutationObserver.observe(container, {
      attributes: true,
      attributeFilter: ['data-active'],
      childList: true,
      subtree: true,
    });

    window.addEventListener('resize', update);
    container.addEventListener('scroll', update, true);
    update();

    return () => {
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      window.removeEventListener('resize', update);
      container.removeEventListener('scroll', update, true);
    };
  }, [activeSelector, containerRef, revision]);

  const measureElement = useCallback(
    (el: HTMLElement | null) => {
      const container = containerRef.current;
      if (!container || !el) return;
      const next = measureRect(container, el);
      if (!next || !next.visible) return;
      setIndicator((previous) => (isSameIndicator(previous, next) ? previous : next));
    },
    [containerRef]
  );

  return { ...indicator, measureElement };
}
