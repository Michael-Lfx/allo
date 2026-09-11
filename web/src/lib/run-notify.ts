/**
 * Background-Run terminal notices (W8 余项, docs/agent-store/16 R13).
 *
 * A Run is projected as `run/events`; `run.status_changed` carries
 * `{status}` and is the only place the client learns that a Run stopped moving.
 * Everything in this module is derived from the event stream alone — no separate
 * run-status polling, so the notice can never disagree with what the review
 * surface shows.
 *
 * Two notices come out of one terminal status:
 *   - an in-tab toast (the landed toast channel, per tab — see the ownership
 *     model in `global-effects.ts`; every tab keeps its own subscription), and
 *   - a *global* reminder (desktop notification + sound) which is only raised
 *     when the tab is in the background, and is elected once per profile.
 */

import type { RunEvent, RunStatus } from "@flowy-agent-store/protocol";

/** Statuses the engine will not move again. */
export const TERMINAL_RUN_STATUSES = [
  "completed",
  "completed_with_failures",
  "failed",
  "cancelled",
] as const;

export type TerminalRunStatus = (typeof TERMINAL_RUN_STATUSES)[number];

/** i18n keys of the in-tab toast per terminal status (both locales). */
export const RUN_TERMINAL_TOAST_KEYS: Record<TerminalRunStatus, string> = {
  completed: "toast.runCompleted",
  completed_with_failures: "toast.runCompletedWithFailures",
  failed: "toast.runFailed",
  cancelled: "toast.runCancelled",
};

/** Toast tone per terminal status (the store maps it to the landed `pushToast`). */
export const RUN_TERMINAL_TONES: Record<TerminalRunStatus, "success" | "error"> = {
  completed: "success",
  completed_with_failures: "success",
  failed: "error",
  cancelled: "error",
};

export function isTerminalRunStatus(status: string): status is TerminalRunStatus {
  return (TERMINAL_RUN_STATUSES as readonly string[]).includes(status);
}

/** Run-event types that carry a run-level status. */
const STATUS_EVENT_TYPES = new Set(["run.started", "run.status_changed"]);

/**
 * The latest status the stream reported for a run, ordered by `sequence` (live
 * events are best-effort and a catch-up replays overlapping sequences, so
 * arrival order is not trustworthy).
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

/** The run's terminal status, or `null` while it may still move. */
export function terminalRunStatus(events: RunEvent[]): TerminalRunStatus | null {
  const status = latestRunStatus(events);
  return typeof status === "string" && isTerminalRunStatus(status) ? status : null;
}

/**
 * Idempotency key for the reminder of one run reaching one terminal status —
 * the value two tabs must agree on to elect a single reminder.
 */
export function runTerminalNoticeKey(runId: string, status: TerminalRunStatus): string {
  return `run:${runId}:terminal:${status}`;
}

export interface TabVisibility {
  hidden: boolean;
  focused: boolean;
}

/**
 * A "background Run" is one that ends while the user is looking elsewhere: the
 * in-tab toast is then invisible, which is exactly when the global reminder
 * (notification + sound) earns its keep. Focused-and-visible tabs stay silent on
 * the global channels.
 */
export function isBackgroundRun(visibility: TabVisibility): boolean {
  return visibility.hidden || !visibility.focused;
}

/** Reads the live tab visibility; the DOM access is isolated to this function. */
export function currentTabVisibility(): TabVisibility {
  return { hidden: document.hidden, focused: document.hasFocus() };
}
