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
  kind: 'matching',
  prompt: '把左列条目与其归属对齐。',
  options: ['左一', '左二'],
  matches: ['右甲'],
  section_key: null,
  position: 0,
};

const MARKED_SUCCESS = 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]';
const MARKED_DANGER = 'border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)]';

const renderBlock = async (result: AttemptResult) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n('zh-CN')}>
      <ActivityBlock
        activity={activity}
        disabled
        result={result}
        initialResponse={['右甲', '右甲']}
        onSubmit={noop}
      />
    </I18nextProvider>
  );

describe('提交后逐项判分透传（pf1，ActivityBlock 接缝）', () => {
  // 行级标记的动机场景（map.md 取证）：三行里哪行错必须可见——此处两行
  // 版本钉住 AttemptResult.item_results → ActivityInput 的透传。
  test('result.item_results 逐行透传：错行红、对行绿', async () => {
    const judged: AttemptResult = {
      id: 'attempt-1',
      score: 0,
      passed: false,
      feedback: '第二行选错了。',
      item_results: [false, true],
    };
    const markup = await renderBlock(judged);
    expect(countOf(markup, MARKED_DANGER)).toBe(1);
    expect(countOf(markup, MARKED_SUCCESS)).toBe(1);
  });

  // 旧 attempt（无逐项字段）向后兼容硬要求：缺席 = 全局文案，不染色。
  test('旧 attempt 无 item_results：降级为全局文案，不染色', async () => {
    const legacy: AttemptResult = {
      id: 'attempt-2',
      score: 0,
      passed: false,
      feedback: '需要再练习。',
    };
    const markup = await renderBlock(legacy);
    expect(markup).not.toContain(MARKED_DANGER);
    expect(markup).not.toContain(MARKED_SUCCESS);
  });
});

const countOf = (haystack: string, needle: string): number => haystack.split(needle).length - 1;
