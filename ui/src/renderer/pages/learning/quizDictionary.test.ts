/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** quiz i18n 词典基座（u1，u4 扩 + u5 边界态扩）的单测：quiz* 词条 zh-CN /
 * en-US 双语对称且非空、插值占位两侧保留、两侧 quiz* 键集合一致
 * （capacityFeedback.test.ts 同款纪律）。覆盖票面文案面清单：进度读数 /
 * 开测按钮 / 会话标题 / 题源节点标注 / 判分反馈（对·错·判底·解析·证据）/
 * 结算分级三档 + 分级语境句（u4 对称追加）+ low 兜底档 / 折叠不计薪 /
 * 边界态（未 due·缺题·加载失败·组卷等待[u5 R1]·异常回合[u5 R3]）/
 * 结算报告面（u4：总净入账·准确率·逐节点行·完成按钮）。
 * 集合断言防两侧漂移。零渲染——纯词典断言。 */
import { describe, expect, test } from 'bun:test';

import enLearning from '@/renderer/services/i18n/locales/en-US/learning.json';
import zhLearning from '@/renderer/services/i18n/locales/zh-CN/learning.json';

/** 票面文案面清单逐面落键（u1 R3 + u4 报告相；键序 = 面清单序）。 */
const QUIZ_KEYS = [
  // 进度读数：「距上次检测 N/K 次学新结算」形态
  'quizProgressReading',
  // 开测按钮
  'quizStartButton',
  // 会话标题
  'quizSessionTitle',
  // 题源节点标注
  'quizSourceNodeTag',
  // 判分反馈：对 / 错 / 判底 / 解析 / 证据标签
  'quizFeedbackCorrect',
  'quizFeedbackIncorrect',
  'quizAnswerLabel',
  'quizExplanationLabel',
  'quizEvidenceLabel',
  // 结算分级三档（settle.rs QUIZ_EXCELLENT/PASS/HALF_RATIO；前端只镜像）
  'quizGradeExcellent',
  'quizGradePass',
  'quizGradeHalf',
  // 分级语境句（u4 R2：激励/建议口语化语境句，与档位标签成对）
  'quizGradeExcellentHint',
  'quizGradePassHint',
  'quizGradeHalfHint',
  // low 兜底档（u4：< 0.3 = settle.rs else 分支四分之一额的如实呈现）
  'quizGradeLow',
  'quizGradeLowHint',
  // 折叠不计薪（earned_xp = null：同日已结算，coverage 保持不重复计薪）
  'quizFoldedNoXp',
  // 边界态：未 due / 缺题 / 加载失败
  'quizNotDueHint',
  'quizMissingAnswers',
  'quizLoadFailed',
  // u5 R1/R3：组卷等待文案（开测慢路径诚实等待）/ 异常回合如实呈现
  'quizPreparing',
  'quizRoundNotActive',
  // 结算报告面（u4 R1：总净入账 / 准确率 / 逐节点对账行 / 完成）
  'quizReportHeading',
  'quizReportTotalXp',
  'quizReportAccuracy',
  'quizReportRowScore',
  'quizReportRowXp',
  'quizReportFinish',
] as const;

const placeholderNames = (text: string): string[] =>
  [...text.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1] as string);

const quizKeysOf = (dict: Record<string, unknown>): string[] =>
  Object.keys(dict).filter((key) => key.startsWith('quiz'));

describe('quiz 词典基座（u1）', () => {
  test('票面全部文案面 zh-CN / en-US 对称且非空', () => {
    for (const key of QUIZ_KEYS) {
      const zh = (zhLearning as Record<string, unknown>)[key];
      const en = (enLearning as Record<string, unknown>)[key];
      expect(typeof zh).toBe('string');
      expect(typeof en).toBe('string');
      expect((zh as string).trim().length).toBeGreaterThan(0);
      expect((en as string).trim().length).toBeGreaterThan(0);
    }
  });

  test('插值占位在两侧逐键对称保留', () => {
    for (const key of QUIZ_KEYS) {
      expect(placeholderNames(zhLearning[key])).toEqual(placeholderNames(enLearning[key]));
    }
  });

  test('两侧 quiz* 键集合一致（防未来单侧增删漂移）', () => {
    expect([...quizKeysOf(zhLearning)].sort()).toEqual([...quizKeysOf(enLearning)].sort());
  });

  test('票面关键读数占位齐备：N/K 读数、缺题计数', () => {
    expect(zhLearning.quizProgressReading).toContain('{{settled}}');
    expect(zhLearning.quizProgressReading).toContain('{{trigger}}');
    expect(enLearning.quizProgressReading).toContain('{{settled}}');
    expect(enLearning.quizProgressReading).toContain('{{trigger}}');
    expect(zhLearning.quizMissingAnswers).toContain('{{count}}');
    expect(enLearning.quizMissingAnswers).toContain('{{count}}');
    expect(zhLearning.quizSourceNodeTag).toContain('{{node}}');
    expect(enLearning.quizSourceNodeTag).toContain('{{node}}');
    expect(zhLearning.quizNotDueHint).toContain('{{trigger}}');
    expect(enLearning.quizNotDueHint).toContain('{{trigger}}');
  });

  test('结算报告面占位齐备（u4 R1/R2）：总净入账 / 准确率 / 逐节点行', () => {
    expect(zhLearning.quizReportTotalXp).toContain('{{xp}}');
    expect(enLearning.quizReportTotalXp).toContain('{{xp}}');
    expect(zhLearning.quizReportAccuracy).toContain('{{percent}}');
    expect(enLearning.quizReportAccuracy).toContain('{{percent}}');
    expect(zhLearning.quizReportRowScore).toContain('{{answered}}');
    expect(zhLearning.quizReportRowScore).toContain('{{correct}}');
    expect(enLearning.quizReportRowScore).toContain('{{answered}}');
    expect(enLearning.quizReportRowScore).toContain('{{correct}}');
    expect(zhLearning.quizReportRowXp).toContain('{{xp}}');
    expect(enLearning.quizReportRowXp).toContain('{{xp}}');
  });
});
