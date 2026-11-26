/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 评审留痕段挂载闭环（m1）：结构图页签在 StructureGraphTab 之后挂
 * StructureFindingsLoader——自取数（GET graph/structure/findings）、取数完成
 * 才出段（无 loading 占位）、空态不渲染、默认折叠、展开见水位语义提示与
 * 留痕行；读数失败走 errorMessage 单源映射，后端原文不上屏。
 * 夹具刻意让结构未生成（structure: null）且从未挂载（mounts: null）：挂载卡
 * 与「详情」折叠此时都不存在，留痕段独立于它们——没跑过挂载的课程也看
 * 得到评审看见了什么。
 * jsdom 宿主：LearningGraphWorkspace.header-collapse.test 同仪式——挂载期读
 * 数（结构投影/声明/注册表快照/补批判定/检测进度/历史/动作簿）一概打补丁
 * 并还原（HTTP 边界；不用 mock.module——进程全局常驻，污染全套件）；
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
import { BackendHttpError } from '@/common/adapter/httpBridge';
// LearningModelSelector → useEffectiveModelTier → useCloudAuth 需要 Provider；
// 用真实 Provider（其 ipc effects 在测试宿主失败即降级、被 catch），不 mock
// （bun 的 mock.module 进程全局常驻，污染 test:ui 全套件——同波测试纪律）。
import { CloudAuthProvider } from '@/renderer/hooks/context/CloudAuthContext';
import { ipcBridge } from '@/common';
import { learningApi } from '../api';
import type { CourseDetail, StructureFindingsView } from '../types';
import { StructureFindingsLoader, StructureFindingsSection } from './structure/StructureFindingsSection';
import LearningGraphWorkspace from './LearningGraphWorkspace';

// ── jsdom 宿主（LearningGraphWorkspace.header-collapse.test 同仪式） ─────────────
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

// 内存 localStorage mock（bun test 宿主无可靠 localStorage）。
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

/** 结构投影夹具：结构未生成 + 从未挂载（structure / mounts 皆 null）。 */
const structureView = {
  course_id: 'course-graph-1',
  structure: null,
  terrain: null,
  generated_at: null,
  mounts: null,
};

const graphDetail = (): CourseDetail =>
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
      goal: '冒烟目标',
      recommended: ['l1'],
      blocked: [],
      ready_count: 1,
      ready_target: 7,
      ready_trigger: 3,
      growth_running: false,
    },
  }) as unknown as CourseDetail;

/** 三条留痕（登记面 / 预算留痕 / 概念缺口）；水位 1 = 只有 seq 1 被修补
 * 回合读过。 */
const findingsView: StructureFindingsView = {
  findings: [
    {
      seq: 3,
      ts: 0,
      source: 'concept-review',
      node: 'Option 类型',
      field: 'title',
      issue: '提名别名「Option 类型」与定义不完全对应',
      advice: '拆分别名或分铸',
      read_by_repair: false,
    },
    {
      seq: 2,
      ts: 0,
      source: 'growth-budget',
      node: '',
      field: '',
      issue: '生长预算到期：停在第 3 批',
      advice: '',
      read_by_repair: false,
    },
    {
      seq: 1,
      ts: 0,
      source: 'concept-review',
      node: '容器与接口',
      field: 'teaches',
      issue: '已教概念账中未见容器、接口、元素',
      advice: '补一个节点教它',
      read_by_repair: true,
    },
  ],
  total: 3,
  watermark: 1,
  unread: 2,
};

const noop = () => undefined;

/** 挂载图工作区（header-collapse 同款补丁：声明/补批判定拒绝防真发、历史
 * 为空）；留痕读数由调用方注入，结构投影默认空壳（要数的用例自己给 stub）。 */
const mountGraphWorkspace = async (
  structureFindings: (courseId: string) => Promise<StructureFindingsView>,
  graphStructure: () => Promise<typeof structureView> = async () => structureView
) => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  storageMap.clear();
  learningApi.quizProgress = async () => ({ settled_count: 2, trigger_count: 6, due: false });
  learningApi.graphStructure = graphStructure;
  learningApi.basisView = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.generationTasks = async () => [];
  learningApi.growthVerdict = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.graphHistory = async () => ({ batches: [] });
  learningApi.systemActions = async () => [];
  learningApi.structureFindings = structureFindings;
  const instance = await initTestI18n('zh-CN');
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <LearningGraphWorkspace
              detail={graphDetail()}
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

