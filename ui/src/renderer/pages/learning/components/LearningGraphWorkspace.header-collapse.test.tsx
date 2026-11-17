/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图课程详情页头部折叠 + 目标段去重（f3 Q5/Q6）：
 * - Q5 折叠：标题块 + 目标段 + 知识结构/生长控制条（含软闸 Alert）收进
 *   一行摘要条（课程名 + structureSummaryText + processing 徽章 + 展开
 *   钮）；默认展开；per-course 持久化（localStorage，键
 *   learning.graph.headerCollapsed，存储注入式——测试装内存 mock）；缓头
 *   返回行 / 检测区 / Tabs 常驻折叠区之外；structure/growth 运行中强制
 *   展开（processingPrimary / growing 任一为真时不可折叠、processing
 *   徽章照常渲染）。
 * - Q6 去重：graph.goal trim 后与 course.title 逐字相同 → 目标段不渲染；
 *   不同（含仅空白差异）才渲染。
 * jsdom 宿主：LearningGraphWorkspace.quiz-entry.test 同仪式——挂载期读数
 * （结构投影/声明/注册表快照/补批判定/检测进度）一概打补丁并还原；检测
 * 进度注入 due:false（检测区与本票折叠语义解耦）。localStorage 装内存
 * mock（bun test 宿主无可靠 localStorage——历史波实证），逐用例清场。
 * WebSocket 换哑插座。 */
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
import type { CourseDetail, LearningStructureView } from '../types';
import { GRAPH_HEADER_COLLAPSED_KEY } from '../constants';
import { loadGraphHeaderCollapsed } from '../headerCollapse';
import LearningGraphWorkspace from './LearningGraphWorkspace';

// ── jsdom 宿主（LearningGraphWorkspace.quiz-entry.test 同仪式） ─────────────
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

// 内存 localStorage mock（f3 已知坑：bun test 宿主无可靠 localStorage——
// headerCollapse 默认读全局 localStorage，测试装内存注入并逐用例清场）。
const storageMap = new Map<string, string>();
globals['localStorage'] = {
  getItem: (key: string) => storageMap.get(key) ?? null,
  setItem: (key: string, value: string) => {
    storageMap.set(key, value);
  },
  removeItem: (key: string) => {
    storageMap.delete(key);
  },
  clear: () => storageMap.clear(),
};

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

const lesson = (id: string, title: string, status: string) =>
  ({
    id,
    title,
    summary: '',
    purpose: '',
    intent: '',
    scope: '',
    position: 0,
    generated: true,
    est: 10,
    source: null,
    sources: [],
    status,
    activities: [],
    sections: [],
  }) as unknown as CourseDetail['modules'][number]['lessons'][number];

/** 结构投影夹具：3 主线 × 4 知识组（structureSummaryText 只读两个 length，
 * 其余字段概不参与——按类型面最小构造）。generated_at 刻意 null：非 null
 * 会触发 c5 结构图自动呈现（切到结构图 Tab → Mindmap 深渲染），与本票
 * 折叠/去重语义无关，不进盘面。 */
const structureView = {
  course_id: 'course-graph-1',
  structure: { spines: [{}, {}, {}], blocks: [{}, {}, {}, {}] },
  terrain: null,
  generated_at: null,
  mounts: null,
} as unknown as LearningStructureView;

const graphDetail = (options?: { goal?: string; growthRunning?: boolean }): CourseDetail =>
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
      enabled: true,
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
        lessons: [lesson('l1', '节点甲', 'not_started')],
      },
    ],
    next_lesson_id: null,
    due_review_count: 0,
    overloaded_lessons: 0,
    graph: {
      goal: options?.goal ?? '冒烟目标',
      recommended: ['l1'],
      blocked: [],
      ready_count: 1,
      ready_target: 7,
      ready_trigger: 3,
      growth_running: options?.growthRunning ?? false,
    },
  }) as unknown as CourseDetail;

const noop = () => undefined;

/** 挂载图工作区：检测进度注入 due:false（检测区与折叠语义解耦）、结构投影
 * 给就绪夹具（摘要条文本面）、补批判定拒绝（防自动补批 POST 真发）。 */
