/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 后台生成悬浮 Pill 的静态渲染钉面：
 * - v2 描述流/知识库流五态输出原样钉住（v3 零变化对照——G1 修复不得波及）；
 * - v3 图课程流：label 覆盖（structure 快照真值新键）替换「课程生成中/✓ 生成
 *   完成」面，运行态保留 Spin + 取消；
 * - en-US 文案 + 无宿主 fixed 定位分支（useLearningFloatingSlot 静态渲染取
 *   getServerSnapshot = null）。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { CourseGenerationPill } from './CourseGenerationPill';

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

const renderPill = async (
  locale: 'zh-CN' | 'en-US',
  options: {
    status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
    topic?: string;
    label?: string;
  }
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <CourseGenerationPill
        status={options.status}
        topic={options.topic ?? '通盘认识股票市场基础'}
        label={options.label}
        onView={noop}
        onCancel={noop}
      />
    </I18nextProvider>
  );
};

describe('CourseGenerationPill（v2 流零变化对照：既有五态输出钉住）', () => {
  test('running：课程生成中 + 单 spinner + 查看 + 取消', async () => {
    const html = await renderPill('zh-CN', { status: 'running' });
    expect(html).toContain('课程生成中');
    expect(count(html, SPINNER)).toBe(1);
    expect(count(html, '查看')).toBe(1);
    expect(count(html, '取消生成')).toBe(1);
  });

  test('queued：课程排队中 + 单 spinner + 取消', async () => {
    const html = await renderPill('zh-CN', { status: 'queued' });
    expect(html).toContain('课程排队中');
    expect(count(html, SPINNER)).toBe(1);
    expect(count(html, '取消生成')).toBe(1);
  });

  test('completed：绿 ✓ 生成完成 + 查看，无取消无 spinner（v2 流 HTTP 终态即真终态）', async () => {
    const html = await renderPill('zh-CN', { status: 'completed' });
    expect(html).toContain('生成完成');
    expect(html).toContain('✓');
    expect(count(html, '查看')).toBe(1);
    expect(count(html, SPINNER)).toBe(0);
    expect(html).not.toContain('取消生成');
  });

  test('failed：红 ✕ 生成失败 + 查看，无取消', async () => {
    const html = await renderPill('zh-CN', { status: 'failed' });
    expect(html).toContain('生成失败');
    expect(html).toContain('✕');
    expect(count(html, SPINNER)).toBe(0);
    expect(html).not.toContain('取消生成');
  });

  test('cancelled：橙 ✕ 生成已取消 + 查看，无取消', async () => {
    const html = await renderPill('zh-CN', { status: 'cancelled' });
    expect(html).toContain('生成已取消');
    expect(count(html, SPINNER)).toBe(0);
    expect(html).not.toContain('取消生成');
  });

  test('topic 副行与来源行照旧（多知识库 t4 面不动）', async () => {
    const instance = await initTestI18n('zh-CN');
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <CourseGenerationPill
          status='running'
          topic='通盘认识股票市场基础'
          sources={['库A', '库B']}
          onView={noop}
          onCancel={noop}
        />
      </I18nextProvider>
    );
    expect(html).toContain('通盘认识股票市场基础');
    expect(html).toContain('来源：库A、库B');
  });
});

describe('CourseGenerationPill（v3 图课程流：label 覆盖 = structure 快照真值）', () => {
  test('label 覆盖替换状态词典：新键文本进主行，既有 genBg 文案不出现', async () => {
    const html = await renderPill('zh-CN', {
      status: 'running',
      label: '结构生成中 · 已 07:31',
    });
    expect(html).toContain('结构生成中 · 已 07:31');
    expect(html).not.toContain('课程生成中');
    expect(html).not.toContain('生成完成');
    expect(count(html, SPINNER)).toBe(1);
    expect(count(html, '查看')).toBe(1);
    expect(count(html, '取消生成')).toBe(1);
  });

  test('覆盖只换主行文本：topic 副行与布局不动', async () => {
    const html = await renderPill('zh-CN', {
      status: 'running',
      topic: '通盘认识股票市场基础',
      label: '结构生成排队中',
    });
    expect(count(html, '结构生成排队中')).toBe(1);
    expect(html).toContain('通盘认识股票市场基础');
    expect(count(html, SPINNER)).toBe(1);
  });

  test('无覆盖时完全回退既有词典（默认参数零变化）', async () => {
    const html = await renderPill('zh-CN', { status: 'completed' });
    expect(html).toContain('生成完成');
    expect(html).not.toContain('结构生成中');
  });
});

describe('CourseGenerationPill（en-US 文案 + 定位分支）', () => {
  test('en-US：既有五态文案照旧（零变化）', async () => {
    const running = await renderPill('en-US', { status: 'running' });
    expect(running).toContain('Generating course');
    expect(running).toContain('Cancel');
    const done = await renderPill('en-US', { status: 'completed' });
    expect(done).toContain('Generation complete');
    const failed = await renderPill('en-US', { status: 'failed' });
    expect(failed).toContain('Generation failed');
  });

  test('en-US：label 覆盖文本原样进主行', async () => {
    const html = await renderPill('en-US', {
      status: 'running',
      label: 'Generating structure · 07:31 elapsed',
    });
    expect(html).toContain('Generating structure · 07:31 elapsed');
    expect(html).not.toContain('Generating course');
  });

  test('无宿主插槽：自带 fixed 定位分支（右下角），行为不变', async () => {
    const html = await renderPill('zh-CN', { status: 'running' });
    expect(html).toContain('fixed');
    expect(html).toContain('right-24px');
  });
});
