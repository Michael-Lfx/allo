/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** QuizCard 题面呈现渲染测试（u3 Acceptance）+ QuizReport 结算报告渲染测试
 * （u4 Acceptance：总净入账 + 逐节点对账行 / 折叠态 null 呈现 / 三档分级
 * 边界值 0.9/0.6/0.3 恰好命中档）。题面契约（map 红线）：prompt 走
 * InlineMarkdown（$…$ 出 KaTeX）；题面只渲染 QuestionView 字段——判底/解析
 * 只随 GradeFeedback 反馈面板出现，作答相零泄漏。
 * ReviewSession.stem.render.test.tsx 同款纪律：renderToStaticMarkup 零 DOM。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import type {
  QuizGradeFeedback,
  QuizRoundNodeReport,
  QuizRoundQuestionView,
  QuizRoundReport,
} from '../types';
import {
  QUIZ_EXCELLENT_RATIO,
  QUIZ_HALF_RATIO,
  QUIZ_PASS_RATIO,
  QuizCard,
  QuizReport,
  quizReportGrade,
  quizReportScore,
} from './QuizSession';

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

const noop = () => {};

const makeQuestion = (overrides: Partial<QuizRoundQuestionView> = {}): QuizRoundQuestionView => ({
  activity_id: 'act-1',
  kind: 'single_choice',
  prompt: '下列哪一个是无理数？$\\sqrt{2}$ 还是 $0.5$？',
  options: ['$\\sqrt{2}$', '$0.5$'],
  matches: [],
  node_id: 'node-1',
  node_name: '实数系',
  difficulty: 2,
  ...overrides,
});

const makeFeedback = (overrides: Partial<QuizGradeFeedback> = {}): QuizGradeFeedback => ({
  activity_id: 'act-1',
  correct: true,
  answer: '$\\sqrt{2}$',
  explanation: '$\\sqrt{2}$ 不能写成两整数之比，故为无理数。',
  evidence: null,
  ...overrides,
});

const renderCard = async (
  props: {
    question?: QuizRoundQuestionView;
    response?: unknown;
    feedback?: QuizGradeFeedback | null;
    busy?: boolean;
  },
  locale: 'zh-CN' | 'en-US' = 'zh-CN'
) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n(locale)}>
      <QuizCard
        question={props.question ?? makeQuestion()}
        index={0}
        total={3}
        response={props.response}
        feedback={props.feedback ?? null}
        busy={props.busy ?? false}
        onResponse={noop}
        onSubmit={noop}
        onNext={noop}
      />
    </I18nextProvider>
  );

describe('QuizCard 题面呈现（u3 R2）', () => {
  test('题干走 InlineMarkdown 管线出 KaTeX，题源节点标注如实呈现', async () => {
    const markup = await renderCard({});
    expect(markup).toContain('data-testid="inline-markdown"');
    expect(markup).toContain('class="katex"');
    expect(markup).toContain('下列哪一个是无理数？');
    expect(markup).toContain('题源：实数系');
  });

  test('difficulty 档位呈现（难度 + 档位值）', async () => {
    const markup = await renderCard({});
    expect(markup).toContain('难度');
    expect(markup).toContain('2');
  });

  test('difficulty None（未铸值）不占位：不渲染难度标注', async () => {
    const markup = await renderCard({ question: makeQuestion({ difficulty: undefined }) });
    expect(markup).not.toContain('难度');
  });

  test('作答相零判底泄漏：无反馈面板、无正确答案/解析/证据字样', async () => {
    const markup = await renderCard({ response: '$\\sqrt{2}$' });
    expect(markup).not.toContain('答对了');
    expect(markup).not.toContain('答错了');
    expect(markup).not.toContain('正确答案');
    expect(markup).not.toContain('解析');
    expect(markup).not.toContain('证据');
    // 判分后才有「下一题」
    expect(markup).not.toContain('下一题');
  });

  test('作答相未作答：提交按钮禁用；作答后可提交（input 无 disabled）', async () => {
    const unanswered = await renderCard({});
    expect(unanswered).toMatch(/<button[^>]*disabled/);
    const answered = await renderCard({ response: '$\\sqrt{2}$' });
    expect(answered).not.toMatch(/arco-radio-disabled/);
    expect(answered).toMatch(/arco-btn-primary(?!.*disabled)/s);
  });
});

