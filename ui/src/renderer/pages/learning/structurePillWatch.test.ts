/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图课程流悬浮 Pill 的快照真值面（v3，G1 谎言面）：HTTP 秒回 completed 后
 * Pill 绑该课 structure 任务快照（running/queued → 新 label；done → 退场；
 * failed/cancelled → 既有终态语义）。测试面：
 * - `formatStructureElapsed` / `findStructureTaskRow`：纯函数矩阵；
 * - `structurePillLabel` / `structurePillSpec`：行形状 → Pill 呈现分流
 *   （键选择 + vars 映射；词典文本本身由真实 JSON 断言钉住）；
 * - `startStructurePillWatch`：轮询循环（首读即取、5s 节拍、终态即停、
 *   快照不可达不臆造终态、行消失 = 注册表清空按退场收口），假数据源注入
 *   ——与 c2 `startRestoredGenerationConvergence` 同形状的第二处实现（票面
 *   明示不抽公共 hook）。
 * i18n（红线 2）：新 key 对真实 zh-CN / en-US 词典 JSON 双语遍历断言。 */
import * as bunTest from 'bun:test';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { LearningGenerationTaskView } from './types';
import type { Translate } from './utils';

const { afterEach, beforeEach, describe, expect, test } = bunTest;
const timers = (bunTest as unknown as { jest: FakeTimerApi }).jest;

interface FakeTimerApi {
  useFakeTimers: () => void;
  useRealTimers: () => void;
  clearAllTimers: () => void;
  advanceTimersByTime: (milliseconds: number) => void;
}

/** 冲刷微任务队列：轮询体内的 await 链（快照读数 → 写回）逐跳推进。 */
const flushPromises = async () => {
  for (let hop = 0; hop < 20; hop += 1) await Promise.resolve();
};

const TASK_KEY = 'structure:course-1:-';

const row = (overrides: Partial<LearningGenerationTaskView> = {}): LearningGenerationTaskView => ({
  key: TASK_KEY,
  kind: 'structure',
  course_id: 'course-1',
  node_id: null,
  topic: '知识结构生成',
  state: 'running',
  step: null,
  detail: null,
  elapsed_secs: 451,
  queued_at_ms: null,
  queue_position: null,
  ...overrides,
});

/** i18n 替身：记录键与 vars，键选择即该层的公开行为；词典文本由真实 JSON
 * 断言另行钉住（红线 2）。 */
const tStub = () => {
  const calls: Array<{ key: string; vars: Record<string, unknown> | undefined }> = [];
  const t = ((key: string, vars?: Record<string, unknown>) => {
    calls.push({ key, vars });
    return key;
  }) as unknown as Translate;
  return { t, calls };
};

describe('formatStructureElapsed（mm:ss，超时进位 h:mm:ss）', () => {
  test('451s → 07:31（票面示例）', async () => {
    const { formatStructureElapsed } = await import('./structurePillWatch');
    expect(formatStructureElapsed(451)).toBe('07:31');
  });
  test('0 → 00:00；59 → 00:59；60 → 01:00', async () => {
    const { formatStructureElapsed } = await import('./structurePillWatch');
    expect(formatStructureElapsed(0)).toBe('00:00');
    expect(formatStructureElapsed(59)).toBe('00:59');
    expect(formatStructureElapsed(60)).toBe('01:00');
  });
  test('3661 → 1:01:01（小时不补零）', async () => {
    const { formatStructureElapsed } = await import('./structurePillWatch');
    expect(formatStructureElapsed(3661)).toBe('1:01:01');
  });
  test('null / 负数 → null（queued 行无 elapsed）', async () => {
    const { formatStructureElapsed } = await import('./structurePillWatch');
    expect(formatStructureElapsed(null)).toBeNull();
    expect(formatStructureElapsed(-1)).toBeNull();
  });
});

