/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 结构生成真话面（v2，G2 哑巴面 / G3 失忆面 / G5 防呆）：工作区结构条
 * 的运行判定与徽章读数。可测面（决策逻辑抽离，跟随 useCourseCreation
 * c2 收敛轮询的形态）：
 * - `structureReadoutFromRow`：快照行 → 实时读数（attempt/elapsed 全取服务
 *   端真相，缺任一即回退静态面）；
 * - `structureBadgeCopy`：徽章文案机面（读数在 → 插值键；缺 → 复用静态键）；
 * - `startStructureReadoutPolling`：running 期间的 5s 收敛轮询循环（终态
 *   即停、快照不可达保留现值、缺行回退静态继续等终态），数据源注入；
 * - `hasActiveStructureRow`：刷新快照并集的 structure 类 active 判定
 *   （d7 `refreshRegistryBusy` 四类漏了本尊的补位）。
 * 工作区接线（三源并集派生 / 生长按钮守卫 / WS 失败清态不回归 / 快照终态
 * 接棒）以源断言钉住（multiSourceDisclosure 的形态——组件无渲染测试基建，
 * ipcBridge 订阅面不宜在单测内挂载）；新词典 key 按红线 2 用真实 JSON
 * 断言，双语对称 + 占位符对称 + 真实例渲染最终句。 */
import { readFileSync } from 'node:fs';
import * as bunTest from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { initReactI18next } from 'react-i18next';
import type { LearningGenerationTaskView } from './types';
import learningEn from '../../services/i18n/locales/en-US/learning.json';
import learningZh from '../../services/i18n/locales/zh-CN/learning.json';

const { describe, expect, test } = bunTest;

const readSource = (url: URL): string => readFileSync(url, 'utf8');

const {
  STRUCTURE_MAX_ATTEMPTS,
  STRUCTURE_READOUT_POLL_MS,
  hasActiveStructureRow,
  structureBadgeCopy,
  structureReadoutFromRow,
  startStructureReadoutPolling,
} = await import('./structureReadout');

interface FakeTimerApi {
  useFakeTimers: () => void;
  useRealTimers: () => void;
  clearAllTimers: () => void;
  advanceTimersByTime: (milliseconds: number) => void;
}

const timers = (bunTest as unknown as { jest: FakeTimerApi }).jest;

/** 冲刷微任务队列：轮询体内的 await 链（快照读数 → 写回/终态）逐跳推进。 */
const flushPromises = async () => {
  for (let hop = 0; hop < 20; hop += 1) await Promise.resolve();
};

/** 推进一个轮询周期并等轮询体跑完。 */
const tick = async () => {
  timers.advanceTimersByTime(STRUCTURE_READOUT_POLL_MS);
  await flushPromises();
};

async function withFakeTimers(body: () => Promise<void>) {
  timers.useFakeTimers();
  try {
    await body();
  } finally {
    timers.clearAllTimers();
    timers.useRealTimers();
  }
}

const COURSE_ID = 'course-1';

const row = (overrides: Partial<LearningGenerationTaskView> = {}): LearningGenerationTaskView => ({
  key: 'structure:course-1:-',
  kind: 'structure',
  course_id: COURSE_ID,
  node_id: null,
  topic: '知识结构生成',
  state: 'running',
  step: null,
  detail: null,
  elapsed_secs: 192,
  queued_at_ms: null,
  queue_position: null,
  // v1 attempt 机器面：结构生成循环逐轮回写（1 起）；None → null。
  attempt: 2,
  ...overrides,
});

interface BadgeCopyLike {
  key: string;
  vars?: Record<string, string | number>;
}

describe('structureReadoutFromRow：快照行 → 实时读数', () => {
  test('running 行带 attempt/elapsed → 读数如实带出', () => {
    expect(structureReadoutFromRow(row())).toEqual({ attempt: 2, elapsedSecs: 192 });
  });

  test('attempt 未回写（null，含旧后端缺字段）→ 回退静态面', () => {
    expect(structureReadoutFromRow(row({ attempt: null }))).toBeNull();
    const legacy = row() as Partial<LearningGenerationTaskView> as LearningGenerationTaskView;
    delete (legacy as { attempt?: number | null }).attempt;
    expect(structureReadoutFromRow(legacy)).toBeNull();
  });

  test('elapsed 缺席（queued 形态：后端仅 running 才计 elapsed_secs）→ 回退静态面', () => {
    expect(structureReadoutFromRow(row({ state: 'queued', elapsed_secs: null }))).toBeNull();
  });
});

