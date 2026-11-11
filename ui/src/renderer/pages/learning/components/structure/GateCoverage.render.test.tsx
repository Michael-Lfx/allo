/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 过门自检卡渲染（o4 B1/B2）：门口 Tag 出判据摘要句，机器 id 退到 aria 与
 * Tooltip，不再直出；已声明门口行只报数量，不再拼接 id 串；确认弹窗不再
 * 内插门口 id 列表（源面契约断言）。中英文案都从字典取。 */
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { BasisView, StructureView, TerrainView } from '../../types';
import { criterionSummary, GateCoverage } from './GateCoverage';

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

const LONG_CRITERION_ZH = '能独立完成从零搭建并运行一个完整项目的全部操作';
const LONG_CRITERION_EN = 'Can set up and run a complete project from scratch without help';

const terrain = (doors: Partial<{ id: string; criterion: string; kind: 'hard' | 'self_check' }>[] = []): TerrainView => ({
  learner_basis_assumed: '',
  disclaimer: '',
  doors: doors.map((door, index) => ({
    id: door.id ?? `G${index + 1}`,
    criterion: door.criterion ?? LONG_CRITERION_ZH,
    kind: door.kind ?? 'hard',
    covered_blocks: 1,
  })),
});

const structure = (doorRefs: string[] = ['G1']): StructureView => ({
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
      door_refs: doorRefs,
    },
  ],
  edges: [],
  gaps: [],
  quality: [],
  groups: [],
  block_names: ['项目起步'],
  matrix: [],
  hubs: [],
});

const basis = (overrides: Partial<BasisView> = {}): BasisView => ({
  course_id: 'course-a',
  doors: [],
  blocks: [],
  concepts: [],
  unresolved_concept_ids: [],
  declarations: 0,
  enabled: true,
  ...overrides,
});

const renderCard = async (
  locale: 'zh-CN' | 'en-US',
  props: {
    terrain: TerrainView;
    structure: StructureView;
    basis?: BasisView | null;
    interactive?: boolean;
  }
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <GateCoverage
        structure={props.structure}
        terrain={props.terrain}
        basis={props.basis ?? null}
        checked={[]}
        onCheckedChange={props.interactive ? () => undefined : undefined}
        onDeclare={props.interactive ? () => undefined : undefined}
      />
    </I18nextProvider>
  );
};

describe('GateCoverage render (o4)', () => {
  test('门口 Tag 出判据摘要句：机器 id 不再直出（zh）', async () => {
    const html = await renderCard('zh-CN', {
      terrain: terrain([{ id: 'G1', criterion: LONG_CRITERION_ZH }]),
      structure: structure(['G1']),
      interactive: true,
    });
    expect(html).toContain('能独立完成从零搭建并运行一个完整项目的全…');
    expect(html).not.toContain('>G1<');
    expect(html).toContain('硬前置');
    // id 保留在 aria（可访问性面不丢机器键）。
    expect(html).toContain('aria-label');
    expect(html).toContain('G1');
  });

  test('en 同口径：判据摘要上屏，id 不直出', async () => {
    const html = await renderCard('en-US', {
      terrain: terrain([{ id: 'G1', criterion: LONG_CRITERION_EN }]),
      structure: structure(['G1']),
      interactive: true,
    });
    expect(html).toContain('Can set up and run a…');
    expect(html).not.toContain('>G1<');
  });

  test('已声明门口行只报数量：不再拼接 id 串（zh）', async () => {
    const html = await renderCard('zh-CN', {
      terrain: terrain([{ id: 'G1' }, { id: 'G2' }]),
      structure: structure(['G1']),
      basis: basis({
        doors: ['G1', 'G2'],
        concepts: [
          { concept_id: 'c1', canonical: '概念一', source: 'basis-declaration', declaration_count: 1 },
          { concept_id: 'c2', canonical: '概念二', source: 'basis-declaration', declaration_count: 1 },
          { concept_id: 'c3', canonical: '概念三', source: 'basis-declaration', declaration_count: 1 },
        ],
        declarations: 1,
      }),
    });
    expect(html).toContain('已声明 2 项入门检查（覆盖 3 个概念）');
    expect(html).not.toContain('已声明门口');
    expect(html).not.toContain('G1、G2');
    expect(html).not.toContain('>G1<');
  });

  test('en 已声明行同口径', async () => {
    const html = await renderCard('en-US', {
      terrain: terrain([{ id: 'G1' }]),
      structure: structure(['G1']),
      basis: basis({
        doors: ['G1'],
        concepts: [
          { concept_id: 'c1', canonical: 'c1', source: 'basis-declaration', declaration_count: 1 },
        ],
        declarations: 1,
      }),
    });
    expect(html).toContain('1 entry checks declared (covering 1 concepts)');
  });

  test('criterionSummary：超长截断加省略号，码点安全，短句原样', () => {
    expect(criterionSummary('短句')).toBe('短句');
    expect(criterionSummary(LONG_CRITERION_ZH)).toBe('能独立完成从零搭建并运行一个完整项目的全…');
    expect(criterionSummary(LONG_CRITERION_ZH).length).toBeLessThanOrEqual(21);
    // 代理对不被劈开：emoji 不会被截成半个码点。
    const emoji = '😀'.repeat(25);
    const summary = criterionSummary(emoji);
    expect(summary.endsWith('…')).toBe(true);
    expect(Array.from(summary.slice(0, -1)).every((ch) => ch === '😀')).toBe(true);
  });

  test('源面契约：确认弹窗与已声明行不再 join 门口 id', () => {
    const source = readFileSync(new URL('./GateCoverage.tsx', import.meta.url), 'utf8');
    expect(source).not.toContain('pendingSelection.join');
    expect(source).not.toContain('basis.doors.join');
  });
});
