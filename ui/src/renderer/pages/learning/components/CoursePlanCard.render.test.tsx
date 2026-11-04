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
import type { CoursePlan } from '../coursePlan';
import type { CourseGenerationEstimateView, GenerateCourseRequest } from '../types';
import { CoursePlanCard } from './CoursePlanCard';

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

const plan = (
  tab: CoursePlan['tab'],
  request: Partial<GenerateCourseRequest> = {}
): CoursePlan => ({
  tab,
  request: { teaching_style: 'standard', provider_id: 'provider-a', model: 'model-a', ...request },
  estimateKey: tab === 'base' ? 'key-1' : null,
});

const estimate = (
  overrides: Partial<CourseGenerationEstimateView> = {}
): CourseGenerationEstimateView => ({
  total_files: 12,
  estimated_batches: 3,
  estimated_llm_calls: 5,
  estimated_seconds: 300,
  needs_confirm: false,
  triage: {
    total_files: 12,
    core: 10,
    prerequisite: 0,
    practice: 0,
    mixed: 1,
    noise: 1,
    duplicate: 0,
    uncertain: 0,
    degraded: true,
  },
  skipped_examples: [],
  ...overrides,
});

const renderCard = async (
  target: CoursePlan,
  view: Parameters<typeof CoursePlanCard>[0]['estimate'],
  sources: string[] = [],
  locale: 'zh-CN' | 'en-US' = 'zh-CN'
): Promise<string> => {
  const i18n = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <CoursePlanCard
        plan={target}
        sources={sources}
        estimate={view}
        onBack={() => {}}
        onConfirm={() => {}}
      />
    </I18nextProvider>
  );
};

describe('建课计划卡（y05）：四槽回显 + 确定性读数 + 两个动作钮', () => {
  test('KB 流回显四槽与估算读数，needs_confirm 在卡内醒目提示', async () => {
    const markup = await renderCard(
      plan('base', { knowledge_base_ids: ['kb-1', 'kb-2'], goal: '掌握 Rust 异步' }),
      { kind: 'ready', estimate: estimate({ needs_confirm: true }) },
      ['金融笔记', '算法笔记']
    );

    // 四槽回显
    expect(markup).toContain('方式 / 来源');
    expect(markup).toContain('金融笔记、算法笔记');
    expect(markup).toContain('目标 / 简报');
    expect(markup).toContain('掌握 Rust 异步');
    expect(markup).toContain('讲解风格');
    expect(markup).toContain('标准');
    expect(markup).toContain('模型');
    expect(markup).toContain('model-a');
    // 确定性读数（四个数字 + 分诊）
    expect(markup).toContain('文件数：12');
    expect(markup).toContain('预计批次：3 批');
    expect(markup).toContain('预计模型调用：约 5 次');
    expect(markup).toContain('预计耗时：约 300 秒（粗估）');
    expect(markup).toContain('扫描 12 个文件');
    // 成本门并入计划卡：超阈值提示在场，且不再有独立弹窗/继续按钮
    expect(markup).toContain('规模较大');
    expect(markup).not.toContain('确认生成成本');
    // 两个动作钮
    expect(markup).toContain('返回修改');
    expect(markup).toContain('开始生成');
  });

  test('KB 估算失败：明示不可得，确认按钮仍在（fail-open）', async () => {
    const markup = await renderCard(plan('base', { knowledge_base_ids: ['kb-1'], goal: '掌握 Rust' }), {
      kind: 'failed',
    });
    expect(markup).toContain('估算不可得，仍可继续');
    expect(markup).not.toContain('预计批次');
    expect(markup).toContain('开始生成');
    expect(markup).toMatch(/<button[^>]*><span>开始生成<\/span><\/button>/);
  });

  test('KB 估算中：明示估算中，不显示假数字', async () => {
    const markup = await renderCard(plan('base', { knowledge_base_ids: ['kb-1'], goal: '掌握 Rust' }), {
      kind: 'loading',
    });
    expect(markup).toContain('正在估算规模');
    expect(markup).not.toContain('预计批次');
  });

  test('描述流：按简报酌定，不显示任何数字区间', async () => {
    const markup = await renderCard(
      plan('description', { description: '从零学会理财' }),
      { kind: 'not_applicable' }
    );
    expect(markup).toContain('按简报酌定');
    expect(markup).not.toContain('预计批次');
    expect(markup).not.toContain('预计模型调用');
    expect(markup).not.toContain('预计耗时');
    expect(markup).not.toContain('文件数');
  });

  test('学习图流：按声明酌定 + 程度/基础声明是否在场，不虚构规模', async () => {
    const markup = await renderCard(
      plan('graph', {
        description: '能独立搭一个 Tauri 应用',
        proficiency_statement: '能独立发布桌面应用',
      }),
      { kind: 'not_applicable' }
    );
    expect(markup).toContain('按声明酌定');
    expect(markup).toContain('程度声明（可选）：已填写');
    expect(markup).toContain('基础声明（可选）：未填写');
    expect(markup).not.toContain('预计批次');
    expect(markup).not.toContain('预计模型调用');
  });

  test('空槽显示「未指定」；双语均不泄露原始 i18n 键', async () => {
    for (const locale of ['zh-CN', 'en-US'] as const) {
      const markup = await renderCard(
        // 直接构造一个缺字段的计划，覆盖空槽渲染
        { tab: 'description', request: {}, estimateKey: null },
        { kind: 'not_applicable' },
        [],
        locale
      );
      expect(markup).toContain(locale === 'zh-CN' ? '未指定' : 'Not specified');
      expect(markup).not.toContain('learning.plan');
      expect(markup).not.toContain('learning.costGate');
      expect(markup).not.toContain('learning.genTriage');
    }
  });
});