/** 直接挂纯展示壳（不走工作区）：重取失败后的「旧读数 + 错误」组合态只有
 * 这里好构造——取数壳的重取由 WS 帧驱动，哑插座宿主发不出帧。 */
const mountSection = async (props: { view: StructureFindingsView; error: string }) => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  const instance = await initTestI18n('zh-CN');
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <StructureFindingsSection view={props.view} error={props.error} />
      </I18nextProvider>
    );
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
  structureFindings: learningApi.structureFindings,
};

afterEach(() => {
  for (const [key, value] of Object.entries(realApi)) {
    (learningApi as unknown as Record<string, unknown>)[key] = value;
  }
  storageMap.clear();
});

/** 当前激活页签（TabPane）的文本。 */
const activePane = (): string =>
  container.querySelector('.arco-tabs-content-item-active')?.textContent ?? '';

const activeTabTitle = (): string =>
  container.querySelector('[role="tab"][aria-selected="true"]')?.textContent ?? '';

const clickTab = async (title: string) => {
  const tab = Array.from(container.querySelectorAll('[role="tab"]')).find((element) =>
    (element.textContent ?? '').includes(title)
  );
  expect(tab).toBeDefined();
  await act(async () => {
    (tab as HTMLElement).click();
    await Promise.resolve();
    await Promise.resolve();
  });
};

/** 留痕段标题（折叠头里的标题 Typography）。 */
const findingsTitle = (): HTMLElement | null =>
  Array.from(container.querySelectorAll<HTMLElement>('.arco-typography')).find(
    (node) => (node.textContent ?? '').trim() === '评审留痕'
  ) ?? null;

