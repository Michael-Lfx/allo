/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import enLearning from './locales/en-US/learning.json';
import zhLearning from './locales/zh-CN/learning.json';

type LocaleJson = Record<string, unknown>;

/**
 * m1 评审留痕读面（结构图页签「评审留痕」折叠段）：标题 / 计数 / 未读 /
 * 修补已读 / 水位语义提示 / 建议前缀 + 来源与字段机器名的词典映射。
 * check:i18n 只按 en-US 生成类型、不校验 zh-CN 与 en-US 的键对称——缺键
 * 会以裸 i18n 键上屏，生成器抓不到（它们只跟踪键集）。
 */
const FINDINGS_KEYS = [
  'learningGraphFindingsTitle',
  'learningGraphFindingsCount',
  'learningGraphFindingsCountTruncated',
  'learningGraphFindingsUnread',
  'learningGraphFindingsReadByRepair',
  'learningGraphFindingsReadHint',
  'learningGraphFindingsAdvicePrefix',
  // 来源机器名（写侧单源：评审 warn 通道 / 预算留痕）
  'learningGraphFindingsSourceConceptReview',
  'learningGraphFindingsSourceGrowthBudget',
  // 字段机器名（评审 enum + 降级 / 归一通道）
  'learningGraphFindingsFieldTeaches',
  'learningGraphFindingsFieldAssumes',
  'learningGraphFindingsFieldMints',
  'learningGraphFindingsFieldTitle',
  'learningGraphFindingsFieldScope',
  'learningGraphFindingsFieldDifficulty',
  'learningGraphFindingsFieldReview',
] as const;

/** Interpolations each string must keep in BOTH locales. */
const PLACEHOLDERS: readonly [string, readonly string[]][] = [
  ['learningGraphFindingsCount', ['{{count}}']],
  ['learningGraphFindingsCountTruncated', ['{{shown}}', '{{total}}']],
  ['learningGraphFindingsUnread', ['{{count}}']],
];

function getLocaleValue(locale: LocaleJson, key: string): unknown {
  if (Object.prototype.hasOwnProperty.call(locale, key)) return locale[key];

  let cursor: unknown = locale;
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor !== 'object' || !Object.prototype.hasOwnProperty.call(cursor, segment)) {
      return undefined;
    }
    cursor = (cursor as LocaleJson)[segment];
  }
  return cursor;
}

function assertStringKeys(localeName: string, locale: LocaleJson, keys: readonly string[]) {
  const failures: string[] = [];
  for (const key of keys) {
    const value = getLocaleValue(locale, key);
    if (value === undefined) {
      failures.push(`${localeName} missing learning.${key}`);
    } else if (typeof value !== 'string') {
      failures.push(`${localeName} learning.${key} should be a string`);
    } else if (!value.trim()) {
      failures.push(`${localeName} learning.${key} should not be blank`);
    }
  }
  expect(failures).toEqual([]);
}

describe('m1 structure findings locale coverage', () => {
  test('review notes section has copy in both locales', () => {
    assertStringKeys('en-US learning', enLearning as LocaleJson, FINDINGS_KEYS);
    assertStringKeys('zh-CN learning', zhLearning as LocaleJson, FINDINGS_KEYS);
  });

  test('interpolated findings copy keeps its placeholders in both locales', () => {
    const failures: string[] = [];
    for (const [name, locale] of [
      ['en-US', enLearning as LocaleJson],
      ['zh-CN', zhLearning as LocaleJson],
    ] as const) {
      for (const [key, placeholders] of PLACEHOLDERS) {
        const value = String(getLocaleValue(locale, key));
        for (const placeholder of placeholders) {
          if (!value.includes(placeholder)) {
            failures.push(`${name} learning.${key} lost ${placeholder}`);
          }
        }
      }
    }
    expect(failures).toEqual([]);
  });

  // 前缀键自带分隔符（同 conversation.agentError.resolutionPrefix /
  // learning.xpLegendIntensity 先例）：组件把前缀与 advice 原文相邻拼接，
  // en 值丢掉冒号后的空格就会粘成 "Suggestion:xxx"。
  test('advice prefix carries its own separator in both locales', () => {
    expect(getLocaleValue(zhLearning, 'learningGraphFindingsAdvicePrefix')).toBe('建议：');
    expect(getLocaleValue(enLearning, 'learningGraphFindingsAdvicePrefix')).toBe('Suggestion: ');
  });

  // 两份词典逐行对齐是本目录的维护约定（同一键在两份里同行）——键序一致
  // 即行对齐；新键加在一份里漏了另一份、或两份插在不同位置都会在这里红。
  test('zh-CN and en-US learning dictionaries keep the same keys in the same order', () => {
    expect(Object.keys(zhLearning)).toEqual(Object.keys(enLearning));
  });
});
