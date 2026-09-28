/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const svgSource = readFileSync(new URL('./SvgBlock.tsx', import.meta.url), 'utf8');
const jsxGraphSource = readFileSync(new URL('./JsxGraphBlock.tsx', import.meta.url), 'utf8');

describe('figure source highlighting', () => {
  test('SvgBlock uses Beautiful UI tokens and does not observe theme for highlighter', () => {
    expect(svgSource.includes('beautifulUiHighlightStyle')).toBe(true);
    expect(svgSource.includes("fontFamily: 'var(--code-font)'")).toBe(true);
    expect(svgSource.includes('vs2015')).toBe(false);
    expect(svgSource.includes('currentTheme')).toBe(false);
  });

  test('JsxGraphBlock uses Beautiful UI tokens while keeping board theme observation', () => {
    expect(jsxGraphSource.includes('beautifulUiHighlightStyle')).toBe(true);
    expect(jsxGraphSource.includes("fontFamily: 'var(--code-font)'")).toBe(true);
    expect(jsxGraphSource.includes('vs2015')).toBe(false);
    expect(jsxGraphSource.includes('currentTheme')).toBe(true);
    expect(jsxGraphSource.includes("const isDark = currentTheme === 'dark'")).toBe(true);
  });
});