describe('LearningGraphWorkspace 评审留痕段（m1）', () => {
  test('挂在结构图页签、取数完成才出段：结构未生成且从未挂载的课程也看得到', async () => {
    const calls: string[] = [];
    let resolveFindings: (view: StructureFindingsView) => void = () => undefined;
    await mountGraphWorkspace((courseId) => {
      calls.push(courseId);
      return new Promise((resolve) => {
        resolveFindings = resolve;
      });
    });
    await clickTab('结构图');
    expect(activeTabTitle()).toBe('结构图');
    // 夹具前提：StructureGraphTab 在场（结构未生成空态），挂载卡不存在。
    expect(activePane()).toContain('尚未生成');
    expect(activePane()).not.toContain('内容与目录对应');
    // 取数未完成：段不渲染（无 loading 占位）。
    expect(activePane()).not.toContain('评审留痕');
    expect(calls).toEqual(['course-graph-1']);

    await act(async () => {
      resolveFindings(findingsView);
    });
    const pane = activePane();
    expect(pane).toContain('评审留痕');
    expect(pane).toContain('最近 3 条');
    expect(pane).toContain('修补未读 2');
    // 位置：紧跟 StructureGraphTab（在其空态之后）。
    expect(pane.indexOf('尚未生成')).toBeLessThan(pane.indexOf('评审留痕'));
  });

  test('默认折叠；展开见水位语义提示与留痕行', async () => {
    await mountGraphWorkspace(async () => findingsView);
    await clickTab('结构图');
    const title = findingsTitle();
    expect(title).not.toBeNull();
    expect(activePane()).not.toContain('只表示修补回合读过这条留痕');
    expect(activePane()).not.toContain('已教概念账中未见容器、接口、元素');

    await act(async () => {
      title!.click();
    });
    const pane = activePane();
    expect(pane).toContain('只表示修补回合读过这条留痕');
    expect(pane).toContain('提名别名「Option 类型」与定义不完全对应');
    expect(pane).toContain('已教概念账中未见容器、接口、元素');
    expect(pane).toContain('生长预算到期：停在第 3 批');
    // 「修补已读」：提示行 1 次 + 仅 seq 1 那一行（水位 = 1）。
    expect(pane.split('修补已读').length - 1).toBe(2);
  });

  test('零留痕：整段不渲染', async () => {
    await mountGraphWorkspace(async () => ({
      findings: [],
      total: 0,
      watermark: 0,
      unread: 0,
    }));
    await clickTab('结构图');
    expect(activePane()).toContain('尚未生成');
    expect(activePane()).not.toContain('评审留痕');
  });

  test('读数失败：段落不消失，展开见映射后的错误句；后端原文不上屏', async () => {
    await mountGraphWorkspace(async () => {
      throw new BackendHttpError({
        method: 'GET',
        path: '/api/learning/courses/course-graph-1/graph/structure/findings?limit=50',
        status: 500,
        body: { success: false, error: 'Internal error: secret-backend-detail', code: 'INTERNAL' },
      });
    });
    await clickTab('结构图');
    // 标题行留在场，无计数标记。
    expect(activePane()).toContain('评审留痕');
    expect(activePane()).not.toContain('最近');
    expect(activePane()).not.toContain('修补未读');

    const title = findingsTitle();
    expect(title).not.toBeNull();
    await act(async () => {
      title!.click();
    });
    const pane = activePane();
    expect(container.querySelectorAll('.arco-alert-error')).toHaveLength(1);
    expect(pane).toContain('服务暂时不可用，请稍后再试');
    expect(pane).not.toContain('secret-backend-detail');
    expect(pane).not.toContain('只表示修补回合读过这条留痕');
  });

  test('重取失败保留上次读数：错误条置顶，标题计数与留痕行照旧（审计面不因一次刷新失败清空）', async () => {
    await mountSection({ view: findingsView, error: '服务暂时不可用，请稍后再试' });
    // 标题行的计数照旧（来自上一次成功的读数）。
    expect(container.textContent).toContain('最近 3 条');
    const title = findingsTitle();
    expect(title).not.toBeNull();
    await act(async () => {
      title!.click();
    });
    const text = container.textContent ?? '';
    expect(container.querySelectorAll('.arco-alert-error')).toHaveLength(1);
    expect(text).toContain('已教概念账中未见容器、接口、元素');
    expect(text).toContain('只表示修补回合读过这条留痕');
    // 错误条在行列表之前。
    expect(text.indexOf('服务暂时不可用，请稍后再试')).toBeLessThan(
      text.indexOf('提名别名「Option 类型」')
    );
  });
});

// ── 修复批 A13 / A14：重取竞态与刷新面 ──────────────────────────────────────

type GenerationFrame = { course_id: string; event?: string };

/** 捕获 `learning.courseGeneration` 的订阅者：工作区里多个消费方各自订阅
 * （结构投影 / 留痕段 / 生成事件 hook），测试里统一收下再手动发帧。直接换
 * 属性、afterEach 还原——不用 mock.module（进程全局常驻，污染全套件）。 */
const originalCourseGenerationOn = ipcBridge.learning.courseGeneration.on;
const captureGenerationFrames = () => {
  const handlers = new Set<(frame: GenerationFrame) => void>();
  (
    ipcBridge.learning.courseGeneration as unknown as {
      on: (callback: (frame: GenerationFrame) => void) => () => void;
    }
  ).on = (callback) => {
    handlers.add(callback);
    return () => {
      handlers.delete(callback);
    };
  };
  return {
    fire: (frame: GenerationFrame) => {
      for (const handler of [...handlers]) handler(frame);
    },
    handlerCount: () => handlers.size,
  };
};

/** 只挂留痕取数壳（取数壳的重取由 WS 帧驱动，工作区里同时有多个订阅方，
 * 单独挂壳才能把 `structureFindings` 的并发请求控准）。 */
const mountFindingsLoader = async (courseId: string) => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  storageMap.clear();
  const instance = await initTestI18n('zh-CN');
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <StructureFindingsLoader courseId={courseId} />
      </I18nextProvider>
    );
  });
};

