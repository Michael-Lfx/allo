/**
 * W6 Run 状态树（R11）—— 由 `run/events` 事件流派生的运行视图。
 *
 * 这个模块只做**投影**：把 `RunEvent[]` 折叠成「运行头 → 计划修订 → 步骤 →
 * 尝试」的树，供 UI 直接渲染。它不发请求、不猜状态、也不补数据。
 *
 * **为什么只能到这一步（重要，勿按理想补字段）**：截至本次实现，`run/events`
 * 的 payload 是**变更标记**而非资源快照——引擎侧写的是 `{"change":…}`、
 * `{"status":…}`、`{"attempt_status":…,"step_status":…}`、`{"reason":…}`、
 * `{"effect":…}` 这类短标记（`nomifun-agent-execution/src/{engine,scheduler}.rs`），
 * 步**标题**、失败**错误文本**、**时间戳**都不在 wire 上，`RunEvent` 本身也没有
 * `timestamp` 字段。因此本树能给出：步骤/尝试的**存在性、次序、最新状态、
 * 重试次数、会话副作用（steer / stop_turn）、审批问题与是否已回答**，
 * 但**给不出**耗时、失败原因原文与步骤标题——这三项已登记为 `16` 已知偏差
 * **D-W6-1**（要补齐需 additive 暴露执行详情，属协议增量，未获批准前不动）。
 */

import type { RunEvent } from "@flowy-agent-store/protocol";
import { isTerminalRunStatus } from "./run-notify";

export interface RunTreeHeader {
  runId: string | null;
  /** 事件流报告的最新状态（`run.started` / `run.status_changed`）。 */
  status: string | null;
  terminal: boolean;
  /** `run.status_changed` 上最近一次 `reason`（如 `plan_approved` / `resumed`）。 */
  statusReason: string | null;
  lastSequence: number;
  eventCount: number;
}

export interface PlanRevisionNode {
  sequence: number;
  /** `initial_plan` / `replanned` / `adjusted` / `steps_added` / `delegated_steps_appended` / … */
  change: string;
  intent: string | null;
  status: string | null;
}

export interface ApprovalNode {
  sequence: number;
  question: string;
  answered: boolean;
  /** 是否带齐三路 CAS（缺则 UI 不应渲染可提交操作）。 */
  answerable: boolean;
}

export interface AttemptNode {
  attemptId: string;
  firstSequence: number;
  lastSequence: number;
  status: string | null;
  /** 该尝试上出现过的标记（`change` / `effect` / `reason` / `reconciliation`）。 */
  markers: string[];
  approval: ApprovalNode | null;
  eventCount: number;
}

export interface StepEffectNode {
  sequence: number;
  /** `steer` / `stop_turn` / `decision_input`。 */
  effect: string;
  delivered: boolean;
}

export interface StepNode {
  stepId: string;
  firstSequence: number;
  lastSequence: number;
  status: string | null;
  kind: string | null;
  /** `change=retry_requested` 的次数（显式重试请求）。 */
  retries: number;
  attempts: AttemptNode[];
  effects: StepEffectNode[];
  markers: string[];
  eventCount: number;
}

export interface RunTree {
  header: RunTreeHeader;
  planRevisions: PlanRevisionNode[];
  steps: StepNode[];
  /** 事件总数里**没有落进任何节点**的条数（既不是运行头状态、也不是计划修订、
   *  也不是某 step/attempt 的事件）——用于让 UI 说明「还有 N 条未能归类」。 */
  unattributedEventCount: number;
}

export type RunStatusTone = "pending" | "active" | "attention" | "ok" | "bad";

