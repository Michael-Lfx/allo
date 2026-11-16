/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 软闸行动闭环（u8 R1/R2 + u9 R3/R4）：u8 落「跳转供给节点」，u9 聚合为
 * 一条建议条并把行动闭环到「复习队列真实推进」（票面机制结论：r 回升唯一
 * 途径 = 复习队列推进）——主行动「复习薄弱节点」回调携带去重后的供给节点
 * 集（index.tsx 接 startScopedReviewSession：due_only=false 早练语义 +
 * node_id 过滤，参数透传在 hook 探针 describe 钉死）；次行动「重读正文」=
 * u8 既有 openNode 跳转降级（仅最差供给节点在本课时可得，跨课程供给不做
 * 假跳转）；R4 文案事实分支（due 求和 > 0 报到期数 / = 0 呈早练语境）；
 * 降噪：选中节点进入薄弱集后建议条收起、离开恢复。R2 学习记录页签节点行
 * 可点不变。行动文案双语断言（en-US 用例零中文回落，
 * QuizOverviewBanner.render.test 双语模式）。
 * jsdom 宿主：LearningGraphWorkspace.quiz-entry.test 同仪式——挂载期读数
 * （结构投影/声明/注册表快照/补批判定/检测进度）一概打补丁并还原（HTTP
 * 边界）；学习记录/系统动作两读数按用例注入批次行；WebSocket 换哑插座。 */
import * as bunTest from 'bun:test';
const { afterEach, describe, expect, test } = bunTest;
// The pinned bun-types in this checkout predate the afterAll declaration
// (same stale-types gap as CourseWorkspace.quiz-entry.test.tsx).
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import { JSDOM } from 'jsdom';
import React, { act, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { AppMessage, notificationStore } from '@/renderer/components/notifications';
// LearningModelSelector → useEffectiveModelTier → useCloudAuth 需要 Provider；
// 用真实 Provider（其 ipc effects 在测试宿主失败即降级、被 catch），不 mock
// （bun 的 mock.module 进程全局常驻，污染 test:ui 全套件——同波测试纪律）。
import { CloudAuthProvider } from '@/renderer/hooks/context/CloudAuthContext';
import { learningApi } from '../api';
import type { CourseDetail, DueReview, GraphBatchView, GraphBlockedView } from '../types';
import { type Translate } from '../utils';
import { useReviewSession } from '../hooks/useReviewSession';
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

/** 软闸盘面（u8→u9）：l1 就绪（推荐首位）/ l2 供给节点（已完成，非就绪
 * 集）/ l3、l4 被挡节点（结构就绪但供给已遗忘）。blocked 行可注入（缺省
 * 双候选共享供给 l2——聚合去重与 due 求和的最小盘面；supplierId 可注入跨
 * 课程 id 以验次行动守卫臂）。 */
const defaultBlocked = (supplierId: string): GraphBlockedView[] => [
  { lesson_id: 'l3', title: '节点丙', supplier_lesson_id: supplierId, supplier_title: '节点乙', r: 0.61, due_count: 3 },
  { lesson_id: 'l4', title: '节点丁', supplier_lesson_id: supplierId, supplier_title: '节点乙', r: 0.74, due_count: 3 },
];

const graphDetail = (options?: {
  supplierId?: string;
  blocked?: GraphBlockedView[];
}): CourseDetail =>
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
      total_lessons: 4,
      completed_lessons: 1,
      enabled: true,
      settled_or_completed: 1,
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
          lesson('l1', '节点甲', 'not_started'),
          lesson('l2', '节点乙', 'completed'),
          lesson('l3', '节点丙', 'not_started'),
          lesson('l4', '节点丁', 'not_started'),
        ],
      },
    ],
    next_lesson_id: null,
    due_review_count: 0,
    overloaded_lessons: 0,
    graph: {
      goal: '冒烟目标',
      recommended: ['l1'],
      blocked: options?.blocked ?? defaultBlocked(options?.supplierId ?? 'l2'),
      ready_count: 1,
      ready_target: 7,
      ready_trigger: 3,
      growth_running: false,
    },
  }) as unknown as CourseDetail;

const batchWith = (nodes: Array<{ lesson_id: string; title: string; status: string }>): GraphBatchView[] => [
  {
    batch_id: 'b2',
    seq: 2,
    status: 'applied',
    note: '',
    created_at: 1_700_000_010_000,
    nodes: nodes.map((node) => ({
      lesson_id: node.lesson_id,
      title: node.title,
      est: 10,
      status: node.status,
      completed_at: node.status === 'completed' ? 1_700_000_005_000 : null,
    })),
  } as unknown as GraphBatchView,
];

const noop = () => undefined;

