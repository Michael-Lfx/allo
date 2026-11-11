/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 测验入口卡（u2）渲染两态 + 行为契约：
 * - due 态：N/K 读数（learnhub #90 形态）+「开始检测」可点（u3 接线位）；
 * - 未 due 态：按钮禁用呈现、不隐藏（与 ReviewBanner 空队列保留入口同理），
 *   附「满 K 次开放」提示；K 值一律来自后端 trigger_count（前端不自定 6）；
 * - 刷新时机（R2）：进页挂载拉一次、refreshToken 变化（会话结束回调后）
 *   再拉一次，无令牌变化不再拉（不轮询）；
 * - 409 兜底（R3）：开测回调被拒（BackendHttpError 409）→ conflictBusy
 *   toast（errorMessage 单源映射）+ 重拉进度呈现归零。
 * 组件自身拉进度（props 只有 courseId/refreshToken/onStartQuiz），故用例在
 * jsdom 宿主里跑真实 effects，进度经 learningApi.quizProgress 逐用例打补丁
 * 并还原（learningApi 是 HTTP 边界，useCourseLearning.test 同纪律）；提示经
 * notificationStore 公开快照求差观察。en-US 文案同卡验证。 */
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
import { learningApi } from '../api';
import type { QuizProgressView } from '../types';
import type { BackendHttpError } from '@/common/adapter/httpBridge';

// ── jsdom 宿主：组件挂载后 effects 才跑（拉进度、绑定点击），SSR 探针不可用 ──
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

const progress = (overrides: Partial<QuizProgressView> = {}): QuizProgressView => ({
  settled_count: 8,
  trigger_count: 6,
  due: true,
  ...overrides,
});

/** 用例间复位：同型组件在同 root 上重渲染会保留实例状态且 effect 不重跑，
 * 必须卸载重建才能观察「进页首次拉取」的真语义 */
const reset = async () => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
};

/** 挂载入口卡：progressPatch = 本用例的进度源（HTTP 边界替身），返回拉取次数 */
const mountCard = async (
  locale: 'zh-CN' | 'en-US',
  progressPatch: () => Promise<QuizProgressView> = async () => progress(),
  props: { refreshToken?: number; onStartQuiz?: () => void | Promise<void> } = {}
): Promise<{
  pulls: () => number;
  html: () => string;
  rerender: (token: number, onStartQuiz?: () => void | Promise<void>) => Promise<void>;
}> => {
  await reset();
  let pulls = 0;
  learningApi.quizProgress = async () => {
    pulls += 1;
    return progressPatch();
  };
  const instance = await initTestI18n(locale);
  if (!root) root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <QuizEntryCard
          courseId='course-1'
          refreshToken={props.refreshToken ?? 0}
          onStartQuiz={props.onStartQuiz ?? (() => undefined)}
        />
      </I18nextProvider>
    );
  });
  await act(async () => {
    await Promise.resolve();
  });
  return {
    pulls: () => pulls,
    html: () => container.innerHTML,
    /** 令牌变化驱动重渲染（模拟 index.tsx 会话结束回调后 bump 令牌） */
    rerender: (token: number, onStartQuiz: () => void | Promise<void> = () => undefined) =>
      rerenderCard(instance, token, onStartQuiz),
  };
};

/** 令牌变化驱动重挂（模拟 index.tsx 在会话结束回调后 bump 令牌） */
const rerenderCard = async (
  instance: Awaited<ReturnType<typeof initTestI18n>>,
  refreshToken: number,
  onStartQuiz: () => void | Promise<void> = () => undefined
) => {
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <QuizEntryCard courseId='course-1' refreshToken={refreshToken} onStartQuiz={onStartQuiz} />
      </I18nextProvider>
    );
  });
  await act(async () => {
    await Promise.resolve();
  });
};

/** 动作期间新弹出的提示（按 key 与动作前快照求差，不受他处残留影响） */
const shownDuring = async (action: () => Promise<void>) => {
  const before = new Set(notificationStore.getSnapshot().map((record) => record.key));
  await action();
  return notificationStore
    .getSnapshot()
    .filter((record) => !before.has(record.key))
    .map((record) => ({ level: record.level, content: record.content }));
};

const http409 = (error: BackendHttpError | Error): unknown => error;

// 组件引用在 jsdom 宿主就绪后取（import 提升不影响：模块图无 DOM 顶层依赖）。
import { QuizEntryCard } from './QuizEntryCard';

const realQuizProgress = learningApi.quizProgress;

afterEach(() => {
  learningApi.quizProgress = realQuizProgress;
  AppMessage.clear();
});

