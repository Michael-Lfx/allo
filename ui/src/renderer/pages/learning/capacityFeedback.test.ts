/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 容量反馈展示投影（y07）的单测：有自报才呈卡（零假信号）、旧选择回显、
 * 课程「N 节超载」0 零版面、三选项 i18n 键 + 双语文案齐备。零渲染——
 * 决策逻辑抽在 `capacityFeedback.ts`（systemActions.test.ts 同款纪律）。 */
import { describe, expect, test } from 'bun:test';

import enLearning from '@/renderer/services/i18n/locales/en-US/learning.json';
import zhLearning from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import {
  capacityChoiceKeys,
  capacityChoices,
  capacityNoteText,
  overloadedBadgeCount,
  selectedCapacityChoice,
} from './capacityFeedback';
import type { CapacityFeedbackView, Lesson } from './types';

const lesson = (overrides: Partial<Lesson> = {}): Lesson => ({
  id: 'lesson-1',
  title: '课时',
  summary: '',
  purpose: '',
  intent: '',
  scope: '',
  position: 0,
  generated: true,
  est: 25,
  source: null,
  sources: [],
  status: 'not_started',
  activities: [],
  sections: [],
  ...overrides,
});

const feedback = (choice: CapacityFeedbackView['choice']): CapacityFeedbackView => ({
  choice,
  at: 2000,
});

describe('capacityNoteText', () => {
  test('有自报（trim 后非空）才呈卡', () => {
    expect(capacityNoteText(lesson({ capacity_note: '  第三节超预算  ' }))).toBe(
      '第三节超预算'
    );
  });

  test('缺席/空白自报 = 零假信号（不呈卡）', () => {
    expect(capacityNoteText(lesson())).toBeNull();
    expect(capacityNoteText(lesson({ capacity_note: null }))).toBeNull();
    expect(capacityNoteText(lesson({ capacity_note: '   \n\t ' }))).toBeNull();
  });
});

describe('selectedCapacityChoice', () => {
  test('有留痕则回显旧选择（重载不失忆）', () => {
    expect(selectedCapacityChoice(feedback('extend'))).toBe('extend');
    expect(selectedCapacityChoice(feedback('split'))).toBe('split');
    expect(selectedCapacityChoice(feedback('accept'))).toBe('accept');
  });

  test('无留痕 = 无选中态', () => {
    expect(selectedCapacityChoice(null)).toBeNull();
    expect(selectedCapacityChoice(undefined)).toBeNull();
  });
});

describe('capacityChoices', () => {
  test('三选一固定顺序且全部有 i18n 键', () => {
    expect(capacityChoices).toEqual(['accept', 'extend', 'split']);
    for (const choice of capacityChoices) {
      expect(capacityChoiceKeys[choice].startsWith('learning.capacityFeedback')).toBe(true);
    }
  });
});

describe('overloadedBadgeCount', () => {
  test('0 / 负 / 非数 → 零版面', () => {
    expect(overloadedBadgeCount(0)).toBeNull();
    expect(overloadedBadgeCount(-1)).toBeNull();
    expect(overloadedBadgeCount(undefined)).toBeNull();
    expect(overloadedBadgeCount(Number.NaN)).toBeNull();
  });

  test('正数原样返回「N 节超载」计数', () => {
    expect(overloadedBadgeCount(1)).toBe(1);
    expect(overloadedBadgeCount(4)).toBe(4);
  });
});

const CAPACITY_KEYS = [
  'capacityFeedbackTitle',
  'capacityFeedbackIntro',
  'capacityFeedbackAccept',
  'capacityFeedbackExtend',
  'capacityFeedbackSplit',
  'capacityFeedbackSaved',
  'capacityFeedbackError',
  'capacityFeedbackHint',
  'capacityOverloadedCount',
  'capacityOverloadedHint',
] as const;

describe('容量反馈双语文案', () => {
  test('zh-CN / en-US 对称且非空', () => {
    for (const key of CAPACITY_KEYS) {
      const zh = (zhLearning as Record<string, unknown>)[key];
      const en = (enLearning as Record<string, unknown>)[key];
      expect(typeof zh).toBe('string');
      expect(typeof en).toBe('string');
      expect((zh as string).trim().length).toBeGreaterThan(0);
      expect((en as string).trim().length).toBeGreaterThan(0);
    }
    // 聚合文案的插值占位在两侧都必须保留。
    expect(zhLearning.capacityOverloadedCount).toContain('{{count}}');
    expect(enLearning.capacityOverloadedCount).toContain('{{count}}');
  });
});
