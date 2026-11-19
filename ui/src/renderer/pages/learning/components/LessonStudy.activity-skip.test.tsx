/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** sq3 AI 批改题跳过（ActivityBlock 接缝）：跳过钮只在 AI 批改题
 * （reflection/open_question）未作答卡出现——客观题不加（防刷门语义，
 * 镜像后端 skip_marker_on_objective_question_is_rejected）；点击跳过
 * 回调携带 `{"skipped":true}` 标记；已跳过态呈中性徽标 + 重答入口
 * （attempt 追加式），danger 红评语与缺失要点块不再出现；文案双语对称
 * 落 activitySkip 语义域（fallback 泄漏红线）。
 * jsdom 宿主：LearningGraphWorkspace.soft-gate.test 同仪式（轻量版——
 * ActivityBlock 只依赖 i18n，无 API/WS 打补丁面）。 */
import * as bunTest from 'bun:test';
const { afterEach, describe, expect, test } = bunTest;
// The pinned bun-types in this checkout predate the afterAll declaration
// (same stale-types gap as LearningGraphWorkspace.soft-gate.test.tsx).
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { ACTIVITY_SKIP_RESPONSE } from '../model';
import type { Activity, AttemptResult } from '../types';
import { ActivityBlock } from './LessonStudy';

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', {
  url: 'http://localhost/',
});
const win = dom.window as unknown as Window & typeof globalThis;
const globals = globalThis as unknown as Record<string, unknown>;
const previousGlobals: Record<string, unknown> = {};
for (const key of [
  'window',
  'document',
  'navigator',
  'HTMLElement',
  'Element',
  'Node',
  'Event',
  'MouseEvent',
  'IS_REACT_ACT_ENVIRONMENT',
] as const) {
  previousGlobals[key] = globals[key];
  globals[key] =
    key === 'IS_REACT_ACT_ENVIRONMENT' ? true : (win as unknown as Record<string, unknown>)[key];
}
// 对照组判错面渲染 QuestionExplanation（ShadowView 读 CSS 变量/挂观察器）：
// soft-gate 宿主同款补丁。
previousGlobals['getComputedStyle'] = globals['getComputedStyle'];
globals['getComputedStyle'] = win.getComputedStyle.bind(win);
previousGlobals['MutationObserver'] = globals['MutationObserver'];
globals['MutationObserver'] = win.MutationObserver;

let root: Root | null = null;
const container = win.document.getElementById('root') as HTMLElement;

afterAll(async () => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  for (const [key, value] of Object.entries(previousGlobals)) {
    if (value === undefined) delete globals[key];
    else globals[key] = value;
  }
});

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

const openQuestion: Activity = {
  id: 'activity-open-1',
  kind: 'open_question',
  prompt: '写一段你自己项目里的代码，并逐值分析它的建立与失效。',
  options: [],
  matches: [],
  section_key: null,
  position: 0,
};

const reflection: Activity = { ...openQuestion, id: 'activity-reflect-1', kind: 'reflection' };

const singleChoice: Activity = {
  id: 'activity-choice-1',
  kind: 'single_choice',
  prompt: '下面哪个说法正确？',
  options: ['甲', '乙', '丙'],
  matches: [],
  section_key: null,
  position: 0,
};

const skippedResult: AttemptResult = {
  id: 'attempt-skip-1',
  score: 0,
  passed: false,
  feedback: 'Skipped: not graded.',
  skipped: true,
};

type Submitted = { activity: Activity; response: unknown };

const mountBlock = async (
  activity: Activity,
  options?: {
    result?: AttemptResult;
    disabled?: boolean;
    locale?: 'zh-CN' | 'en-US';
    onSubmit?: (activity: Activity, response: unknown) => void;
  }
) => {
  const submitted: Submitted[] = [];
  const onSubmit =
    options?.onSubmit ??
    ((callActivity: Activity, callResponse: unknown) => {
      submitted.push({ activity: callActivity, response: callResponse });
    });
  const instance = await initTestI18n(options?.locale ?? 'zh-CN');
  await act(async () => {
    root?.unmount();
    root = createRoot(container);
    root.render(
      <I18nextProvider i18n={instance}>
        <ActivityBlock
          activity={activity}
          disabled={options?.disabled ?? false}
          result={options?.result}
          onSubmit={onSubmit}
        />
      </I18nextProvider>
    );
  });
  return { submitted, instance };
};

const buttonWithText = (text: string): HTMLElement | null => {
  const buttons = Array.from(container.querySelectorAll('button'));
  return buttons.find((button) => button.textContent?.includes(text)) ?? null;
};