describe('structureBadgeCopy：徽章文案机面', () => {
  test('读数在 → structureBadgeRunning，attempt/max/elapsed 插值（elapsed 走 m:ss 格式）', () => {
    const copy = structureBadgeCopy({ attempt: 2, elapsedSecs: 192 });
    expect(copy.key).toBe('learning.structureBadgeRunning');
    expect((copy as BadgeCopyLike).vars).toEqual({ attempt: 2, max: 3, elapsed: '3:12' });
  });

  test('读数缺席（缺行 / attempt 未回写）→ 复用静态「生成中…」键，无插值', () => {
    const copy = structureBadgeCopy(null);
    expect(copy.key).toBe('learning.learningGraphStructureGenerating');
    expect('vars' in copy).toBe(false);
  });

  test('max 与后端 MAX_ATTEMPTS 同源（3）', () => {
    expect(STRUCTURE_MAX_ATTEMPTS).toBe(3);
  });
});

describe('startStructureReadoutPolling：running 期间的收敛轮询', () => {
  /** 假数据源驱动一轮轮询：快照按轮询序逐帧给出（末帧常驻；Error 帧 =
   * 快照不可达），读数写回与终态接棒各自记账。 */
  function startPolling(options: {
    frames: Array<LearningGenerationTaskView[] | Error>;
    onTerminal?: (state: 'done' | 'failed' | 'cancelled') => void;
  }) {
    const readouts: Array<ReturnType<typeof structureReadoutFromRow>> = [];
    const terminals: Array<'done' | 'failed' | 'cancelled'> = [];
    let polls = 0;
    const stop = startStructureReadoutPolling({
      courseId: COURSE_ID,
      fetchTasks: async () => {
        const frame = options.frames[Math.min(polls, options.frames.length - 1)];
        polls += 1;
        if (frame instanceof Error) throw frame;
        return frame;
      },
      onReadout: (readout) => {
        readouts.push(readout);
      },
      onTerminal: (state) => {
        terminals.push(state);
        options.onTerminal?.(state);
      },
    });
    return { stop, readouts, terminals, polls: () => polls };
  }

  test('running 帧翻读数，快照终态（done）→ 接棒一次并停表', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [
          [row()],
          [row({ attempt: 2, elapsed_secs: 195 })],
          [row({ state: 'done', detail: '知识结构已生成', attempt: 2, elapsed_secs: 751 })],
        ],
      });

      await tick();
      expect(run.readouts).toEqual([{ attempt: 2, elapsedSecs: 192 }]);

      await tick();
      expect(run.readouts).toEqual([
        { attempt: 2, elapsedSecs: 192 },
        { attempt: 2, elapsedSecs: 195 },
      ]);

      // 终态：接棒恰好一次（既有 load() 接棒面），此后不再轮询。
      await tick();
      expect(run.terminals).toEqual(['done']);
      expect(run.polls()).toBe(3);

      await tick();
      await tick();
      expect(run.polls()).toBe(3);
      expect(run.terminals).toHaveLength(1);
    });
  });

  test('failed / cancelled 终态原样交接（工作区侧镜像 WS 清旗臂）', async () => {
    await withFakeTimers(async () => {
      const failed = startPolling({
        frames: [[row()], [row({ state: 'failed', detail: 'Provider 连续失败，已停止' })]],
      });
      await tick();
      await tick();
      expect(failed.terminals).toEqual(['failed']);
      expect(failed.polls()).toBe(2);
    });

    await withFakeTimers(async () => {
      const cancelled = startPolling({
        frames: [[row()], [row({ state: 'cancelled', detail: '已取消' })]],
      });
      await tick();
      await tick();
      expect(cancelled.terminals).toEqual(['cancelled']);
    });
  });

  test('快照不可达：保留现值不臆造（不写回），下一轮继续取', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [
          [row({ elapsed_secs: 192 })],
          new Error('network down'),
          new Error('network down'),
          [row({ state: 'done', attempt: 2, elapsed_secs: 300 })],
        ],
      });

      await tick();
      expect(run.readouts).toEqual([{ attempt: 2, elapsedSecs: 192 }]);

      await tick();
      await tick();
      expect(run.readouts).toHaveLength(1);
      expect(run.polls()).toBe(3);

      await tick();
      expect(run.terminals).toEqual(['done']);
    });
  });

  test('快照缺行（任务未登记 / 注册表被清）→ 回退静态面并继续等终态', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [[row()], [], [row({ attempt: 1, elapsed_secs: 30 })]],
      });

      await tick();
      expect(run.readouts).toEqual([{ attempt: 2, elapsedSecs: 192 }]);

      await tick();
      expect(run.readouts).toEqual([
        { attempt: 2, elapsedSecs: 192 },
        null,
      ]);
      expect(run.terminals).toEqual([]);

      await tick();
      expect(run.readouts).toEqual([
        { attempt: 2, elapsedSecs: 192 },
        null,
        { attempt: 1, elapsedSecs: 30 },
      ]);
    });
  });

  test('attempt 未回写的 running 行 → 回退静态面，轮询继续等读数', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [[row({ attempt: null })], [row()]],
      });

      await tick();
      expect(run.readouts).toEqual([null]);

      await tick();
      expect(run.readouts).toEqual([null, { attempt: 2, elapsedSecs: 192 }]);
    });
  });

  test('queued 行仍属活跃（资源闸）：继续轮询等终态', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [
          [row({ state: 'queued', attempt: null, elapsed_secs: null, queue_position: 1 })],
          [row({ state: 'queued', attempt: null, elapsed_secs: null, queue_position: 1 })],
          [row({ state: 'done', attempt: 3, elapsed_secs: 500 })],
        ],
      });

      await tick();
      await tick();
      expect(run.readouts).toEqual([null, null]);
      expect(run.terminals).toEqual([]);

      await tick();
      expect(run.terminals).toEqual(['done']);
    });
  });

  test('只认本课程的 structure 行：他课/他类不翻读数不判终态', async () => {
    await withFakeTimers(async () => {
      const run = startPolling({
        frames: [
          [row({ course_id: 'course-2' }), row({ kind: 'lesson', node_id: 'n1' })],
          [row({ course_id: 'course-2' }), row({ kind: 'lesson', node_id: 'n1' })],
        ],
      });

      await tick();
      await tick();
      expect(run.readouts).toEqual([null, null]);
      expect(run.terminals).toEqual([]);
    });
  });

  test('stop 之后不再轮询，在途读数也不写回（卸载/翻态时防迟写）', async () => {
    await withFakeTimers(async () => {
      const readouts: Array<ReturnType<typeof structureReadoutFromRow>> = [];
      let polls = 0;
      let release: (tasks: LearningGenerationTaskView[]) => void = () => undefined;
      const stop = startStructureReadoutPolling({
        courseId: COURSE_ID,
        fetchTasks: () => {
          polls += 1;
          return new Promise<LearningGenerationTaskView[]>((resolve) => {
            release = resolve;
          });
        },
        onReadout: (readout) => {
          readouts.push(readout);
        },
        onTerminal: () => undefined,
      });

      await tick();
      expect(polls).toBe(1);

      stop();
      release([row({ state: 'done', attempt: 2, elapsed_secs: 300 })]);
      await flushPromises();
      expect(readouts).toEqual([]);

      await tick();
      await tick();
      expect(polls).toBe(1);
    });
  });
});

