/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const FLOWY_MOTION_MS = {
  fast: 120,
  base: 180,
  slow: 240,
} as const;

export const FLOWY_EASE = {
  enter: [0.16, 1, 0.3, 1] as const,
  exit: [0.4, 0, 1, 1] as const,
  move: [0.22, 1, 0.36, 1] as const,
};

export type FlowyMotionPreset = 'enter' | 'exit' | 'move';

export function flowyTransition(preset: FlowyMotionPreset, duration: keyof typeof FLOWY_MOTION_MS = 'base') {
  const ease = FLOWY_EASE[preset === 'move' ? 'move' : preset];
  return {
    duration: FLOWY_MOTION_MS[duration] / 1000,
    ease: [...ease],
  };
}

/** High-probability next chunks after auth / home. */
export function preloadCommercialPathChunks() {
  void import('@renderer/pages/guid');
  void import('@renderer/pages/conversation');
  void import('@renderer/pages/cloudLogin');
}

export function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}
