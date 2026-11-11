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
import type { MountObservationView } from '../../types';
import { MountObservationSummary } from './StructureGraphTab';

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