/** 挂载图工作区：注入软闸批次（缺省含被挡行与已完成行各一）与挂载期读数
 * 补丁（growthVerdict 必须拒绝，防自动补批 POST 真发）。onStartWeakReview
 * 缺省把回调实参记入 capturedWeakCalls（主行动回调契约断言用）。 */
const mountGraphWorkspace = async (options?: {
  supplierId?: string;
  blocked?: GraphBlockedView[];
  batches?: GraphBatchView[];
  locale?: 'zh-CN' | 'en-US';
  onStartWeakReview?: (nodeIds: string[]) => void;
}) => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  learningApi.quizProgress = async () => ({ settled_count: 8, trigger_count: 6, due: false });
  learningApi.graphStructure = async () =>
    ({ course_id: 'course-graph-1', structure: null, terrain: null, generated_at: null, mounts: null });
  learningApi.basisView = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.generationTasks = async () => [];
  learningApi.growthVerdict = async () => {
    throw new Error('off (test fixture)');
  };
  learningApi.graphHistory = async () => ({
    batches: options?.batches ?? batchWith([
      { lesson_id: 'l3', title: '节点丙', status: 'not_started' },
      { lesson_id: 'l2', title: '节点乙', status: 'completed' },
    ]),
  });
  learningApi.systemActions = async () => [];
  const instance = await initTestI18n(options?.locale ?? 'zh-CN');
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <I18nextProvider i18n={instance}>
        <CloudAuthProvider>
          <MemoryRouter>
            <LearningGraphWorkspace
              detail={graphDetail(options)}
              busyId={null}
              attemptResults={{}}
              quizProgressToken={0}
              onBack={noop}
              onProgress={noop}
              onAttempt={noop}
              onGenerate={noop}
              onStartQuiz={noop}
              onStartWeakReview={options?.onStartWeakReview ?? noop}
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
  listDueReviews: learningApi.listDueReviews,
};

afterEach(() => {
  for (const [key, value] of Object.entries(realApi)) {
    (learningApi as unknown as Record<string, unknown>)[key] = value;
  }
  // toast 落 notificationStore 是进程全局（AppMessage 无宿主也可入队），
  // 清场防跨用例渗漏（QuizOverviewBanner.render.test 同仪式）。
  AppMessage.clear();
});

const buttonWithText = (text: string): HTMLButtonElement | null => {
  const buttons = Array.from(container.querySelectorAll('button'));
  return buttons.find((button) => (button.textContent ?? '').includes(text)) ?? null;
};

/** 学习视图正文标题（选中节点的 h4；侧栏/推荐卡/历史行均不用 h4）。 */
const selectedHeading = (): string | null => {
  const heading = container.querySelector('.arco-tabs-content-item-active h4');
  return heading ? (heading.textContent ?? '') : null;
};

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
  });
};

const alertCount = (): number => container.querySelectorAll('.arco-alert').length;