/** 一帧读数（只填断言要用的字段）。 */
const findingsPage = (
  seq: number,
  overrides: Partial<StructureFindingsView> = {}
): StructureFindingsView => ({
  findings: [
    {
      seq,
      ts: 0,
      source: 'concept-review',
      node: '',
      field: 'teaches',
      issue: `第 ${seq} 条留痕`,
      advice: '',
      read_by_repair: true,
    },
  ],
  total: 1,
  watermark: seq,
  unread: 0,
  ...overrides,
});

afterEach(() => {
  (
    ipcBridge.learning.courseGeneration as unknown as {
      on: typeof originalCourseGenerationOn;
    }
  ).on = originalCourseGenerationOn;
});

describe('评审留痕段的重取（修复批）', () => {
  test('重取重叠：迟到的旧响应不覆盖较新的读数（A13）', async () => {
    const frames = captureGenerationFrames();
    const pending: ((view: StructureFindingsView) => void)[] = [];
    learningApi.structureFindings = () =>
      new Promise<StructureFindingsView>((resolve) => {
        pending.push(resolve);
      });

    await mountFindingsLoader('course-graph-1');
    expect(pending).toHaveLength(1);

    // 两帧连到（growth_* 成串到达的常态）：第二个请求在第一个未回时发出。
    await act(async () => {
      frames.fire({ course_id: 'course-graph-1', event: 'growth_completed' });
      frames.fire({ course_id: 'course-graph-1', event: 'growth_completed' });
    });
    expect(pending).toHaveLength(3);

    // 先回最新的（第 3 个请求），再回最早那个——旧响应迟到。
    await act(async () => {
      pending[2](findingsPage(9, { unread: 0 }));
      await Promise.resolve();
    });
    await act(async () => {
      pending[0](findingsPage(1, { unread: 5 }));
      await Promise.resolve();
    });

    // 行在折叠内容里（默认收起）：先看标题读数，再展开看行。
    expect(container.textContent ?? '').toContain('最近 1 条');
    expect(container.textContent ?? '').not.toContain('修补未读');
    const title = findingsTitle();
    expect(title).not.toBeNull();
    await act(async () => {
      title!.click();
    });
    const text = container.textContent ?? '';
    expect(text).toContain('第 9 条留痕');
    expect(text).not.toContain('第 1 条留痕');
  });
});

describe('结构投影跟随后台回合的终态帧（A14 / B7）', () => {
  test('修补 / 挂载终态帧各自重取一次；开始帧与别的课程不重取', async () => {
    const frames = captureGenerationFrames();
    let structureCalls = 0;
    await mountGraphWorkspace(async () => findingsView, async () => {
      structureCalls += 1;
      return structureView;
    });
    const baseline = structureCalls;
    expect(baseline).toBeGreaterThan(0);

    const fireAndSettle = async (frame: GenerationFrame) => {
      await act(async () => {
        frames.fire(frame);
        await Promise.resolve();
        await Promise.resolve();
      });
    };

    for (const event of [
      'structure_repair_completed',
      'structure_repair_failed',
      'structure_repair_cancelled',
      'structure_mount_completed',
      'structure_mount_failed',
      'structure_mount_cancelled',
    ]) {
      const before = structureCalls;
      await fireAndSettle({ course_id: 'course-graph-1', event });
      // 每个终态帧恰重取一次。
      if (structureCalls !== before + 1) {
        throw new Error(`${event} 必须重取一次结构投影（实际 ${structureCalls - before} 次）`);
      }
    }

    // 开始帧：那一刻投影还没变，不重取。
    for (const event of ['structure_repair_started', 'structure_mount_started']) {
      const before = structureCalls;
      await fireAndSettle({ course_id: 'course-graph-1', event });
      if (structureCalls !== before) {
        throw new Error(`${event} 不该重取结构投影（实际重取 ${structureCalls - before} 次）`);
      }
    }
    // 别的课程的终态帧不重取本课的投影。
    const beforeOther = structureCalls;
    await fireAndSettle({ course_id: 'another-course', event: 'structure_repair_completed' });
    expect(structureCalls).toBe(beforeOther);
  });
});
