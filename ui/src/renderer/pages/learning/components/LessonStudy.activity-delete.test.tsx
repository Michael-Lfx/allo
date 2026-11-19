/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** sq5 删题入口（LessonStudy 接缝）：练习轮/题目卡管理位「删除本题」→
 * 确认对话框（照既有删除惯例）→ DELETE API → 练习轮即时收敛（重新拉取
 * 的最新课时详情整体替换，rewriteSection 同款）。
 *
 * 两组用例：
 * ① ActivityBlock 静态渲染接缝（renderToStaticMarkup）：onDelete 缺席不
 *    渲染入口（旧调用面零变化），在场渲染双语标签（零中文回落断言）。
 * ② LessonBlock jsdom 宿主（CourseWorkspace.quiz-entry 同仪式）：点删除 →
 *    确认框出现 → 确认 → learningApi.deleteLessonActivity 以
 *    (lessonId, activityId) 调用恰一次 → 列表收敛（坏题题面消失、兄弟
 *    题保留）。已答态（attemptResults 有记录）入口照常在场——救济面的
 *    典型场景就是答完才发现题坏。
 * learningApi.getLesson / deleteLessonActivity 逐用例打补丁并还原
 * （HTTP 边界，useCourseLearning.test 同纪律）。 */
import * as bunTest from 'bun:test';
const { describe, expect, test } = bunTest;
// The pinned bun-types in this checkout predate the afterAll declaration
// (same stale-types gap as SpeechInputButton.dictationActivation.test.tsx).
const afterAllMaybe = (bunTest as unknown as { afterAll?: (callback: () => void) => void })
  .afterAll;
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
// Arco 2.66 + React 19：`ReactDOM.render` 已移除，确认框（Modal.confirm 的
// copyRender 面）需要 main.tsx 同款 react-19-adapter 引导，否则点删除即
// TypeError（Message 同款先例，main.tsx:23 注记）。bun test 无 vite 别名，
// `@arco-design/web-react` 落在 CJS lib 构建——补丁须打同一实例（es 侧
// 引导管不到 lib 的 copyRender），应用构建走 vite es 别名不受影响。
import '@arco-design/web-react/lib/_util/react-19-adapter';
// LearningModelSelector → useEffectiveModelTier → useCloudAuth 需要 Provider；
// 用真实 Provider（其 ipc effects 在测试宿主失败即降级、被 catch），不 mock
// （bun 的 mock.module 进程全局常驻，污染 test:ui 全套件——useCourseLearning
// .test 纪律）。
import { CloudAuthProvider } from '@/renderer/hooks/context/CloudAuthContext';
import { learningApi } from '../api';
import type { Activity, Lesson } from '../types';
import { ActivityBlock, LessonBlock } from './LessonStudy';

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
  'HTMLIFrameElement',
  'HTMLBodyElement',
  'HTMLInputElement',
  'Element',
  'Node',
  'Event',
  'MouseEvent',
  'localStorage',
  'sessionStorage',
  'getComputedStyle',
  'MutationObserver',
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

