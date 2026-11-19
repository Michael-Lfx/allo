/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** sq5 题删除（activityDelete 语义域）词典单测：入口按钮 / 确认框标题与
 * 提示 / 成功回执 zh-CN / en-US 双语对称且非空、占位两侧对称保留、两侧
 * activityDelete* 键集合一致（quizDictionary.test.ts 同款纪律）。覆盖
 * 票面 Terms R-b 文案面：确认对话框照既有删除惯例 + 账本保留语义如实
 * 告知（作答记录保留）。
 * 集合断言防两侧漂移。零渲染——纯词典断言。 */
import { describe, expect, test } from 'bun:test';

import enLearning from '@/renderer/services/i18n/locales/en-US/learning.json';
import zhLearning from '@/renderer/services/i18n/locales/zh-CN/learning.json';

/** 票面文案面清单逐面落键（sq5 R-b；键序 = 面清单序）。 */
const ACTIVITY_DELETE_KEYS = [
  // 练习轮/题目卡管理位入口按钮
  'activityDelete',
  // 确认对话框（单题级不进危险区：标题/提示两件套）
  'activityDeleteConfirmTitle',
  'activityDeleteConfirmHint',
  // 成功回执
  'activityDeleteDone',
] as const;

const placeholderNames = (text: string): string[] =>
  [...text.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1] as string);

const activityDeleteKeysOf = (dict: Record<string, unknown>): string[] =>
  Object.keys(dict).filter((key) => key.startsWith('activityDelete'));

describe('activityDelete 词典基座（sq5）', () => {
  test('票面全部文案面 zh-CN / en-US 对称且非空', () => {
    for (const key of ACTIVITY_DELETE_KEYS) {
      const zh = (zhLearning as Record<string, unknown>)[key];
      const en = (enLearning as Record<string, unknown>)[key];
      expect(typeof zh).toBe('string');
      expect(typeof en).toBe('string');
      expect((zh as string).trim().length).toBeGreaterThan(0);
      expect((en as string).trim().length).toBeGreaterThan(0);
    }
  });

  test('插值占位在两侧逐键对称保留', () => {
    for (const key of ACTIVITY_DELETE_KEYS) {
      expect(placeholderNames(zhLearning[key])).toEqual(placeholderNames(enLearning[key]));
    }
  });

  test('两侧 activityDelete* 键集合一致（防未来单侧增删漂移）', () => {
    expect([...activityDeleteKeysOf(zhLearning)].sort()).toEqual(
      [...activityDeleteKeysOf(enLearning)].sort()
    );
    // 语义域收口：本票全部键都在票面清单内（无野生 activityDelete 键）。
    const expected = [...ACTIVITY_DELETE_KEYS].sort();
    expect([...activityDeleteKeysOf(zhLearning)].sort()).toEqual(expected);
  });

  test('确认提示如实告知账本语义：作答记录保留（zh/en 同语义）', () => {
    expect(zhLearning.activityDeleteConfirmHint).toContain('作答记录');
    expect(enLearning.activityDeleteConfirmHint).toContain('attempt');
  });
});
