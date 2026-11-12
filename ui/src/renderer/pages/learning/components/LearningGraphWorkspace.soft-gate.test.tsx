/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 软闸行动承诺兑现（u8，Q4 裁决）：R1 软闸建议条带「去复习供给节点」行动
 * ——点击选中供给节点进学习视图（选中即进内容的既有主推荐卡路径）；供给
 * 节点不在本课（跨课程供给）时不出行动钮（保持纯文字建议，不做假跳转）。
 * R2 学习记录页签节点行可点——被挡节点与已完成节点都可进（CONTEXT.md 软闸
 * 词条「学习者仍可从历史记录进入任何节点」）。行动文案双语断言（en-US 用例
 * 零中文回落，QuizOverviewBanner.render.test 双语模式）。
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
import type { CourseDetail, GraphBatchView } from '../types';
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

/** 软闸盘面（u8）：l1 就绪（推荐首位）/ l2 供给节点（已完成，非就绪集）/
 * l3 被挡节点（结构就绪但供给已遗忘）。supplierId 可注入跨课程 id 以验
 * 行动钮守卫臂。 */
const graphDetail = (supplierId: string): CourseDetail =>
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
      total_lessons: 3,
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
        ],
      },
    ],
    next_lesson_id: null,
    due_review_count: 0,
    overloaded_lessons: 0,
    graph: {
      goal: '冒烟目标',
      recommended: ['l1'],
      blocked: [
        {
          lesson_id: 'l3',
          title: '节点丙',
          supplier_lesson_id: supplierId,
          supplier_title: '节点乙',
          r: 0.6,
          due_count: 3,
        },
      ],
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
 * 补丁（growthVerdict 必须拒绝，防自动补批 POST 真发）。 */
const mountGraphWorkspace = async (options?: {
  supplierId?: string;
  batches?: GraphBatchView[];
  locale?: 'zh-CN' | 'en-US';
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
              detail={graphDetail(options?.supplierId ?? 'l2')}
              busyId={null}
              attemptResults={{}}
              quizProgressToken={0}
              onBack={noop}
              onProgress={noop}
              onAttempt={noop}
              onGenerate={noop}
              onStartQuiz={noop}
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
};

afterEach(() => {
  for (const [key, value] of Object.entries(realApi)) {
    (learningApi as unknown as Record<string, unknown>)[key] = value;
  }
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

describe('LearningGraphWorkspace 软闸行动（u8 R1/R2）', () => {
  test('R1：软闸建议条带「去复习供给节点」行动，点击选中供给节点（非就绪集渲染）', async () => {
    await mountGraphWorkspace();
    const action = buttonWithText('去复习供给节点');
    expect(action).not.toBeNull();
    // 初始选中 = 推荐首位 l1（节点甲）
    expect(selectedHeading()).toBe('节点甲');
    await act(async () => {
      action!.click();
      await Promise.resolve();
    });
    // 点击后：供给节点 l2（已完成、非就绪集）成为当前选中
    expect(selectedHeading()).toBe('节点乙');
    expect(activeTabTitle()).toBe('学习');
  });

  test('R1 en-US：行动文案同走词典（零中文回落），点击语义不变', async () => {
    await mountGraphWorkspace({ locale: 'en-US' });
    const action = buttonWithText('Review supplier node');
    expect(action).not.toBeNull();
    expect(selectedHeading()).toBe('节点甲');
    await act(async () => {
      action!.click();
      await Promise.resolve();
    });
    expect(selectedHeading()).toBe('节点乙');
    // en-US 词典的页签标题（节点名是夹具数据不走词典，保持 zh）
    expect(activeTabTitle()).toBe('Study');
    // 零中文回落
    expect(container.textContent).not.toContain('去复习供给节点');
  });

  test('R1 守卫：供给节点不在本课（跨课程供给）时不出行动钮（无假跳转）', async () => {
    await mountGraphWorkspace({ supplierId: 'l-other-course' });
    expect(buttonWithText('去复习供给节点')).toBeNull();
    // 建议文案仍在（纯文字降级，与既有形态一致）
    expect(container.textContent).toContain('节点乙');
  });

  test('R2：学习记录行可点——被挡节点行点击进该节点课时', async () => {
    await mountGraphWorkspace();
    await clickTab('学习记录');
    const row = buttonWithText('节点丙');
    expect(row).not.toBeNull();
    await act(async () => {
      row!.click();
      await Promise.resolve();
    });
    expect(activeTabTitle()).toBe('学习');
    expect(selectedHeading()).toBe('节点丙');
  });

  test('R2：学习记录行可点——已完成节点行点击进该节点课时', async () => {
    await mountGraphWorkspace();
    await clickTab('学习记录');
    const row = buttonWithText('节点乙');
    expect(row).not.toBeNull();
    await act(async () => {
      row!.click();
      await Promise.resolve();
    });
    expect(activeTabTitle()).toBe('学习');
    expect(selectedHeading()).toBe('节点乙');
  });
});
