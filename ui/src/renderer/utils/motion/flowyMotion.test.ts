/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { FLOWY_EASE, FLOWY_MOTION_MS, flowyTransition } from './flowyMotion';

const css = readFileSync(new URL('../../styles/flowy-motion.css', import.meta.url), 'utf8');

describe('flowy motion system', () => {
  test('exposes three duration tiers and three easings', () => {
    expect(FLOWY_MOTION_MS).toEqual({ fast: 120, base: 180, slow: 240 });
    expect(FLOWY_EASE.enter).toHaveLength(4);
    expect(flowyTransition('enter').duration).toBe(0.18);
    expect(flowyTransition('exit', 'fast').duration).toBe(0.12);
  });

  test('css prefers transform/opacity and respects reduced motion', () => {
    expect(css.includes('--flowy-motion-fast: 120ms')).toBe(true);
    expect(css.includes('--flowy-motion-base: 180ms')).toBe(true);
    expect(css.includes('--flowy-motion-slow: 240ms')).toBe(true);
    expect(css.includes('prefers-reduced-motion: reduce')).toBe(true);
    expect(css.includes('flowy-task-reveal')).toBe(true);
    expect(css.includes('cubic-bezier')).toBe(true);
    expect(/animation:\s*[^;]*spring/.test(css)).toBe(false);
  });
});
