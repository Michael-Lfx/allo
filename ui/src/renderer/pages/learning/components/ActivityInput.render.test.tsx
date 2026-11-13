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

import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import { ActivityInput } from './ActivityInput';

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

const renderInput = async (element: React.ReactElement, locale: 'zh-CN' | 'en-US' = 'zh-CN') =>
  renderToStaticMarkup(<I18nextProvider i18n={await initTestI18n(locale)}>{element}</I18nextProvider>);

const noop = () => {};

const countOccurrences = (haystack: string, needle: string): number =>
  haystack.split(needle).length - 1;

describe('题面作答面渲染管线（n1 页面集成断言）', () => {
  // 真机报障同款场景：无理数选择题选项 $…$ 原样露出——选项必须出 KaTeX。
  test('单选选项行内公式出 KaTeX，判分 value 保持原始选项串', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='single_choice'
        options={['$0.375$', '$0.303003\\ldots$']}
        value={undefined}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(markup).toContain('arco-radio');
    // 判分锚点不受渲染影响：Radio value 仍是原始选项串
    expect(markup).toContain('$0.375$');
  });

  test('多选选项同样走渲染管线', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='multi_choice'
        options={['$\\sqrt{2}$ 是无理数', '$0.5$ 是有理数']}
        value={[]}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(markup).toContain('arco-checkbox');
  });

  test('排序条目走渲染管线', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='ordering'
        options={['$\\sqrt{2}$', '$\\pi$']}
        value={undefined}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
  });

  test('英文主题下同一断言成立（选项文案来自数据面，与 locale 无关）', async () => {
    const markup = await renderInput(
      <ActivityInput kind='single_choice' options={['$\\pi$']} value={undefined} onChange={noop} />,
      'en-US'
    );
    expect(countOccurrences(markup, 'class="katex"')).toBe(1);
  });

  test('fill_in_blank 作答位仍是输入框（空位渲染只发生在题干，不在作答控件）', async () => {
    const markup = await renderInput(
      <ActivityInput kind='fill_in_blank' options={[]} value={''} onChange={noop} />
    );
    expect(markup).toContain('arco-input');
    expect(markup).not.toContain('data-blank-slot');
  });
});
