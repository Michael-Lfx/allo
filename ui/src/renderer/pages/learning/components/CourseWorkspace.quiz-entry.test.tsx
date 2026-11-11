/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 课程工作区的测验入口挂点（u2 R4）：测验区块跟随课程存在性语义——
 * 归档课程（enabled=0）不渲染区块、也不拉进度读数；现存课程挂区块且
 * 进页拉一次 quizProgress。删除课程（墓碑）详情页本就不挂载工作区
 * （详情 404 → 列表页），由页面级语义覆盖，不在本用例面。
 * jsdom 宿主：CourseWorkspace 经 useConfig（useSyncExternalStore 无
 * getServerSnapshot）不能走 SSR 探针；进度经 learningApi.quizProgress
 * 逐用例打补丁并还原（HTTP 边界，useCourseLearning.test 同纪律）。 */
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
import { MemoryRouter } from 'react-router-dom';

import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
// LearningModelSelector → useEffectiveModelTier → useCloudAuth 需要 Provider；
// 用真实 Provider（其 ipc effects 在测试宿主失败即降级、被 catch），不 mock
// （bun 的 mock.module 进程全局常驻，污染 test:ui 全套件——useCourseLearning
// .test 纪律）。
import { CloudAuthProvider } from '@/renderer/hooks/context/CloudAuthContext';
import { learningApi } from '../api';
import type { CourseDetail } from '../types';

// ── jsdom 宿主（SpeechInputButton.dictationActivation 同仪式） ──────────────
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

const initTestI18n = async () => {
  const instance = createInstance();
  await instance.use(initReactI18next).init({
    lng: 'zh-CN',
    fallbackLng: 'zh-CN',
    resources: { 'zh-CN': { translation: { learning: learningZh } } },
    interpolation: { escapeValue: false },
  });
  return instance;
};

const detail = (enabled: boolean): CourseDetail =>
  ({
    course: {
      id: 'course-1',
      title: '线性代数',
      description: '',
      domain: '',
      source_kb_id: null,
      source_kb_ids: [],
      version: 1,
      enrolled: true,
      total_lessons: 1,
      completed_lessons: 0,
      enabled,
      settled_or_completed: 0,
      due_today: 0,
      total_xp: null,
      updated_at: 0,
      tags: [],
      course_kind: 'traditional',
      learning_goal: null,
      sampling: null,
      source_plan: null,
    },
    enrollment_id: null,
    modules: [
      {
        id: 'm1',
        title: '模块',
        description: '',
        position: 0,
        lessons: [
          {
            id: 'l1',
            title: '向量',
            summary: '',
            purpose: '',
            intent: '',
            scope: '',
            position: 0,
            generated: true,
            est: 10,
            source: null,
            sources: [],
            status: 'not_started',
            activities: [],
            sections: [],
          },
        ],
      },
    ],
    next_lesson_id: 'l1',
    due_review_count: 0,
    overloaded_lessons: 0,
    graph: null,
  }) as unknown as CourseDetail;

const noop = () => undefined;

/** 挂载课程工作区：返回进度拉取计数与标记读取器 */
const mountWorkspace = async (enabled: boolean) => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  let pulls = 0;
  learningApi.quizProgress = async () => {
    pulls += 1;
    return { settled_count: 8, trigger_count: 6, due: true };
  };
  const instance = await initTestI18n();
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <CourseWorkspace
            detail={detail(enabled)}
            busyId={null}
            attemptResults={{}}
            onBack={noop}
            onProgress={noop}
            onAttempt={noop}
            onGenerate={noop}
            onRefresh={noop}
            quizProgressToken={0}
            onStartQuiz={noop}
          />
          </MemoryRouter>
        </CloudAuthProvider>
      </I18nextProvider>
    );
  });
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  return { pulls: () => pulls, html: () => container.innerHTML };
};

const realQuizProgress = learningApi.quizProgress;

afterEach(() => {
  learningApi.quizProgress = realQuizProgress;
});

import { CourseWorkspace } from './CourseWorkspace';

describe('CourseWorkspace 测验入口挂点（u2 R4）', () => {
  test('现存课程：挂测验区块，进页拉一次进度读数', async () => {
    const { pulls, html } = await mountWorkspace(true);
    expect(pulls()).toBe(1);
    const markup = html();
    expect(markup).toContain('开始检测');
    expect(markup).toContain('距上次检测已积累 8/6 次学新结算');
  });

  test('归档课程（enabled=0）：不渲染测验区块，也不拉进度', async () => {
    const { pulls, html } = await mountWorkspace(false);
    expect(pulls()).toBe(0);
    const markup = html();
    expect(markup).not.toContain('开始检测');
    expect(markup).not.toContain('知识检测');
  });
});
