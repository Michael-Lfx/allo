/**
 * 后台运行（Run）的终端提示（W8 余项，docs/agent-store/16 R13）。
 *
 * 一个 Run 被投影为 `run/events`；`run.status_changed` 携带 `{status}`，是客户端
 * 得知一个 Run 停止推进的唯一途径。本模块一切都仅从事件流派生——没有独立的运行
 * 状态轮询，因此提示绝不会与 review 界面所展示的内容相左。
 *
 * 同一个终端状态会产出两类提示：
 *   - 一个标签页内的 toast（已落地的 toast 通道，每标签页一份——见 `global-effects.ts`
 *     中的归属模型；每个标签页持有自己的订阅），以及
 *   - 一个*全局*提醒（桌面通知 + 声音），仅当标签页处于后台时才触发，且每个 profile
 *     选举一次。
 */

import type { RunEvent, RunStatus } from "@flowy-agent-store/protocol";

/** 引擎不会再推进的状态。 */
export const TERMINAL_RUN_STATUSES = [
  "completed",
  "completed_with_failures",
  "failed",
  "cancelled",
] as const;

export type TerminalRunStatus = (typeof TERMINAL_RUN_STATUSES)[number];

/** 按终端状态的标签页内 toast 的 i18n 键（两种语言环境）。 */
export const RUN_TERMINAL_TOAST_KEYS: Record<TerminalRunStatus, string> = {
  completed: "toast.runCompleted",
  completed_with_failures: "toast.runCompletedWithFailures",
  failed: "toast.runFailed",
  cancelled: "toast.runCancelled",
};

/** 按终端状态的 toast 语气（store 将其映射到已落地的 `pushToast`）。 */
export const RUN_TERMINAL_TONES: Record<TerminalRunStatus, "success" | "error"> = {
  completed: "success",
  completed_with_failures: "success",
  failed: "error",
  cancelled: "error",
};

export function isTerminalRunStatus(status: string): status is TerminalRunStatus {
  return (TERMINAL_RUN_STATUSES as readonly string[]).includes(status);
}

/** 携带运行级状态的 Run 事件类型。 */
const STATUS_EVENT_TYPES = new Set(["run.started", "run.status_changed"]);

/**
 * 流所上报的、某个 run 的最新状态，按 `sequence` 排序（实时事件是尽力而为，而一次
 * 追赶会重放重叠的序列，因此到达顺序不可信）。
 */
export function latestRunStatus(events: RunEvent[]): RunStatus | string | null {
  let latest: { sequence: number; status: string } | null = null;
  for (const event of events) {
    if (!STATUS_EVENT_TYPES.has(event.event_type)) continue;
    const status = event.payload?.status;
    if (typeof status !== "string" || status.length === 0) continue;
    if (!latest || event.sequence >= latest.sequence) latest = { sequence: event.sequence, status };
  }
  return latest?.status ?? null;
}

/** 该 run 的终端状态；只要它可能仍在推进，就为 `null`。 */
export function terminalRunStatus(events: RunEvent[]): TerminalRunStatus | null {
  const status = latestRunStatus(events);
  return typeof status === "string" && isTerminalRunStatus(status) ? status : null;
}

/**
 * 一个 run 到达某个终端状态时的提醒所使用幂等键——两个标签页必须据此达成一致的那个
 * 值，以选举出唯一的提醒。
 */
export function runTerminalNoticeKey(runId: string, status: TerminalRunStatus): string {
  return `run:${runId}:terminal:${status}`;
}

export interface TabVisibility {
  hidden: boolean;
  focused: boolean;
}

/**
 * “后台 Run”是指用户正看着别处时结束的 run：此时标签页内的 toast 不可见，恰恰是全局
 * 提醒（通知 + 声音）发挥作用的时候。已聚焦且可见的标签页在全局通道上保持沉默。
 */
export function isBackgroundRun(visibility: TabVisibility): boolean {
  return visibility.hidden || !visibility.focused;
}

/** 读取实时标签页可见性；DOM 访问被隔离到此函数。 */
export function currentTabVisibility(): TabVisibility {
  return { hidden: document.hidden, focused: document.hasFocus() };
}