describe('findStructureTaskRow（只认 structure 本尊 + 课程 id 匹配）', () => {
  test('命中 kind=structure 且 course_id 匹配的行', async () => {
    const { findStructureTaskRow } = await import('./structurePillWatch');
    const hit = row();
    expect(
      findStructureTaskRow([row({ kind: 'outline', course_id: 'course-1' }), hit], 'course-1')
    ).toBe(hit);
  });
  test('structure_mount / structure_repair / 其他课程 / 缺行都不认', async () => {
    const { findStructureTaskRow } = await import('./structurePillWatch');
    expect(
      findStructureTaskRow([row({ kind: 'structure_mount', course_id: 'course-1' })], 'course-1')
    ).toBeUndefined();
    expect(
      findStructureTaskRow([row({ kind: 'structure_repair', course_id: 'course-1' })], 'course-1')
    ).toBeUndefined();
    expect(findStructureTaskRow([row()], 'course-2')).toBeUndefined();
    expect(findStructureTaskRow([], 'course-1')).toBeUndefined();
  });
});

describe('structurePillLabel（快照行 → 新 label 键选择）', () => {
  test('running + elapsed：耗时键 + 已格式化耗时 vars', async () => {
    const { structurePillLabel } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(structurePillLabel(row({ elapsed_secs: 451 }), t)).toBe(
      'learning.genBgStructureRunningElapsed'
    );
    expect(calls[0]?.vars).toEqual({ elapsed: '07:31' });
  });
  test('running + attempt（v1 机器面）：轮次键 + attempt/elapsed 双 vars', async () => {
    const { structurePillLabel } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(structurePillLabel(row({ attempt: 2, elapsed_secs: 451 }), t)).toBe(
      'learning.genBgStructureRunningAttempt'
    );
    expect(calls[0]?.vars).toEqual({ attempt: 2, elapsed: '07:31' });
  });
  test('running 但 elapsed 缺失：退回无耗时键（不渲染「已 」空尾巴）', async () => {
    const { structurePillLabel } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(structurePillLabel(row({ elapsed_secs: null, attempt: null }), t)).toBe(
      'learning.genBgStructureRunning'
    );
    expect(calls[0]?.vars).toBeUndefined();
  });
  test('queued：排队键（无 vars）', async () => {
    const { structurePillLabel } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(
      structurePillLabel(row({ state: 'queued', elapsed_secs: null, queue_position: 1 }), t)
    ).toBe('learning.genBgStructureQueued');
    expect(calls[0]?.vars).toBeUndefined();
  });
  test('终态行 → null（failed/cancelled 走 Pill 既有词典，done 无 label）', async () => {
    const { structurePillLabel } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(structurePillLabel(row({ state: 'done', elapsed_secs: null }), t)).toBeNull();
    expect(structurePillLabel(row({ state: 'failed', elapsed_secs: null }), t)).toBeNull();
    expect(structurePillLabel(row({ state: 'cancelled', elapsed_secs: null }), t)).toBeNull();
  });
});

describe('structurePillSpec（watch 态 → Pill 呈现分流；null = 退场）', () => {
  test('idle → null（未进图课程完成流，Pill 不接管）', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(structurePillSpec({ phase: 'idle', row: null }, t)).toBeNull();
  });
  test('watching + 行未读到（首读在途）：乐观 running（无耗时键）+ taskKey null 取消守卫', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    const spec = structurePillSpec({ phase: 'watching', row: null }, t);
    expect(spec).toEqual({
      status: 'running',
      label: 'learning.genBgStructureRunning',
      taskKey: null,
    });
  });
  test('watching + running 行：status=running + 新 label + taskKey（取消接线）', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    const spec = structurePillSpec({ phase: 'watching', row: row({ attempt: 2 }) }, t);
    expect(spec).toEqual({
      status: 'running',
      label: 'learning.genBgStructureRunningAttempt',
      taskKey: TASK_KEY,
    });
  });
  test('watching + queued 行：status=queued + 排队 label', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    const spec = structurePillSpec(
      { phase: 'watching', row: row({ state: 'queued', elapsed_secs: null }) },
      t
    );
    expect(spec).toEqual({
      status: 'queued',
      label: 'learning.genBgStructureQueued',
      taskKey: TASK_KEY,
    });
  });
  test('watching + done 行：null（落库退场，不显示完成态——G1 修复本体）', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(
      structurePillSpec({ phase: 'watching', row: row({ state: 'done', elapsed_secs: null }) }, t)
    ).toBeNull();
  });
  test('watching + failed / cancelled 行：沿用既有终态语义（label null → Pill 词典）', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(
      structurePillSpec({ phase: 'watching', row: row({ state: 'failed', elapsed_secs: null }) }, t)
    ).toEqual({ status: 'failed', label: null, taskKey: TASK_KEY });
    expect(
      structurePillSpec(
        { phase: 'watching', row: row({ state: 'cancelled', elapsed_secs: null }) },
        t
      )
    ).toEqual({ status: 'cancelled', label: null, taskKey: TASK_KEY });
  });
  test('settled + done：null（退场定格）；settled + missing（row null）：null', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(
      structurePillSpec({ phase: 'settled', row: row({ state: 'done', elapsed_secs: null }) }, t)
    ).toBeNull();
    expect(structurePillSpec({ phase: 'settled', row: null }, t)).toBeNull();
  });
  test('settled + failed / cancelled：终态呈现定格（轮询已停，面孔保留）', async () => {
    const { structurePillSpec } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(
      structurePillSpec({ phase: 'settled', row: row({ state: 'failed', elapsed_secs: null }) }, t)
    ).toEqual({ status: 'failed', label: null, taskKey: TASK_KEY });
  });
});

