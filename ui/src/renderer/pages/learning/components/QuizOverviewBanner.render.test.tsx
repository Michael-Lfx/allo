/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 检测感知横幅（u7）渲染两态 + 行为契约：
 * - 有 due 课程：横幅出现，每行课程名 + N/K 读数（复用 quizProgressReading
 *   词典键）+ due 标记；行点击 → onOpenCourse(course_id)（index.tsx 接线
 *   navigate，本组件不持路由）；非 due 课程不占行（留在课程页内入口）；
 *   后端已按 due 优先排序，前端不重排（按传入序渲染）。
 * - 无 due（空数组 / 全非 due）：整块缺席——与 ReviewBanner 常驻策略刻意
 *   不同（复习天然有货，检测是稀事件），空壳不渲染。
 * - 拉取失败：整块缺席 + 零 toast（感知面不渲染失败噪音；课程页入口
 *   QuizEntryCard 不依赖本横幅）。
 * 组件自拉数据（props 只有 onOpenCourse），故用例在 jsdom 宿主里跑真实
 * effects，数据经 learningApi.quizOverview 逐用例打补丁并还原（HTTP 边界，
 * QuizEntryCard.render.test 同纪律）。en-US 文案同卡验证。 */
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
import type { QuizOverviewCourse } from '../types';

// ── jsdom 宿主：组件挂载后 effects 才跑（拉取、绑定点击），SSR 探针不可用 ──
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

const course = (overrides: Partial<QuizOverviewCourse> & { course_id: string }): QuizOverviewCourse => ({
  title: `课程 ${overrides.course_id}`,
  settled_count: 8,
  trigger_count: 6,
  due: true,
  ...overrides,
});

/** 用例间复位：同型组件在同 root 上重渲染会保留实例状态且 effect 不重跑 */
const reset = async () => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
};

/** 挂载横幅：overviewPatch = 本用例的数据源（HTTP 边界替身） */
const mountBanner = async (
  locale: 'zh-CN' | 'en-US',
  overviewPatch: () => Promise<QuizOverviewCourse[]> = async () => [],
  onOpenCourse: (courseId: string) => void = () => undefined
): Promise<{ html: () => string }> => {
  await reset();
  learningApi.quizOverview = overviewPatch;
  const instance = await initTestI18n(locale);
  if (!root) root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <QuizOverviewBanner onOpenCourse={onOpenCourse} />
      </I18nextProvider>
    );
  });
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  return { html: () => container.innerHTML };
};

// 组件引用在 jsdom 宿主就绪后取（import 提升不影响：模块图无 DOM 顶层依赖）。
import { QuizOverviewBanner } from './QuizOverviewBanner';

const realQuizOverview = learningApi.quizOverview;

afterEach(() => {
  learningApi.quizOverview = realQuizOverview;
  AppMessage.clear();
});

describe('QuizOverviewBanner（u7 检测感知横幅）', () => {
  test('有 due 课程：横幅出现，行含课程名 + N/K 读数 + due 标记；非 due 课程不占行', async () => {
    const { html } = await mountBanner('zh-CN', async () => [
      course({ course_id: 'due-a', title: 'Alpha course', settled_count: 8, due: true }),
      course({ course_id: 'due-b', title: 'Beta course', settled_count: 6, due: true }),
      course({
        course_id: 'notdue-c',
        title: 'Gamma course',
        settled_count: 2,
        due: false,
      }),
    ]);
    const markup = html();
    expect(markup).toContain('检测到期');
    expect(markup).toContain('Alpha course');
    expect(markup).toContain('Beta course');
    // due 标记逐行在场
    expect(markup).toContain('已到期');
    // N/K 读数复用课程页入口同款词典键
    expect(markup).toContain('距上次检测已积累 8/6 次学新结算');
    expect(markup).toContain('距上次检测已积累 6/6 次学新结算');
    // 非 due 课程不占横幅（留在课程页内入口——票面 R2）
    expect(markup).not.toContain('Gamma course');
    // 后端排序即渲染序：due 行在前（Alpha 先于 Beta，前端不重排）
    expect(markup.indexOf('Alpha course')).toBeLessThan(markup.indexOf('Beta course'));
  });

  test('无 due：空数组与全非 due 两种形态整块缺席（与 ReviewBanner 常驻策略刻意不同）', async () => {
    const empty = await mountBanner('zh-CN', async () => []);
    expect(empty.html()).toBe('');

    const allNotDue = await mountBanner('zh-CN', async () => [
      course({ course_id: 'notdue-c', title: 'Gamma course', settled_count: 3, due: false }),
    ]);
    expect(allNotDue.html()).toBe('');
    expect(allNotDue.html()).not.toContain('检测到期');
  });

  test('行点击 → onOpenCourse(course_id)（index.tsx 接线 navigate /learn/{id}）', async () => {
    const opened: string[] = [];
    await mountBanner('zh-CN', async () => [
      course({ course_id: 'due-a', title: 'Alpha course' }),
      course({ course_id: 'due-b', title: 'Beta course' }),
    ], (courseId) => {
      opened.push(courseId);
    });
    const rows = () => Array.from(container.querySelectorAll('button'));
    expect(rows().length).toBe(2);
    await act(async () => {
      rows()[1].dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      await Promise.resolve();
    });
    await act(async () => {
      rows()[0].dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      await Promise.resolve();
    });
    expect(opened).toEqual(['due-b', 'due-a']);
  });

  test('拉取失败：整块缺席 + 零 toast（感知面不渲染失败噪音）', async () => {
    await reset();
    learningApi.quizOverview = async () => {
      throw new Error('network down');
    };
    const instance = await initTestI18n('zh-CN');
    root = createRoot(container);
    const before = new Set(notificationStore.getSnapshot().map((record) => record.key));
    await act(async () => {
      root!.render(
        <I18nextProvider i18n={instance}>
          <QuizOverviewBanner onOpenCourse={() => undefined} />
        </I18nextProvider>
      );
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(container.innerHTML).toBe('');
    const newToasts = notificationStore
      .getSnapshot()
      .filter((record) => !before.has(record.key));
    expect(newToasts).toEqual([]);
  });

  test('en-US：标题/标记/读数同走词典（零中文回落）', async () => {
    const { html } = await mountBanner('en-US', async () => [
      course({ course_id: 'due-a', title: 'Alpha course', settled_count: 8 }),
    ]);
    const markup = html();
    expect(markup).toContain('Quiz due');
    expect(markup).toContain('Due');
    expect(markup).toContain('8/6 new-lesson settlements');
    expect(markup).not.toContain('检测到期');
  });
});
