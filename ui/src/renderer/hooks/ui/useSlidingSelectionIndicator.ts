import { useLayoutEffect, useState, type RefObject } from 'react';

export type SlidingSelectionIndicatorState = {
  top: number;
  left: number;
  width: number;
  height: number;
  visible: boolean;
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
}: UseSlidingSelectionIndicatorOptions): SlidingSelectionIndicatorState {
  const [indicator, setIndicator] = useState<SlidingSelectionIndicatorState>(hiddenIndicator);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const update = () => {
      const activeEntry = container.querySelector<HTMLElement>(activeSelector);
      if (!activeEntry) {
        setIndicator((previous) => (isSameIndicator(previous, hiddenIndicator) ? previous : hiddenIndicator));
        return;
      }

      const containerRect = container.getBoundingClientRect();
      const entryRect = activeEntry.getBoundingClientRect();
      const next: SlidingSelectionIndicatorState = {
        top: entryRect.top - containerRect.top + container.scrollTop,
        left: entryRect.left - containerRect.left + container.scrollLeft,
        width: entryRect.width,
        height: entryRect.height,
        visible: entryRect.width > 0 && entryRect.height > 0,
      };

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

  return indicator;
}