/** 徽标色调（CSS class 后缀），把协议状态收敛成 5 档。 */
export function runStatusTone(status: string | null): RunStatusTone {
  switch (status) {
    case "planning":
    case "running":
      return "active";
    case "paused":
    case "waiting_input":
    case "awaiting_approval":
    case "recovery_required":
      return "attention";
    case "completed":
      return "ok";
    case "completed_with_failures":
    case "failed":
      return "bad";
    case "cancelled":
      return "pending";
    default:
      return "pending";
  }
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** 事件去重（按 `run_id:sequence`）并按 sequence 升序——实时投递是尽力而为的。 */
export function orderRunEvents(events: RunEvent[]): RunEvent[] {
  const byKey = new Map<string, RunEvent>();
  for (const event of events) {
    byKey.set(`${event.run_id}:${event.sequence}`, event);
  }
  return [...byKey.values()].sort((a, b) => a.sequence - b.sequence);
}

/** 一个事件在 payload 里报告的「对**步骤**状态的最新说法」。
 *
 * 两个字段都只在**无 attempt 作用域**时才当作步骤状态：引擎同时用 `status`
 * 表达「尝试排队/运行」（`{"status":"queued"}`），带 attempt 的事件不再反推步骤。 */
function payloadStepStatus(event: RunEvent): string | null {
  return text(event.payload?.step_status) ?? (event.attempt_id ? null : text(event.payload?.status));
}

function payloadAttemptStatus(event: RunEvent): string | null {
  return text(event.payload?.attempt_status) ?? text(event.payload?.status);
}

function markerOf(event: RunEvent): string[] {
  const markers: string[] = [];
  for (const key of ["change", "effect", "reason", "reconciliation"]) {
    const value = text(event.payload?.[key]);
    if (value) markers.push(`${key}=${value}`);
  }
  return markers;
}

/**
 * 折叠事件流为状态树。**幂等**：同一批事件无论顺序/重复，结果一致。
 */
export function buildRunTree(events: RunEvent[]): RunTree {
  const ordered = orderRunEvents(events);
  const steps = new Map<string, StepNode>();
  const attempts = new Map<string, AttemptNode>();
  const planRevisions: PlanRevisionNode[] = [];
  let unattributedEventCount = 0;
  let runId: string | null = null;
  let status: string | null = null;
  let statusReason: string | null = null;
  let lastSequence = 0;

  for (const event of ordered) {
    runId = runId ?? event.run_id;
    lastSequence = Math.max(lastSequence, event.sequence);
    const stepId = text(event.step_id);
    const attemptId = text(event.attempt_id);
    let placed = false;

    if (event.event_type === "run.started" || event.event_type === "run.status_changed") {
      placed = true;
      const reported = text(event.payload?.status);
      if (reported) status = reported;
      const reason = text(event.payload?.reason);
      if (reason) statusReason = reason;
    }

    if (event.event_type === "run.plan_changed") {
      placed = true;
      planRevisions.push({
        sequence: event.sequence,
        change: text(event.payload?.change) ?? "changed",
        intent: text(event.payload?.intent),
        status: text(event.payload?.status),
      });
      // A plan change without a step scope is run-level; one carried by an
      // attempt (delegated steps) still belongs to that step/attempt below.
      if (!stepId) {
        continue;
      }
    }

    if (!stepId && !attemptId) {
      if (!placed) unattributedEventCount += 1;
      continue;
    }

    if (stepId) {
      const step =
        steps.get(stepId) ??
        ({
          stepId,
          firstSequence: event.sequence,
          lastSequence: event.sequence,
          status: null,
          kind: null,
          retries: 0,
          attempts: [],
          effects: [],
          markers: [],
          eventCount: 0,
        } satisfies StepNode);
      step.lastSequence = Math.max(step.lastSequence, event.sequence);
      step.firstSequence = Math.min(step.firstSequence, event.sequence);
      step.eventCount += 1;
      steps.set(stepId, step);
      const reported = payloadStepStatus(event);
      if (reported) step.status = reported;
      const kind = text(event.payload?.control) ?? text(event.payload?.kind);
      if (kind) step.kind = kind;
      step.markers.push(...markerOf(event));
      if (text(event.payload?.change) === "retry_requested") step.retries += 1;
      const effect = text(event.payload?.effect);
      if (effect) {
        step.effects.push({
          sequence: event.sequence,
          effect,
          delivered: text(event.payload?.change) === "conversation_effect_delivered",
        });
      }
      if (!attemptId) {
        continue;
      }
      let attempt = attempts.get(attemptId);
      if (!attempt) {
        attempt = {
          attemptId,
          firstSequence: event.sequence,
          lastSequence: event.sequence,
          status: null,
          markers: [],
          approval: null,
          eventCount: 0,
        };
        attempts.set(attemptId, attempt);
        step.attempts.push(attempt);
      }
      attempt.lastSequence = Math.max(attempt.lastSequence, event.sequence);
      attempt.firstSequence = Math.min(attempt.firstSequence, event.sequence);
      attempt.eventCount += 1;
      const attemptStatus = payloadAttemptStatus(event);
      if (attemptStatus) attempt.status = attemptStatus;
      attempt.markers.push(...markerOf(event));
      if (event.event_type === "approval.requested") {
        attempt.approval = {
          sequence: event.sequence,
          question: text(event.payload?.question) ?? "",
          answered: false,
          answerable:
            typeof event.expected_execution_version === "number" &&
            typeof event.expected_step_version === "number" &&
            typeof event.expected_attempt_version === "number",
        };
      }
      if (event.event_type === "approval.responded" && attempt.approval) {
        attempt.approval.answered = true;
      }
      continue;
    }

    // attempt-scoped without a step scope: keep it counted as unplaced (there is
    // no step node to hang it on).
    unattributedEventCount += 1;
  }

  return {
    header: {
      runId,
      status,
      terminal: status !== null && isTerminalRunStatus(status),
      statusReason,
      lastSequence,
      eventCount: ordered.length,
    },
    planRevisions,
    steps: [...steps.values()].sort((a, b) => a.firstSequence - b.firstSequence || a.stepId.localeCompare(b.stepId)),
    unattributedEventCount,
  };
}
