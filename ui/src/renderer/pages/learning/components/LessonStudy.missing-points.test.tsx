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
import type { Activity, AttemptResult } from '../types';
import { ActivityBlock } from './LessonStudy';

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

const activity: Activity = {
  id: 'activity-1',
  kind: 'open_question',
  prompt: '用你自己的话解释：为什么说复利的关键不在收益率高低？',
  options: [],
  matches: [],
  section_key: null,
  position: 0,
};

const failingWithPoints: AttemptResult = {
  id: 'attempt-1',
  score: 0.3,
  passed: false,
  feedback: '回应了收益率，但没答到时间维度。',
  missing_points: ['利息并入本金再生息', '时间放大微小差异'],
};

const renderBlock = async (result: AttemptResult, locale: 'zh-CN' | 'en-US') =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n(locale)}>
      <ActivityBlock
        activity={activity}
        disabled={false}
        result={result}
        onSubmit={noop}
      />
    </I18nextProvider>
  );

describe('判错缺失要点面（n5）', () => {
  // 双语断言（InlineMarkdown inlineRenderFailed 同款写法）：AI 评审判错
  // 点名的缺失要点标题必须双语对称落字典——漏一侧 = 界面裸出 i18n 键
  // （fallback 泄漏红线）。
  test('判错结果渲染缺失要点标题与逐条要点，zh-CN 与 en-US 对称', async () => {
    const zh = await renderBlock(failingWithPoints, 'zh-CN');
    expect(zh).toContain(learningZh.missingPointsTitle);
    expect(zh).toContain('利息并入本金再生息');
    expect(zh).toContain('时间放大微小差异');

    const en = await renderBlock(failingWithPoints, 'en-US');
    expect(en).toContain(learningEn.missingPointsTitle);
    expect(en).toContain('利息并入本金再生息');
  });

  // 判对-缺失互斥（n4 D3）的渲染半边：判对（缺失恒空）不渲染标题块。
  test('判对结果不渲染缺失要点块', async () => {
    const passing: AttemptResult = {
      id: 'attempt-2',
      score: 0.9,
      passed: true,
      feedback: '覆盖完整。',
      missing_points: [],
    };
    const zh = await renderBlock(passing, 'zh-CN');
    expect(zh).not.toContain(learningZh.missingPointsTitle);
  });
});