describe('startStructurePillWatch（假数据源 + 假定时器）', () => {
  beforeEach(() => timers.useFakeTimers());
  afterEach(() => {
    timers.clearAllTimers();
    timers.useRealTimers();
  });

  /** 帧序假数据源：逐帧给出快照（末帧常驻），记账调用次数。 */
  function framesSource(frames: Array<LearningGenerationTaskView[] | Error>) {
    let calls = 0;
    const fetchTasks = async () => {
      const frame = frames[Math.min(calls, frames.length - 1)];
      calls += 1;
      if (frame instanceof Error) throw frame;
      return frame;
    };
    return { fetchTasks, calls: () => calls };
  }

  test('首读即取（不等一个周期）：running 行经 onRow 写回并按 5s 节拍续拍', async () => {
    const { startStructurePillWatch, STRUCTURE_PILL_POLL_MS } = await import(
      './structurePillWatch'
    );
    expect(STRUCTURE_PILL_POLL_MS).toBe(5_000);
    const source = framesSource([[row()]]);
    const seen: LearningGenerationTaskView[] = [];
    let terminal = 0;
    startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: (r) => seen.push(r),
      onTerminal: () => {
        terminal += 1;
      },
    });
    await flushPromises();
    expect(seen).toHaveLength(1);
    expect(terminal).toBe(0);
    // 续拍：节拍 = 5s（差 1ms 不触发，满 5s 恰好一次读数）
    timers.advanceTimersByTime(STRUCTURE_PILL_POLL_MS - 1);
    await flushPromises();
    expect(source.calls()).toBe(1);
    timers.advanceTimersByTime(1);
    await flushPromises();
    expect(source.calls()).toBe(2);
    expect(seen).toHaveLength(2);
  });

  test('done → onTerminal("done", row) 且停拍（落库退场的循环面）', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    const source = framesSource([
      [row()],
      [row({ state: 'done', elapsed_secs: null })],
    ]);
    const outcomes: string[] = [];
    startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: () => undefined,
      onTerminal: (outcome) => {
        outcomes.push(outcome);
      },
    });
    await flushPromises();
    timers.advanceTimersByTime(5_000);
    await flushPromises();
    timers.advanceTimersByTime(60_000);
    await flushPromises();
    expect(outcomes).toEqual(['done']);
    expect(source.calls()).toBe(2);
  });

  test('failed / cancelled 如实上抛（终态面孔的数据面）', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    for (const state of ['failed', 'cancelled'] as const) {
      const source = framesSource([[row({ state })]]);
      const outcomes: string[] = [];
      startStructurePillWatch({
        courseId: 'course-1',
        fetchTasks: source.fetchTasks,
        onRow: () => undefined,
        onTerminal: (outcome) => {
          outcomes.push(outcome);
        },
      });
      await flushPromises();
      expect(outcomes).toEqual([state]);
    }
  });

  test('行消失（注册表清空，空快照）→ onTerminal("missing", null) 退场收口，不臆造 cancelled', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    const source = framesSource([[]]);
    const outcomes: Array<{ outcome: string; row: unknown }> = [];
    startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: () => undefined,
      onTerminal: (outcome, r) => {
        outcomes.push({ outcome, row: r });
      },
    });
    await flushPromises();
    expect(outcomes).toEqual([{ outcome: 'missing', row: null }]);
  });

  test('本课行缺席但注册表在（快照只有其他课程的行）→ 同样 missing 退场：后端从不摘行，缺席即异常/未来剪枝，退场是唯一不撒谎面孔且不空转', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    const other = row({ course_id: 'course-9', key: 'structure:course-9:-' });
    const source = framesSource([[other]]);
    let writes = 0;
    const outcomes: Array<{ outcome: string; row: unknown }> = [];
    startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: () => {
        writes += 1;
      },
      onTerminal: (outcome, r) => {
        outcomes.push({ outcome, row: r });
      },
    });
    await flushPromises();
    timers.advanceTimersByTime(60_000);
    await flushPromises();
    // 其他课程的行不写回；本课行缺席判 missing 终态（退场面孔 = spec null，
    // 与 done 同面），循环停拍不空转。
    expect(writes).toBe(0);
    expect(outcomes).toEqual([{ outcome: 'missing', row: null }]);
    expect(source.calls()).toBe(1);
  });

  test('快照不可达：不臆造终态，续拍重试', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    const source = framesSource([
      new Error('backend down'),
      new Error('backend down'),
      [row({ state: 'done', elapsed_secs: null })],
    ]);
    const outcomes: string[] = [];
    startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: () => undefined,
      onTerminal: (outcome) => {
        outcomes.push(outcome);
      },
    });
    await flushPromises();
    expect(outcomes).toEqual([]);
    timers.advanceTimersByTime(5_000);
    await flushPromises();
    expect(outcomes).toEqual([]);
    timers.advanceTimersByTime(5_000);
    await flushPromises();
    expect(outcomes).toEqual(['done']);
    expect(source.calls()).toBe(3);
  });

  test('stop() 作废在途读数与挂起定时器', async () => {
    const { startStructurePillWatch } = await import('./structurePillWatch');
    const source = framesSource([[row()]]);
    let writes = 0;
    const stop = startStructurePillWatch({
      courseId: 'course-1',
      fetchTasks: source.fetchTasks,
      onRow: () => {
        writes += 1;
      },
      onTerminal: () => undefined,
    });
    stop();
    await flushPromises();
    timers.advanceTimersByTime(60_000);
    await flushPromises();
    expect(writes).toBe(0);
    expect(source.calls()).toBe(1);
  });
});

