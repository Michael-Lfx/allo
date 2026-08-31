/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./CollapsibleContent.tsx', import.meta.url), 'utf8');

describe('CollapsibleContent structure', () => {
  test('exposes stable accessibility state and semantic styling hooks', () => {
    expect(source).toContain('useId');
    expect(source).toContain('aria-expanded');
    expect(source).toContain('aria-controls');
    expect(source).toContain('collapsible-content');
    expect(source).toContain('collapsible-content__body');
    expect(source).toContain('collapsible-content__toggle');
  });

  test('uses functional state updates and cancels observer work during cleanup', () => {
    expect(source).toContain('setIsCollapsed((value) => !value)');
    expect(source).toContain('cancelAnimationFrame');
    expect(source).toContain('ResizeObserver');
    expect(source).toContain('setNeedsCollapse((value) =>');
  });

  test('keeps observer setup independent from children identity changes', () => {
    expect(source).toContain('}, [maxHeight]);');
    expect(source).toContain('scheduleHeightCheckRef.current?.();');
  });
});
