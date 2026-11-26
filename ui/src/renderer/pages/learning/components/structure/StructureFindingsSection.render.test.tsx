/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 评审留痕读面渲染（m1）：行 = 时间 + 节点 + 字段/来源/「修补已读」标记 +
 * issue 原文 + advice 原文（带「建议：」前缀）。中英文案都从字典取；机器名
 * （source / field）已知值走词典、未知值原样透出。折叠段外壳
 * （StructureFindingsSection）静态渲染下只有标题行——折叠内容不出现。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { StructureFindingView, StructureFindingsView } from '../../types';
import { StructureFindingsList, StructureFindingsSection } from './StructureFindingsSection';

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

const finding = (overrides: Partial<StructureFindingView>): StructureFindingView => ({
  seq: 1,
  // ts = 0 → 1970-01-01：任何时区/区域格式下年份恒为 1970（断言不依赖宿主
  // locale 的时间格式）。
  ts: 0,
  source: 'concept-review',
  node: '',
  field: '',
  issue: '留痕原文',
  advice: '',
  read_by_repair: false,
  ...overrides,
});

/** 三条留痕，照 map.md 的三类真实内容取形（登记面 / 预算留痕 / 概念缺口）；
 * 预算留痕的 node / field / advice 恒空。 */
const findings: StructureFindingView[] = [
  finding({
    seq: 3,
    node: 'Option 类型',
    field: 'title',
    issue: '提名别名「Option 类型」与定义不完全对应',
    advice: '拆分别名或分铸',
  }),
  finding({
    seq: 2,
    source: 'growth-budget',
    issue: '生长预算到期：停在第 3 批',
    read_by_repair: true,
  }),
  finding({
    seq: 1,
    node: '容器与接口',
    field: 'teaches',
    issue: '已教概念账中未见容器、接口、元素',
    advice: '补一个节点教它',
    read_by_repair: true,
  }),
];

const renderList = async (locale: 'zh-CN' | 'en-US', rows: StructureFindingView[]): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <StructureFindingsList findings={rows} />
    </I18nextProvider>
  );
};

describe('StructureFindingsList render (m1)', () => {
  test('每条留痕：时间 + 节点 + 字段/来源标记 + issue 原文 + advice 原文（带「建议：」前缀）（zh）', async () => {
    const html = await renderList('zh-CN', findings);
    // 字段标记走词典。
    expect(html).toContain('标题');
    expect(html).toContain('教学');
    // issue / advice 原文逐字上屏；advice 带「建议：」前缀。
    expect(html).toContain('提名别名「Option 类型」与定义不完全对应');
    expect(html).toContain('建议：拆分别名或分铸');
    expect(html).toContain('建议：补一个节点教它');
    expect(html).toContain('已教概念账中未见容器、接口、元素');
    // 来源标记。
    expect(html).toContain('概念评审');
    expect(html).toContain('生长预算');
    // 时间戳与节点上下文。
    expect(html).toContain('1970');
    expect(html).toContain('Option 类型');
    expect(html).toContain('容器与接口');
  });

  test('en 同口径：字段/来源/前缀走词典，原文照出', async () => {
    const html = await renderList('en-US', [
      ...findings,
      finding({ seq: 0, field: 'assumes', issue: 'en 探针', advice: 'split the alias' }),
    ]);
    expect(html).toContain('Title');
    expect(html).toContain('Teaches');
    expect(html).toContain('Assumes');
    expect(html).toContain('Suggestion: split the alias');
    expect(html).toContain('Concept review');
    expect(html).toContain('Growth budget');
    expect(html).toContain('1970');
    // 中文词典文案不回落。
    expect(html).not.toContain('建议：');
    expect(html).not.toContain('概念评审');
    expect(html).not.toContain('教学');
  });

  test('「修补已读」只出现在 read_by_repair = true 的行上', async () => {
    const html = await renderList('zh-CN', findings);
    expect(html.split('修补已读').length - 1).toBe(2);

    const read = await renderList('zh-CN', [finding({ read_by_repair: true })]);
    expect(read.split('修补已读').length - 1).toBe(1);
    const unread = await renderList('zh-CN', [finding({ read_by_repair: false })]);
    expect(unread).not.toContain('修补已读');
    expect(unread).not.toContain('未读');
  });

  test('未知 source / field 原样透出机器名，不出裸词典键', async () => {
    const html = await renderList('zh-CN', [
      finding({ source: 'future-source', field: 'future_field' }),
    ]);
    expect(html).toContain('future-source');
    expect(html).toContain('future_field');
    expect(html).not.toContain('learning.learningGraphFindings');
  });

  // field / source 是评审写的自由文本（无枚举校验）：撞上对象原型成员名
  // （constructor / toString / __proto__ / valueOf）时不得被当成已知值——照旧
  // 原样透出机器名，也不得把函数对象当文案塞上屏（`[native code]`）。
  test('source / field 撞上对象原型成员名：原样透出机器名，不把函数当文案', async () => {
    const machineNames = ['constructor', 'toString', '__proto__', 'valueOf'];
    const html = await renderList('zh-CN', [
      finding({ seq: 0, field: 'constructor', issue: '探针甲' }),
      finding({ seq: 1, field: 'toString', issue: '探针乙' }),
      finding({ seq: 2, field: '__proto__', issue: '探针丙' }),
      finding({ seq: 3, source: 'valueOf', issue: '探针丁' }),
    ]);
    for (const machineName of machineNames) {
      expect(html).toContain(machineName);
    }
    // 原型成员被当成已知值时，`t` 会收到函数：文案里出现函数源码或 undefined。
    expect(html).not.toContain('[native code]');
    expect(html).not.toContain('undefined');
    // 前三行仍是正常的评审来源（source 正常，只有 field 是原型名）。
    expect(html.split('概念评审').length - 1).toBe(3);
  });

  // 评审把 finding 挂在节点意图上（findings[].field 是自由文本，真数据出现过
  // intent）；未处置片段 / 评审问题转留痕 / 术语出处 / 新概念 / 新别名是降级
  // 与归一通道的留痕类别——都该走词典，不把机器名甩给用户。
  test('意图字段与降级 / 归一通道的字段机器名走词典', async () => {
    const machineNames = [
      'intent',
      'fragments',
      'review_problems',
      'term_provenance',
      'new_concepts',
      'new_aliases',
    ];
    const html = await renderList(
      'zh-CN',
      machineNames.map((field, index) => finding({ seq: index, field, issue: `探针 ${index}` })),
    );
    for (const label of ['意图', '片段', '评审问题', '术语出处', '新概念', '新别名']) {
      expect(html).toContain(label);
    }
    for (const machineName of machineNames) {
      expect(html).not.toContain(machineName);
    }
  });

  test('空字段不出字段标记；空 advice 不出建议行', async () => {
    const html = await renderList('zh-CN', [
      finding({ source: 'growth-budget', issue: '生长预算到期：停在第 3 批' }),
    ]);
    for (const label of [
      '教学',
      '前置',
      '铸名',
      '标题',
      '范围',
      '难度',
      '评审',
      '意图',
      '片段',
      '术语出处',
      '新概念',
      '新别名',
    ]) {
      expect(html).not.toContain(label);
    }
    expect(html).not.toContain('建议');
    // 来源标记与原文照常在场（空的是字段，不是整行）。
    expect(html).toContain('生长预算');
    expect(html).toContain('生长预算到期：停在第 3 批');
  });

  test('空列表：不渲染任何内容', async () => {
    const html = await renderList('zh-CN', []);
    expect(html).toBe('');
  });
});