describe('v3 新 i18n key：真实词典双语对称断言（红线 2）', () => {
  const short = (key: string) => key.replace(/^learning\./, '');

  test('zh-CN 与 en-US learning.json 都含全部新 key（非空字符串）', async () => {
    const { STRUCTURE_PILL_I18N_KEYS } = await import('./structurePillWatch');
    expect(STRUCTURE_PILL_I18N_KEYS.length).toBeGreaterThanOrEqual(4);
    const zh = learningZh as unknown as Record<string, string>;
    const en = learningEn as unknown as Record<string, string>;
    for (const key of STRUCTURE_PILL_I18N_KEYS) {
      expect(typeof zh[short(key)]).toBe('string');
      expect(zh[short(key)].length).toBeGreaterThan(0);
      expect(typeof en[short(key)]).toBe('string');
      expect(en[short(key)].length).toBeGreaterThan(0);
    }
  });

  test('zh 文案钉住票面语义：「结构生成中 · 已 {{elapsed}}」；en 同构插值', async () => {
    const { STRUCTURE_PILL_I18N_KEYS } = await import('./structurePillWatch');
    const zh = learningZh as unknown as Record<string, string>;
    const en = learningEn as unknown as Record<string, string>;
    const elapsedKey = STRUCTURE_PILL_I18N_KEYS.find((k) => short(k) === 'genBgStructureRunningElapsed');
    expect(zh[short(elapsedKey!)]).toContain('结构生成中');
    expect(zh[short(elapsedKey!)]).toContain('{{elapsed}}');
    expect(en[short(elapsedKey!)]).toContain('{{elapsed}}');
    // attempt（v1 机器面）与排队键也在列
    expect(STRUCTURE_PILL_I18N_KEYS).toContain('learning.genBgStructureRunningAttempt');
    expect(STRUCTURE_PILL_I18N_KEYS).toContain('learning.genBgStructureQueued');
  });
});

