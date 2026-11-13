/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图课程流悬浮 Pill 的快照真值面（v3，G1 谎言面）。
 *
 * 图课程（`course_kind === 'learning_graph'`）HTTP 秒回 completed 后，悬浮
 * Pill 不再显示「✓ 生成完成」——结构生成此后还要在后台跑十余分钟，完成态
 * 对它是半程谎言。本模块把 Pill 绑到该课 structure 任务快照：
 *
 * - running/queued → 新 label（「结构生成中 · 已 m:ss」，elapsed 来自快照
 *   行、`formatGrowthElapsed` 单一格式器（F1 收口，与工作区徽章/生长 Pill
 *   同口径）；attempt（v1 机器面）在场时带轮次）；
 * - done → Pill 退场（完成的真话在工作区徽章/摘要行与对话框卡片，不再造
 *   一个秒级翻转的完成面）；
 * - failed/cancelled → 沿用 Pill 既有终态语义（genBgFailed/genCancelled +
 *   「查看」）。
 *
 * v4 起同一 watch 快照还供图课程 completed 卡片的结构态行消费
 * （`structureCardLine`，index 接线经 props 链下发）——仍是单一轮询源，
 * 只是在 Pill 隐藏（对话框打开）期间 watch 也保持激活。
 *
 * 轮询与 c2 `startRestoredGenerationConvergence` 同形状（5s 节拍、终态即停、
 * 假数据源可注入）——票面明示不抽公共 hook（评审 judgement「第三处手写轮
 * 询」留痕：这是第二处同形状实现，若日后出现第三处再议收口）。
 *
 * 行消失语义与收敛循环不同：structure 任务在请求生命周期内 pre-register
 * （`admit_structure_generation`），HTTP 返回时快照行必已在场；行此后消失
 * 只可能是注册表被清（后端重启，任务随之死亡）——按退场收口，不臆造
 * cancelled（那是恢复态收敛循环的兜底，此处没有「必须等到终态」的挂起承诺）。
 */

import { useEffect, useState } from 'react';

import { learningApi } from './api';
import { formatGrowthElapsed } from './growthTaskPill';
import type { LearningGenerationTaskView } from './types';
import type { Translate } from './utils';

/** 轮询节拍（与恢复态收敛循环同口径）：非终态期间 5s 一拍，终态即停。 */
export const STRUCTURE_PILL_POLL_MS = 5_000;

/** v3 新增的悬浮 Pill 词典键（红线 2：双语对称由真实词典 JSON 断言钉住）。 */
export const STRUCTURE_PILL_I18N_KEYS = [
  'learning.genBgStructureRunning',
  'learning.genBgStructureRunningElapsed',
  'learning.genBgStructureRunningAttempt',
  'learning.genBgStructureQueued',
] as const;

/** v4 新增的 completed 卡片结构态行词典键（红线 2：双语对称由真实词典 JSON
 * 断言钉住）。卡片行只披露「在跑」；数据与 Pill 共用同一 watch 快照（单一
 * 轮询源），不新增第二份轮询。 */
export const STRUCTURE_CARD_I18N_KEYS = [
  'learning.genCardStructureRunning',
  'learning.genCardStructureRunningElapsed',
] as const;

/** 已运行时长读数（单一格式器，F1 收口）：复用既有 `formatGrowthElapsed`
 * （m:ss，满一小时 h:mm:ss）——与工作区徽章/生长 Pill 同口径，同 451s 三面
 * 同显「7:31」。elapsed_secs 缺席（queued 行）→ null，调用面回退无耗时键。 */
function structureElapsed(secs: number | null): string | null {
  return secs == null ? null : formatGrowthElapsed(secs);
}

/** 快照列表里找该课的 structure 本尊行：只认 `kind === 'structure'` 且
 * `course_id` 匹配——structure_mount/repair 是另外的任务类别（补挂/修补），
 * 不是 Pill 追踪的首轮结构生成；同课多行时注册序倒序的首行即最新。 */
export function findStructureTaskRow(
  tasks: LearningGenerationTaskView[],
  courseId: string
): LearningGenerationTaskView | undefined {
  return tasks.find((task) => task.kind === 'structure' && task.course_id === courseId);
}

/** 快照行 → Pill 主行 label（新键；终态行返回 null 走 Pill 既有词典）。 */
export function structurePillLabel(
  rowState: LearningGenerationTaskView,
  t: Translate
): string | null {
  if (rowState.state === 'running') {
    const elapsed = structureElapsed(rowState.elapsed_secs);
    if (rowState.attempt != null) {
      return t('learning.genBgStructureRunningAttempt', {
        attempt: rowState.attempt,
        elapsed: elapsed ?? '',
      });
    }
    if (elapsed != null) {
      return t('learning.genBgStructureRunningElapsed', { elapsed });
    }
    return t('learning.genBgStructureRunning');
  }
  if (rowState.state === 'queued') {
    return t('learning.genBgStructureQueued');
  }
  return null;
}

/** watch 态：idle = 未接管；watching = 轮询中；settled = 终态已落（轮询停）。 */
export interface StructurePillWatchState {
  phase: 'idle' | 'watching' | 'settled';
  /** 最新快照行（null = 首读在途，或 missing 退场） */
  row: LearningGenerationTaskView | null;
}

/** watch 态 → 图课程 Pill 呈现分流（null = Pill 退场）。
 * label 为 null 时主行走 Pill 既有状态词典（failed/cancelled 终态语义）；
 * taskKey 供 per-key 取消接线（乐观期 null——行还没读到，取消点击守卫为
 * no-op，窗口只有一次首读往返）。 */
