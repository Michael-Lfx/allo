/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { createElement } from 'react';
import { renderToString } from 'react-dom/server';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { remarkSafeBreaks } from './remarkSafeBreaks';

const markdownSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

const render = (markdown: string): string =>
  renderToString(
    createElement(ReactMarkdown, { remarkPlugins: [remarkGfm, remarkSafeBreaks] }, markdown)
  );

describe('remarkSafeBreaks', () => {
  test('turns table-cell <br> tags into real line breaks instead of visible text', () => {
    const html = render(
      '| 方式 | 费用 |\n| :--- | :--- |\n| 多导睡眠监测 (PSG) <br> 医院睡眠中心过夜 | 800 |\n| 便携式家庭睡眠监测 (HST) <br/> 租设备回家睡一晚 | 300 |\n'
    );

    expect(html).not.toContain('&lt;br');
    expect(html).toContain('多导睡眠监测 (PSG)');
    expect(html).toContain('医院睡眠中心过夜');
    expect(html).toContain('便携式家庭睡眠监测 (HST)');
    expect(html).toContain('租设备回家睡一晚');
    expect(html.match(/<br\s*\/?>/g)?.length).toBe(2);
  });

  test('does not enable other raw HTML', () => {
    const html = render('hello <script>alert(1)</script> <strong>x</strong>');

    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<strong>');
    expect(html).toContain('&lt;script&gt;');
  });

  test('leaves <br> inside fenced and inline code unchanged', () => {
    const html = render('use `<br>` in tables\n\n```html\n<br>\n```\n');

    expect(html).toContain('&lt;br&gt;');
  });

  test('MarkdownView registers the plugin without turning on raw HTML', () => {
    expect(markdownSource).toContain('remarkSafeBreaks');
    expect(markdownSource).toContain('const REMARK_PLUGINS = [remarkGfm, remarkMath, remarkBreaks, remarkSafeBreaks]');
  });
});