/** ── 折叠段外壳（StructureFindingsSection）：默认折叠，静态渲染下只有标题
 * 行——标题 + 计数 Tag（截断时换「最近 N 条 · 共 M 条」）+ 未读 Tag；内容
 * （提示行 + 行列表）不出。空态整段不渲染；读数失败时标题行仍在。 ── */

const view = (overrides: Partial<StructureFindingsView> = {}): StructureFindingsView => ({
  findings,
  total: findings.length,
  watermark: 0,
  unread: 0,
  ...overrides,
});

const renderSection = async (
  locale: 'zh-CN' | 'en-US',
  props: { view?: StructureFindingsView | null; error?: string | null }
): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <StructureFindingsSection view={props.view ?? null} error={props.error ?? null} />
    </I18nextProvider>
  );
};

describe('StructureFindingsSection render (m1)', () => {
  test('有条目：标题 + 「最近 N 条」在场；未截断不出「共 M 条」，无未读不出未读标记（zh）', async () => {
    const html = await renderSection('zh-CN', { view: view({ unread: 0 }) });
    expect(html).toContain('评审留痕');
    expect(html).toContain('最近 3 条');
    expect(html).not.toContain('共 3 条');
    expect(html).not.toContain('修补未读');
    // 默认折叠：提示行与行内容静态不出。
    expect(html).not.toContain('只表示修补回合读过这条留痕');
    expect(html).not.toContain('提名别名「Option 类型」');
  });

  test('截断 + 有未读：「最近 N 条 · 共 M 条」+「修补未读 K」（zh / en）', async () => {
    const truncated = view({ findings: [findings[0]], total: 16, watermark: 1, unread: 15 });
    const zh = await renderSection('zh-CN', { view: truncated });
    expect(zh).toContain('最近 1 条 · 共 16 条');
    expect(zh).toContain('修补未读 15');
    const en = await renderSection('en-US', { view: truncated });
    expect(en).toContain('Review notes');
    expect(en).toContain('Latest 1 of 16');
    expect(en).toContain('Not read by repair: 15');
  });

  test('未截断 + 有未读：计数照常「最近 N 条」，未读标记在场；不出截断形（en）', async () => {
    const html = await renderSection('en-US', { view: view({ unread: 1 }) });
    expect(html).toContain('Review notes');
    expect(html).toContain('Latest 3');
    expect(html).toContain('Not read by repair: 1');
    expect(html).not.toContain(' of 3');
  });

  test('空态整段不渲染：view 为 null / undefined / 空列表 / 无 props', async () => {
    expect(await renderSection('zh-CN', { view: null })).toBe('');
    expect(await renderSection('zh-CN', { view: undefined })).toBe('');
    expect(
      await renderSection('zh-CN', {
        view: { findings: [], total: 0, watermark: 0, unread: 0 },
      })
    ).toBe('');
    const instance = await initTestI18n('zh-CN');
    expect(
      renderToStaticMarkup(
        <I18nextProvider i18n={instance}>
          <StructureFindingsSection />
        </I18nextProvider>
      )
    ).toBe('');
  });

  test('读数失败：标题行仍在（段落不消失），不出计数标记（zh / en）', async () => {
    const zh = await renderSection('zh-CN', { error: '服务暂时不可用' });
    expect(zh).toContain('评审留痕');
    expect(zh).not.toContain('最近');
    expect(zh).not.toContain('修补未读');
    const en = await renderSection('en-US', { error: 'The service is temporarily unavailable.' });
    expect(en).toContain('Review notes');
    expect(en).not.toContain('Latest');
    expect(en).not.toContain('Not read by repair');
  });
});
