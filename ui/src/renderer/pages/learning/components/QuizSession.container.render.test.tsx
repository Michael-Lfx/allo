/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** QuizSession 容器级呈现测试（u5 修复 R1/R2/R3）。Arco Modal 走 Portal，
 * renderToStaticMarkup 渲染不出内容（零字节）——容器面须 jsdom 宿主
 * （QuizEntryCard.render.test 同仪式）；QuizCard/QuizReport 的 props 面与
 * 阈值钉值留在 QuizSession.render.test（SSR 探针）。覆盖：
 * - R1：loading 相（开测在途）Spin 旁呈现组卷等待文案（zh/en 双语）；
 * - R2：start 409（due 门竞态）→ 容器 onConflict/onClose 双通道各恰一次
 *   （index.tsx 据此 bump quizProgressToken + 卸载），人面 toast =
 *   quizNotDueHint（K 提取自 Conflict 报文）；
 * - R3：异常回合（completed/abandoned）不进作答流——error 相如实呈现
 *   quizRoundNotActive 专用文案 + 重试/关闭按钮（出口 = onClose）。
 * 开测数据源经 learningApi.quizStart 逐用例打补丁并还原（HTTP 边界，
 * useCourseLearning.test 同纪律）；提示经 notificationStore 快照求差观察。 */
import * as bunTest from 'bun:test';
const { afterEach, describe, expect, test } = bunTest;
// The pinned bun-types in this checkout predate the afterAll declaration
// (same stale-types gap as SpeechInputButton.dictationActivation.test.tsx).
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { AppMessage, notificationStore } from '@/renderer/components/notifications';
import { BackendHttpError } from '@/common/adapter/httpBridge';
import { learningApi } from '../api';
import type { QuizRoundView } from '../types';

// ── jsdom 宿主：容器开测/状态机经 effects 驱动，SSR 探针不可用 ─────────────
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
  'HTMLIFrameElement',
  'Element',
  'Node',
  'Event',
  'MouseEvent',
  'localStorage',
  'sessionStorage',
  'getComputedStyle',
  'requestAnimationFrame',
  'cancelAnimationFrame',
  'IS_REACT_ACT_ENVIRONMENT',
] as const) {
  previousGlobals[key] = globals[key];
  globals[key] =
    key === 'getComputedStyle'
      ? win.getComputedStyle.bind(win)
      : key === 'IS_REACT_ACT_ENVIRONMENT'
        ? true
        : (win as unknown as Record<string, unknown>)[key];
}
const raf = (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 16) as unknown as number;
globals.requestAnimationFrame = raf;
globals.cancelAnimationFrame = (id: number) => clearTimeout(id);

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

/** 用例间复位：Modal 内容 Portal 到 document.body，须整树卸载防串场 */
const reset = async () => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  win.document.body.innerHTML = '';
};

/** 挂载会话容器：quizStartPatch = 本用例的开测数据源（HTTP 边界替身），
 * onConflict/onClose 挂 spy 计数（index.tsx 接线的替身）。 */
const mountSession = async (
  locale: 'zh-CN' | 'en-US',
  quizStartPatch: () => Promise<QuizRoundView>,
  spies: { onConflict?: () => void; onClose?: () => void } = {}
) => {
  await reset();
  learningApi.quizStart = quizStartPatch;
  const instance = await initTestI18n(locale);
  if (!root) root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <QuizSession
          courseId='course-1'
          onConflict={spies.onConflict ?? (() => undefined)}
          onFinished={() => undefined}
          onClose={spies.onClose ?? (() => undefined)}
        />
      </I18nextProvider>
    );
  });
  // flush 开测 promise 链（挂载 effect → start → api → emit → setState）
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
};

/** 动作期间新弹出的提示（按 key 与动作前快照求差） */
const shownDuring = async (action: () => Promise<void>) => {
  const before = new Set(notificationStore.getSnapshot().map((record) => record.key));
  await action();
  return notificationStore
    .getSnapshot()
    .filter((record) => !before.has(record.key))
    .map((record) => ({ level: record.level, content: record.content }));
};

// 组件引用在 jsdom 宿主就绪后取（import 提升不影响：模块图无 DOM 顶层依赖）。
import { QuizSession } from './QuizSession';

const realQuizStart = learningApi.quizStart;

afterEach(() => {
  learningApi.quizStart = realQuizStart;
  AppMessage.clear();
});

describe('QuizSession 容器（u5 R1/R2/R3）', () => {
  test('R1：loading 相（开测在途）Spin 旁呈现组卷等待文案，注明「正在组卷」', async () => {
    await mountSession(
      'zh-CN',
      () => new Promise<QuizRoundView>(() => undefined)
    );
    const markup = win.document.body.innerHTML;
    expect(markup).toContain('arco-spin');
    expect(markup).toContain('正在组卷，请稍候…');
  });

  test('R1：en-US loading 相同走词典（零中文回落）', async () => {
    await mountSession(
      'en-US',
      () => new Promise<QuizRoundView>(() => undefined)
    );
    const markup = win.document.body.innerHTML;
    expect(markup).toContain('Preparing your quiz, please wait…');
    expect(markup).not.toContain('正在组卷');
  });

  test('R2：start 409 → onConflict/onClose 各恰一次（进度刷新 + 关闭通道），toast = quizNotDueHint', async () => {
    let conflicts = 0;
    let closes = 0;
    await shownDuring(async () => {
      await mountSession(
        'zh-CN',
        async () => {
          throw new BackendHttpError({
            method: 'POST',
            path: '/api/learning/courses/course-1/quiz/start',
            status: 409,
            body: { error: 'quiz not due: 4/6 settlements since last quiz' },
          });
        },
        { onConflict: () => (conflicts += 1), onClose: () => (closes += 1) }
      );
    });
    // 容器接线替身（index.tsx：onConflict bump 令牌 / onClose 卸载）
    expect(conflicts).toBe(1);
    expect(closes).toBe(1);
    // 人面：K 提取自 Conflict 报文填 {{trigger}} 槽，原始报文不上屏
    const toasts = notificationStore.getSnapshot();
    const contents = toasts.map((record) => String(record.content));
    expect(contents).toContain('尚未到期，满 6 次学新结算后开放检测');
    expect(contents.some((content) => content.includes('quiz not due'))).toBe(false);
  });

  test('R3：异常回合（abandoned）不进作答流——error 相如实呈现专用文案 + 重试/关闭', async () => {
    await mountSession('zh-CN', async () => ({
      round_id: 'round-1',
      course_id: 'course-1',
      status: 'abandoned',
      settled_count: 6,
      trigger_count: 6,
      budget: 15,
      questions: [],
    }));
    const markup = win.document.body.innerHTML;
    // 如实呈现：专用文案（引导回课程页）在 error 相，作答流不可达
    expect(markup).toContain('该轮检测已结束或已被新一轮取代');
    expect(markup).not.toContain('提交答案');
    // 出口：重试 + 关闭（onClose 语义回课程页）
    expect(markup).toContain('重试');
    expect(markup).toContain('关闭');
  });
});
