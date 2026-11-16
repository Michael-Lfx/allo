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
import type { ILearningCourseGenerationEvent } from '@/common/adapter/ipcBridge';
import type { CourseDetail, CourseKind } from '../types';
import { CourseGenerationProgress, TriageCompletedFrame } from './CourseGenerationProgress';
import { PHASE } from '../phaseMeta';

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

/** 子串出现次数：钉「只出现一次」这类呈现约束。 */
const count = (html: string, needle: string): number => html.split(needle).length - 1;

/** Arco Spin 根节点标记（静态标记里一个 spinner 一个）。 */
const SPINNER = 'class="arco-spin"';

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
    error?: string | null;
    restored?: boolean;
    /** completed 卡片结构态行（v4）：index 由 structurePillWatch 派生的已
     * 翻译文案；null/缺省 = 不渲染。 */
    structureLine?: string | null;
  }
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <CourseGenerationProgress
        status={options.status}
        courseKind={options.courseKind}
        result={options.result ?? null}
        error={options.error ?? null}
        restored={options.restored ?? false}
        structureLine={options.structureLine ?? null}
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

  test('graph + running：后台提示与取消同处一行——不叠加 v2 运行提示与第二个 spinner', async () => {
    const html = await renderProgress('zh-CN', { status: 'running', courseKind: 'learning_graph' });
    expect(count(html, '结构生成已在后台启动，可在课程工作区查看进度')).toBe(1);
    expect(html).not.toContain('生成需要一些时间');
    expect(count(html, SPINNER)).toBe(1);
    expect(count(html, '取消生成')).toBe(1);
  });

  test('graph + queued：排队提示只出现一次，取消入口保留', async () => {
    const html = await renderProgress('zh-CN', { status: 'queued', courseKind: 'learning_graph' });
    expect(count(html, '任务已提交，正在排队等待空闲算力；排队期间可取消。')).toBe(1);
    expect(count(html, SPINNER)).toBe(1);
    expect(count(html, '取消生成')).toBe(1);
    expect(html).not.toContain('结构生成已在后台启动');
    expect(html).not.toContain('准备');
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

  test('v2 流 running / queued：底部提示行与取消入口照旧（学习图单行收口不波及 v2）', async () => {
    const running = await renderProgress('zh-CN', { status: 'running' });
    expect(running).toContain('生成需要一些时间，请耐心等待；关闭对话框不会终止生成。');
    expect(count(running, '取消生成')).toBe(1);

    const queued = await renderProgress('zh-CN', { status: 'queued' });
    expect(queued).toContain('准备');
    expect(queued).toContain('任务已提交，正在排队等待空闲算力；排队期间可取消。');
    expect(count(queued, '取消生成')).toBe(1);
  });
});

describe('CourseGenerationProgress（completed 卡片结构态行，v4）', () => {
  const LINE = '知识结构生成中 · 已 7:31';

  test('graph + completed + 在跑行：卡片出现「知识结构生成中 · 已 …」secondary 文案行', async () => {
    const html = await renderProgress('zh-CN', {
      status: 'completed',
      courseKind: 'learning_graph',
      result: card('通盘认识股票市场基础'),
      structureLine: LINE,
    });
    expect(count(html, LINE)).toBe(1);
    // 位置：在「N 个模块 · 0 个课时」读数行下方（同一卡片内）
    expect(html.indexOf('个模块')).toBeGreaterThan(-1);
    expect(html.indexOf('个模块')).toBeLessThan(html.indexOf(LINE));
    // 无交互面：该行是纯文案——卡片里仍只有「开始学习」一个按钮
    expect(count(html, '<button')).toBe(1);
    expect(html).toContain('开始学习');
  });

  test('graph + completed + 终态/无行（structureLine null）：结构态行不渲染（本票不刷新卡片）', async () => {
    const html = await renderProgress('zh-CN', {
      status: 'completed',
      courseKind: 'learning_graph',
      result: card('通盘认识股票市场基础'),
      structureLine: null,
    });
    expect(html).not.toContain('知识结构生成中');
    expect(html).toContain('通盘认识股票市场基础');
  });

  test('graph + completed 缺省（不传 structureLine）：零变化（结构态行不渲染）', async () => {
    const html = await renderProgress('zh-CN', {
      status: 'completed',
      courseKind: 'learning_graph',
      result: card('通盘认识股票市场基础'),
    });
    expect(html).not.toContain('知识结构生成中');
  });

  test('v2 流 completed 即使误传 structureLine 也不渲染（仅 learning_graph 渲染，钉死）', async () => {
    const html = await renderProgress('zh-CN', {
      status: 'completed',
      result: card('证券入门知识库课'),
      structureLine: LINE,
    });
    expect(html).toContain('证券入门知识库课');
    expect(html).not.toContain('知识结构生成中');
    expect(count(html, '<button')).toBe(1);
  });
});

