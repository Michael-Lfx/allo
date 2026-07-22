/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('./flowy-visual-system.css', import.meta.url), 'utf8');
const direction = readFileSync(
  new URL('../../../../docs/superpowers/design/2026-07-22-flowy-visual-direction.md', import.meta.url),
  'utf8'
);

const REQUIRED_TOKENS = [
  '--flowy-text-display',
  '--flowy-space-1',
  '--flowy-space-2',
  '--flowy-radius-md',
  '--flowy-surface-0',
  '--flowy-surface-1',
  '--flowy-accent',
  '--flowy-success',
  '--flowy-warning',
  '--flowy-danger',
  '--flowy-density-rail',
  '--flowy-density-session',
  '--flowy-density-workspace',
  '--flowy-focus-ring',
];

describe('flowy visual system', () => {
  test('locks Ink Studio as the shipped direction', () => {
    expect(direction.includes('已选：Ink Studio')).toBe(true);
    expect(direction.includes('Warm Atelier（否决）')).toBe(true);
  });

  test('declares light and dark semantic token blocks', () => {
    for (const token of REQUIRED_TOKENS) {
      expect(css.includes(`${token}:`)).toBe(true);
    }
    expect(css.includes("[data-theme='dark']")).toBe(true);
    expect(css.includes("body[arco-theme='dark']")).toBe(true);
  });
});
