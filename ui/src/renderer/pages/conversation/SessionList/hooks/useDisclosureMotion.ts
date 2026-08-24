import { useEffect, useRef, useState } from 'react';

export type DisclosureMotionPhase = 'closed' | 'open' | 'entering' | 'exiting';

export const DISCLOSURE_MOTION_DURATION_MS = 180;

type DisclosureMotion = {
  shouldRender: boolean;
  phase: DisclosureMotionPhase;
};

/**
 * Keeps disclosure content mounted for a short exit transition while only
 * animating opacity and transform. `userToggleKey` lets route-driven or
 * persisted state changes remain instant, while explicit user toggles animate.
 */
export const useDisclosureMotion = (open: boolean, userToggleKey: number): DisclosureMotion => {
  const [reducedMotion, setReducedMotion] = useState(false);
  const [shouldRender, setShouldRender] = useState(open);
  const [phase, setPhase] = useState<DisclosureMotionPhase>(open ? 'open' : 'closed');
  const previousOpenRef = useRef(open);
  const previousUserToggleKeyRef = useRef(userToggleKey);
  const firstEffectRef = useRef(true);
  const exitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const enterFrameRef = useRef<number | null>(null);

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    const syncReducedMotion = () => setReducedMotion(mediaQuery.matches);
    syncReducedMotion();
    mediaQuery.addEventListener('change', syncReducedMotion);

    return () => mediaQuery.removeEventListener('change', syncReducedMotion);
  }, []);

  useEffect(() => {
    return () => {
      if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
      if (enterFrameRef.current !== null) cancelAnimationFrame(enterFrameRef.current);
    };
  }, []);

  useEffect(() => {
    const userInitiated = userToggleKey !== previousUserToggleKeyRef.current;
    previousUserToggleKeyRef.current = userToggleKey;

    if (firstEffectRef.current) {
      firstEffectRef.current = false;
      previousOpenRef.current = open;
      return;
    }

    if (previousOpenRef.current === open) return;
    previousOpenRef.current = open;

    if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
    if (enterFrameRef.current !== null) cancelAnimationFrame(enterFrameRef.current);

    if (!userInitiated || reducedMotion) {
      setShouldRender(open);
      setPhase(open ? 'open' : 'closed');
      return;
    }

    if (open) {
      setShouldRender(true);
      setPhase('entering');
      enterFrameRef.current = requestAnimationFrame(() => {
        enterFrameRef.current = null;
        setPhase('open');
      });
      return;
    }

    setPhase('exiting');
    exitTimerRef.current = setTimeout(() => {
      exitTimerRef.current = null;
      setShouldRender(false);
      setPhase('closed');
    }, DISCLOSURE_MOTION_DURATION_MS);
  }, [open, reducedMotion, userToggleKey]);

  return { shouldRender, phase };
};
