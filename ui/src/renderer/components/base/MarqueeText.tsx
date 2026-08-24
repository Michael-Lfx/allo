import classNames from 'classnames';
import React, { useCallback, useEffect, useRef, useState } from 'react';

import {
  getMarqueeTravelDuration,
  MARQUEE_EASE,
  MARQUEE_END_PAUSE_MS,
  MARQUEE_RETURN_MS,
  MARQUEE_START_DELAY_MS,
} from './marqueeTextTiming';
import styles from './MarqueeText.module.css';

type MarqueeTextProps = {
  /** Complete text used for measurement, accessibility, and the reveal track. */
  text: string;
  /** Optional resting representation, such as PathText's middle-truncated path. */
  staticContent?: React.ReactNode;
  /** Optional semantic/test hook for the visible viewport. */
  testId?: string;
  /** Classes for the fixed viewport, including typography and layout. */
  className?: string;
  /** Disable pointer/focus activation for mobile, editing, menus, or transitions. */
  disabled?: boolean;
  /** Optional external hover scope, such as a parent workspace identity row. */
  active?: boolean;
  /** Hover-only for list rows, or hover/focus for an interactive title. */
  trigger?: 'hover' | 'hoverOrFocus';
  /** Full-text hover affordance. Defaults to the complete text. */
  title?: string;
} & Omit<React.HTMLAttributes<HTMLSpanElement>, 'aria-label' | 'children' | 'className' | 'title'>;

type CSSVariableStyle = React.CSSProperties & {
  '--marquee-duration'?: string;
  '--marquee-ease'?: string;
  '--marquee-offset'?: string;
};

const getReducedMotionPreference = (): boolean =>
  typeof window !== 'undefined' &&
  typeof window.matchMedia === 'function' &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const scheduleFrame = (callback: FrameRequestCallback): number => {
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    return window.requestAnimationFrame(callback);
  }
  return window.setTimeout(() => callback(Date.now()), 0);
};

const cancelFrame = (frame: number | null): void => {
  if (frame === null || typeof window === 'undefined') return;
  if (typeof window.cancelAnimationFrame === 'function') {
    window.cancelAnimationFrame(frame);
  } else {
    window.clearTimeout(frame);
  }
};