const cleanup = async () => {
  if (root) {
    await act(async () => {
      root?.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
};

describe('跳过钮面（sq3 R-b，ActivityBlock 接缝）', () => {
  afterEach(cleanup);

  test('AI 批改题未作答出「跳过」钮，点击提交 {"skipped":true} 标记', async () => {
    const { submitted } = await mountBlock(openQuestion);
    const skip = buttonWithText(learningZh.activitySkipButton);
    expect(skip).not.toBeNull();
    await act(async () => {
      skip!.click();
    });
    expect(JSON.stringify(submitted)).toEqual(
      JSON.stringify([{ activity: openQuestion, response: ACTIVITY_SKIP_RESPONSE }])
    );
  });

  test('reflection 卡同款出钮（两种 AI 批改题型全覆盖）', async () => {
    await mountBlock(reflection);
    expect(buttonWithText(learningZh.activitySkipButton)).not.toBeNull();
  });

  test('客观题不加跳过钮（防刷门语义）', async () => {
    await mountBlock(singleChoice);
    expect(buttonWithText(learningZh.activitySkipButton)).toBeNull();
  });

  test('已作答/禁用卡不再出跳过钮', async () => {
    const graded: AttemptResult = {
      id: 'attempt-graded-1',
      score: 0.8,
      passed: true,
      feedback: '覆盖完整。',
      missing_points: [],
      item_results: [],
    };
    await mountBlock(openQuestion, { result: graded });
    expect(buttonWithText(learningZh.activitySkipButton)).toBeNull();
    await mountBlock(openQuestion, { disabled: true });
    expect(buttonWithText(learningZh.activitySkipButton)).toBeNull();
  });
});

describe('已跳过态与重答入口（sq3 R-b）', () => {
  afterEach(cleanup);

  test('已跳过态：中性徽标 + 提示 + 重答钮；danger 红评语与缺失要点不出现', async () => {
    const failingElsewhere: AttemptResult = {
      id: 'attempt-skip-1',
      score: 0,
      passed: false,
      feedback: '## 评价\n缺失六条要点。',
      missing_points: ['要点一', '要点二'],
    };
    const zh = await mountBlock(openQuestion, { result: failingElsewhere });
    // 对照组（普通判错）：danger 判语 + 缺失要点标题都渲染。
    expect(container.textContent).toContain(learningZh.incorrect);
    expect(container.textContent).toContain(learningZh.missingPointsTitle);
    await cleanup();
    // 已跳过态：中性徽标 + 重答入口；判错面不渲染。
    await mountBlock(openQuestion, { result: skippedResult });
    expect(container.textContent).toContain(learningZh.activitySkippedBadge);
    expect(container.textContent).toContain(learningZh.activitySkippedHint);
    expect(container.textContent).not.toContain(learningZh.incorrect);
    expect(container.textContent).not.toContain(learningZh.missingPointsTitle);
    expect(buttonWithText(learningZh.activitySkipReanswer)).not.toBeNull();
  });

  test('重答入口点开即恢复作答态（attempt 追加式，提交走既有 onSubmit）', async () => {
    await mountBlock(openQuestion, { result: skippedResult });
    const reanswer = buttonWithText(learningZh.activitySkipReanswer);
    expect(reanswer).not.toBeNull();
    await act(async () => {
      reanswer!.click();
    });
    // 重答态下：已跳过徽标退场，跳过钮与提交钮回到未作答形态。
    expect(container.textContent).not.toContain(learningZh.activitySkippedBadge);
    expect(buttonWithText(learningZh.activitySkipButton)).not.toBeNull();
    expect(buttonWithText(learningZh.submit)).not.toBeNull();
  });

  test('文案双语对称：zh-CN 与 en-US 各出本域词条（零裸键）', async () => {
    await mountBlock(openQuestion, { result: skippedResult, locale: 'zh-CN' });
    expect(container.textContent).toContain(learningZh.activitySkippedBadge);
    expect(container.textContent).not.toContain('activitySkipped');
    await cleanup();
    await mountBlock(openQuestion, { result: skippedResult, locale: 'en-US' });
    expect(container.textContent).toContain(learningEn.activitySkippedBadge);
    expect(container.textContent).toContain(learningEn.activitySkipReanswer);
    expect(container.textContent).not.toContain('activitySkipped');
    await cleanup();
    await mountBlock(openQuestion, { locale: 'en-US' });
    expect(buttonWithText(learningEn.activitySkipButton)).not.toBeNull();
  });
});
