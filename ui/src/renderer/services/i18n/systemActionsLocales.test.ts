/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import enLearning from './locales/en-US/learning.json';
import zhLearning from './locales/zh-CN/learning.json';
import { typeLabelKeys } from '@/renderer/pages/learning/systemActions';

type LocaleJson = Record<string, unknown>;

/**
 * 系统动作簿的机器名 → 词典 key 单源是 `pages/learning/systemActions.ts` 的
 * `typeLabelKeys`：映射里每一个 key 都必须在两份词典里都有非空文案。check:i18n
 * 只按 en-US 生成类型、不校验 zh-CN 与 en-US 的键对称——映射加了机器名却漏了
 * 一侧词典时，缺键会以裸 i18n 键上屏，生成器抓不到（它们只跟踪键集）。
 *
 * 键清单直接取自映射而不是另抄一份：以后映射再加机器名、词典没跟上，这里
 * 自己就红。
 */
function mappedLeafKeys(): string[] {
  const keys: string[] = [];
  for (const group of Object.values(typeLabelKeys)) {
    for (const key of Object.values(group)) {
      // 映射写的是完整键（learning.<leaf>）；词典文件里是 leaf。
      expect(key.startsWith('learning.')).toBe(true);
      keys.push(key.slice('learning.'.length));
    }
  }
  return keys;
}

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

describe('system action ledger locale coverage', () => {
  test('every machine-name label the mapping can emit has copy in both locales', () => {
    const keys = mappedLeafKeys();
    // 空清单会让下面的断言空过——映射至少有 kind / trigger / result 三组。
    expect(keys.length).toBeGreaterThanOrEqual(3);
    assertStringKeys('en-US learning', enLearning as LocaleJson, keys);
    assertStringKeys('zh-CN learning', zhLearning as LocaleJson, keys);
  });

  // m2：修补后的全量重挂。与生长唤醒的挂载行（「随生长附带」）用不同文案，
  // 动作簿里一眼分得清这次挂载是谁触发的。
  test('repair_followup reads as the post-repair remount in both locales', () => {
    expect(getLocaleValue(zhLearning, 'learningGraphSystemActionTriggerRepairFollowup')).toBe(
      '修补后重挂'
    );
    expect(getLocaleValue(enLearning, 'learningGraphSystemActionTriggerRepairFollowup')).toBe(
      'Remount after repair'
    );
  });

  // 两份词典逐行对齐是本目录的维护约定（同一键在两份里同行）——键序一致
  // 即行对齐；新键加在一份里漏了另一份、或两份插在不同位置都会在这里红。
  test('zh-CN and en-US learning dictionaries keep the same keys in the same order', () => {
    expect(Object.keys(zhLearning)).toEqual(Object.keys(enLearning));
  });
});
