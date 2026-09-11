/**
 * Pending approvals derived from the projected Run event stream (W2 审批卡).
 *
 * The engine answers exactly one decision per waiting attempt: `run/events`
 * projects `approval.requested` with the attempt scope (`step_id` /
 * `attempt_id`) and the three CAS versions a valid answer must echo, then
 * `approval.responded` closes it. This module turns that stream into the single
 * card a review surface may show, and never invents a version of its own — a
 * decision without a complete CAS context is reported as unanswerable instead of
 * being rendered as an action that would fail server-side.
 */

import type { RunEvent } from "@flowy-agent-store/protocol";

/**
 * Dedupe a live/subscribed stream against the replayed `run/events` page by
 * `(run_id, sequence)` and keep it ascending. Live events arrive best-effort and
 * a catch-up intentionally replays overlapping sequences, so the card state must
 * be derived from a sequence-keyed set rather than from arrival order.
 */
export function mergeRunEvents(existing: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  const byKey = new Map<string, RunEvent>();
  for (const event of [...existing, ...incoming]) {
    byKey.set(`${event.run_id}:${event.sequence}`, event);
  }
  return [...byKey.values()].sort((a, b) => a.sequence - b.sequence);
}

/** The pending decision, or `null` when there is nothing (valid) to answer. */
export function pendingApproval(events: RunEvent[]): PendingDecision | null {
  const state = pendingDecision(events);
  return state.status === "pending" ? state.decision : null;
}

export interface PendingDecision {
  /** The `approval.requested` event that opened the decision. */
  event: RunEvent;
  question: string;
  stepId: string;
  attemptId: string;
  expectedExecutionVersion: number;
  expectedStepVersion: number;
  expectedAttemptVersion: number;
}

export type PendingDecisionState =
  | { status: "none" }
  /** The server answered a previous (or concurrent) request: nothing to do. */
  | { status: "answered"; stepId: string | null; attemptId: string | null }
  /** A decision is pending but the projection cannot build a valid answer. */
  | { status: "unanswerable"; reason: string }
  | { status: "pending"; decision: PendingDecision };

function payloadText(payload: Record<string, unknown>, key: string): string | null {
  const value = payload[key];
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function attemptKey(event: RunEvent): string | null {
  const stepId = event.step_id ?? null;
  const attemptId = event.attempt_id ?? null;
  // An event that is not attempt-scoped cannot participate in approval state.
  return stepId && attemptId ? `${stepId}:${attemptId}` : null;
}

/**
 * The newest unresolved decision of one run.
 *
 * Events arrive best-effort and may be replayed by a catch-up, so the scan is
 * keyed by attempt and by `sequence` rather than by arrival order.
 */
export function pendingDecision(events: RunEvent[]): PendingDecisionState {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
  const answered = new Set<string>();
  let candidate: RunEvent | null = null;
  let candidateKey: string | null = null;

  for (const event of ordered) {
    const key = attemptKey(event);
    if (!key) {
      continue;
    }
    if (event.event_type === "approval.responded") {
      answered.add(key);
      if (key === candidateKey) {
        candidate = null;
        candidateKey = null;
      }
      continue;
    }
    if (event.event_type !== "approval.requested") {
      continue;
    }
    if (answered.has(key)) {
      continue;
    }
    // Later requests supersede earlier ones for the same attempt.
    candidate = event;
    candidateKey = key;
  }

  const lastAnswered = [...ordered].reverse().find((event) => event.event_type === "approval.responded");
  if (!candidate || !candidateKey) {
    return lastAnswered
      ? {
          status: "answered",
          stepId: lastAnswered.step_id ?? null,
          attemptId: lastAnswered.attempt_id ?? null,
        }
      : { status: "none" };
  }

  const [stepId, attemptId] = candidateKey.split(":");
  const versions = {
    expectedExecutionVersion: candidate.expected_execution_version,
    expectedStepVersion: candidate.expected_step_version,
    expectedAttemptVersion: candidate.expected_attempt_version,
  };
  const missing = Object.entries(versions)
    .filter(([, value]) => typeof value !== "number")
    .map(([name]) => name);
  if (missing.length > 0) {
    return {
      status: "unanswerable",
      reason: `the projected decision is missing its CAS context: ${missing.join(", ")}`,
    };
  }

  return {
    status: "pending",
    decision: {
      event: candidate,
      question: payloadText(candidate.payload, "question") ?? "The run is waiting for your decision.",
      stepId,
      attemptId,
      expectedExecutionVersion: versions.expectedExecutionVersion as number,
      expectedStepVersion: versions.expectedStepVersion as number,
      expectedAttemptVersion: versions.expectedAttemptVersion as number,
    },
  };
}