const MarqueeText: React.FC<MarqueeTextProps> = ({
  text,
  staticContent,
  testId,
  disabled = false,
  active = false,
  trigger = 'hover',
  title,
  className,
  onPointerEnter,
  onPointerLeave,
  onFocus,
  onBlur,
  ...rest
}) => {
  const viewportRef = useRef<HTMLSpanElement | null>(null);
  const measureRef = useRef<HTMLSpanElement | null>(null);
  const trackRef = useRef<HTMLSpanElement | null>(null);
  const timerRefs = useRef<number[]>([]);
  const frameRef = useRef<number | null>(null);
  const mountedRef = useRef(true);
  const playingRef = useRef(false);
  const [isHovered, setIsHovered] = useState(false);
  const [isFocused, setIsFocused] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [overflowDistance, setOverflowDistance] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);

  const clearTimers = useCallback(() => {
    for (const timer of timerRefs.current) window.clearTimeout(timer);
    timerRefs.current = [];
  }, []);

  const finishPlayback = useCallback(() => {
    playingRef.current = false;
    if (mountedRef.current) setIsPlaying(false);
  }, []);

  const stopPlayback = useCallback(() => {
    clearTimers();
    cancelFrame(frameRef.current);
    frameRef.current = null;

    if (!playingRef.current) {
      if (mountedRef.current) setIsPlaying(false);
      return;
    }

    const track = trackRef.current;
    if (!track) {
      finishPlayback();
      return;
    }

    // Release the playback guard immediately so a rapid re-entry can start a
    // fresh cycle while the current track is returning to its origin.
    playingRef.current = false;
    track.style.setProperty('--marquee-duration', `${MARQUEE_RETURN_MS}ms`);
    track.style.setProperty('--marquee-offset', '0px');
    timerRefs.current.push(window.setTimeout(finishPlayback, MARQUEE_RETURN_MS));
  }, [clearTimers, finishPlayback]);

  const startPlayback = useCallback(() => {
    if (overflowDistance <= 0 || reducedMotion || disabled || playingRef.current) return;

    clearTimers();
    playingRef.current = true;
    setIsPlaying(true);
    const travelDuration = getMarqueeTravelDuration(overflowDistance);

    frameRef.current = scheduleFrame(() => {
      frameRef.current = null;
      const track = trackRef.current;
      if (!track || !mountedRef.current) {
        finishPlayback();
        return;
      }

      track.style.setProperty('--marquee-ease', MARQUEE_EASE);
      track.style.setProperty('--marquee-duration', `${travelDuration}ms`);
      track.style.setProperty('--marquee-offset', `-${overflowDistance}px`);
      timerRefs.current.push(
        window.setTimeout(() => {
          if (!mountedRef.current || !playingRef.current || !trackRef.current) return;
          track.style.setProperty('--marquee-duration', `${MARQUEE_RETURN_MS}ms`);
          track.style.setProperty('--marquee-offset', '0px');
          timerRefs.current.push(window.setTimeout(finishPlayback, MARQUEE_RETURN_MS));
        }, travelDuration + MARQUEE_END_PAUSE_MS)
      );
    });
  }, [clearTimers, disabled, finishPlayback, overflowDistance, reducedMotion]);

  const measureOverflow = useCallback(() => {
    const viewport = viewportRef.current;
    const measure = measureRef.current;
    if (!viewport || !measure) return;

    const nextDistance = Math.max(0, measure.scrollWidth - viewport.clientWidth);
    setOverflowDistance((previous) => (previous === nextDistance ? previous : nextDistance));
  }, []);

  const triggerActive = active || (trigger === 'hover' ? isHovered : isHovered || isFocused);
  const shouldMeasure = triggerActive && !disabled && !reducedMotion;

  useEffect(() => {
    mountedRef.current = true;
    setReducedMotion(getReducedMotionPreference());

    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return undefined;
    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    const handlePreferenceChange = () => setReducedMotion(mediaQuery.matches);
    mediaQuery.addEventListener?.('change', handlePreferenceChange);
    return () => mediaQuery.removeEventListener?.('change', handlePreferenceChange);
  }, []);

  useEffect(() => {
    if (!shouldMeasure) {
      setOverflowDistance(0);
      return undefined;
    }

    measureOverflow();
    const viewport = viewportRef.current;
    const measure = measureRef.current;
    if (!viewport || !measure) return undefined;

    if (typeof ResizeObserver !== 'undefined') {
      const observer = new ResizeObserver(measureOverflow);
      observer.observe(viewport);
      observer.observe(measure);
      return () => observer.disconnect();
    }

    window.addEventListener('resize', measureOverflow);
    return () => window.removeEventListener('resize', measureOverflow);
  }, [measureOverflow, shouldMeasure, text]);

  // A new string or measured viewport width invalidates the current travel
  // distance. Stop the old cycle before the playback effect schedules a fresh
  // delayed pass against the new geometry.
  useEffect(() => {
    stopPlayback();
  }, [overflowDistance, stopPlayback, text]);

  const canPlay = triggerActive && overflowDistance > 0 && !disabled && !reducedMotion;

  useEffect(() => {
    if (!canPlay) {
      stopPlayback();
      return undefined;
    }

    const startTimer = window.setTimeout(startPlayback, MARQUEE_START_DELAY_MS);
    timerRefs.current.push(startTimer);
    return () => window.clearTimeout(startTimer);
  }, [canPlay, startPlayback, stopPlayback]);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
      clearTimers();
      cancelFrame(frameRef.current);
      frameRef.current = null;
    };
  }, [clearTimers]);

  const handlePointerEnter = (event: React.PointerEvent<HTMLSpanElement>) => {
    if (event.pointerType !== 'touch') setIsHovered(true);
    onPointerEnter?.(event);
  };

  const handlePointerLeave = (event: React.PointerEvent<HTMLSpanElement>) => {
    setIsHovered(false);
    onPointerLeave?.(event);
  };

  const handleFocus = (event: React.FocusEvent<HTMLSpanElement>) => {
    setIsFocused(true);
    onFocus?.(event);
  };

  const handleBlur = (event: React.FocusEvent<HTMLSpanElement>) => {
    setIsFocused(false);
    onBlur?.(event);
  };

  const style: CSSVariableStyle = {
    '--marquee-ease': MARQUEE_EASE,
  };

  return (
    <span
      {...rest}
      ref={viewportRef}
      className={classNames(styles.viewport, className)}
      data-testid={testId}
      title={title ?? text}
      aria-label={text}
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      onFocus={handleFocus}
      onBlur={handleBlur}
      style={{ ...style, ...rest.style }}
    >
      {!isPlaying && (
        <span className={styles.staticContent} aria-hidden='true'>
          {staticContent ?? text}
        </span>
      )}
      {isPlaying && (
        <span
          ref={trackRef}
          className={classNames(styles.track, styles.trackActive)}
          style={style}
          aria-hidden='true'
        >
          {text}
        </span>
      )}
      {shouldMeasure && (
        <span ref={measureRef} className={styles.measure} aria-hidden='true'>
          {text}
        </span>
      )}
      <span className='sr-only'>{text}</span>
    </span>
  );
};

export default MarqueeText;
