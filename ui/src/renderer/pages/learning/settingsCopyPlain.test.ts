/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** o5 设置页与遗留文案口语化的词典断言：票面 key 双语同改、必改词/病词
 * 在本域 key 内归零、插值占位保留。零渲染——只读两份 learning.json
 * （capacityFeedback.test.ts 同款 seam）；期望值来自票面改写方向，
 * 不重算实现。growthPillStep* / taskKind* 等邻票 key 不在断言内（归 o3）。 */
import { describe, expect, test } from 'bun:test';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';

type LocaleJson = Record<string, unknown>;

const zh = learningZh as unknown as LocaleJson;
const en = learningEn as unknown as LocaleJson;

/** 顶层 key（o5 票面表 + 同域顺手键 courseArchiveDone、o3 移交的挂载段
 * 近邻键、系统动作 kind）。 */
const TOP_LEVEL_KEYS = [
  'deleteCourseConsequenceRecords',
  'courseArchiveDone',
  'questionQueueLegend',
  'capacityFeedbackIntro',
  'capacityFeedbackHint',
  'learningGraphRepairStaleBanner',
  'learningGraphMountCounts',
  'learningGraphMountStale',
  'learningGraphMountRun',
  'learningGraphMountTitle',
  'learningGraphSystemActionKindMount',
  'sourceCoverageFallback',
  'sourceCoverageAnchorUncertain',
] as const;

/** settings.* 段内 key（含新增 tooltip 明细键 wideContentDetail）。 */
const SETTINGS_KEYS = [
  'maintenanceAutoEnabledHint',
  'maintenanceIntervalMinutesHint',
  'autoRefillEnabledHint',
  'fsrsParametersHint',
  'wideContentHint',
  'wideContentDetail',
] as const;

const getCopy = (locale: LocaleJson, key: string): string => {
  const value =
    locale[key] ?? (locale.settings as LocaleJson | undefined)?.[key];
  return typeof value === 'string' ? value : '';
};

// map.md 裁决 3 必改词 + o5 票面病词；只断 o5 域内 key（其他 key 归邻票）。
const ZH_BANNED = [
  '留痕',
  '补挂',
  '回合',
  '环境开关',
  '安排面',
  '挂载',
  '兜底',
  '锚点',
  '自报',
  'px',
];
const EN_BANNED = [
  'arrangement surface',
  'environment switch',
  'mount round',
  'fallback',
  'anchor',
  'px',
];

describe('o5 口语化词典（双语对称）', () => {
  test('票面 key 在 zh/en 都是非空字符串', () => {
    for (const key of TOP_LEVEL_KEYS) {
      expect(getCopy(zh, key).trim().length).toBeGreaterThan(0);
      expect(getCopy(en, key).trim().length).toBeGreaterThan(0);
    }
    for (const key of SETTINGS_KEYS) {
      expect(getCopy(zh, key).trim().length).toBeGreaterThan(0);
      expect(getCopy(en, key).trim().length).toBeGreaterThan(0);
    }
  });

  test('zh 域内 key 禁词归零（留痕/补挂/回合/环境开关/安排面/挂载/兜底/锚点）', () => {
    for (const key of TOP_LEVEL_KEYS) {
      const copy = getCopy(zh, key);
      for (const banned of ZH_BANNED) {
        expect(copy.includes(banned)).toBe(false);
      }
    }
    for (const key of SETTINGS_KEYS) {
      const copy = getCopy(zh, key);
      for (const banned of ZH_BANNED) {
        expect(copy.includes(banned)).toBe(false);
      }
    }
  });

  test('en 域内 key 同病归零（dev 语直译/像素值）', () => {
    for (const key of TOP_LEVEL_KEYS) {
      const copy = getCopy(en, key).toLowerCase();
      for (const banned of EN_BANNED) {
        expect(copy.includes(banned)).toBe(false);
      }
    }
    for (const key of SETTINGS_KEYS) {
      const copy = getCopy(en, key).toLowerCase();
      for (const banned of EN_BANNED) {
        expect(copy.includes(banned)).toBe(false);
      }
    }
  });

  test('learningGraphMountCounts 换用 {{mounted}}/{{rest}} 占位（双语）', () => {
    for (const locale of [zh, en]) {
      const copy = getCopy(locale, 'learningGraphMountCounts');
      expect(copy.includes('{{mounted}}')).toBe(true);
      expect(copy.includes('{{rest}}')).toBe(true);
      expect(copy.includes('mounted_count')).toBe(false);
      expect(copy.includes('outside_count')).toBe(false);
      expect(copy.includes('not_evaluated_count')).toBe(false);
    }
  });

  test('questionQueueLegend 逐状态分行（至少三行，双语）', () => {
    for (const locale of [zh, en]) {
      const lines = getCopy(locale, 'questionQueueLegend').split('\n');
      expect(lines.length).toBeGreaterThanOrEqual(3);
      for (const line of lines) {
        expect(line.trim().length).toBeGreaterThan(0);
      }
    }
  });

  test('sourceCoverage 两键保留 {{count}} 占位（双语）', () => {
    for (const locale of [zh, en]) {
      expect(getCopy(locale, 'sourceCoverageFallback').includes('{{count}}')).toBe(
        true
      );
      expect(
        getCopy(locale, 'sourceCoverageAnchorUncertain').includes('{{count}}')
      ).toBe(true);
    }
  });
});
