/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 挂载观察行渲染（o4 B6）：可见面只出按 kind 的计数（词典模板），后端
 * 观察句（中文模板内嵌 slug/节点串）不再内插 Tag；原始观察句收进「观察
 * 详情」折叠（诊断面如实保留，静态渲染下折叠内容不出现）。中英文案都
 * 从字典取。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type {
  BasisView,
  LearningStructureView,
  MountObservationView,
  StructureMountsView,
  StructureView,
  TerrainView,
} from '../../types';
import { MountObservationSummary, StructureGraphTab } from './StructureGraphTab';

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

/** 后端观察句原样（mount_observations 的 detail 形状：中文模板句，内嵌
 * 节点名/slug/计数——这正是不能内插上屏的原因）。 */
const observations: MountObservationView[] = [
  { kind: 'outside-node', detail: '节点「甲」声明在结构外：纯练习' },
  { kind: 'empty-block', detail: '块「进阶一」（block-21）没有任何节点挂载' },
  { kind: 'empty-block', detail: '块「进阶二」（block-22）没有任何节点挂载' },
  {
    kind: 'overloaded-block',
    detail: '块「基础一」（block-01）挂了 6 个节点（≥5，建议后续修补拆分）：甲、乙、丙、丁、戊、己',
  },
  { kind: 'future-kind', detail: '未来新增的观察种类' },
];

const renderSummary = async (locale: 'zh-CN' | 'en-US'): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <MountObservationSummary observations={observations} />
    </I18nextProvider>
  );
};

describe('MountObservationSummary render (o4 B6)', () => {
  test('可见面只出 kind 计数标签，后端观察句不再内插（zh）', async () => {
    const html = await renderSummary('zh-CN');
    expect(html).toContain('1 个节点不在结构里');
    expect(html).toContain('2 个知识块还没有节点');
    expect(html).toContain('1 个知识块节点偏多');
    expect(html).toContain('其他观察 ×1');
    // detail 是后端中文观察句 + 机器 slug：可见面零直出。
    expect(html).not.toContain('声明在结构外：纯练习');
    expect(html).not.toContain('block-21');
    expect(html).not.toContain('建议后续修补拆分');
    expect(html).not.toContain('未来新增的观察种类');
    // 原始观察句收进「观察详情」折叠（入口在场）。
    expect(html).toContain('观察详情');
  });

  test('en 同口径：计数标签走词典，观察句不上屏', async () => {
    const html = await renderSummary('en-US');
    expect(html).toContain('Nodes not placed in the structure: 1');
    expect(html).toContain('Knowledge blocks without nodes: 2');
    expect(html).toContain('Knowledge blocks with too many nodes: 1');
    expect(html).toContain('Other observations ×1');
    expect(html).toContain('Observation details');
    expect(html).not.toContain('block-21');
    expect(html).not.toContain('声明在结构外');
  });

  test('观察全空：不渲染任何标签与折叠', async () => {
    const instance = await initTestI18n('zh-CN');
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <MountObservationSummary observations={[]} />
      </I18nextProvider>
    );
    expect(html).toBe('');
  });
});

/** ── o6 呈现密度：整 Tab 布局 ──────────────────────────────────────────
 * 双挂载去重（学习 Tab 声明窗口保留整卡为主挂载点，结构图 Tab 降为默认
 * 收起的摘要入口）+ 挂载/修补条收纳（顶条只留结论，stale 指引 / 挂载
 * 索引 / 挂载观察收进单一「详情」折叠）。静态渲染下折叠内容不出现。 */

const STRUCTURE: StructureView = {
  scalars: {
    goal: '',
    goal_type: 'capability',
    proficiency_statement: '',
    domain_type: 'engineering',
    form_reference: '',
    source_tier: 'catalog',
    generated_by: '',
    disclaimer: '',
  },
  domain: {
    core_definition: '',
    includes: [],
    excludes: [],
    prerequisite_doors: [],
    audience: '',
    scenarios: [],
    granularity: '',
  },
  spines: [],
  blocks: [
    {
      name: '项目起步',
      slug: 'block-01',
      spine: '主线一',
      stage: 'basic',
      note: '',
      layer: 0,
      no_prereq: true,
      door_refs: ['G1'],
    },
  ],
  edges: [],
  gaps: [],
  quality: [],
  groups: [],
  block_names: ['项目起步'],
  matrix: [],
  hubs: [],
};