describe('QuizCard 判分反馈两态（u3 R3）', () => {
  test('correct 态：答对了 + 判底答案 + 解析 + 下一题，零错误面板', async () => {
    const markup = await renderCard({ response: '$\\sqrt{2}$', feedback: makeFeedback() });
    expect(markup).toContain('答对了');
    expect(markup).not.toContain('答错了');
    expect(markup).toContain('正确答案');
    expect(markup).toContain('解析');
    expect(markup).toContain('下一题');
    // 判底答案走 InlineMarkdown（公式出 KaTeX）
    expect(markup.split('class="katex"').length - 1).toBeGreaterThanOrEqual(2);
  });

  test('incorrect 态：答错了 + 判底答案 + 解析，对错标识互斥', async () => {
    const markup = await renderCard({
      response: '$0.5$',
      feedback: makeFeedback({ correct: false }),
    });
    expect(markup).toContain('答错了');
    expect(markup).not.toContain('答对了');
    expect(markup).toContain('正确答案');
    expect(markup).toContain('解析');
  });

  test('证据有则呈现，无则不占位', async () => {
    const withEvidence = await renderCard({
      feedback: makeFeedback({ evidence: '参考：课时《实数的定义》第二节。' }),
    });
    expect(withEvidence).toContain('证据');
    expect(withEvidence).toContain('参考：课时《实数的定义》第二节。');
    const withoutEvidence = await renderCard({ feedback: makeFeedback({ evidence: null }) });
    expect(withoutEvidence).not.toContain('证据');
  });

  test('反馈期作答禁用（R4）：作答控件 disabled、提交按钮消失、推进按钮在位', async () => {
    const markup = await renderCard({ feedback: makeFeedback() });
    expect(markup).toContain('arco-radio-disabled');
    expect(markup).not.toMatch(/<button[^>]*>[\s\S]*?提交答案/);
    expect(markup).toMatch(/<button[^>]*>[\s\S]*?下一题/);
  });

  test('英文主题同一断言成立（反馈两态 + 禁用）', async () => {
    const correct = await renderCard({ feedback: makeFeedback() }, 'en-US');
    expect(correct).toContain('Correct');
    expect(correct).not.toContain('Incorrect');
    const incorrect = await renderCard(
      { feedback: makeFeedback({ correct: false }) },
      'en-US'
    );
    expect(incorrect).toContain('Incorrect');
    expect(incorrect).toContain('arco-radio-disabled');
  });
});

// ── u4：结算报告呈现（R1 对账 + R2 分级） ─────────────────────────────────

const makeNode = (overrides: Partial<QuizRoundNodeReport> = {}): QuizRoundNodeReport => ({
  node_id: 'node-1',
  node_name: '实数系',
  answered: 10,
  correct: 9,
  earned_xp: 30,
  questions: [],
  ...overrides,
});

const makeReport = (
  covered: QuizRoundNodeReport[],
  earnedXp?: number
): QuizRoundReport => ({
  round_id: 'round-1',
  course_id: 'course-1',
  earned_xp: earnedXp ?? covered.reduce((sum, node) => sum + (node.earned_xp ?? 0), 0),
  covered,
});

const renderReport = async (
  report: QuizRoundReport,
  locale: 'zh-CN' | 'en-US' = 'zh-CN'
) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={await initTestI18n(locale)}>
      <QuizReport report={report} onFinish={noop} />
    </I18nextProvider>
  );

describe('QuizReport 结算报告呈现（u4 R1）', () => {
  test('总净入账 + 准确率 + 逐节点对账行（node_name/answered/correct/earned_xp）+ 完成按钮', async () => {
    const markup = await renderReport(makeReport([makeNode()]));
    expect(markup).toContain('本卷结算');
    expect(markup).toContain('净入账 30 XP');
    expect(markup).toContain('准确率 90%');
    expect(markup).toContain('实数系');
    expect(markup).toContain('作答 10 · 答对 9');
    expect(markup).toContain('+30 XP');
    expect(markup).toMatch(/<button[^>]*>[\s\S]*?完成/);
  });

  test('折叠态 earned_xp=null 呈「同日已结算 · 不计薪」，绝不渲染 0 或空', async () => {
    const markup = await renderReport(
      makeReport(
        [
          makeNode({ node_id: 'node-1', node_name: '实数系', answered: 2, correct: 2, earned_xp: null }),
          makeNode({ node_id: 'node-2', node_name: '虚数系', answered: 1, correct: 1, earned_xp: 12 }),
        ],
        12
      )
    );
    // 折叠行：整句如实呈现，coverage 行保持（作答/答对照常可读）
    expect(markup).toContain('同日已结算，本次覆盖保持但不重复计薪');
    expect(markup).toContain('作答 2 · 答对 2');
    // 不渲染成 0 或空：零「+0 XP」，计薪节点照常 +12 XP
    expect(markup).not.toContain('+0 XP');
    expect(markup).toContain('+12 XP');
    // 总净入账 = 后端 Σ（折叠不计薪已扣），前端原样呈现不重算
    expect(markup).toContain('净入账 12 XP');
  });

  test('逐题校准点 questions[] 不呈现（已落盘喂校准面，R3 裁决记录制）', async () => {
    const markup = await renderReport(
      makeReport([
        makeNode({
          questions: [{ activity_id: 'cal-q1', correct: true, predicted_r: 0.42 }],
        }),
      ])
    );
    expect(markup).not.toContain('0.42');
    expect(markup).not.toContain('cal-q1');
  });

  test('英文主题同一断言成立（总入账/准确率/行/完成）', async () => {
    const markup = await renderReport(makeReport([makeNode()]), 'en-US');
    expect(markup).toContain('Round settlement');
    expect(markup).toContain('Net XP 30');
    expect(markup).toContain('Accuracy 90%');
    expect(markup).toContain('10 answered · 9 correct');
    expect(markup).toContain('+30 XP');
    expect(markup).toMatch(/<button[^>]*>[\s\S]*?Done/);
  });
});

