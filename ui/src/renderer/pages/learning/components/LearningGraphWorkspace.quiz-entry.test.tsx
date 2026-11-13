/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图工作区的知识检测入口挂点（u6 挂点、u10 R5 上移）：入口挂在页签头部
 * 区——知识结构/生长控制条（含软闸建议条）之下、Tabs 之前，自然流位置
 * 恒可见；前向推荐（继续学习）与回向检测（知识检测）靠语境句区分方向。
 * 用例面：①due 态——入口卡渲染 N/K 读数 + 开测可点（onStartQuiz 透传）
 * 且位于结构条之后 Tabs 之前；②not-due 态——按钮禁用 + 未到期提示；
 * ③归档课程（enabled=0）不渲染也不拉读数（与 CourseWorkspace 挂点同
 * 存在性语义，上移后原样保留）；④en-US 态——语境句/读数/开测钮同走
 * 词典零中文回落（QuizOverviewBanner.render.test 双语模式）。
 * jsdom 宿主：CourseWorkspace.quiz-entry.test 同仪式——进度经
 * learningApi.quizProgress 逐用例打补丁并还原（HTTP 边界）；图工作区挂载
 * 期的结构/声明/注册表/补批判定读数一概打补丁（补批判定必须拒绝，防自动
 * 补批 POST 真发）；WebSocket 换哑插座（事件订阅在 jsdom 无后端可达）。 */
import * as bunTest from 'bun:test';
const { afterEach, describe, expect, test } = bunTest;
// The pinned bun-types in this checkout predate the afterAll declaration
// (same stale-types gap as CourseWorkspace.quiz-entry.test.tsx).
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
// LearningModelSelector → useEffectiveModelTier → useCloudAuth 需要 Provider；
// 用真实 Provider（其 ipc effects 在测试宿主失败即降级、被 catch），不 mock
// （bun 的 mock.module 进程全局常驻，污染 test:ui 全套件——同波测试纪律）。
import { CloudAuthProvider } from '@/renderer/hooks/context/CloudAuthContext';
import { learningApi } from '../api';
import type { CourseDetail } from '../types';
import LearningGraphWorkspace from './LearningGraphWorkspace';

// ── jsdom 宿主（CourseWorkspace.quiz-entry.test 同仪式） ───────────────────
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
// 哑 WebSocket：构造即挂起（永不 open/error），事件订阅链路在测试宿主保持
// 惰性——重连/看门狗定时器随订阅注销（afterAll 卸载）一并清理。
class InertWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  CONNECTING = 0;
  OPEN = 1;
  CLOSING = 2;
  CLOSED = 3;
  readyState = 0;
  addEventListener = () => undefined;
  removeEventListener = () => undefined;
  send = () => undefined;
  close = () => undefined;
}
previousGlobals['WebSocket'] = globals['WebSocket'];
globals['WebSocket'] = InertWebSocket;

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