describe('LearningGraphWorkspace 软闸行动闭环（u9 R3/R4）', () => {
  test('R3：多条 blocked 聚合为一条——节点名按最差 r 升序去重列出，due 求和入文案', async () => {
    // 两个候选、两个供给节点（l2 r0.61 / l3 r0.74）——后端 r 升序保序。
    // 供给不取 l1（推荐首位 = 初始选中）：选中 ∈ 薄弱集会让降噪臂收起建议
    // 条（规格行为，R4 降噪用例单独验），本用例只看聚合形态。
    await mountGraphWorkspace({
      blocked: [
        { lesson_id: 'l3', title: '节点丙', supplier_lesson_id: 'l2', supplier_title: '节点乙', r: 0.61, due_count: 3 },
        { lesson_id: 'l4', title: '节点丁', supplier_lesson_id: 'l3', supplier_title: '节点丙', r: 0.74, due_count: 1 },
      ],
    });
    expect(alertCount()).toBe(1);
    const text = container.textContent ?? '';
    expect(text).toContain('薄弱节点：节点乙、节点丙');
    expect(text).toContain('预测回忆最低 61%');
    // R4 文案事实分支：due 求和（跨供给 3+1）> 0 → 报到期数
    expect(text).toContain('4 道到期卡');
    expect(text).not.toContain('卡未到期');
  });

  test('R3：主行动「复习薄弱节点」回调携带去重后的供给节点集（跨课程供给不受守卫限）', async () => {
    const captured: string[][] = [];
    await mountGraphWorkspace({
      blocked: [
        { lesson_id: 'l3', title: '节点丙', supplier_lesson_id: 'l2', supplier_title: '节点乙', r: 0.61, due_count: 3 },
        { lesson_id: 'l4', title: '节点丁', supplier_lesson_id: 'l-other-course', supplier_title: '别课节点', r: 0.74, due_count: 1 },
      ],
      onStartWeakReview: (nodeIds) => captured.push(nodeIds),
    });
    const action = buttonWithText('复习薄弱节点');
    expect(action).not.toBeNull();
    await act(async () => {
      action!.click();
      await Promise.resolve();
    });
    // 去重保序 + 跨课程供给照常进集合（复习会话按用户取卡，可跨课程推卡）
    expect(captured).toEqual([['l2', 'l-other-course']]);
  });

  test('R3：次行动「重读正文」= u8 跳转降级——点击选中最差供给节点（非就绪集渲染）', async () => {
    await mountGraphWorkspace();
    expect(selectedHeading()).toBe('节点甲');
    const reread = buttonWithText('重读正文');
    expect(reread).not.toBeNull();
    await act(async () => {
      reread!.click();
      await Promise.resolve();
    });
    // 最差供给节点 l2（已完成、非就绪集）成为当前选中
    expect(selectedHeading()).toBe('节点乙');
    expect(activeTabTitle()).toBe('学习');
  });

  test('R3 守卫：最差供给节点跨课程时不出「重读正文」（无假跳转），建议文案仍在', async () => {
    await mountGraphWorkspace({ supplierId: 'l-other-course' });
    expect(buttonWithText('重读正文')).toBeNull();
    // 主行动不受守卫限（复习会话可达跨课程卡）
    expect(buttonWithText('复习薄弱节点')).not.toBeNull();
    // 建议文案仍在（供给标题是词典外夹具数据，zh/en 均原样）
    expect(container.textContent).toContain('节点乙');
  });

  test('R4 降噪：选中节点进入薄弱集后建议条收起，离开薄弱集恢复', async () => {
    await mountGraphWorkspace();
    expect(alertCount()).toBe(1);
    // 重读正文 → 选中 = 供给节点 l2（薄弱集成员）→ 整条收起
    await act(async () => {
      buttonWithText('重读正文')!.click();
      await Promise.resolve();
    });
    expect(alertCount()).toBe(0);
    // 经学习记录行进入非薄弱节点 l3 → 建议条恢复
    await clickTab('学习记录');
    await act(async () => {
      buttonWithText('节点丙')!.click();
      await Promise.resolve();
    });
    expect(activeTabTitle()).toBe('学习');
    expect(alertCount()).toBe(1);
  });

  test('R4 文案分支：due 求和 = 0 → 呈早练语境（非死态），不报到期数', async () => {
    await mountGraphWorkspace({
      blocked: [
        { lesson_id: 'l3', title: '节点丙', supplier_lesson_id: 'l2', supplier_title: '节点乙', r: 0.61, due_count: 0 },
      ],
    });
    const text = container.textContent ?? '';
    expect(text).toContain('卡未到期，可提前复习巩固');
    expect(text).not.toContain('道到期卡');
  });

  test('R4 en-US：聚合条与双行动同走词典（零中文回落）', async () => {
    await mountGraphWorkspace({ locale: 'en-US' });
    const text = container.textContent ?? '';
    expect(text).toContain('Weak nodes:');
    // 缺省夹具两候选共享供给（同供给同 due_count，真实口径），去重后求和 3
    expect(text).toContain('3 due cards');
    expect(buttonWithText('Review weak nodes')).not.toBeNull();
    expect(buttonWithText('Re-read content')).not.toBeNull();
    expect(buttonWithText('Review weak nodes')!.textContent).not.toContain('复习薄弱节点');
    expect(text).not.toContain('薄弱节点');
    // en-US 词典的页签标题（节点名是夹具数据不走词典，保持 zh）
    expect(activeTabTitle()).toBe('Study');
  });
});

// ── scoped 会话参数透传（u9 R3；hook 探针零 HTTP——api 替身记账） ───────────

const reviewCard = (id: string): DueReview =>
  ({
    id,
    source: 'course',
    enrollment_id: 'enr-1',
    course_id: 'course-graph-1',
    course_title: '图课程',
    module_title: '学习图',
    lesson_title: '节点乙',
    question: { activity_id: `act-${id}`, kind: 'single_choice', prompt: 'P?', options: [], matches: [] },
    due_at: 1,
    stability_days: 1,
    difficulty: 5,
    review_count: 1,
    lapse_count: 0,
    r: 0.6,
    edit_pending: false,
    edit_note: null,
  }) as unknown as DueReview;

interface ScopedProbe {
  startScopedReviewSession: (nodeIds: string[]) => Promise<boolean>;
  getState: () => { sessionOpen: boolean; queueLen: number };
}

/** useReviewSession 探针：t 用记参替身（learning.* 无参键走 en-US 词典
 * 实证——空队列 toast 断言真实文案而非键名，同文件 R4 en-US 用例模式；
 * 未收录键与带参键保持替身原样），api 替身在用例内打补丁（afterEach 统一
 * 还原）。onReady 只发一次稳定句柄——start 与 getState 都经 ref 间接，读
 * 的永远是最新渲染的 state。 */
