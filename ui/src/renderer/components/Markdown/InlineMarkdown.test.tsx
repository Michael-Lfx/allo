/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { InlineMarkdown, InlineRenderBoundary } from './InlineMarkdown';

const initTestI18n = async (locale: 'zh-CN' | 'en-US') => {
  const instance = createInstance();
  await instance.use(initReactI18next).init({
    lng: locale,
    fallbackLng: 'zh-CN',
    resources: {
      'zh-CN': { translation: { learning: learningZh } },
      'en-US': { translation: { learning: learningEn } },
    },
    interpolation: { escapeValue: false },
  });
  return instance;
};

const renderInline = async (text: string, locale: 'zh-CN' | 'en-US' = 'zh-CN'): Promise<string> =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n(locale)}>
      <InlineMarkdown text={text} />
    </I18nextProvider>
  );

const countOccurrences = (haystack: string, needle: string): number =>
  haystack.split(needle).length - 1;

describe('InlineMarkdown 题面内联通道', () => {
  test('行内公式走 KaTeX，原始 $ 记号不再露出', async () => {
    const markup = await renderInline('比较 $0.375$ 与 $0.303003\\ldots$ 的大小。');
    expect(countOccurrences(markup, 'class="katex"')).toBe(2);
    expect(markup).not.toContain('$');
    expect(markup).toContain('比较');
    expect(markup).toContain('的大小。');
  });

  test('段落解包为 span（合法嵌入 <p>/<button>/<label> 等短语内容位）', async () => {
    const markup = await renderInline('第一段结论。\n\n第二段补充。');
    expect(markup).not.toContain('<p');
    expect(markup).toContain('第一段结论。');
    expect(markup).toContain('第二段补充。');
  });

  test('坏公式不炸面：降级为可读文本（不静默丢正文）', async () => {
    const markup = await renderInline('残缺公式 $x^{$ 与普通文字');
    expect(markup).toContain('残缺公式');
    expect(markup).toContain('与普通文字');
  });

  test('渲染失败边界 fail-visible：兜底文案（i18n）+ 原文都在，且换文自动复位', async () => {
    // getDerivedStateFromError 进入失败态（与 SyntaxHighlightBoundary 同款结构断言）
    const next = (
      InlineRenderBoundary as unknown as {
        getDerivedStateFromError: () => { failed: boolean };
      }
    ).getDerivedStateFromError();
    expect(next).toEqual({ failed: true });

    // 失败态渲染 = i18n 兜底文案 + 原文（fail-visible，不静默丢）；双语对称断言
    const boundary = new InlineRenderBoundary({ text: '原始题面内容', children: null });
    boundary.state = { failed: true };
    const fallbackZh = renderToStaticMarkup(
      <I18nextProvider i18n={await initTestI18n('zh-CN')}>{boundary.render()}</I18nextProvider>
    );
    expect(fallbackZh).toContain(learningZh.inlineRenderFailed);
    expect(fallbackZh).toContain('原始题面内容');
    const fallbackEn = renderToStaticMarkup(
      <I18nextProvider i18n={await initTestI18n('en-US')}>{boundary.render()}</I18nextProvider>
    );
    expect(fallbackEn).toContain(learningEn.inlineRenderFailed);
    expect(fallbackEn).toContain('原始题面内容');

    // 文本变化时自动复位（复习卡原地编辑不换 key 的场景）；未挂载实例上
    // React 的 setState 是 no-op，这里桩掉以断言复位契约本身。
    type BoundaryInstance = React.Component<
      { text: string; children: React.ReactNode },
      { failed: boolean }
    > & { componentDidUpdate: (previous: { text: string; children: React.ReactNode }) => void };
    const resettable = boundary as unknown as {
      props: { text: string; children: React.ReactNode };
      state: { failed: boolean };
      setState: (patch: Record<string, unknown>) => void;
      componentDidUpdate: BoundaryInstance['componentDidUpdate'];
    };
    resettable.props = { text: '新题面', children: null };
    const setStateCalls: Array<Record<string, unknown>> = [];
    resettable.setState = (patch) => {
      setStateCalls.push(patch);
    };
    resettable.componentDidUpdate({ text: '旧题面', children: null });
    expect(setStateCalls).toEqual([{ failed: false }]);
    // 文本没变不复位（保持失败态，避免无意义重试）
    resettable.componentDidUpdate({ text: '新题面', children: null });
    expect(setStateCalls).toHaveLength(1);
  });

  test('fill_in_blank 的 ___ 空位：渲染成空位 span，不再被 GFM 吃成 <hr>/强调', async () => {
    // 行中空位
    const inlineMarkup = await renderInline('牛顿第二定律公式 ___ 揭示了力与加速度的关系。');
    expect(inlineMarkup).toContain('data-blank-slot');
    expect(inlineMarkup).not.toContain('___');
    expect(inlineMarkup).not.toContain('<hr');
    expect(inlineMarkup).not.toContain('<strong');

    // 独占行首的空位（GFM 会把 ___ 行吃成水平线）
    const leadingMarkup = await renderInline('___ 是本题要填的结论。');
    expect(leadingMarkup).not.toContain('<hr');
    expect(leadingMarkup).toContain('data-blank-slot');

    // 成对空位（GFM 会把 ___x___ 吃成粗斜体）
    const pairedMarkup = await renderInline('记号 ___A___ 表示第一个测量值。');
    expect(countOccurrences(pairedMarkup, 'data-blank-slot')).toBe(2);
    expect(pairedMarkup).not.toContain('<strong');
    expect(pairedMarkup).not.toContain('<em>');
    expect(pairedMarkup).toContain('A');
  });

  test('裸 HTML 默认转义（不开 allowHtml 的 XSS 底线）', async () => {
    const markup = await renderInline('看到 <img src=x onerror=alert(1)> 就');
    expect(markup).not.toContain('<img');
    expect(markup).toContain('onerror=alert(1)');
  });

  test('\\(...\\) LaTeX 分隔符复用 convertLatexDelimiters 出 KaTeX', async () => {
    const markup = await renderInline('设实数 \\(x\\) 满足条件。');
    expect(countOccurrences(markup, 'class="katex"')).toBe(1);
    expect(markup).not.toContain('\\(x\\)');
  });
});
