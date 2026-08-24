import { useCallback, useLayoutEffect, useRef, useSyncExternalStore, useState, type Ref, type RefCallback } from 'react';

const BLOCKER_GAP = 12;
const registry = new Set<HTMLElement>();
const listeners = new Set<() => void>();
let registryVersion = 0;

const emit = (): void => {
  registryVersion += 1;
  listeners.forEach((listener) => listener());
};

const setRef = <T,>(ref: Ref<T> | undefined, value: T | null): void => {
  if (!ref) return;
  if (typeof ref === 'function') {
    ref(value);
  } else {
    (ref as { current: T | null }).current = value;
  }
};

export const mergeRefs = <T,>(...refs: Array<Ref<T> | undefined>): RefCallback<T> =>
  (value) => refs.forEach((ref) => setRef(ref, value));

export const useNotificationBlocker = (enabled = true): RefCallback<HTMLElement> => {
  const currentRef = useRef<HTMLElement | null>(null);
  const callback = useCallback(
    (element: HTMLElement | null) => {
      if (currentRef.current && registry.delete(currentRef.current)) emit();
      currentRef.current = element;
      if (enabled && element) {
        registry.add(element);
        emit();
      }
    },
    [enabled],
  );

  useLayoutEffect(
    () => () => {
      if (currentRef.current && registry.delete(currentRef.current)) emit();
      currentRef.current = null;
    },
    [],
  );

  return callback;
};

const subscribe = (listener: () => void): (() => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

const getVersion = (): number => registryVersion;

export const calculateNotificationBottomInset = (
  viewportBottom: number,
  blockerTops: readonly number[],
  baseInset: number,
): number => {
  const blockerInset = blockerTops.reduce(
    (maxInset, top) => Math.max(maxInset, Math.max(0, viewportBottom - top) + BLOCKER_GAP),
    0,
  );
  return Math.ceil(Math.max(baseInset, blockerInset));
};

export const useNotificationBottomInset = (): number => {
  const version = useSyncExternalStore(subscribe, getVersion, getVersion);
  const [inset, setInset] = useState(24);

  useLayoutEffect(() => {
    if (typeof window === 'undefined') return undefined;

    const update = (): void => {
      const viewport = window.visualViewport;
      const viewportBottom = viewport ? viewport.offsetTop + viewport.height : window.innerHeight;
      const baseInset = window.innerWidth <= 720 ? 16 : 24;
      const blockerTops = Array.from(registry)
        .filter((element) => element.isConnected)
        .map((element) => element.getBoundingClientRect().top)
        .filter((top) => Number.isFinite(top) && top < viewportBottom);
      setInset(calculateNotificationBottomInset(viewportBottom, blockerTops, baseInset));
    };

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(update) : null;
    registry.forEach((element) => resizeObserver?.observe(element));
    window.addEventListener('resize', update);
    // Blockers can move when an ancestor scroll container (for example the
    // home page) scrolls without changing the blocker's own size.
    window.addEventListener('scroll', update, true);
    window.visualViewport?.addEventListener('resize', update);
    window.visualViewport?.addEventListener('scroll', update);
    // Transform-driven blockers (e.g. MobileActionSheet sliding in over 0.28s)
    // never trigger ResizeObserver, so recompute at animation end states...
    window.addEventListener('transitionend', update, true);
    window.addEventListener('animationend', update, true);
    update();

    // ...and keep sampling briefly after every registry change, covering
    // transitions that never fire `transitionend` (reduced-motion overrides,
    // `transition: none`) without permanent polling.
    let rafId = 0;
    if (registry.size > 0) {
      const startedAt = performance.now();
      const tick = (): void => {
        update();
        if (performance.now() - startedAt < 400) rafId = requestAnimationFrame(tick);
      };
      rafId = requestAnimationFrame(tick);
    }

    return () => {
      resizeObserver?.disconnect();
      cancelAnimationFrame(rafId);
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
      window.visualViewport?.removeEventListener('resize', update);
      window.visualViewport?.removeEventListener('scroll', update);
      window.removeEventListener('transitionend', update, true);
      window.removeEventListener('animationend', update, true);
    };
  }, [version]);

  return inset;
};