if (afterAllMaybe) {
  afterAllMaybe(async () => {
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
}

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

const noop = () => undefined;

const BAD_PROMPT = '坏题：把演示中值建立与失效的先后顺序排好。';
const GOOD_PROMPT = '好题：向量的大小叫什么？';

const activity = (id: string, prompt: string): Activity => ({
  id,
  kind: 'single_choice',
  prompt,
  options: ['栈区', '堆区'],
  matches: [],
  section_key: 's-practice',
  position: 0,
});

/** 新规范课时：一个练习节 + 绑定的练习轮（删题主战场）。 */
const lessonFixture = (): Lesson =>
  ({
    id: 'lesson-1',
    title: '值存储位置',
    summary: '',
    purpose: '',
    intent: '',
    scope: '',
    position: 0,
    generated: true,
    est: 10,
    source: null,
    sources: [],
    status: 'in_progress',
    activities: [
      activity('act-bad', BAD_PROMPT),
      activity('act-good', GOOD_PROMPT),
    ],
    sections: [
      {
        section_key: 's-practice',
        kind: 'practice',
        title: '练习',
        points: '',
        body_md: '把上面的判定走一遍。',
        status: 'ready',
        version: 1,
        position: 0,
      },
    ],
  }) as unknown as Lesson;

const lessonWithout = (activityId: string): Lesson => {
  const lesson = lessonFixture();
  lesson.activities = lesson.activities.filter((entry) => entry.id !== activityId);
  return lesson;
};

const realGetLesson = learningApi.getLesson;
const realDeleteLessonActivity = learningApi.deleteLessonActivity;

const mountLessonBlock = async (locale: 'zh-CN' | 'en-US' = 'zh-CN') => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  const deletions: Array<{ lessonId: string; activityId: string }> = [];
  learningApi.getLesson = async () => lessonFixture();
  learningApi.deleteLessonActivity = async (lessonId: string, activityId: string) => {
    deletions.push({ lessonId, activityId });
    return lessonWithout(activityId);
  };
  const instance = await initTestI18n(locale);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <LessonBlock
              lesson={lessonFixture()}
              sourceKbId={null}
              busyId={null}
              attemptResults={{}}
              onProgress={noop}
              onAttempt={noop}
              onGenerate={noop}
              onRefresh={noop}
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
  return {
    deletions,
    html: () => container.innerHTML,
    clickButtonWithText: async (text: string) => {
      const buttons = [...container.querySelectorAll('button')];
      const target = buttons.find((button) => button.textContent?.includes(text));
      if (!target) throw new Error(`button with text ${text} not found`);
      target.click();
      // Modal.confirm 经 createRoot 异步渲染 + CSSTransition（arco confirm.js
      // / modal.js），真钟表走一拍再查 DOM（rAF shim 是 setTimeout）。
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 100));
        await Promise.resolve();
      });
    },
    confirmDialog: () => win.document.body.querySelector('.arco-modal'),
    confirmOk: async () => {
      const modal = win.document.body.querySelector('.arco-modal');
      if (!modal) throw new Error('confirm dialog not found');
      const ok = modal.querySelector('.arco-btn-primary') as HTMLButtonElement | null;
      if (!ok) throw new Error('confirm OK button not found');
      ok.click();
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 100));
        await Promise.resolve();
        await Promise.resolve();
      });
    },
  };
};

describe('ActivityBlock 删题入口（sq5 R-b 静态接缝）', () => {
  test('onDelete 缺席不渲染入口（旧调用面零变化）', async () => {
    const instance = await initTestI18n('zh-CN');
    const markup = renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <ActivityBlock
          activity={activity('act-1', BAD_PROMPT)}
          disabled
          onSubmit={noop}
        />
      </I18nextProvider>
    );
    expect(markup).not.toContain('删除本题');
    expect(markup).not.toContain('Delete question');
  });

  test('zh-CN：入口标签「删除本题」在场', async () => {
    const instance = await initTestI18n('zh-CN');
    const markup = renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <ActivityBlock
          activity={activity('act-1', BAD_PROMPT)}
          disabled
          onDelete={noop}
          onSubmit={noop}
        />
      </I18nextProvider>
    );
    expect(markup).toContain('删除本题');
  });

  test('en-US：入口标签走词典（零中文回落）', async () => {
    const instance = await initTestI18n('en-US');
    const markup = renderToStaticMarkup(
      <I18nextProvider i18n={instance}>
        <ActivityBlock
          activity={activity('act-1', BAD_PROMPT)}
          disabled
          onDelete={noop}
          onSubmit={noop}
        />
      </I18nextProvider>
    );
    expect(markup).toContain('Delete question');
    expect(markup).not.toContain('删除本题');
  });
});

describe('LessonBlock 删题交互（sq5 R-b：确认框 / 列表收敛）', () => {
  const restore = () => {
    learningApi.getLesson = realGetLesson;
    learningApi.deleteLessonActivity = realDeleteLessonActivity;
  };

  test('zh-CN：点删除 → 确认框 → 确认 → API 恰一次 → 练习轮收敛', async () => {
    try {
      const harness = await mountLessonBlock('zh-CN');
      expect(harness.html()).toContain(BAD_PROMPT);
      // 已答态入口照常在场（attemptResults 有记录）——本用例 face 空作答
      // 即可达，已答态由静态接缝的 disabled 透传覆盖。
      await harness.clickButtonWithText('删除本题');
      // 确认框出现且告知账本保留语义。
      expect(harness.confirmDialog()).not.toBeNull();
      expect(win.document.body.textContent).toContain('作答记录');
      await harness.confirmOk();
      expect(harness.deletions).toEqual([{ lessonId: 'lesson-1', activityId: 'act-bad' }]);
      // 列表即时收敛：坏题题面消失，兄弟题保留。
      expect(harness.html()).not.toContain(BAD_PROMPT);
      expect(harness.html()).toContain(GOOD_PROMPT);
    } finally {
      restore();
    }
  });

  test('en-US：入口零中文回落（Delete question 在场）', async () => {
    try {
      const harness = await mountLessonBlock('en-US');
      expect(harness.html()).toContain('Delete question');
      expect(harness.html()).not.toContain('删除本题');
    } finally {
      restore();
    }
  });
});
