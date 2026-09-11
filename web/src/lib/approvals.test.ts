import { describe, expect, it } from "vitest";
import { mergeRunEvents, pendingApproval, pendingDecision } from "./approvals";
import type { RunEvent } from "./protocol";

const STEP = "0190f5fe-7c00-7a00-8000-000000000011";
const ATTEMPT = "0190f5fe-7c00-7a00-8000-000000000012";
const OTHER_ATTEMPT = "0190f5fe-7c00-7a00-8000-000000000013";

function requested(
  sequence: number,
  overrides: Partial<RunEvent> = {},
  attemptId = ATTEMPT,
): RunEvent {
  return {
    run_id: "run_01",
    sequence,
    event_type: "approval.requested",
    payload: { question: "Continue with the deployment?", stop_turn_operation_id: "op_01" },
    step_id: STEP,
    attempt_id: attemptId,
    expected_execution_version: 4,
    expected_step_version: 5,
    expected_attempt_version: 6,
    ...overrides,
  };
}

function responded(sequence: number, attemptId = ATTEMPT): RunEvent {
  return {
    run_id: "run_01",
    sequence,
    event_type: "approval.responded",
    payload: { answered: true, operation_id: "op_02" },
    step_id: STEP,
    attempt_id: attemptId,
  };
}

describe("pendingDecision", () => {
  it("returns none for a run without a decision request", () => {
    expect(
      pendingDecision([
        { run_id: "run_01", sequence: 1, event_type: "run.started", payload: {} },
      ]),
    ).toEqual({ status: "none" });
  });

  it("exposes the question, the attempt scope and the three CAS versions", () => {
    const state = pendingDecision([requested(2)]);
    expect(state.status).toBe("pending");
    if (state.status !== "pending") {
      return;
    }
    expect(state.decision.question).toBe("Continue with the deployment?");
    expect(state.decision.stepId).toBe(STEP);
    expect(state.decision.attemptId).toBe(ATTEMPT);
    expect(state.decision.expectedExecutionVersion).toBe(4);
    expect(state.decision.expectedStepVersion).toBe(5);
    expect(state.decision.expectedAttemptVersion).toBe(6);
  });

  it("is answered once a later approval.responded covers the same attempt", () => {
    const state = pendingDecision([requested(2), responded(3)]);
    expect(state).toEqual({ status: "answered", stepId: STEP, attemptId: ATTEMPT });
  });

  it("ignores events that are not attempt-scoped", () => {
    const unscoped = { ...requested(2), step_id: null, attempt_id: null };
    const state = pendingDecision([unscoped]);
    expect(state).toEqual({ status: "none" });
  });

  it("does not let a response for another attempt close this decision", () => {
    const state = pendingDecision([requested(2), responded(3, OTHER_ATTEMPT)]);
    expect(state.status).toBe("pending");
  });

  it("keeps the newest request when a catch-up replays the stream out of order", () => {
    const older = requested(2, { expected_attempt_version: 6 });
    const newer = requested(9, { expected_attempt_version: 8 });
    const state = pendingDecision([newer, older]);
    expect(state.status).toBe("pending");
    if (state.status !== "pending") {
      return;
    }
    expect(state.decision.event.sequence).toBe(9);
    expect(state.decision.expectedAttemptVersion).toBe(8);
  });

  it("refuses to render an answerable card without a complete CAS context", () => {
    const state = pendingDecision([requested(2, { expected_step_version: null })]);
    expect(state.status).toBe("unanswerable");
    if (state.status !== "unanswerable") {
      return;
    }
    expect(state.reason).toContain("expectedStepVersion");
  });
});

describe("mergeRunEvents", () => {
  it("dedupes a replayed catch-up page by sequence and keeps the order ascending", () => {
    const history = [requested(3), responded(4)].map((event) => ({ ...event, run_id: "run_1" }));
    const live = [responded(4), requested(5)].map((event) => ({ ...event, run_id: "run_1" }));
    const merged = mergeRunEvents(history, live);
    expect(merged.map((event) => event.sequence)).toEqual([3, 4, 5]);
    expect(merged).toHaveLength(3);
  });

  it("keeps the recomputed (newer) copy of a duplicated sequence", () => {
    const stale: RunEvent = { ...requested(7), run_id: "run_1", expected_attempt_version: 1 };
    const refreshed: RunEvent = { ...requested(7), run_id: "run_1", expected_attempt_version: 9 };
    const merged = mergeRunEvents([stale], [refreshed]);
    expect(merged).toHaveLength(1);
    expect(merged[0].expected_attempt_version).toBe(9);
  });
});

describe("pendingApproval", () => {
  it("returns the decision the card may submit, and null otherwise", () => {
    expect(pendingApproval([requested(2)])).toMatchObject({
      stepId: STEP,
      attemptId: ATTEMPT,
      expectedStepVersion: 5,
    });
    expect(pendingApproval([requested(2), responded(3)])).toBeNull();
    expect(pendingApproval([])).toBeNull();
  });
});