const TERRAIN: TerrainView = {
  learner_basis_assumed: '',
  disclaimer: '',
  doors: [
    { id: 'G1', criterion: '能独立完成从零搭建并运行一个完整项目的全部操作', kind: 'hard', covered_blocks: 1 },
  ],
};

const MOUNTS: StructureMountsView = {
  stale: false,
  entries: [],
  index: [{ block: 'block-01', block_name: '项目起步', nodes: ['节点甲'] }],
  states: [],
  observations: [{ kind: 'outside-node', detail: '节点「乙」声明在结构外：纯练习' }],
  mounted_count: 2,
  outside_count: 1,
  not_evaluated_count: 0,
};

const BASIS_DECLARED: BasisView = {
  course_id: 'course-a',
  doors: ['G1'],
  blocks: [],
  concepts: [
    { concept_id: 'c1', canonical: '概念一', source: 'basis-declaration', declaration_count: 1 },
  ],
  unresolved_concept_ids: [],
  declarations: 1,
  enabled: true,
};

const view = (overrides: Partial<LearningStructureView> = {}): LearningStructureView => ({
  course_id: 'course-a',
  structure: STRUCTURE,
  terrain: TERRAIN,
  generated_at: 1700000000000,
  mounts: MOUNTS,
  ...overrides,
});

const renderTab = async (
  locale: 'zh-CN' | 'en-US',
  current: LearningStructureView,
  options: { basis?: BasisView | null } = {}
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <StructureGraphTab
        view={current}
        basis={options.basis ?? null}
        onMount={() => undefined}
        onRepair={() => undefined}
      />
    </I18nextProvider>
  );
};

describe('StructureGraphTab 布局收敛（o6）', () => {
  test('过门自检卡降为默认收起的摘要入口：标题与计数在场，整卡行不渲染（zh）', async () => {
    const html = await renderTab('zh-CN', view());
    // 摘要入口在场：标题 + 门口计数。
    expect(html).toContain('入门检查');
    expect(html).toContain('1 条入门自测');
    // 整卡默认不渲染（折叠内容静态不出）：判据行 / 门口种类 Tag / 动机
    // Alert / 声明按钮都不出现——学习 Tab 声明窗口仍是整卡主挂载点。
    expect(html).not.toContain('能独立完成从零搭建');
    expect(html).not.toContain('硬前置');
    expect(html).not.toContain('这些自测题对应的知识组');
    expect(html).not.toContain('为什么要声明');
    expect(html).not.toContain('覆盖 1 块');
  });

  test('已声明读数进摘要入口（zh / en）', async () => {
    const zh = await renderTab('zh-CN', view(), { basis: BASIS_DECLARED });
    expect(zh).toContain('已声明 1 项入门检查（覆盖 1 个概念）');
    const en = await renderTab('en-US', view(), { basis: BASIS_DECLARED });
    expect(en).toContain('1 entry checks declared (covering 1 concepts)');
    expect(en).toContain('1 self-check question');
  });

  test('挂载条顶条只留结论：计数与按钮在场，索引/观察/stale 指引收进「详情」折叠', async () => {
    const html = await renderTab('zh-CN', view({ mounts: { ...MOUNTS, stale: true } }));
    // 顶条结论（o5 口径）与动作按钮在场。
    expect(html).toContain('内容与目录对应');
    expect(html).toContain('已对应 2 个，待检查 1 个');
    expect(html).toContain('重新检查');
    expect(html).toContain('运行修补');
    expect(html).toContain('结构已更新，待重新检查');
    // 单一「详情」折叠入口在场。
    expect(html).toContain('详情');
    // 折叠内容静态不出（默认收起）：挂载索引逐行 / 观察计数 Tag /
    // stale 修补指引 / 观察详情二级折叠。
    expect(html).not.toContain('项目起步（1）');
    expect(html).not.toContain('节点甲');
    expect(html).not.toContain('1 个节点不在结构里');
    expect(html).not.toContain('点击「重新检查」');
    expect(html).not.toContain('观察详情');
  });

  test('en 同口径：顶条结论 + 详情折叠（en）', async () => {
    const html = await renderTab('en-US', view());
    expect(html).toContain('Content and outline mapping');
    expect(html).toContain('2 matched, 1 to check');
    expect(html).toContain('Details');
    expect(html).not.toContain('项目起步 (1)');
    expect(html).not.toContain('Nodes not placed in the structure: 1');
  });
});
