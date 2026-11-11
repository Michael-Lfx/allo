/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 已会声明区口语化的词典契约测试（o2）：票面改写句直书为独立事实源，双语
 * 遍历真实词典 JSON（红线 2）；声明区 key 家族负向断言——zh 侧「门口/铸出/
 * 登记表/幂等/读侧/过门」、en 侧 mint/registry/idempotent/gate/criteria/
 * read-only 在已会声明区归零（票面验收的可执行化，防回潮栅栏）。 */
import { describe, expect, test } from 'bun:test';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';

/** 本票改写 key 的票面固定句（措辞以票面方向为准，微调不保留实现词）。 */
const PLAIN_KEYS: Record<string, { zh: string; en: string }> = {
  learningGraphGateTitle: { zh: '入门检查', en: 'Entry check' },
  learningGraphGateCounts: {
    zh: '{{doors}} 条入门自测',
    en: '{{doors}} self-check questions',
  },
  learningGraphGateHint: {
    zh: '这些自测题对应的知识组：能答上来 = 这部分你已会。',
    en: 'Each self-check question maps to a group of knowledge: if you can answer it, you already know that part.',
  },
  learningGraphGateDoorCoversNone: {
    zh: '暂无对应知识块',
    en: 'No knowledge blocks mapped yet',
  },
  learningGraphBasisConfirmContent: {
    zh: '确认后，勾选的 {{count}} 项内容视为你已掌握：不再生成或安排这部分学习，也不计 XP。声明只能进行一次，确认后不可修改。',
    en: "After you confirm, the {{count}} item(s) you ticked count as already mastered: they won't be generated or scheduled, and no XP is awarded. Declaring is a one-time action and can't be changed afterwards.",
  },
  learningGraphBasisDeclaredDoors: {
    zh: '已声明：{{doors}}（覆盖 {{count}} 个概念）',
    en: 'Declared: {{doors}} (covering {{count}} concepts)',
  },
  learningGraphBasisDeclareNoop: {
    zh: '这些内容之前已声明过，本次没有变化。',
    en: 'These were already declared before — nothing changed this time.',
  },
  learningGraphBasisDeclareDone: {
    zh: '已记录这些内容你已会：{{doors}}；不再安排学习，也不计 XP。对应知识块：{{blocks}}。',
    en: "Saved as already known: {{doors}}; they won't be scheduled and no XP is awarded. Knowledge blocks covered: {{blocks}}.",
  },
  learningGraphBasisOff: {
    zh: '此课程暂不支持声明已会',
    en: "This course doesn't support declaring what you already know yet.",
  },
  learningGraphBasisUnresolved: {
    zh: '{{count}} 条已声明的内容已不在当前课程中',
    en: '{{count}} declared item(s) are no longer in this course',
  },
  learningGraphBasisDoorAria: {
    zh: '勾选你已经掌握的内容',
    en: 'Tick content you have already mastered',
  },
  learningGraphBasisFrozenHint: {
    zh: '已会声明只能进行一次，以下为你的声明结果：已勾选 = 已声明为已会。',
    en: 'Declaring what you already know is a one-time action; below is your declaration — checked means declared as known.',
  },
  learningGraphWindowBody: {
    zh: '开始前可先勾选你已会的内容并确认，这部分不再安排学习；不勾也行，直接开始。',
    en: "Before you start, you can tick what you already know and confirm it — that part won't be scheduled. Ticking is optional; you can also just start.",
  },
  learningGraphStructureReadyToast: {
    zh: '知识结构已生成，已为你打开结构图；可先勾选你已会的内容并声明，再开始学习。',
    en: 'The knowledge structure is ready — opened the structure graph for you; you can tick what you already know and declare it before starting.',
  },
};

/** 已会声明区 key 家族：已会声明（Basis）+ 入门自测卡（Gate）+ 声明窗口
 * （Window）+ 声明后台 toast + 结构就绪 toast 的声明邀请句。 */
const DECLARED_KNOWN_PATTERN =
  /^(learningGraph(?:Basis|Gate|Window)|inflightBasisToast|learningGraphStructureReadyToast)/;

const stripPlaceholders = (value: string) => value.replaceAll(/\{\{[^}]+\}\}/g, '');

describe('已会声明区票面固定句（真实词典 JSON 断言，红线 2）', () => {
  test('改写 key 在 zh-CN 与 en-US 均为票面固定句', () => {
    const zh = learningZh as Record<string, unknown>;
    const en = learningEn as Record<string, unknown>;
    for (const [name, expected] of Object.entries(PLAIN_KEYS)) {
      expect(zh[name]).toBe(expected.zh);
      expect(en[name]).toBe(expected.en);
    }
  });
});

describe('已会声明区负向断言（实现词归零，防回潮栅栏）', () => {
  test('zh-CN 声明区不再出现 门口/铸出/登记表/幂等/读侧/过门', () => {
    const zh = learningZh as Record<string, unknown>;
    const offenders = Object.entries(zh)
      .filter(
        ([name, value]) =>
          DECLARED_KNOWN_PATTERN.test(name) &&
          typeof value === 'string' &&
          /门口|铸出|登记表|幂等|读侧|过门/.test(stripPlaceholders(value))
      )
      .map(([name, value]) => `${name} → ${value}`);
    expect(offenders).toEqual([]);
  });

  test('en-US 声明区不再出现 mint/registry/idempotent/gate/criteria/read-only', () => {
    const en = learningEn as Record<string, unknown>;
    const offenders = Object.entries(en)
      .filter(
        ([name, value]) =>
          DECLARED_KNOWN_PATTERN.test(name) &&
          typeof value === 'string' &&
          /\b(mint\w*|registry|idempotent|gates?|criteria|read-only)\b/i.test(
            stripPlaceholders(value)
          )
      )
      .map(([name, value]) => `${name} → ${value}`);
    expect(offenders).toEqual([]);
  });

  test('声明区 key 集合双语对称', () => {
    const zh = Object.keys(learningZh as Record<string, unknown>)
      .filter((name) => DECLARED_KNOWN_PATTERN.test(name))
      .sort();
    const en = Object.keys(learningEn as Record<string, unknown>)
      .filter((name) => DECLARED_KNOWN_PATTERN.test(name))
      .sort();
    expect(en).toEqual(zh);
  });
});
