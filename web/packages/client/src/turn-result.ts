/**
 * TurnResult aggregation (REQ-PAR-05c): one terminal object per run built
 * from the authoritative `run/result` view plus the persisted `run/events`
 * stream.
 *
 * Token usage is projected only when the runtime publishes it on the event
 * stream (`usage` / `context_usage` payloads); the field stays absent
 * otherwise — callers must not assume it is populated.
 */

import type { RunEvent, RunStatus, RunView } from "@agent-store/protocol";

/** Token/context usage for the turn (absent until the runtime projects it). */
export interface TurnUsage {
  input_tokens?: number;
  output_tokens?: number;
  total_tokens?: number;
}

/** One event-derived item (plan revision, task, attempt, approval, ...). */
export interface TurnItem {
  sequence: number;
  event_type: string;
  kind: "plan" | "task" | "attempt" | "status" | "approval" | "other";
  /** Attempt/step identity when the event payload carries one. */
  ref_id?: string;
  status?: string;
  reason?: string;
}

/** Aggregated terminal result returned by `AgentRunHandle.finished`. */
export interface TurnResult {
  run_id: string;
  status: RunStatus;
  version: number;
  /** Terminal assistant text (`run/result` summary). */
  final_response: string | null;
  output_files: string[];
  preset_revision?: number | null;
  content_digest?: string | null;
  /** Persisted event stream in sequence order (authoritative backfill). */
  events: RunEvent[];
  /** Event-derived items in the same order. */
  items: TurnItem[];
  usage?: TurnUsage;
}

const KIND_BY_EVENT: Record<string, TurnItem["kind"]> = {
  "run.plan_changed": "plan",
  "task.updated": "task",
  "attempt.updated": "attempt",
  "run.status_changed": "status",
  "approval.requested": "approval",
  "approval.responded": "approval",
};

function asRecord(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === "object" ? (payload as Record<string, unknown>) : {};
}

function pickString(source: Record<string, unknown>, key: string): string | undefined {
  const value = source[key];
  return typeof value === "string" ? value : undefined;
}

function toItem(event: RunEvent): TurnItem {
  const payload = asRecord(event.payload);
  return {
    sequence: event.sequence,
    event_type: event.event_type,
    kind: KIND_BY_EVENT[event.event_type] ?? "other",
    ref_id: pickString(payload, "attempt_id") ?? pickString(payload, "step_id"),
    status: pickString(payload, "status"),
    reason: pickString(payload, "reason") ?? pickString(payload, "change"),
  };
}

function extractUsage(events: RunEvent[]): TurnUsage | undefined {
  for (const event of events) {
    const payload = asRecord(event.payload);
    const candidate = asRecord(payload["usage"] ?? payload["context_usage"]);
    const input = candidate["input_tokens"] ?? candidate["inputTokens"];
    const output = candidate["output_tokens"] ?? candidate["outputTokens"];
    const total = candidate["total_tokens"] ?? candidate["totalTokens"];
    if (typeof input === "number" || typeof output === "number" || typeof total === "number") {
      return {
        ...(typeof input === "number" ? { input_tokens: input } : {}),
        ...(typeof output === "number" ? { output_tokens: output } : {}),
        ...(typeof total === "number" ? { total_tokens: total } : {}),
      };
    }
  }
  return undefined;
}

/** Build the terminal turn result from the authoritative view + event stream. */
export function aggregateTurnResult(view: RunView, events: RunEvent[]): TurnResult {
  const ordered = [...events].sort((left, right) => left.sequence - right.sequence);
  const usage = extractUsage(ordered);
  return {
    run_id: view.run_id,
    status: view.status,
    version: view.version,
    final_response: view.summary ?? null,
    output_files: view.output_files ?? [],
    preset_revision: view.preset_revision ?? null,
    content_digest: view.content_digest ?? null,
    events: ordered,
    items: ordered.map(toItem),
    ...(usage ? { usage } : {}),
  };
}