describe('hasActiveStructureRow：刷新快照并集的 structure 判定（G3）', () => {
  test('本课程 running/queued 的 structure 行 → active', () => {
    expect(hasActiveStructureRow([row()], COURSE_ID)).toBe(true);
    expect(hasActiveStructureRow([row({ state: 'queued' })], COURSE_ID)).toBe(true);
  });

  test('终态行（done/failed/cancelled）不算 active', () => {
    expect(hasActiveStructureRow([row({ state: 'done' })], COURSE_ID)).toBe(false);
    expect(hasActiveStructureRow([row({ state: 'failed' })], COURSE_ID)).toBe(false);
    expect(hasActiveStructureRow([row({ state: 'cancelled' })], COURSE_ID)).toBe(false);
  });

  test('他课结构行与他类任务不误报', () => {
    expect(hasActiveStructureRow([row({ course_id: 'course-2' })], COURSE_ID)).toBe(false);
    expect(hasActiveStructureRow([row({ kind: 'lesson', node_id: 'n1' })], COURSE_ID)).toBe(false);
    expect(hasActiveStructureRow([], COURSE_ID)).toBe(false);
  });
});

describe('词典（红线 2）：新 key 双语对称 + 真实例渲染', () => {
  /** 本波新增的词典 key——断言必须遍历全量新 key（教训 F6：漏一个就可能在
   * 某语言回落到键名原文）。 */
  const NEW_KEYS = ['structureBadgeRunning'] as const;

  test('新 key 双语在场且插值占位符对称', () => {
    const placeholders = (template: string): string[] =>
      (template.match(/\{\{\w+\}\}/g) ?? []).sort();
    for (const key of NEW_KEYS) {
      const zh: string = learningZh[key as keyof typeof learningZh] as string;
      const en: string = learningEn[key as keyof typeof learningEn] as string;
      expect(typeof zh).toBe('string');
      expect(typeof en).toBe('string');
      expect(placeholders(en)).toEqual(placeholders(zh));
    }
    expect(placeholders(learningZh.structureBadgeRunning)).toEqual([
      '{{attempt}}',
      '{{elapsed}}',
      '{{max}}',
    ]);
  });

  test('回退静态键在场（复用既有 learningGraphStructureGenerating）', () => {
    expect(learningZh.learningGraphStructureGenerating).toBe('生成中…');
    expect(learningEn.learningGraphStructureGenerating).toBe('Generating…');
  });

  const renderBadgeCopy = async (locale: 'zh-CN' | 'en-US', copy: BadgeCopyLike) => {
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
    return renderToStaticMarkup(
      React.createElement(
        'span',
        null,
        instance.t(copy.key, 'vars' in copy ? copy.vars : undefined)
      )
    );
  };

  test('读数面最终句（zh/en 双语实例渲染 attempt/elapsed）', async () => {
    const copy = structureBadgeCopy({ attempt: 2, elapsedSecs: 192 }) as BadgeCopyLike;
    expect(await renderBadgeCopy('zh-CN', copy)).toContain('结构生成中 · 第 2/3 次 · 已 3:12');
    expect(await renderBadgeCopy('en-US', copy)).toContain('Generating structure');
    expect(await renderBadgeCopy('en-US', copy)).toContain('attempt 2/3');
  });

  test('回退面最终句 = 静态「生成中…」', async () => {
    const copy = structureBadgeCopy(null) as BadgeCopyLike;
    expect(await renderBadgeCopy('zh-CN', copy)).toContain('生成中…');
    expect(await renderBadgeCopy('en-US', copy)).toContain('Generating…');
  });
});

