/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { formatGrowthElapsed } from './growthTaskPill';
import type { LearningGenerationTaskView } from './types';

/**
 * 结构生成真话面的纯读数模块（v2，G2 哑巴面 / G3 失忆面 / G5 防呆）。
 *
 * 状态纪律与 c2 恢复态收敛轮询同款：
 * - **快照为准**：attempt/elapsed 全部取 `GET /api/learning/generation-tasks`
 *   服务端真相（不自算钟——徽章的耗时不是本地秒表，是快照行的 elapsed_secs）；
 * - **读数宁缺不臆造**：快照不可达保留现值；attempt 未回写或快照缺行回退
 *   静态「生成中…」，绝不编一个轮次出来。
 */

/** 快照 `kind` 的结构生成任务机器名（后端 KIND_STRUCTURE；重生成纠错通道
 * 同类——d3 已移除产品面，运行判定不分首生/重生）。 */
export const STRUCTURE_TASK_KIND = 'structure';

/** 结构生成整批重试预算：与后端 `structure_gen.rs::MAX_ATTEMPTS` 同源
 * （learnhub MAX_ATTEMPTS 同款，契约门拒收后重试的上限）。前端机器面常量
 * 对齐——帧/快照不携带 max（票面取侵入最小者）。 */
export const STRUCTURE_MAX_ATTEMPTS = 3;

/** 运行读数的轮询间隔（v2）：与 c2 恢复态收敛轮询同节奏（5s、终态即停）。
 * WS structure_* 帧仍是提前翻态的主路（best-effort），轮询只管读数与
 * WS 丢帧时的终态兜底。 */
export const STRUCTURE_READOUT_POLL_MS = 5_000;

/** 一轮结构生成的实时读数（G2）：徽章插值的机器面。attempt/elapsed 同时
 * 在场才构成读数——后端只在 running 态计 elapsed_secs（queued 为 null），
 * attempt 只由生成循环回写（首轮即 1），两者天然同相。 */
export interface StructureReadout {
  /** 当前重试轮次（v1 attempt 机器面，1 起） */
  attempt: number;
  /** 已运行秒数（快照行 elapsed_secs，服务端真相） */
  elapsedSecs: number;
}

/** 快照行 → 读数：attempt 或 elapsed 缺任一即 null（徽章回退静态面）。
 * `attempt` 兼容旧后端缺字段（undefined 与 null 同判）。 */
export function structureReadoutFromRow(
  row: LearningGenerationTaskView
): StructureReadout | null {
  if (row.attempt === null || row.attempt === undefined) return null;
  if (row.elapsed_secs === null || row.elapsed_secs === undefined) return null;
  return { attempt: row.attempt, elapsedSecs: row.elapsed_secs };
}

/** 徽章文案机面（c3 契约：人面全走前端词典）：读数在 → 插值键；缺 →
 * 复用静态键。elapsed 由 `formatGrowthElapsed` 出 m:ss（满一小时 h:mm:ss，
 * 语言无关读数）。 */
export type StructureBadgeCopy =
  | { key: 'learning.learningGraphStructureGenerating' }
  | {
      key: 'learning.structureBadgeRunning';
      vars: { attempt: number; max: number; elapsed: string };
    };

export function structureBadgeCopy(readout: StructureReadout | null): StructureBadgeCopy {
  if (!readout) return { key: 'learning.learningGraphStructureGenerating' };
  return {
    key: 'learning.structureBadgeRunning',
    vars: {
      attempt: readout.attempt,
      max: STRUCTURE_MAX_ATTEMPTS,
      elapsed: formatGrowthElapsed(readout.elapsedSecs),
    },
  };
}

/** 刷新快照并集的 structure 类 active 判定（G3 失忆面）：d7
 * `refreshRegistryBusy` 的 active 过滤只认 basis/mount/repair/lesson 四类，
 * 恰好漏了本尊——刷新后结构明明在跑却呈现「尚未生成 + 可点生成按钮」。 */
export function hasActiveStructureRow(
  tasks: readonly LearningGenerationTaskView[],
  courseId: string
): boolean {
  return tasks.some(
    (task) =>
      task.kind === STRUCTURE_TASK_KIND &&
      task.course_id === courseId &&
      (task.state === 'running' || task.state === 'queued')
  );
}

export interface StructureReadoutPollingOptions {
  courseId: string;
  /** 快照读数面（`GET /api/learning/generation-tasks`），注入以便测试 */
  fetchTasks: () => Promise<LearningGenerationTaskView[]>;
  /** 每轮读数写回（null = 回退静态面）；快照不可达不写回（保留现值） */
  onReadout: (readout: StructureReadout | null) => void;
  /** 快照终态接棒：至多一次，调用后循环自停。调用方镜像 WS 终态臂语义
   * （failed/cancelled 显式清旗、投影重取消解、注册表并集重算）。 */
  onTerminal: (state: 'done' | 'failed' | 'cancelled') => void;
}

/** 运行读数的收敛轮询循环（v2，照 c2 `startRestoredGenerationConvergence`
 * 形状）：running/queued 期间 5s 读一次快照翻读数；终态接棒一次即停；
 * 快照不可达保留现值下一轮再取；快照缺行（任务尚未登记 / 注册表被清）
 * 回退静态面并继续等终态。返回停止函数。 */
export function startStructureReadoutPolling(
  options: StructureReadoutPollingOptions
): () => void {
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    timer = setTimeout(() => void poll(), STRUCTURE_READOUT_POLL_MS);
  };

  const poll = async () => {
    timer = null;
    let tasks: LearningGenerationTaskView[];
    try {
      tasks = await options.fetchTasks();
    } catch {
      // 快照不可达（后端重启 / 断网）：读数证据不足保留现值，下一轮再取。
      if (!disposed) schedule();
      return;
    }
    if (disposed) return;
    const row = tasks.find(
      (task) => task.kind === STRUCTURE_TASK_KIND && task.course_id === options.courseId
    );
    if (!row) {
      // 快照缺行：无读数可翻（回退静态面），但缺行不等于终态——继续等。
      options.onReadout(null);
      schedule();
      return;
    }
    if (row.state === 'done' || row.state === 'failed' || row.state === 'cancelled') {
      options.onTerminal(row.state);
      return;
    }
    options.onReadout(structureReadoutFromRow(row));
    schedule();
  };

  schedule();
  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
  };
}