const initTestI18n = async (locale: 'zh-CN' | 'en-US' = 'zh-CN') => {
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

const graphDetail = (enabled: boolean): CourseDetail =>
  ({
    course: {
      id: 'course-graph-1',
      title: '图课程',
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
      course_kind: 'learning_graph',
      learning_goal: null,
      sampling: null,
      source_plan: null,
    },
    enrollment_id: null,
    modules: [
      {
        id: 'm1',
        title: '学习图',
        description: '',
        position: 0,
        lessons: [
          {
            id: 'l1',
            title: '节点甲',
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
    next_lesson_id: null,
    due_review_count: 0,
    overloaded_lessons: 0,
    graph: {
      goal: '冒烟目标',
      recommended: ['l1'],
      blocked: [],
      ready_count: 1,
      ready_target: 7,
      ready_trigger: 3,
      growth_running: false,
    },
  }) as unknown as CourseDetail;

const noop = () => undefined;

/** 挂载图工作区：返回进度拉取计数、开测回调计数与标记读取器。
 * quizProgress 按用例注入 due 态；挂载期其余读数（结构投影/声明/注册表
 * 快照/补批判定）统一打离场补丁——growthVerdict 必须拒绝（防自动补批
 * POST 真发），结构投影给 Missing 空投影（合法空状态）。 */
const mountGraphWorkspace = async (
  enabled: boolean,
  progress: { settled_count: number; trigger_count: number; due: boolean },
  locale: 'zh-CN' | 'en-US' = 'zh-CN'
) => {
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
    return progress;
  };
  learningApi.graphStructure = async () =>
    ({ course_id: 'course-graph-1', structure: null, terrain: null, generated_at: null, mounts: null });
  learningApi.basisView = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.generationTasks = async () => [];
  learningApi.growthVerdict = async () => {
    throw new Error('off (test fixture)');
  };
  const onStartQuiz = () => {
    starts += 1;
  };
  let starts = 0;
  const instance = await initTestI18n(locale);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <LearningGraphWorkspace
              detail={graphDetail(enabled)}
              busyId={null}
              attemptResults={{}}
              quizProgressToken={0}
              onBack={noop}
              onProgress={noop}
              onAttempt={noop}
              onGenerate={noop}
              onStartQuiz={onStartQuiz}
              onStartWeakReview={noop}
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
    pulls: () => pulls,
    starts: () => starts,
    html: () => container.innerHTML,
  };
};

const realApi = {
  quizProgress: learningApi.quizProgress,
  graphStructure: learningApi.graphStructure,
  basisView: learningApi.basisView,
  generationTasks: learningApi.generationTasks,
  growthVerdict: learningApi.growthVerdict,
};

afterEach(() => {
  learningApi.quizProgress = realApi.quizProgress;
  learningApi.graphStructure = realApi.graphStructure;
  learningApi.basisView = realApi.basisView;
  learningApi.generationTasks = realApi.generationTasks;
  learningApi.growthVerdict = realApi.growthVerdict;
});

const buttonWithText = (text: string): HTMLButtonElement | null => {
  const buttons = Array.from(container.querySelectorAll('button'));
  return buttons.find((button) => (button.textContent ?? '').includes(text)) ?? null;
};

describe('LearningGraphWorkspace 知识检测入口挂点（u6）', () => {
  test('due 态：入口卡在页签头部区（结构条之后、Tabs 之前），语境句区分回向，开测可点', async () => {
    const { pulls, starts, html } = await mountGraphWorkspace(true, {
      settled_count: 8,
      trigger_count: 6,
      due: true,
    });
    expect(pulls()).toBe(1);
    const markup = html();
    // 回向语境句在场（前向/回向视觉区分的锚点）
    expect(markup).toContain('检测与推荐方向相反：推荐带你学新节点，检测考的是已结算的节点');
    // 位置（u10 R5）：页签头部区——知识结构条之后、Tabs 之前（自然流，
    // 学习 Tab 两栏区之上）。「知识结构」首现即结构条标题；「学习记录」
    // 首现即 Tabs 页签条标题——晚于入口即入口在 Tabs 之前、两栏区之上。
    expect(markup.indexOf('知识结构')).toBeLessThan(markup.indexOf('检测与推荐方向相反'));
    expect(markup.indexOf('检测与推荐方向相反')).toBeLessThan(markup.indexOf('学习记录'));
    // 学习 Tab 两栏区内容照常渲染（滚动语义不受上移影响）
    expect(markup).toContain('继续学习');
    expect(markup).toContain('节点甲');
    // N/K 读数与开测按钮
    expect(markup).toContain('距上次检测已积累 8/6 次学新结算');
    const startButton = buttonWithText('开始检测');
    expect(startButton).not.toBeNull();
    expect(startButton!.disabled).toBe(false);
    await act(async () => {
      startButton!.click();
    });
    expect(starts()).toBe(1);
  });

  test('not-due 态：入口卡渲染未到期提示，开测按钮禁用', async () => {
    const { pulls, starts, html } = await mountGraphWorkspace(true, {
      settled_count: 2,
      trigger_count: 6,
      due: false,
    });
    expect(pulls()).toBe(1);
    const markup = html();
    expect(markup).toContain('距上次检测已积累 2/6 次学新结算');
    expect(markup).toContain('尚未到期，满 6 次学新结算后开放检测');
    const startButton = buttonWithText('开始检测');
    expect(startButton).not.toBeNull();
    expect(startButton!.disabled).toBe(true);
    await act(async () => {
      startButton!.click();
    });
    expect(starts()).toBe(0);
  });

  test('en-US：语境句/读数/开测钮同走词典（零中文回落）', async () => {
    const { pulls, starts, html } = await mountGraphWorkspace(
      true,
      { settled_count: 8, trigger_count: 6, due: true },
      'en-US'
    );
    expect(pulls()).toBe(1);
    const markup = html();
    // 回向语境句与读数、开测钮全部走 en-US 词典
    expect(markup).toContain('Quizzes run opposite to recommendations');
    expect(markup).toContain('8/6 new-lesson settlements');
    const startButton = buttonWithText('Start quiz');
    expect(startButton).not.toBeNull();
    expect(startButton!.disabled).toBe(false);
    await act(async () => {
      startButton!.click();
    });
    expect(starts()).toBe(1);
    // 零中文回落
    expect(markup).not.toContain('检测与推荐方向相反');
    expect(markup).not.toContain('开始检测');
  });

  test('归档课程（enabled=0）：不渲染检测入口，也不拉进度', async () => {
    const { pulls, html } = await mountGraphWorkspace(false, {
      settled_count: 8,
      trigger_count: 6,
      due: true,
    });
    expect(pulls()).toBe(0);
    const markup = html();
    expect(markup).not.toContain('开始检测');
    expect(markup).not.toContain('检测与推荐方向相反');
  });
});
