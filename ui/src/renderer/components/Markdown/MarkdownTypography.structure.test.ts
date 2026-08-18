/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const markdownSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const markdownPropsSource = readFileSync(new URL('./markdownViewProps.ts', import.meta.url), 'utf8');
const shadowSource = readFileSync(new URL('./ShadowView.tsx', import.meta.url), 'utf8');

describe('Markdown typography controls', () => {
  test('lets message surfaces override the Shadow DOM body typography', () => {
    expect(markdownPropsSource.includes('fontSize?: string')).toBe(true);
    expect(markdownPropsSource.includes('lineHeight?: string')).toBe(true);
    expect(markdownSource.includes('<ShadowView fontSize={fontSize} lineHeight={lineHeight}>')).toBe(true);
    expect(shadowSource.includes('fontSize?: string')).toBe(true);
    expect(shadowSource.includes('lineHeight?: string')).toBe(true);
    expect(shadowSource.includes("const resolvedFontSize = fontSize ?? (isMobile ? '14px' : '16px');")).toBe(true);
    expect(shadowSource.includes("const resolvedLineHeight = lineHeight ?? (isMobile ? '19.6px' : '28px');")).toBe(true);
    expect(shadowSource.includes('const usesExplicitTypography = Boolean(fontSize || lineHeight);')).toBe(true);
    expect(shadowSource.includes("margin-block-start: ${usesExplicitTypography ? '10px' : '16px'};")).toBe(true);
    expect(shadowSource.includes('.markdown-shadow-body>h1:first-child')).toBe(true);
    expect(shadowSource.includes("font-size: ${usesExplicitTypography ? '18px' : '24px'};")).toBe(true);
    expect(shadowSource.includes("font-size: ${usesExplicitTypography ? '16px' : '18px'};")).toBe(true);
    expect(shadowSource.includes("font-size: ${usesExplicitTypography ? '15px' : '16px'};")).toBe(true);
    expect(shadowSource.includes('a:focus-visible')).toBe(true);
    expect(shadowSource.includes("'--color-link':")).toBe(true);
    expect(shadowSource.includes('markdown-code-toolbar')).toBe(true);
    expect(shadowSource.includes('markdown-code-block:focus-within')).toBe(true);
    expect(shadowSource.includes('.markdown-shadow-body .hljs code')).toBe(true);
    expect(shadowSource.includes('.markdown-code-content::-webkit-scrollbar')).toBe(true);
  });

  test('keeps inline code in the sentence instead of painting a gray pill on every path', () => {
    expect(shadowSource.includes('.markdown-shadow-body code:not(pre code)')).toBe(true);
    expect(shadowSource.includes('background: none')).toBe(true);
    expect(shadowSource.includes('padding: 0')).toBe(true);
    expect(shadowSource.includes('border-radius: 0')).toBe(true);
    expect(shadowSource.includes('padding: 2px 6px')).toBe(false);
  });
});