export interface StructurePillSpec {
  status: 'queued' | 'running' | 'failed' | 'cancelled';
  label: string | null;
  taskKey: string | null;
}

export function structurePillSpec(
  watch: StructurePillWatchState,
  t: Translate
): StructurePillSpec | null {
  const rowState = watch.row;
  if (watch.phase === 'idle') return null;
  if (rowState == null) {
    // 首读在途：乐观 running（structure 任务在 HTTP 返回前已 pre-register，
    // 面孔先立、读数随后）；settled + row null = missing 退场。
    return watch.phase === 'watching'
      ? { status: 'running', label: t('learning.genBgStructureRunning'), taskKey: null }
      : null;
  }
  if (rowState.state === 'done') return null;
  if (rowState.state === 'failed' || rowState.state === 'cancelled') {
    return { status: rowState.state, label: null, taskKey: rowState.key };
  }
  return {
    status: rowState.state === 'queued' ? 'queued' : 'running',
    label: structurePillLabel(rowState, t),
    taskKey: rowState.key,
  };
}

/** watch 态 → 图课程 completed 卡片结构态行文案（v4；null = 该行不渲染）。
 * 在跑 → 「知识结构生成中 · 已 m:ss」（卡片行用「知识结构」全称前缀，与
 * Pill 的「结构生成中」区分开——票面文案锚定 taskKindStructure 全称）；
 * elapsed 缺失退无耗时键；attempt 不上卡片行（无交互无轮次——轮次真话在
 * Pill/工作区）。queued / 终态（done 落库）/ 未接管 → null：卡片是 HTTP
 * 响应快照，本票不刷新它，行只披露「在跑」这一件事。 */
export function structureCardLine(
  watch: StructurePillWatchState,
  t: Translate
): string | null {
  if (watch.phase === 'idle') return null;
  const rowState = watch.row;
  if (rowState == null) {
    // 首读在途：乐观「在跑」无耗时（与 Pill 乐观面孔同口径）；settled +
    // row null = missing 退场，不渲染。
    return watch.phase === 'watching' ? t('learning.genCardStructureRunning') : null;
  }
  if (rowState.state !== 'running') return null;
  const elapsed = structureElapsed(rowState.elapsed_secs);
  if (elapsed != null) {
    return t('learning.genCardStructureRunningElapsed', { elapsed });
  }
  return t('learning.genCardStructureRunning');
}

export interface StructurePillWatchOptions {
  courseId: string;
  /** 快照读数面（`GET /api/learning/generation-tasks`）——测试注入假源 */
  fetchTasks: () => Promise<LearningGenerationTaskView[]>;
  /** 非终态推进：每拍到本课 structure 行（running/queued）写回 */
  onRow: (row: LearningGenerationTaskView) => void;
  /** 终态：done（落库退场）/ failed / cancelled（终态面孔）/ missing（注册
   * 表清空退场）。至多调用一次，调用后循环自停。 */
  onTerminal: (
    outcome: 'done' | 'failed' | 'cancelled' | 'missing',
    row: LearningGenerationTaskView | null
  ) => void;
}

/** structure 快照轮询循环：首读即取（HTTP completed 返回时行已 pre-register，
 * 无须先空等一拍），非终态按 `STRUCTURE_PILL_POLL_MS` 续拍，终态即停。快照
 * 不可达（后端重启/断网）不臆造终态，下一拍再取——与恢复态收敛循环同一
 * 口径。返回停止函数。 */
export function startStructurePillWatch(options: StructurePillWatchOptions): () => void {
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    timer = setTimeout(() => void poll(), STRUCTURE_PILL_POLL_MS);
  };

  const poll = async () => {
    timer = null;
    let tasks: LearningGenerationTaskView[];
    try {
      tasks = await options.fetchTasks();
    } catch {
      if (!disposed) schedule();
      return;
    }
    if (disposed) return;
    const rowState = findStructureTaskRow(tasks, options.courseId);
    if (!rowState) {
      options.onTerminal('missing', null);
      return;
    }
    if (
      rowState.state === 'done' ||
      rowState.state === 'failed' ||
      rowState.state === 'cancelled'
    ) {
      options.onTerminal(rowState.state, rowState);
      return;
    }
    options.onRow(rowState);
    if (!disposed) schedule();
  };

  void poll();
  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
  };
}

/** Pill 装配侧的 watch 接线：courseId 非空即开轮询、终态写回 settled、置空
 * 即回 idle（effect 清理停拍）。fetchTasks 缺省走 learningApi（测试可注入）。
 * 优化期面孔：courseId 非空的首帧即立 watching（effect 尚未跑也先立，避免
 * Pill 闪没一帧）。 */
export function useStructurePillWatch(
  courseId: string | null,
  fetchTasks: () => Promise<LearningGenerationTaskView[]> = () => learningApi.generationTasks()
): StructurePillWatchState {
  const [watch, setWatch] = useState<StructurePillWatchState>(() =>
    courseId ? { phase: 'watching', row: null } : { phase: 'idle', row: null }
  );
  useEffect(() => {
    if (!courseId) {
      setWatch({ phase: 'idle', row: null });
      return;
    }
    setWatch({ phase: 'watching', row: null });
    return startStructurePillWatch({
      courseId,
      fetchTasks,
      onRow: (rowState) => setWatch({ phase: 'watching', row: rowState }),
      onTerminal: (outcome, rowState) =>
        setWatch({ phase: 'settled', row: outcome === 'missing' ? null : rowState }),
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fetchTasks 由调用方持有稳定引用；本 watch 只随 courseId 增减
  }, [courseId]);
  return watch;
}
