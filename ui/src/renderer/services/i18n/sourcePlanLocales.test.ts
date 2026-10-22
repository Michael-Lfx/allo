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
 * t5 source-plan surfaces: the modern generation stepper (triage → source
 * plan → expansion), the cost-gate confirmation modal, and the course-detail
 * "Source coverage" card (SourcePlan / SourceCoverage vocabulary — never the
 * learning-graph Structure / Coverage Gauge). A missing key renders as a raw
 * i18n key in the UI, which the generators cannot catch (they only track key
 * sets).
 */
const SOURCE_PLAN_KEYS = [
  // 现代管线步骤条（知识库流）：准备 → 来源分诊 → 来源结构计划 → 课时展开 → 完成
  'genStepTriage',
  'genStepSourcePlan',
  'genStepExpansion',
  // triage_completed 帧的读数与可展开明细
  'genTriageLine',
  'genTriageDegraded',
  'genTriageExcludedTitle',
  'genTriageDuplicatesTitle',
  'genTriageUncertainTitle',
  'genUncertainTooMany',
  // source_plan_ready / coverage_ready 帧
  'genSourcePlanReady',
  'genCoverageReady',
  // 成本门确认弹窗
  'costGateTitle',
  'costGateConfirm',
  'costGateCancel',
  'costGateHint',
  'costGateFiles',
  'costGateBatches',
  'costGateCalls',
  'costGateDuration',
  // 课程详情「来源覆盖」卡
  'sourceCoverageTitle',
  'sourceCoverageUnavailable',
  'sourceCoverageTopics',
  'sourceCoverageDestinations',
  'sourceCoverageMixed',
  'sourceCoverageAnchorUncertain',
  'sourceCoverageFallback',
  'sourceCoverageDuplicates',
  'sourceCoverageSkippedTopics',
  'sourceCoverageUncertainTopics',
  'sourceCoverageGaps',
  'sourceCoverageWarnings',
  'sourceCoverageExcludedFiles',
  'sourceCoverageChaptersTitle',
  'sourceCoverageChapterLine',
  'sourceCoverageChapterSkipped',
] as const;

/** Interpolations each string must keep in BOTH locales. */
const PLACEHOLDERS: readonly [string, readonly string[]][] = [
  ['genTriageLine', ['{{total}}', '{{core}}', '{{mixed}}', '{{skipped}}', '{{duplicate}}', '{{uncertain}}']],
  ['genUncertainTooMany', ['{{count}}']],
  ['genSourcePlanReady', ['{{chapters}}', '{{concepts}}']],
  ['genCoverageReady', ['{{covered}}', '{{skipped}}', '{{fallback}}']],
  ['costGateFiles', ['{{count}}']],
  ['costGateBatches', ['{{count}}']],
  ['costGateCalls', ['{{count}}']],
  ['costGateDuration', ['{{minutes}}']],
  ['sourceCoverageTopics', ['{{covered}}', '{{total}}', '{{skipped}}']],
  ['sourceCoverageDestinations', ['{{prereq}}', '{{practice}}']],
  ['sourceCoverageMixed', ['{{count}}']],
  ['sourceCoverageAnchorUncertain', ['{{count}}']],
  ['sourceCoverageFallback', ['{{count}}']],
  ['sourceCoverageDuplicates', ['{{count}}']],
  ['sourceCoverageExcludedFiles', ['{{count}}']],
  ['sourceCoverageChaptersTitle', ['{{count}}']],
  ['sourceCoverageChapterLine', ['{{title}}', '{{lessons}}', '{{sources}}']],
  ['sourceCoverageChapterSkipped', ['{{reason}}']],
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

describe('t5 source plan locale coverage', () => {
  test('stepper, cost gate and source coverage card have copy in both locales', () => {
    assertStringKeys('en-US learning', enLearning as LocaleJson, SOURCE_PLAN_KEYS);
    assertStringKeys('zh-CN learning', zhLearning as LocaleJson, SOURCE_PLAN_KEYS);
  });

  test('interpolated source-plan copy keeps its placeholders in both locales', () => {
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