const mountGraphWorkspace = async (
  options?: { goal?: string; growthRunning?: boolean; locale?: 'zh-CN' | 'en-US' } | string,
  locale: 'zh-CN' | 'en-US' = 'zh-CN'
) => {
  const opts = typeof options === 'string' ? { goal: options } : (options ?? {});
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  learningApi.quizProgress = async () => ({ settled_count: 2, trigger_count: 6, due: false });
  learningApi.graphStructure = async () => structureView;
  learningApi.basisView = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.generationTasks = async () => [];
  learningApi.growthVerdict = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.graphHistory = async () => ({ batches: [] });
  learningApi.systemActions = async () => [];
  const instance = await initTestI18n(opts.locale ?? locale);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <LearningGraphWorkspace
              detail={graphDetail(opts)}
              busyId={null}
              attemptResults={{}}
              quizProgressToken={0}
              onBack={noop}
              onProgress={noop}
              onAttempt={noop}
              onGenerate={noop}
              onStartQuiz={noop}
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
};

const realApi = {
  quizProgress: learningApi.quizProgress,
  graphStructure: learningApi.graphStructure,
  basisView: learningApi.basisView,
  generationTasks: learningApi.generationTasks,
  growthVerdict: learningApi.growthVerdict,
  graphHistory: learningApi.graphHistory,
  systemActions: learningApi.systemActions,
  growGraph: learningApi.growGraph,
};

afterEach(() => {
  for (const [key, value] of Object.entries(realApi)) {
    (learningApi as unknown as Record<string, unknown>)[key] = value;
  }
  storageMap.clear();
});

/** 头部折叠钮（aria-label 定位，双语各按词典）。 */
const headerToggle = (label: string): HTMLButtonElement | null =>
  container.querySelector(`button[aria-label="${label}"]`);

/** 目标段 Paragraph（arco Paragraph 渲染为 div.arco-typography；Title 渲染
 * 为 h1-h6——按块级标签区分，文本精确等于 goal 才算目标段在渲染）。 */
const goalParagraphs = (goal: string): HTMLElement[] =>
  Array.from(container.querySelectorAll<HTMLElement>('.arco-typography')).filter(
    (node) => node.tagName === 'DIV' && (node.textContent ?? '').trim() === goal
  );

