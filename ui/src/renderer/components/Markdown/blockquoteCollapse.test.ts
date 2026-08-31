/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { shouldCollapseMarkdownBlockquote } from './blockquoteCollapse';

const markdownSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('markdown blockquote collapse', () => {
  test('allows a simple blockquote to use the collapse wrapper', () => {
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'blockquote',
        children: [{ type: 'paragraph', children: [{ type: 'text', value: 'text' }] }],
      }),
    ).toBe(true);
  });

  test('does not add a second control around nested quotes or fenced code', () => {
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'blockquote',
        children: [{ type: 'blockquote', children: [] }],
      }),
    ).toBe(false);
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'blockquote',
        children: [{ type: 'code', lang: 'ts', value: 'const value = 1;' }],
      }),
    ).toBe(false);
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'element',
        tagName: 'blockquote',
        children: [
          {
            type: 'element',
            tagName: 'pre',
            children: [{ type: 'element', tagName: 'code', children: [] }],
          },
        ],
      }),
    ).toBe(false);
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'element',
        tagName: 'blockquote',
        children: [{ type: 'element', tagName: 'blockquote', children: [] }],
      }),
    ).toBe(false);
    expect(
      shouldCollapseMarkdownBlockquote({
        type: 'element',
        tagName: 'blockquote',
        children: [{ type: 'element', tagName: 'p', children: [{ type: 'element', tagName: 'code' }] }],
      }),
    ).toBe(true);
    expect(shouldCollapseMarkdownBlockquote(undefined)).toBe(false);
  });

  test('keeps blockquote collapsing opt-in at the MarkdownView boundary', () => {
    expect(markdownSource).toContain('blockquote:');
    expect(markdownSource).toContain('collapsibleBlockquotes');
    expect(markdownSource).toContain('maxHeight={200}');
    expect(markdownSource).toContain('defaultCollapsed');
    expect(markdownSource).toContain('useMask');
  });
});
