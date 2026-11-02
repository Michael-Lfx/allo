/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * WS 进度帧 phase 的前端单源（x11）：与 Rust `nomifun_learning::events::phase`
 * 常量对齐。后端新增 phase 时先更新本文件（x12 互钉脚本在 CI 红提示）；
 * 其余前端文件只允许通过本模块引用 phase 字面量。
 */
export const PHASE = {
  started: 'started',
  completed: 'completed',
  failed: 'failed',
  cancelled: 'cancelled',
  publishing: 'publishing',
  triageStarted: 'triage_started',
  triageCompleted: 'triage_completed',
  sourcePlanStarted: 'source_plan_started',
  sourcePlanReady: 'source_plan_ready',
  coverageReady: 'coverage_ready',
  basisDeclared: 'basis_declared',
  basisRevoked: 'basis_revoked',
  resumed: 'resumed',
  retry: 'retry',
  grounding: 'grounding',
  degraded: 'degraded',
  round: 'round',
  audit: 'audit',
  generate: 'generate',
  repair: 'repair',
} as const;

export type Phase = (typeof PHASE)[keyof typeof PHASE];

/** 课程流可出现的 phase（与 Rust `phase::CORE + phase::COURSE + phase::LOOP` 对齐）。 */
export const COURSE_PHASES: readonly string[] = [
  PHASE.started,
  PHASE.completed,
  PHASE.failed,
  PHASE.cancelled,
  PHASE.publishing,
  PHASE.triageStarted,
  PHASE.triageCompleted,
  PHASE.sourcePlanStarted,
  PHASE.sourcePlanReady,
  PHASE.coverageReady,
  PHASE.basisDeclared,
  PHASE.basisRevoked,
  PHASE.round,
  PHASE.audit,
  PHASE.generate,
  PHASE.repair,
];

/** 课时流可出现的 phase（与 Rust `phase::CORE + phase::LESSON + phase::LOOP` 对齐）。 */
export const LESSON_PHASES: readonly string[] = [
  PHASE.started,
  PHASE.completed,
  PHASE.failed,
  PHASE.cancelled,
  PHASE.resumed,
  PHASE.retry,
  PHASE.grounding,
  PHASE.degraded,
  PHASE.round,
  PHASE.audit,
  PHASE.generate,
  PHASE.repair,
];

/** 新管线帧里算「可见活动」的 phase（triage/source_plan 阶段有读数行或
 * 至少推进步骤条——不能让等待提示把它们挤掉）。 */
export const MODERN_VISIBLE_PHASES: readonly string[] = [
  PHASE.started,
  PHASE.triageStarted,
  PHASE.triageCompleted,
  PHASE.sourcePlanStarted,
  PHASE.sourcePlanReady,
  PHASE.coverageReady,
  PHASE.round,
  PHASE.audit,
  PHASE.publishing,
];

/** 描述流/旧客户端的可见活动集合（零回归）。 */
export const LEGACY_VISIBLE_PHASES: readonly string[] = [
  PHASE.round,
  PHASE.audit,
  PHASE.publishing,
  PHASE.started,
];

/** 宠物气泡的情绪别名（A4：12 键覆盖四种情绪，不为 20 个 phase 各配文案）。 */
export const PET_PHASE_ALIAS: Record<string, string> = {
  [PHASE.triageStarted]: PHASE.started,
  [PHASE.triageCompleted]: PHASE.started,
  [PHASE.sourcePlanStarted]: PHASE.started,
  [PHASE.sourcePlanReady]: PHASE.started,
  [PHASE.coverageReady]: PHASE.publishing,
  [PHASE.resumed]: PHASE.started,
  [PHASE.retry]: PHASE.started,
  [PHASE.grounding]: PHASE.started,
  [PHASE.basisDeclared]: PHASE.completed,
  [PHASE.basisRevoked]: PHASE.completed,
  [PHASE.generate]: PHASE.round,
  [PHASE.repair]: PHASE.round,
  [PHASE.cancelled]: PHASE.failed,
};

export const isKnownPhase = (phases: readonly string[], phase: string): boolean =>
  phases.includes(phase);