describe('LearningGraphWorkspace 头部折叠（f3 Q5）', () => {
  test('默认展开：标题块 + 结构条 + 软闸面齐全，折叠钮在标题行（aria-expanded=true）', async () => {
    await mountGraphWorkspace();
    const text = container.textContent ?? '';
    expect(text).toContain('图课程');
    expect(text).toContain('冒烟目标');
    expect(text).toContain('知识结构');
    expect(text).toContain('补充节点（就绪 1/7）');
    const toggle = headerToggle('收起头部');
    expect(toggle).not.toBeNull();
    expect(toggle!.getAttribute('aria-expanded')).toBe('true');
    // 折叠摘要条不在场（摘要文本与grow 按钮同文面时无法区分，此处结构
    // 投影在场：摘要 = 3 条主线 …，展开态它只出现在结构条内部）
    expect(text).toContain('3 条主线');
  });

  test('点击收起：标题块/目标段/结构条收进一行摘要条（课程名 + 结构摘要 + 展开钮），持久化写入', async () => {
    await mountGraphWorkspace();
    await act(async () => {
      headerToggle('收起头部')!.click();
      await Promise.resolve();
    });
    const text = container.textContent ?? '';
    // 折叠区内容退场
    expect(text).not.toContain('冒烟目标');
    expect(text).not.toContain('知识结构');
    expect(text).not.toContain('补充节点（就绪 1/7）');
    // 摘要条在场：课程名 + 结构摘要 + 展开钮（aria-expanded=false）
    expect(text).toContain('图课程');
    expect(text).toContain('3 条主线');
    const toggle = headerToggle('展开头部');
    expect(toggle).not.toBeNull();
    expect(toggle!.getAttribute('aria-expanded')).toBe('false');
    // 持久化写入（per-course）
    expect(storageMap.get(GRAPH_HEADER_COLLAPSED_KEY)).toBe('{"course-graph-1":true}');
    // 折叠区外常驻面：返回钮 + Tabs 行 + 学习视图两栏区照常
    expect(text).toContain('返回');
    expect(text).toContain('学习记录');
    expect(text).toContain('节点甲');
  });

  test('持久化回读：重挂载同一课从折叠态起步；展开写回清空', async () => {
    await mountGraphWorkspace();
    await act(async () => {
      headerToggle('收起头部')!.click();
      await Promise.resolve();
    });
    // 重挂载（同 storage）：首帧即折叠
    await mountGraphWorkspace();
    expect(container.textContent).not.toContain('知识结构');
    expect(headerToggle('展开头部')).not.toBeNull();
    // 展开写回：键下该课条目清除
    await act(async () => {
      headerToggle('展开头部')!.click();
      await Promise.resolve();
    });
    expect(container.textContent).toContain('知识结构');
    const raw = storageMap.get(GRAPH_HEADER_COLLAPSED_KEY);
    const parsed: unknown = raw === undefined ? null : JSON.parse(raw!);
    expect((parsed as Record<string, unknown> | null)?.['course-graph-1']).toBeUndefined();
    // 读侧同构：loadGraphHeaderCollapsed 对该课返回 false
    expect(loadGraphHeaderCollapsed('course-graph-1')).toBe(false);
  });

  test('per-course 隔离：A 课折叠不波及 B 课（读侧默认展开）', async () => {
    await mountGraphWorkspace();
    await act(async () => {
      headerToggle('收起头部')!.click();
      await Promise.resolve();
    });
    expect(loadGraphHeaderCollapsed('course-graph-1')).toBe(true);
    expect(loadGraphHeaderCollapsed('course-graph-2')).toBe(false);
  });

  test('growth_running 强制展开：持久化折叠被压住，折叠钮禁用，processing 徽章在场', async () => {
    storageMap.set(GRAPH_HEADER_COLLAPSED_KEY, '{"course-graph-1":true}');
    await mountGraphWorkspace({ growthRunning: true });
    const text = container.textContent ?? '';
    // 强制展开：折叠态摘要被完整头部替代
    expect(text).toContain('知识结构');
    expect(text).toContain('补充节点（就绪 1/7）');
    expect(headerToggle('收起头部')).not.toBeNull();
    // 不可折叠：钮禁用
    expect(headerToggle('收起头部')!.disabled).toBe(true);
    // processing 徽章不可被折叠吞
    expect(text).toContain('生长中…');
  });

  test('本地 growing 强制展开：生长在途时折叠钮禁用（点击不落持久化）', async () => {
    await mountGraphWorkspace();
    learningApi.growGraph = () => new Promise(() => undefined);
    await act(async () => {
      (Array.from(container.querySelectorAll('button')).find((button) =>
        (button.textContent ?? '').includes('补充节点')
      ) as HTMLElement).click();
      await Promise.resolve();
    });
    const toggle = headerToggle('收起头部');
    expect(toggle).not.toBeNull();
    expect(toggle!.disabled).toBe(true);
    await act(async () => {
      toggle!.click();
      await Promise.resolve();
    });
    // 点击无效：仍展开，持久化无写入
    expect(container.textContent).toContain('知识结构');
    expect(storageMap.get(GRAPH_HEADER_COLLAPSED_KEY)).toBeUndefined();
  });

  test('en-US：摘要条/展开钮同走词典（零中文回落）', async () => {
    await mountGraphWorkspace({ locale: 'en-US' });
    await act(async () => {
      headerToggle('Collapse header')!.click();
      await Promise.resolve();
    });
    const text = container.textContent ?? '';
    expect(text).toContain('3 main lines');
    expect(headerToggle('Expand header')).not.toBeNull();
    // 零中文回落
    expect(text).not.toContain('收起头部');
    expect(text).not.toContain('条主线');
    expect(container.querySelector('button[aria-label="收起头部"]')).toBeNull();
  });
});

describe('LearningGraphWorkspace 目标段去重（f3 Q6）', () => {
  test('goal 与 title 不同：目标段渲染', async () => {
    await mountGraphWorkspace();
    expect(goalParagraphs('冒烟目标').length).toBe(1);
  });

  test('goal trim 后与 title 逐字相同：目标段不渲染（含仅空白差异）', async () => {
    await mountGraphWorkspace({ goal: '图课程' });
    expect(goalParagraphs('图课程').length).toBe(0);
    await mountGraphWorkspace({ goal: ' 图课程 ' });
    expect(goalParagraphs('图课程').length).toBe(0);
  });
});