describe('structureCardLine（watch 态 → completed 卡片结构态行；null = 不渲染）', () => {
  test('watching + running + elapsed：卡片耗时键 + 已格式化耗时 vars', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(structureCardLine({ phase: 'watching', row: row({ elapsed_secs: 451 }) }, t)).toBe(
      'learning.genCardStructureRunningElapsed'
    );
    expect(calls[0]?.vars).toEqual({ elapsed: '07:31' });
  });

  test('attempt 在场也只走耗时键（卡片行无轮次，轮次真话在 Pill）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(
      structureCardLine({ phase: 'watching', row: row({ attempt: 2, elapsed_secs: 451 }) }, t)
    ).toBe('learning.genCardStructureRunningElapsed');
    expect(calls[0]?.vars).toEqual({ elapsed: '07:31' });
  });

  test('watching + running 但 elapsed 缺失：无耗时键（不渲染「已 」空尾巴）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(
      structureCardLine(
        { phase: 'watching', row: row({ elapsed_secs: null, attempt: null }) },
        t
      )
    ).toBe('learning.genCardStructureRunning');
    expect(calls[0]?.vars).toBeUndefined();
  });

  test('watching + 行未读到（首读在途）：乐观无耗时键（面孔先立、读数随后，与 Pill 同口径）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t, calls } = tStub();
    expect(structureCardLine({ phase: 'watching', row: null }, t)).toBe(
      'learning.genCardStructureRunning'
    );
    expect(calls[0]?.vars).toBeUndefined();
  });

  test('queued 行 → null（卡片只披露「在跑」；排队真话在 Pill/工作区）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(
      structureCardLine(
        { phase: 'watching', row: row({ state: 'queued', elapsed_secs: null }) },
        t
      )
    ).toBeNull();
  });

  test('终态行（done/failed/cancelled）→ null（落库后本票不刷新卡片，该行退场）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t } = tStub();
    for (const state of ['done', 'failed', 'cancelled'] as const) {
      expect(
        structureCardLine(
          { phase: 'watching', row: row({ state, elapsed_secs: null }) },
          t
        )
      ).toBeNull();
    }
    expect(
      structureCardLine(
        { phase: 'settled', row: row({ state: 'done', elapsed_secs: null }) },
        t
      )
    ).toBeNull();
  });

  test('idle / settled + missing（row null）→ null（未接管与退场都不渲染）', async () => {
    const { structureCardLine } = await import('./structurePillWatch');
    const { t } = tStub();
    expect(structureCardLine({ phase: 'idle', row: null }, t)).toBeNull();
    expect(structureCardLine({ phase: 'settled', row: null }, t)).toBeNull();
  });
});

describe('v4 新 i18n key：真实词典双语对称断言（红线 2）', () => {
  const short = (key: string) => key.replace(/^learning\./, '');

  test('zh-CN 与 en-US learning.json 都含卡片新 key（非空字符串）', async () => {
    const { STRUCTURE_CARD_I18N_KEYS } = await import('./structurePillWatch');
    expect(STRUCTURE_CARD_I18N_KEYS).toEqual([
      'learning.genCardStructureRunning',
      'learning.genCardStructureRunningElapsed',
    ]);
    const zh = learningZh as unknown as Record<string, string>;
    const en = learningEn as unknown as Record<string, string>;
    for (const key of STRUCTURE_CARD_I18N_KEYS) {
      expect(typeof zh[short(key)]).toBe('string');
      expect(zh[short(key)].length).toBeGreaterThan(0);
      expect(en[short(key)].length).toBeGreaterThan(0);
    }
  });

  test('zh 文案钉住票面语义：「知识结构生成中 · 已 {{elapsed}}」；en 同构插值', async () => {
    const { STRUCTURE_CARD_I18N_KEYS } = await import('./structurePillWatch');
    const zh = learningZh as unknown as Record<string, string>;
    const en = learningEn as unknown as Record<string, string>;
    const elapsedKey = STRUCTURE_CARD_I18N_KEYS.find(
      (k) => short(k) === 'genCardStructureRunningElapsed'
    );
    expect(zh[short(elapsedKey!)]).toContain('知识结构生成中');
    expect(zh[short(elapsedKey!)]).toContain('{{elapsed}}');
    expect(zh[short(elapsedKey!)]).toContain('· 已');
    expect(en[short(elapsedKey!)]).toContain('{{elapsed}}');
    const plainKey = STRUCTURE_CARD_I18N_KEYS.find(
      (k) => short(k) === 'genCardStructureRunning'
    );
    expect(zh[short(plainKey!)]).toBe('知识结构生成中');
  });
});
