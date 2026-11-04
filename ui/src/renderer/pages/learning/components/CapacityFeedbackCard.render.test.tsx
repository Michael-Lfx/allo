/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 容量反馈卡渲染（y07）：有自报才呈卡（无自报渲染空串）、三选项与自报原因
 * 均在（回显选择走初始状态）、初始无错误行；中英文案都从字典取。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { Lesson } from '../types';
import { CapacityFeedbackCard } from './CapacityFeedbackCard';

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

const lesson = (overrides: Partial<Lesson> = {}): Lesson => ({
  id: 'lesson-1',
  title: '课时',
  summary: '',
  purpose: '',
  intent: '',
  scope: '',
  position: 0,
  generated: true,
  est: 25,
  source: null,
  sources: [],
  status: 'in_progress',
  activities: [],
  sections: [],
  ...overrides,
});

const renderCard = async (
  locale: 'zh-CN' | 'en-US',
  target: Lesson
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <CapacityFeedbackCard lesson={target} />
    </I18nextProvider>
  );
};

describe('CapacityFeedbackCard', () => {
  test('无自报 = 零假信号（不渲染卡片）', async () => {
    expect(await renderCard('zh-CN', lesson())).toBe('');
    expect(await renderCard('zh-CN', lesson({ capacity_note: '   ' }))).toBe('');
  });

  test('有自报：原因 + 三选项 + 留痕说明都在，初始无错误行', async () => {
    const html = await renderCard(
      'zh-CN',
      lesson({ capacity_note: '保覆盖就必超：第三节推导超出 25 分钟预算' })
    );
    expect(html).toContain('data-testid="capacity-feedback-card"');
    expect(html).toContain('保覆盖就必超：第三节推导超出 25 分钟预算');
    expect(html).toContain('接受现状');
    expect(html).toContain('预算加长');
    expect(html).toContain('拆成两节');
    expect(html).toContain('只记录你的选择');
    expect(html).not.toContain('role="alert"');
  });

  test('已选过则回显（重载不失忆）', async () => {
    const html = await renderCard(
      'zh-CN',
      lesson({
        capacity_note: '例子太多',
        capacity_feedback: { choice: 'split', at: 2000 },
      })
    );
    // Arco primary 按钮是唯一选中态：三个按钮里恰好一个带 primary 类。
    const primaryButtons = html.match(/arco-btn-primary/g) ?? [];
    expect(primaryButtons.length).toBe(1);
    expect(html).toContain('拆成两节');
    // 「已记录」只在本次会话提交成功后出现——重载回显只体现选中态。
    expect(html).not.toContain('已记录你的选择');
  });

  test('en-US 文案同样齐备', async () => {
    const html = await renderCard(
      'en-US',
      lesson({ capacity_note: 'Exceeds the budget by ~8 minutes' })
    );
    expect(html).toContain('Capacity feedback');
    expect(html).toContain('Accept as is');
    expect(html).toContain('Extend budget');
    expect(html).toContain('Split into two');
  });
});
