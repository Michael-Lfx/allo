/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const markdownSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const shadowSource = readFileSync(new URL('./ShadowView.tsx', import.meta.url), 'utf8');

const tableCss = (): string => {
  const wrap = shadowSource.indexOf('.markdown-table-wrap');
  expect(wrap).toBeGreaterThan(-1);
  return shadowSource.slice(wrap);
};

describe('Markdown table chrome', () => {
  test('wraps GFM tables in a scroll shell instead of painting a cell grid', () => {
    expect(markdownSource.includes("className='markdown-table-wrap'")).toBe(true);
    expect(markdownSource.includes("overflowX: 'auto'")).toBe(false);
    expect(markdownSource.includes("minWidth: '120px'")).toBe(false);
    expect(markdownSource.includes("border: '1px solid var(--bg-3)'")).toBe(false);
    expect(markdownSource.includes('td: ({')).toBe(false);

    const css = tableCss();
    expect(css.includes('overflow-x: auto')).toBe(true);
    expect(css.includes('border-radius: 12px')).toBe(true);
    expect(css.includes('position: sticky')).toBe(true);
    expect(css.includes('left: 0')).toBe(true);
    expect(css.includes('min-width: 120px')).toBe(false);
    expect(css.includes('text-align: left')).toBe(false);
    expect(css.includes('border: 1px solid var(--bg-3)')).toBe(false);
    expect(css.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('forwards Beautiful UI surface tokens into the Shadow DOM host', () => {
    expect(shadowSource.includes("'--color-border-2':")).toBe(true);
    expect(shadowSource.includes("'--color-fill-1':")).toBe(true);
    expect(shadowSource.includes("'--color-bg-1':")).toBe(true);
  });
});
