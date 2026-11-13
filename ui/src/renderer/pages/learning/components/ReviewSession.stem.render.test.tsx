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
import type { DueReview } from '../types';
import { ReviewCard } from './ReviewSession';

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

const noop = () => {};
const noopAsync = async () => undefined;

const makeReview = (prompt: string): DueReview => ({
  id: 'review-1',
  source: 'course',
  enrollment_id: null,
  course_id: 'course-1',
  course_title: '课程',
  module_title: '模块',
  lesson_title: '课时',
  question: {
    activity_id: null,
    kind: 'single_choice',
    prompt,
    options: ['$0.375$', '$0.303003\\ldots$'],
    matches: [],
  },
  due_at: 0,
  stability_days: 0,
  difficulty: 0,
  review_count: 0,
  lapse_count: 0,
  r: null,
  edit_pending: false,
  edit_note: null,
});

const renderCard = async (prompt: string, locale: 'zh-CN' | 'en-US' = 'zh-CN') =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n(locale)}>
      <ReviewCard
        review={makeReview(prompt)}
        busy={false}
        locked={false}
        onAnswer={noopAsync}
        onForget={noopAsync}
        onRate={noop}
        onSkip={noop}
        onNext={noop}
        onEdit={noop}
        onMarkEdit={noop}
        onArchive={noop}
        onDelete={noop}
      />
    </I18nextProvider>
  );

describe('复习卡题干渲染管线（n1）', () => {
  test('复习题干走内联通道，公式出 KaTeX（与课时面同口径）', async () => {
    const markup = await renderCard('下列哪一个是无理数？');
    expect(markup).toContain('data-testid="inline-markdown"');
    expect(markup).toContain('下列哪一个是无理数？');
    // 作答选项共用 ActivityInput，公式同样出 KaTeX
    expect(markup.split('class="katex"').length - 1).toBeGreaterThanOrEqual(2);
  });

  test('复习题干含 $…$ 与 ___ 空位时分别消费（fill_in_blank 复习卡）', async () => {
    const markup = await renderCard('补全：光在真空中的速度 $c$ ___ 是常数。');
    expect(markup).toContain('class="katex"');
    expect(markup).toContain('data-blank-slot');
    expect(markup).not.toContain('___');
    expect(markup).not.toContain('<hr');
  });
});
