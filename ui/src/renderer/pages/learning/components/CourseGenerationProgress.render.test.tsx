/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 学习图建课进度呈现（c1 最小修）：graph 流 running = 单行后台提示（无 v2
 * 步骤条/活动流），completed = 只留课程卡片；v2 描述流/知识库流呈现零变化。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { CourseDetail, CourseKind } from '../types';
import { CourseGenerationProgress } from './CourseGenerationProgress';

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

const noop = () => undefined;

const card = (title: string) =>
  ({
    course: { title, total_lessons: 11 },
    modules: [{}],
  }) as unknown as CourseDetail;

const renderProgress = async (
  locale: 'zh-CN' | 'en-US',
  options: {
    status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
    courseKind?: CourseKind;
    result?: CourseDetail | null;
  }
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <CourseGenerationProgress
        status={options.status}
        courseKind={options.courseKind}
        result={options.result ?? null}
        error={null}
        onStartLearning={noop}
        onRetry={noop}
        onCancel={noop}
      />
    </I18nextProvider>
  );
};

describe('CourseGenerationProgress（学习图流，c1）', () => {
  test('graph + running：单行后台提示 + 取消按钮，无 v2 步骤条/活动流', async () => {
    const html = await renderProgress('zh-CN', { status: 'running', courseKind: 'learning_graph' });
    expect(html).toContain('结构生成已在后台启动，可在课程工作区查看进度');
    expect(html).toContain('取消生成');
    expect(html).not.toContain('准备');
    expect(html).not.toContain('构建大纲');
    expect(html).not.toContain('正在等待生成进度…');
  });

  test('graph + completed：只留课程卡片（无步骤条/无后台提示/无取消）', async () => {
    const html = await renderProgress('zh-CN', {
      status: 'completed',
      courseKind: 'learning_graph',
      result: card('通盘认识股票市场基础'),
    });
    expect(html).toContain('通盘认识股票市场基础');
    expect(html).toContain('开始学习');
    expect(html).not.toContain('准备');
    expect(html).not.toContain('结构生成已在后台启动');
    expect(html).not.toContain('取消生成');
  });

  test('graph + running（en-US）：英文提示齐备', async () => {
    const html = await renderProgress('en-US', { status: 'running', courseKind: 'learning_graph' });
    expect(html).toContain('Structure generation has started in the background');
    expect(html).toContain('Cancel');
  });

  test('v2 流（缺省 courseKind）：步骤条与活动流照旧（零变化）', async () => {
    const html = await renderProgress('zh-CN', { status: 'running' });
    expect(html).toContain('准备');
    expect(html).toContain('构建大纲');
    expect(html).toContain('正在等待生成进度…');
    expect(html).not.toContain('结构生成已在后台启动');
  });
});