describe('TriageCompletedFrame（F4：混合/重复/不确定残余计数收进明细折叠）', () => {
  const counts = {
    total: 8,
    core: 2,
    prerequisite: 1,
    practice: 1,
    mixed: 3,
    noise: 1,
    duplicate: 2,
    uncertain: 5,
  };

  const renderFrame = async (
    locale: 'zh-CN' | 'en-US',
    event: Partial<ILearningCourseGenerationEvent> = {}
  ): Promise<string> => {
    const instance = await initTestI18n(locale);
    return renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <TriageCompletedFrame
          event={{ phase: PHASE.triageCompleted, ...event }}
          index={0}
        />
      </I18nextProvider>
    );
  };

  test('mixed > 0 且无文件列表：主行照常，「详情」折叠在场（mixed 计数不再无处呈现）', async () => {
    const html = await renderFrame('zh-CN', { counts });
    // 主行读数（跳过 = noise + duplicate = 3，票面句直书）。
    expect(html).toContain('已整理 8 个来源文件：核心 2、跳过 3');
    expect(html).toContain('详情');
  });

  test('计数行收进折叠内，不上主面（o3「其余进明细」口径不回潮）', async () => {
    const html = await renderFrame('zh-CN', { counts });
    expect(html).not.toContain('其中：混合 3、重复 2、不确定 5');
  });

  test('三个计数全为 0 且无文件列表：不出「详情」折叠（无空折叠）', async () => {
    const zero = { total: 8, core: 8, prerequisite: 0, practice: 0, mixed: 0, noise: 0, duplicate: 0, uncertain: 0 };
    const html = await renderFrame('zh-CN', { counts: zero });
    expect(html).toContain('已整理 8 个来源文件：核心 8、跳过 0');
    expect(html).not.toContain('详情');
  });
});

describe('CourseGenerationProgress（恢复态终态面板）', () => {
  test('恢复态 failed：本地化指引，不提供重发式重试（合成请求会以 topic 标签为名新建课程）', async () => {
    const zh = await renderProgress('zh-CN', {
      status: 'failed',
      restored: true,
      error: '后台任务已失败。',
    });
    expect(zh).toContain('课程生成失败');
    // o6：错误长文收进「错误详情」折叠 + 「复制详情」按钮——静态渲染下
    // 原文不再平铺上屏。
    expect(zh).not.toContain('后台任务已失败。');
    expect(zh).toContain('错误详情');
    expect(zh).toContain('复制详情');
    expect(zh).toContain('可在课程工作区重新发起。');
    expect(zh).not.toContain('重试');
    expect(zh).not.toContain('按原请求重新发起');

    const en = await renderProgress('en-US', {
      status: 'failed',
      restored: true,
      error: 'The background task failed.',
    });
    expect(en).toContain('Course generation failed');
    expect(en).not.toContain('The background task failed.');
    expect(en).toContain('Error details');
    expect(en).toContain('Copy details');
    expect(en).toContain('You can start a new generation from the course workspace.');
    expect(en).not.toContain('Retry');
  });

  test('恢复态 cancelled：中性说明，不提供重新生成', async () => {
    const zh = await renderProgress('zh-CN', { status: 'cancelled', restored: true });
    expect(zh).toContain('生成已取消');
    expect(zh).toContain('可在课程工作区重新发起。');
    expect(zh).not.toContain('重新生成');
    expect(zh).not.toContain('<button');

    const en = await renderProgress('en-US', { status: 'cancelled', restored: true });
    expect(en).toContain('Generation cancelled');
    expect(en).toContain('You can start a new generation from the course workspace.');
    expect(en).not.toContain('Start over');
    expect(en).not.toContain('<button');
  });

  test('in-session failed / cancelled：重试照旧，错误长文收进折叠（o6）', async () => {
    const failed = await renderProgress('zh-CN', { status: 'failed', error: '网络错误' });
    expect(failed).toContain('重试');
    expect(failed).not.toContain('网络错误');
    expect(failed).toContain('错误详情');
    expect(failed).toContain('复制详情');
    expect(failed).toContain('接着上次进度继续');

    const cancelled = await renderProgress('zh-CN', { status: 'cancelled' });
    expect(cancelled).toContain('重新生成');
    expect(cancelled).toContain('<button');
  });
});
