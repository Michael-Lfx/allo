import type { RunPlan, RunPlanAttempt, RunPlanStep } from "./protocol";
import { type RunStatusTone, runStatusTone } from "./run-tree";

/**
 * W4 / W6（`run/plan`，解 D-W6-1）—— 计划与待办视图的纯投影。
 *
 * 数据来自 `run/plan` 的**权威快照**（步骤标题、状态、成员归属、每次尝试的原因 /
 * 错误 / 起止时间），事件日志里没有这些字段，所以这个模块只做「快照 → 视图行」的
 * 归一化：不推断、不补默认值、不把缺失的时间算成 0。
 */

/** 一次尝试的展示行。`durationMs` 只在**两端都有**时间戳时才给出。 */
export interface PlanAttemptView {
  attemptId: string;
  /** 展示序号（从 1 起，按快照顺序）——与引擎的 `attempt_no` 解耦，后者从 0 起。 */
  ordinal: number;
  attemptNo: number;
  status: string;
  tone: RunStatusTone;
  triggerReason: string;
  durationMs: number | null;
  error: string | null;
  outputSummary: string | null;
  outputFiles: string[];
  tokens: number | null;
  member: string | null;
}

/** 一个步骤（＝一条待办）的展示行。 */
export interface PlanStepView {
  stepId: string;
  /** 1 起的展示序号，按快照顺序（引擎的 created_at 序）。 */
  index: number;
  title: string;
  kind: string;
  status: string;
  tone: RunStatusTone;
  member: string | null;
  /** 这一步是在第几个计划修订里引入的。 */
  introducedInRevision: number;
  /** 已被后续修订取代（保留在快照里，供历史追溯）。 */
  superseded: boolean;
  attempts: PlanAttemptView[];
}

/**
 * 成员归属：只有 `role` 和 `model` 上 wire（内部 participant id 有意不投影）。
 * 两者都缺时返回 `null`——界面不显示这一段，而不是显示「未知成员」。
 */
export function memberLabel(role?: string | null, model?: string | null): string | null {
  const parts = [role?.trim(), model?.trim()].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : null;
}

/**
 * 尝试耗时：`finished_at - started_at`，**只在两端都有且非负**时返回毫秒。
 * 进行中的尝试（无 finished_at）返回 `null`：把「还没结束」渲染成 0ms 会让
 * 用户以为瞬间完成。时钟回拨等异常值同样返回 `null`，不输出负数时长。
 */
export function attemptDurationMs(attempt: Pick<RunPlanAttempt, "started_at" | "finished_at">): number | null {
  const { started_at: started, finished_at: finished } = attempt;
  if (typeof started !== "number" || typeof finished !== "number") return null;
  if (!Number.isFinite(started) || !Number.isFinite(finished)) return null;
  const duration = finished - started;
  return duration >= 0 ? duration : null;
}

/**
 * 步骤状态 → 徽标色调。复用运行级 `runStatusTone` 的五档，另收 `superseded`
 * （被修订取代：既不是成功也不是失败，用 `pending` 档的灰）。
 */
export function planStepTone(status: string | null): RunStatusTone {
  if (status === "superseded" || status === "skipped") return "pending";
  return runStatusTone(status);
}

/** 快照 → 视图行。空快照返回空数组（界面走空态文案，不造占位行）。 */
export function planStepViews(plan: RunPlan | null | undefined): PlanStepView[] {
  if (!plan?.steps?.length) return [];
  return plan.steps.map((step: RunPlanStep, position) => ({
    stepId: step.step_id,
    index: position + 1,
    title: step.title?.trim() || step.step_id,
    kind: step.kind,
    status: step.status,
    tone: planStepTone(step.status),
    member: memberLabel(step.role, step.model),
    introducedInRevision: step.introduced_in_revision,
    superseded: step.superseded_in_revision != null,
    attempts: (step.attempts ?? []).map((attempt, position) => ({
      attemptId: attempt.attempt_id,
      ordinal: position + 1,
      attemptNo: attempt.attempt_no,
      status: attempt.status,
      tone: planStepTone(attempt.status),
      triggerReason: attempt.trigger_reason,
      durationMs: attemptDurationMs(attempt),
      error: attempt.error?.trim() ? attempt.error : null,
      outputSummary: attempt.output_summary?.trim() ? attempt.output_summary : null,
      outputFiles: attempt.output_files ?? [],
      tokens: attempt.tokens ?? null,
      member: memberLabel(attempt.role, attempt.model),
    })),
  }));
}

/** 进度：完成 / 总数（只数未被子修订取代的步骤，历史步骤不算进待办）。 */
export function planProgress(steps: PlanStepView[]): { done: number; total: number } {
  const live = steps.filter((step) => !step.superseded);
  return { done: live.filter((step) => step.tone === "ok").length, total: live.length };
}

/** 人话时长：`820ms` / `3.4s` / `2m 05s`；`null` 返回 `null`（调用方走占位）。 */
export function formatDuration(durationMs: number | null): string | null {
  if (durationMs === null || !Number.isFinite(durationMs) || durationMs < 0) return null;
  if (durationMs < 1000) return `${Math.round(durationMs)}ms`;
  if (durationMs < 60_000) return `${(durationMs / 1000).toFixed(1)}s`;
  const minutes = Math.floor(durationMs / 60_000);
  const seconds = Math.round((durationMs % 60_000) / 1000);
  return `${minutes}m ${String(seconds).padStart(2, "0")}s`;
}

/**
 * 「与 W6 step 树互跳」用的锚点 id。两处（待办行 / 事件树步骤行）都用它，
 * 所以相互跳转不需要额外的状态同步——`document.getElementById` 就能定位。
 */
export function planStepAnchor(stepId: string): string {
  return `plan-step-${stepId}`;
}

export function runStepAnchor(stepId: string): string {
  return `run-step-${stepId}`;
}

/**
 * 滚动到锚点并高亮一瞬；**找到目标才返回 `true`**。
 *
 * 返回值是给「锚点可能还没渲染出来」的调用方用的（R20a 的产物归属跳转要等
 * `run/plan` 落库后 `RunDetail` 才画出那一步），它们可以据此短重试。
 * 没有 DOM 的环境（SSR / 单测）直接返回 `false`：不假装滚过。
 */
export function scrollToAnchor(anchor: string): boolean {
  if (typeof document === "undefined" || typeof document.getElementById !== "function") return false;
  const target = document.getElementById(anchor);
  if (!target) return false;

  target.scrollIntoView?.({ behavior: "smooth", block: "center" });
  target.classList?.add("is-jump-target");
  // 高亮是纯视觉反馈：没有 window/setTimeout 的环境静默跳过。
  if (typeof window !== "undefined" && typeof window.setTimeout === "function") {
    window.setTimeout(() => target.classList?.remove("is-jump-target"), 1200);
  }
  return true;
}
