import React, { useEffect, useRef, useState } from 'react';
import { GodRays } from '@paper-design/shaders-react';
import { useThemeContext } from '@renderer/hooks/context/ThemeContext';
import styles from './CreateAmbientWall.module.css';

const DARK_PALETTE = {
  colorBack: '#090a11',
  colorBloom: '#2b1637',
  colors: ['#ff6b7fa8', '#b48cff9e', '#7dd3fc94'],
};

const LIGHT_PALETTE = {
  colorBack: '#f6f1eb',
  colorBloom: '#fffaf4',
  colors: ['#ff9eb0a8', '#b8a9ffa8', '#ffd6a59e'],
};

const IMAGE_DARK_PALETTE = {
  colorBack: '#0b0908',
  colorBloom: '#2c1812',
  colors: ['#ffb27aa8', '#ef8fb5a0', '#7dd3fc8c'],
};

const IMAGE_LIGHT_PALETTE = {
  colorBack: '#f8f1e9',
  colorBloom: '#fff4e7',
  colors: ['#ffb27ab0', '#ef8fb5a0', '#8ed8e69e'],
};

type CreateAmbientVariant = 'create' | 'image';

/**
 * Create-directory ambient layer. Paper Shaders' God Rays gives the page one
 * cinematic light field instead of a row of literal slide-card silhouettes.
 * The shader is capped by pixel count, stops for reduced motion, and freezes
 * while the document is hidden.
 */
const CreateAmbientWall: React.FC<{ variant?: CreateAmbientVariant }> = ({ variant = 'create' }) => {
  const { theme } = useThemeContext();
  const rootRef = useRef<HTMLDivElement>(null);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [documentVisible, setDocumentVisible] = useState(true);
  const [lowPower, setLowPower] = useState(false);
  const [webglReady, setWebglReady] = useState(false);
  const isDark = theme === 'dark';
  const palette =
    variant === 'image'
      ? isDark
        ? IMAGE_DARK_PALETTE
        : IMAGE_LIGHT_PALETTE
      : isDark
        ? DARK_PALETTE
        : LIGHT_PALETTE;

  useEffect(() => {
    const probe = document.createElement('canvas');
    const gl = probe.getContext('webgl2');
    setWebglReady(Boolean(gl));
    gl?.getExtension('WEBGL_lose_context')?.loseContext();
  }, []);

  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const syncMotion = () => setReducedMotion(media.matches);
    syncMotion();
    media.addEventListener?.('change', syncMotion);
    return () => media.removeEventListener?.('change', syncMotion);
  }, []);

  useEffect(() => {
    const navigatorWithMemory = navigator as Navigator & { deviceMemory?: number };
    const cores = navigator.hardwareConcurrency ?? 8;
    const memory = navigatorWithMemory.deviceMemory ?? 8;
    const coarsePointer = window.matchMedia?.('(pointer: coarse)')?.matches ?? false;
    setLowPower(memory <= 4 || cores <= 4 || coarsePointer);
  }, []);

  useEffect(() => {
    const syncVisibility = () => setDocumentVisible(!document.hidden);
    syncVisibility();
    document.addEventListener('visibilitychange', syncVisibility);
    return () => document.removeEventListener('visibilitychange', syncVisibility);
  }, []);

  useEffect(() => {
    if (reducedMotion) return undefined;
    const root = rootRef.current;
    if (!root) return undefined;
    let pending = false;
    let frame = 0;
    let lastEvent: PointerEvent | null = null;

    const flush = () => {
      pending = false;
      const event = lastEvent;
      if (!event || !rootRef.current) return;
      const rect = rootRef.current.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      const nx = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width)) * 2 - 1;
      const ny = Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height)) * 2 - 1;
      rootRef.current.style.setProperty('--ambient-shift-x', `${(nx * 16).toFixed(2)}px`);
      rootRef.current.style.setProperty('--ambient-shift-y', `${(ny * 12).toFixed(2)}px`);
      rootRef.current.style.setProperty('--ambient-glow-x', `${(nx * 72).toFixed(2)}px`);
      rootRef.current.style.setProperty('--ambient-glow-y', `${(ny * 48).toFixed(2)}px`);
    };

    const onPointerMove = (event: PointerEvent) => {
      if (event.pointerType && event.pointerType !== 'mouse') return;
      lastEvent = event;
      if (pending) return;
      pending = true;
      frame = window.requestAnimationFrame(flush);
    };

    const reset = () => {
      lastEvent = null;
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      pending = false;
      rootRef.current?.style.setProperty('--ambient-shift-x', '0px');
      rootRef.current?.style.setProperty('--ambient-shift-y', '0px');
      rootRef.current?.style.setProperty('--ambient-glow-x', '0px');
      rootRef.current?.style.setProperty('--ambient-glow-y', '0px');
    };

    window.addEventListener('pointermove', onPointerMove, { passive: true });
    window.addEventListener('blur', reset);
    document.addEventListener('pointerleave', reset);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('blur', reset);
      document.removeEventListener('pointerleave', reset);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [reducedMotion]);

  return (
    <div
      ref={rootRef}
      className={styles.wall}
      data-testid='guid-create-ambient'
      data-quality={lowPower ? 'low' : undefined}
      aria-hidden='true'
    >
      {webglReady ? (
        <GodRays
          className={styles.shader}
          style={{ width: '100%', height: '100%' }}
          colors={palette.colors}
          colorBack={palette.colorBack}
          colorBloom={palette.colorBloom}
          bloom={isDark ? 0.22 : 0.14}
          intensity={isDark ? 0.36 : 0.27}
          density={isDark ? 0.18 : 0.22}
          spotty={isDark ? 0.46 : 0.36}
          midSize={isDark ? 0.48 : 0.36}
          midIntensity={isDark ? 0.13 : 0.08}
          offsetX={0.04}
          offsetY={-0.55}
          scale={1.18}
          fit='cover'
          speed={reducedMotion || !documentVisible ? 0 : lowPower ? 0.35 : 0.55}
          frame={0}
          minPixelRatio={1}
          maxPixelCount={lowPower ? 550_000 : 1_100_000}
        />
      ) : (
        <div className={styles.fallback} />
      )}
      <div className={styles.pointerGlow} />
      <div className={styles.veil} />
    </div>
  );
};

export default CreateAmbientWall;