describe('QuizReport 分级边界（u4 R2：镜像 settle.rs 0.9/0.6/0.3，恰好命中档）', () => {
  test('quizReportGrade 边界值直测：0.9/0.6/0.3 恰好命中档，<0.3 落 low 兜底', () => {
    expect(quizReportGrade(9, 10)).toBe('excellent');
    expect(quizReportGrade(6, 10)).toBe('pass');
    expect(quizReportGrade(3, 10)).toBe('half');
    expect(quizReportGrade(2, 10)).toBe('low');
    expect(quizReportGrade(0, 10)).toBe('low');
    // 零作答 = 契约外形态（settle.rs fail loud），不呈现分级
    expect(quizReportGrade(0, 0)).toBeNull();
    // Σcovered 聚合口径：分子分母跨节点求和
    expect(quizReportScore([makeNode(), makeNode({ answered: 10, correct: 8 })])).toEqual({
      answered: 20,
      correct: 17,
    });
  });

  test('准确率 0.9 恰好命中 excellent：优秀 + 激励语境句', async () => {
    const markup = await renderReport(makeReport([makeNode({ answered: 10, correct: 9 })]));
    expect(markup).toContain('优秀');
    expect(markup).toContain('几乎全对');
    expect(markup).not.toContain('通过');
    expect(markup).not.toContain('半额入账');
  });

  test('准确率 0.6 恰好命中 pass：通过', async () => {
    const markup = await renderReport(makeReport([makeNode({ answered: 10, correct: 6 })]));
    expect(markup).toContain('通过');
    expect(markup).not.toContain('优秀');
    expect(markup).not.toContain('半额入账');
  });

  test('准确率 0.3 恰好命中 half：半额入账', async () => {
    const markup = await renderReport(makeReport([makeNode({ answered: 10, correct: 3 })]));
    expect(markup).toContain('半额入账');
    expect(markup).not.toContain('优秀');
    expect(markup).not.toContain('需巩固');
  });

  test('跨节点聚合：17/20 = 0.85 落 pass（Σcorrect/Σanswered 全卷口径）', async () => {
    const markup = await renderReport(
      makeReport([makeNode({ answered: 10, correct: 9 }), makeNode({ answered: 10, correct: 8 })])
    );
    expect(markup).toContain('通过');
    expect(markup).toContain('准确率 85%');
  });

  test('英文主题分级镜像（0.9 → Excellent / 0.3 → Half credit）', async () => {
    const excellent = await renderReport(
      makeReport([makeNode({ answered: 10, correct: 9 })]),
      'en-US'
    );
    expect(excellent).toContain('Excellent');
    const half = await renderReport(
      makeReport([makeNode({ answered: 10, correct: 3 })]),
      'en-US'
    );
    expect(half).toContain('Half credit');
  });
});

// ── u5 修复：分级阈值钉值（R5）。容器级呈现（u5 R1 loading 相 / R2 冲突
// 接线 / R3 异常回合面）在 QuizSession.container.render.test.tsx——Arco
// Modal 走 Portal，renderToStaticMarkup 渲染不出内容（零字节），须 jsdom
// 宿主。 ──────────────────────────────────────────────────────────────

describe('分级阈值钉值（u5 R5：与 settle.rs 单源注释双保险防漂移）', () => {
  test('QUIZ_EXCELLENT/PASS/HALF_RATIO 钉 0.9/0.6/0.3，quizReportGrade 与钉值联动', () => {
    expect(QUIZ_EXCELLENT_RATIO).toBe(0.9);
    expect(QUIZ_PASS_RATIO).toBe(0.6);
    expect(QUIZ_HALF_RATIO).toBe(0.3);
    // 联动钉值：档位判定消费的就是这三个 export 常量（镜像 settle.rs）
    expect(quizReportGrade(QUIZ_EXCELLENT_RATIO * 10, 10)).toBe('excellent');
    expect(quizReportGrade(QUIZ_PASS_RATIO * 10, 10)).toBe('pass');
    expect(quizReportGrade(QUIZ_HALF_RATIO * 10, 10)).toBe('half');
    expect(quizReportGrade(QUIZ_HALF_RATIO * 10 - 1, 10)).toBe('low');
  });
});