describe('QuizEntryCard（u2 测验入口卡）', () => {
  test('due 态：进页拉一次进度，N/K 读数 + 开始检测按钮可点（不出现未到期提示）', async () => {
    let pullsCount = 0;
    const { pulls, html } = await mountCard('zh-CN', async () => {
      pullsCount += 1;
      return progress();
    });
    expect(pulls()).toBe(1);
    const markup = html();
    expect(markup).toContain('知识检测');
    expect(markup).toContain('距上次检测已积累 8/6 次学新结算');
    expect(markup).toContain('开始检测');
    // 可点：不出现 disabled 呈现
    expect(markup).not.toContain('disabled');
    expect(markup).not.toContain('尚未到期');
    void pullsCount;
  });

  test('en-US due 态：读数与按钮同样走词典（零中文回落）', async () => {
    const { html } = await mountCard('en-US');
    const markup = html();
    expect(markup).toContain('8/6 new-lesson settlements');
    expect(markup).toContain('Start quiz');
    expect(markup).not.toContain('开始检测');
  });

  test('未 due 态：按钮禁用呈现、不隐藏，读数照出 + 满 K 次开放提示（K=trigger_count）', async () => {
    const { html } = await mountCard('zh-CN', async () =>
      progress({ settled_count: 3, trigger_count: 6, due: false })
    );
    const markup = html();
    // 不隐藏：区块标题与读数照常在，按钮也在（禁用呈现）
    expect(markup).toContain('知识检测');
    expect(markup).toContain('距上次检测已积累 3/6 次学新结算');
    expect(markup).toContain('开始检测');
    expect(markup).toContain('disabled');
    expect(markup).toContain('尚未到期，满 6 次学新结算后开放检测');
  });

  test('进度拉取失败：降级呈现加载失败句 + 按钮禁用，不渲染空、不吐 toast', async () => {
    await reset();
    learningApi.quizProgress = async () => {
      throw new Error('network down');
    };
    const instance = await initTestI18n('zh-CN');
    root = createRoot(container);
    const before = new Set(notificationStore.getSnapshot().map((record) => record.key));
    await act(async () => {
      root!.render(
        <I18nextProvider i18n={instance}>
          <QuizEntryCard courseId='course-1' refreshToken={0} onStartQuiz={() => undefined} />
        </I18nextProvider>
      );
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    const markup = container.innerHTML;
    expect(markup).toContain('测验加载失败，请重试');
    expect(markup).toContain('disabled');
    const newToasts = notificationStore
      .getSnapshot()
      .filter((record) => !before.has(record.key));
    expect(newToasts).toEqual([]);
  });

  test('刷新时机（R2）：进页拉一次，令牌 bump 再拉一次，之后等待不再拉（不轮询）', async () => {
    const { pulls, rerender } = await mountCard('zh-CN');
    expect(pulls()).toBe(1);
    await rerender(1);
    expect(pulls()).toBe(2);
    // 无令牌变化的等待面：读数非门禁，没有轮询定时器
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 60));
    });
    expect(pulls()).toBe(2);
  });

  test('due 态点击开始检测 → 回调被调（u3 接线位）；未 due 态点击不触发', async () => {
    let startCalls = 0;
    const card = await mountCard('zh-CN', undefined, {
      onStartQuiz: () => {
        startCalls += 1;
      },
    });
    const button = () => container.querySelector('button') as HTMLButtonElement;
    await act(async () => {
      button().dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true })
      );
      await Promise.resolve();
    });
    expect(startCalls).toBe(1);

    // 未 due：入口照在（不隐藏），点击零回调
    const notDue = await mountCard('zh-CN', async () =>
      progress({ settled_count: 3, trigger_count: 6, due: false })
    );
    await act(async () => {
      button().dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true })
      );
      await Promise.resolve();
    });
    expect(startCalls).toBe(1);
    void notDue;
  });

  test('409 兜底（R3）：开测被拒 → conflictBusy toast + 重拉进度读数', async () => {
    const conflict = new (await import('@/common/adapter/httpBridge')).BackendHttpError({
      method: 'POST',
      path: '/api/learning/courses/course-1/quiz/start',
      status: 409,
      body: { error: 'quiz not due' },
    });
    let startCalls = 0;
    const card = await mountCard('zh-CN', undefined, {
      onStartQuiz: async () => {
        startCalls += 1;
        throw http409(conflict);
      },
    });
    expect(card.pulls()).toBe(1);
    const button = () => container.querySelector('button') as HTMLButtonElement;
    const toasts = await shownDuring(async () => {
      await act(async () => {
        button().dispatchEvent(
          new MouseEvent('click', { bubbles: true, cancelable: true })
        );
        await Promise.resolve();
        await Promise.resolve();
      });
    });
    expect(startCalls).toBe(1);
    // 报实情：409 → conflictBusy（errorMessage 单源映射），原始报文不上人面
    expect(toasts).toEqual([
      { level: 'error', content: '当前有任务正在进行，请稍后再试' },
    ]);
    // 兜底刷新：拒绝后重拉读数（呈现如实归零的通道）
    expect(card.pulls()).toBe(2);
  });
});