const ScopedHookProbe: React.FC<{ onReady: (probe: ScopedProbe) => void }> = ({ onReady }) => {
  const session = useReviewSession({
    load: async () => undefined,
    t: ((key: string, params?: Record<string, unknown>) => {
      if (params !== undefined) return `${key}:${JSON.stringify(params)}`;
      const [scope, leaf] = key.split('.');
      const resolved =
        scope === 'learning' ? (learningEn as Record<string, unknown>)[leaf] : undefined;
      return typeof resolved === 'string' ? resolved : key;
    }) as unknown as Translate,
    reviewCourseFilter: [],
    reviewTagFilter: [],
    reviewSessionLimit: 30,
    setBusyId: () => undefined,
    setReviews: () => undefined,
  });
  const stateRef = React.useRef({ sessionOpen: false, queueLen: 0 });
  stateRef.current = { sessionOpen: session.sessionOpen, queueLen: session.sessionQueue.length };
  const startRef = React.useRef(session.startScopedReviewSession);
  startRef.current = session.startScopedReviewSession;
  const readyRef = React.useRef(onReady);
  readyRef.current = onReady;
  useEffect(() => {
    readyRef.current({
      startScopedReviewSession: (nodeIds) => startRef.current(nodeIds),
      getState: () => stateRef.current,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return null;
};

const mountScopedProbe = async (): Promise<ScopedProbe> => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  container.innerHTML = '';
  let probe: ScopedProbe | null = null;
  root = createRoot(container);
  await act(async () => {
    root!.render(<ScopedHookProbe onReady={(value) => { probe = value; }} />);
  });
  return probe as unknown as ScopedProbe;
};

describe('startScopedReviewSession 参数透传（u9 R3）', () => {
  test('透传 dueOnly=false + 多值 nodeIds（limit 100、无课程过滤）并开窗', async () => {
    const calls: Array<{ limit: number; courseIds: string[] | string; options?: { dueOnly?: boolean; nodeIds?: string[] } }> = [];
    learningApi.listDueReviews = (async (
      limit: number,
      courseIds: string[] | string,
      options?: { dueOnly?: boolean; nodeIds?: string[] }
    ) => {
      calls.push({ limit, courseIds, options });
      return [reviewCard('r-1')];
    }) as typeof learningApi.listDueReviews;
    const probe = await mountScopedProbe();
    let opened = false;
    await act(async () => {
      opened = await probe.startScopedReviewSession(['l2', 'l1']);
    });
    expect(opened).toBe(true);
    expect(calls).toEqual([
      { limit: 100, courseIds: [], options: { dueOnly: false, nodeIds: ['l2', 'l1'] } },
    ]);
    expect(probe.getState().sessionOpen).toBe(true);
    expect(probe.getState().queueLen).toBe(1);
  });

  test('空队列不开窗（noScopedReviews 提示语义）；空 nodeIds 直接拒绝零请求', async () => {
    const calls: unknown[] = [];
    learningApi.listDueReviews = (async () => {
      calls.push(1);
      return [];
    }) as typeof learningApi.listDueReviews;
    const toastKeysBefore = new Set(notificationStore.getSnapshot().map((record) => record.key));
    const probe = await mountScopedProbe();
    let opened = true;
    await act(async () => {
      opened = await probe.startScopedReviewSession(['l2']);
    });
    expect(opened).toBe(false);
    expect(probe.getState().sessionOpen).toBe(false);
    expect(calls.length).toBe(1);
    // 空队列 toast 文案断言（红线 2.3.3）：恰好一条 info，走 en-US 词典
    // 真实文案（探针 t 以 learningEn 实证解析 learning.* 键），零中文回落。
    const newToasts = notificationStore
      .getSnapshot()
      .filter((record) => !toastKeysBefore.has(record.key));
    expect(newToasts.length).toBe(1);
    expect(newToasts[0].level).toBe('info');
    expect(newToasts[0].content).toBe(learningEn.noScopedReviews);
    expect(newToasts[0].content).not.toBe(learningZh.noScopedReviews);
    let rejected = true;
    await act(async () => {
      rejected = await probe.startScopedReviewSession([]);
    });
    expect(rejected).toBe(false);
    expect(calls.length).toBe(1);
    // 空 nodeIds 臂在拉取前直接拒绝：不追加请求也不追加 toast
    const toastsAfterReject = notificationStore
      .getSnapshot()
      .filter((record) => !toastKeysBefore.has(record.key));
    expect(toastsAfterReject.length).toBe(1);
  });
});
