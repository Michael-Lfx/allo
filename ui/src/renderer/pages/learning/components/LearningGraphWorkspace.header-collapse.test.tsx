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
 *   返回行 / 检测区 / Tabs 常驻折叠区之外；processing（a3 修订）：后台
 *   运行不再强制展开、不再禁用折叠钮——徽章在展开/折叠两态照常渲染，
 *   折叠判定与 processing 解耦。
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

const graphDetail = (options?: {
  goal?: string;
  growthRunning?: boolean;
  courseId?: string;
}): CourseDetail =>
  ({
    course: {
      id: options?.courseId ?? 'course-graph-1',
      title: '图课程',
      description: '',
      domain: '',
      source_kb_id: null,
      source_kb_ids: [],
      version: 1,
      enrolled: true,
      total_lessons: 1,
      completed_lessons: 0,
      skipped_lessons: 0,
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

  test('growth_running 不再强制展开（a3）：持久化折叠保持折叠，展开钮不禁用，摘要条徽章在场', async () => {
    storageMap.set(GRAPH_HEADER_COLLAPSED_KEY, '{"course-graph-1":true}');
    await mountGraphWorkspace({ growthRunning: true });
    const text = container.textContent ?? '';
    // 持久化折叠不被 processing 压住：仍是折叠态（完整头部退场）
    expect(text).not.toContain('知识结构');
    expect(text).not.toContain('补充节点（就绪 1/7）');
    // 展开钮在场且恒可点（后台运行不剥夺折叠控制权）
    const toggle = headerToggle('展开头部');
    expect(toggle).not.toBeNull();
    expect(toggle!.disabled).toBe(false);
    // processing 徽章不可被折叠吞：摘要条兜底渲染生长徽章
    expect(text).toContain('生长中…');
  });

  test('本地 growing 不再禁用折叠（a3）：生长在途可折叠、落持久化', async () => {
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
    // 钮恒可点（growing 在途不进折叠判定；o6：徽章只认服务端 growth_running
    // 真相，本地在途态不点亮徽章——徽章兜底断言在 growth_running 用例）
    expect(toggle!.disabled).toBe(false);
    await act(async () => {
      toggle!.click();
      await Promise.resolve();
    });
    // 折叠生效且持久化写入
    expect(container.textContent).not.toContain('知识结构');
    expect(storageMap.get(GRAPH_HEADER_COLLAPSED_KEY)).toBe('{"course-graph-1":true}');
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

describe('LearningGraphWorkspace 头部滚动自动折叠（a1，临时层不落盘）', () => {
  /** 学习 Tab 正文阅读容器（a1 唯一触发源）：h-full overflow-y-auto p-16px。 */
  const readingPane = (): HTMLDivElement | null =>
    container.querySelector<HTMLDivElement>('div.overflow-y-auto.p-16px');

  /** 模拟滚动：置 scrollTop 后派发冒泡 scroll 事件，等 rAF 节拍落地
   * （宿主 requestAnimationFrame = 16ms setTimeout）。 */
  const scrollPane = async (scrollTop: number) => {
    const pane = readingPane();
    expect(pane).not.toBeNull();
    pane!.scrollTop = scrollTop;
    await act(async () => {
      pane!.dispatchEvent(new Event('scroll', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 60));
    });
  };

  /** 存储零改动断言（a1 核心：自动层不落盘）：键下本课永不出现 true 条目
   * （手动展开写回 false 删条目为 f3 既有语义，不视为自动层落盘）。 */
  const expectStorageUntouched = () => {
    const raw = storageMap.get(GRAPH_HEADER_COLLAPSED_KEY);
    const parsed: unknown = raw === undefined ? null : JSON.parse(raw!);
    expect((parsed as Record<string, unknown> | null)?.['course-graph-1']).toBeUndefined();
  };

  const tabByTitle = (title: string): HTMLElement | null =>
    Array.from(container.querySelectorAll<HTMLElement>('.arco-tabs-header-title')).find(
      (tab) => (tab.textContent ?? '').includes(title)
    ) ?? null;

  test('滚过阈值（200 ≥ 160）自动折叠：摘要条在场、存储零改动', async () => {
    await mountGraphWorkspace();
    await scrollPane(200);
    const text = container.textContent ?? '';
    // 完整头部退场，摘要条接管
    expect(text).not.toContain('知识结构');
    expect(text).toContain('3 条主线');
    expect(headerToggle('展开头部')).not.toBeNull();
    // 自动态不落盘（滚动深度是视口语境不是偏好）
    expectStorageUntouched();
  });

  test('回顶（0 < 80）展开：完整头部恢复', async () => {
    await mountGraphWorkspace();
    await scrollPane(200);
    expect(container.textContent).not.toContain('知识结构');
    await scrollPane(0);
    expect(container.textContent).toContain('知识结构');
    expect(headerToggle('收起头部')).not.toBeNull();
    expectStorageUntouched();
  });

  test('否决臂：自动折叠中手动展开 → 深滚不再自动折叠；回顶复位后新周期恢复', async () => {
    await mountGraphWorkspace();
    await scrollPane(200);
    expect(container.textContent).not.toContain('知识结构');
    // auto 折叠态点展开钮：否决臂置位（本滚动周期压制自动折叠）
    await act(async () => {
      headerToggle('展开头部')!.click();
      await Promise.resolve();
    });
    expect(container.textContent).toContain('知识结构');
    expectStorageUntouched();
    // 深滚：否决臂在场，不自动折叠
    await scrollPane(300);
    expect(container.textContent).toContain('知识结构');
    // 回顶复位 → 再下滑：自动折叠恢复
    await scrollPane(0);
    await scrollPane(200);
    expect(container.textContent).not.toContain('知识结构');
  });

  test('persisted 恒折叠：偏好折叠下任何滚动深度都折叠（尊重偏好）', async () => {
    storageMap.set(GRAPH_HEADER_COLLAPSED_KEY, '{"course-graph-1":true}');
    await mountGraphWorkspace();
    expect(container.textContent).not.toContain('知识结构');
    await scrollPane(0);
    expect(container.textContent).not.toContain('知识结构');
    await scrollPane(300);
    expect(container.textContent).not.toContain('知识结构');
  });

  test('切 TabPane 复位：滚深自动折叠后切走页签，临时态清零', async () => {
    await mountGraphWorkspace();
    await scrollPane(200);
    expect(container.textContent).not.toContain('知识结构');
    await act(async () => {
      tabByTitle('学习记录')!.click();
      await Promise.resolve();
    });
    // 临时态清零：头部展开（摘要条退场）
    expect(container.textContent).toContain('知识结构');
    expectStorageUntouched();
  });

  test('切课复位：自动折叠不落盘，换课重挂载从展开态起步', async () => {
    await mountGraphWorkspace();
    await scrollPane(200);
    expect(container.textContent).not.toContain('知识结构');
    expectStorageUntouched();
    // 换课（重挂载另一课）：临时层随卸载消亡 + 无持久折叠 → 展开
    await mountGraphWorkspace({ courseId: 'course-graph-2' });
    expect(container.textContent).toContain('知识结构');
    expect(headerToggle('收起头部')).not.toBeNull();
  });
});

describe('LearningGraphWorkspace 折叠钮显性化（a2）', () => {
  // Tooltip 弹层断言走 LearningGraphWorkspace.header-collapse.structure.test.ts
  // （仓库先例：bun test 宿主 import 提升先于 jsdom globals 注入，arco
  // isServerRendering 在加载期恒 true → portal 弹层不渲染，悬停显形无法在
  // 本宿主观察；DOM 面断 hit area / 图标 / 摘要条钮序，接线面断 Tooltip）。
  test('展开态收起钮：hit area 常规（无 !px-2px）、图标 14px', async () => {
    await mountGraphWorkspace();
    const toggle = headerToggle('收起头部')!;
    expect(toggle.className).not.toContain('!px-2px');
    const icon = toggle.querySelector('.arco-icon');
    expect(icon).not.toBeNull();
    // arco 图标是 <svg>（className 为 SVGAnimatedString），走 classList 断言
    expect(icon!.classList.contains('text-14px')).toBe(true);
    expect(icon!.classList.contains('text-12px')).toBe(false);
  });

  test('折叠态展开钮：移到摘要文本之后、图标 14px、课程名转纯文本', async () => {
    await mountGraphWorkspace();
    await act(async () => {
      headerToggle('收起头部')!.click();
      await Promise.resolve();
    });
    const toggle = headerToggle('展开头部')!;
    expect(toggle.className).not.toContain('!px-2px');
    const icon = toggle.querySelector('.arco-icon');
    expect(icon).not.toBeNull();
    expect(icon!.classList.contains('text-14px')).toBe(true);
    // 视觉动线：先读摘要再见钮——展开钮 DOM 位置在摘要文本之后
    const summary = Array.from(
      container.querySelectorAll('.arco-typography')
    ).find((node) => (node.textContent ?? '').includes('条主线'));
    expect(summary).toBeDefined();
    expect(
      summary!.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // 课程名从钮内图标+文本组合转纯文本 span（truncate 保留）
    expect(container.textContent).toContain('图课程');
    expect(toggle.textContent).not.toContain('图课程');
    // 点击仍生效（折叠 ⇄ 展开）
    await act(async () => {
      toggle.click();
      await Promise.resolve();
    });
    expect(container.textContent).toContain('知识结构');
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
