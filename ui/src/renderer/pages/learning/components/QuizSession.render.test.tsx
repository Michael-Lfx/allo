/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** QuizCard 题面呈现渲染测试（u3 Acceptance）：题面含 node_name 标注 +
 * difficulty 档位（None 不占位）/ 判分反馈两态（对/错）/ 反馈期间作答禁用。
 * 题面契约（map 红线）：prompt 走 InlineMarkdown（$…$ 出 KaTeX）；题面只渲染
 * QuestionView 字段——判底/解析只随 GradeFeedback 反馈面板出现，作答相零泄漏。
 * ReviewSession.stem.render.test.tsx 同款纪律：renderToStaticMarkup 零 DOM。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import type { QuizGradeFeedback, QuizRoundQuestionView } from '../types';
import { QuizCard } from './QuizSession';

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
