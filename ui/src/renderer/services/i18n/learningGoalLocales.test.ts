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
 * Goal input (t1) + goal-aware sampling disclosure (t2) copy: the create-course
 * dialog's knowledge-base tab, its required-goal warning, and the course view's
 * sampling line. A missing key renders as a raw i18n key in the UI, which the
 * generators cannot catch (they only track key sets).
 */
const GOAL_KEYS = [
  'courseGoal',
  'courseGoalPlaceholder',
  'courseGoalHint',
  'goalRequired',
  'samplingTitle',
  'samplingLine',
  'samplingLineFallback',
  'samplingCoherence',
  'samplingLowScore',
  'samplingTooltip',
  'samplingTooltipEmpty',
  'samplingTooltipFallback',
] as const;

/** Interpolations each string must keep in BOTH locales. */
const PLACEHOLDERS: readonly [string, readonly string[]][] = [
  ['samplingLine', ['{{sampled}}', '{{total}}']],
  ['samplingLineFallback', ['{{sampled}}', '{{total}}']],
  ['samplingCoherence', ['{{count}}']],
  ['samplingLowScore', ['{{count}}']],
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

describe('learning goal + sampling locale coverage', () => {
  test('goal input and sampling disclosure have copy in both locales', () => {
    assertStringKeys('en-US learning', enLearning as LocaleJson, GOAL_KEYS);
    assertStringKeys('zh-CN learning', zhLearning as LocaleJson, GOAL_KEYS);
  });

  test('interpolated sampling copy keeps its placeholders in both locales', () => {
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
});
