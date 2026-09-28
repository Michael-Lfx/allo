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
  test('SvgBlock uses Shiki highlighting and does not observe theme for highlighter', () => {
    expect(svgSource.includes('ShikiCodeFence')).toBe(true);
    expect(svgSource.includes("language='xml'")).toBe(true);
    expect(svgSource.includes('vs2015')).toBe(false);
    expect(svgSource.includes('currentTheme')).toBe(false);
  });

  test('JsxGraphBlock uses Shiki highlighting while keeping board theme observation', () => {
    expect(jsxGraphSource.includes('ShikiCodeFence')).toBe(true);
    expect(jsxGraphSource.includes("language='javascript'")).toBe(true);
    expect(jsxGraphSource.includes('vs2015')).toBe(false);
    expect(jsxGraphSource.includes('currentTheme')).toBe(true);
    expect(jsxGraphSource.includes("const isDark = currentTheme === 'dark'")).toBe(true);
  });
});