describe('工作区接线（源断言，multiSourceDisclosure 形态）', () => {
  const workspaceSource = readSource(
    new URL('./components/LearningGraphWorkspace.tsx', import.meta.url)
  );

  test('structureRunning 是三源并集派生：WS 旗 ∪ 注册表快照行', () => {
    expect(
      workspaceSource.includes('const structureRunning = structureRunningWs || registryBusy.structure')
    ).toBe(true);
  });

  test('refreshRegistryBusy 补 structure 类（d7 四类漏本尊的补位）', () => {
    expect(
      workspaceSource.includes("task.kind === 'basis_declare'")
    ).toBe(true);
    expect(
      workspaceSource.includes('structure: hasActiveStructureRow(tasks, courseId)')
    ).toBe(true);
  });

  test('WS 失败/取消清旗臂不回归（原 :181-185 语义原样保留）', () => {
    expect(
      workspaceSource.includes(
        "if (event.event === 'structure_failed' || event.event === 'structure_cancelled')"
      )
    ).toBe(true);
    expect(workspaceSource.includes('setStructureRunningWs(false);')).toBe(true);
  });

  test('生长按钮守卫追加 structureRunning（G5 防呆）', () => {
    expect(
      workspaceSource.includes(
        'disabled={growing || graph.growth_running || busyId !== null || structureRunning}'
      )
    ).toBe(true);
  });

  test('快照终态接棒走既有 load 消解柄 + 注册表并集重算', () => {
    expect(workspaceSource.includes('structureLoadRef.current()')).toBe(true);
    expect(workspaceSource.includes('void refreshRegistryBusy()')).toBe(true);
  });

  test('徽章经 structureBadgeCopy 渲染（单渲染点，不走内联文案）', () => {
    expect(workspaceSource.includes('structureBadgeCopy(structureReadout)')).toBe(true);
  });
});
