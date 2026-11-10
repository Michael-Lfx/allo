/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 生长悬浮 Pill 渲染（g4）：活跃态 = 转圈 + 进度 + 回图工作区/取消；终态
 * = ✓/✕ + 关闭（不再可取消）；课程名去掉后端主题前缀；中英文案都从字典取。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { GrowthPillItem } from '../growthTaskPill';
import { GrowthTaskPill } from './GrowthTaskPill';

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

const item = (overrides: Partial<GrowthPillItem> = {}): GrowthPillItem => ({
  key: 'growth:course-a:-',
  courseId: 'course-a',
  topic: '学习图生长 · 线性代数',
  status: 'running',
  step: 'draft',
  detail: '教练正在阅读结构参考与花名册，起草本批节点…',
  elapsedSecs: 192,
  queuePosition: null,
  redraft: null,
  ...overrides,
});

const noop = () => undefined;

const renderPill = async (
  locale: 'zh-CN' | 'en-US',
  target: GrowthPillItem,
  options: { withView?: boolean } = {}
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <GrowthTaskPill
        item={target}
        onView={options.withView === false ? undefined : noop}
        onCancel={noop}
        onDismiss={noop}
      />
    </I18nextProvider>
  );
};

describe('GrowthTaskPill', () => {
  test('运行中：状态 + 已运行时长 + 课程名 + 步骤文案，可回图工作区与取消', async () => {
    const html = await renderPill('zh-CN', item());
    expect(html).toContain('学习图生长中 · 3:12');
    expect(html).toContain('线性代数');
    expect(html).not.toContain('学习图生长 · 线性代数');
    expect(html).toContain('教练正在起草本批节点…');
    expect(html).toContain('回到图工作区');
    expect(html).toContain('取消生成');
    expect(html).not.toContain('关闭');
  });

  test('打回重起草（WS 增强）优先于步骤文案；排队显示位次', async () => {
    const redraft = await renderPill(
      'zh-CN',
      item({ redraft: { stage: 'concept_review', count: 2 } })
    );
    expect(redraft).toContain('概念评审打回，正在重新起草（第 2 次）');
    expect(redraft).not.toContain('教练正在起草本批节点…');

    const queued = await renderPill(
      'zh-CN',
      item({ status: 'queued', step: null, detail: null, elapsedSecs: null, queuePosition: 2 })
    );
    expect(queued).toContain('学习图生长排队中');
    expect(queued).toContain('排队第 2 位');
    expect(queued).toContain('取消生成');
  });

  test('终态失败：✕ + 失败原因 + 关闭，不再可取消', async () => {
    const html = await renderPill(
      'zh-CN',
      item({ status: 'failed', step: null, detail: '模型调用超时', elapsedSecs: null })
    );
    expect(html).toContain('✕');
    expect(html).toContain('学习图生长失败');
    expect(html).toContain('模型调用超时');
    expect(html).toContain('关闭');
    expect(html).not.toContain('取消生成');
  });

  test('已在该课程图工作区时不出「回到图工作区」', async () => {
    const html = await renderPill('zh-CN', item(), { withView: false });
    expect(html).not.toContain('回到图工作区');
    expect(html).toContain('取消生成');
  });

  test('en-US 文案同样齐备', async () => {
    const running = await renderPill('en-US', item());
    expect(running).toContain('Growing learning graph · 3:12');
    expect(running).toContain('Coach is drafting this batch of nodes…');
    expect(running).toContain('Open workspace');
    const done = await renderPill('en-US', item({ status: 'done', elapsedSecs: null }));
    expect(done).toContain('Learning graph growth complete');
    expect(done).toContain('Dismiss');
  });
});
