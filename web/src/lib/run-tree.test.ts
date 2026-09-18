import { describe, expect, it } from "vitest";

import type { RunEvent } from "@flowy-agent-store/protocol";
import { buildRunTree, orderRunEvents, runStatusTone } from "./run-tree";

function event(
  sequence: number,
  event_type: string,
  options: {
    payload?: Record<string, unknown>;
    stepId?: string | null;
    attemptId?: string | null;
    runId?: string;
    cas?: boolean;
  } = {},
): RunEvent {
  const base: RunEvent = {
    run_id: options.runId ?? "run-1",
    sequence,
    event_type,
    payload: options.payload ?? {},
    step_id: options.stepId ?? null,
    attempt_id: options.attemptId ?? null,
  };
  if (options.cas) {
    return {
      ...base,
      expected_execution_version: 3,
      expected_step_version: 2,
      expected_attempt_version: 1,
    };
  }
  return base;
}

const RUN_STARTED = event(1, "run.started", { payload: { status: "planning" } });

describe("orderRunEvents", () => {
  it("dedupes by run+sequence and sorts ascending", () => {
    const ordered = orderRunEvents([event(2, "b"), event(1, "a"), event(2, "b")]);
    expect(ordered.map((item) => item.sequence)).toEqual([1, 2]);
  });
});

describe("buildRunTree · header", () => {
  it("reports the latest status by sequence, not by arrival order", () => {
    const tree = buildRunTree([
      event(5, "run.status_changed", { payload: { status: "cancelled" } }),
      RUN_STARTED,
      event(3, "run.status_changed", { payload: { status: "running", reason: "plan_approved" } }),
    ]);
    expect(tree.header.status).toBe("cancelled");
    expect(tree.header.terminal).toBe(true);
    expect(tree.header.runId).toBe("run-1");
    expect(tree.header.lastSequence).toBe(5);
    expect(tree.header.eventCount).toBe(3);
  });

  it("keeps the last reason without treating a running run as terminal", () => {
    const tree = buildRunTree([
      RUN_STARTED,
      event(2, "run.status_changed", { payload: { status: "running", reason: "plan_approved" } }),
    ]);
    expect(tree.header.status).toBe("running");
    expect(tree.header.statusReason).toBe("plan_approved");
    expect(tree.header.terminal).toBe(false);
  });

  it("is terminal for every terminal status the engine emits", () => {
    for (const status of ["completed", "completed_with_failures", "failed", "cancelled"]) {
      const tree = buildRunTree([RUN_STARTED, event(2, "run.status_changed", { payload: { status } })]);
      expect(tree.header.terminal).toBe(true);
    }
  });
});

describe("buildRunTree · plan revisions", () => {
  it("collects plan changes in sequence order with their intent", () => {
    const tree = buildRunTree([
      event(1, "run.plan_changed", { payload: { status: "running", change: "initial_plan" } }),
      event(2, "run.plan_changed", { payload: { change: "replanned" } }),
      event(3, "run.plan_changed", { payload: { change: "adjusted", intent: "narrow the scope" } }),
    ]);
    expect(tree.planRevisions.map((revision) => revision.change)).toEqual([
      "initial_plan",
      "replanned",
      "adjusted",
    ]);
    expect(tree.planRevisions[0].status).toBe("running");
    expect(tree.planRevisions[2].intent).toBe("narrow the scope");
    // A plan change with no step scope is a run-level plan revision, so it is
    // placed (not counted as unplaced).
    expect(tree.steps).toHaveLength(0);
    expect(tree.unattributedEventCount).toBe(0);
  });
});

describe("buildRunTree · steps and attempts", () => {
  it("keeps two parallel steps independent", () => {
    const tree = buildRunTree([
      RUN_STARTED,
      event(2, "task.updated", { stepId: "s-a", payload: { status: "running" } }),
      event(3, "task.updated", { stepId: "s-b", payload: { status: "running" } }),
      event(4, "attempt.updated", { stepId: "s-a", attemptId: "at-1", payload: { attempt_status: "running", step_status: "running" } }),
      event(5, "attempt.updated", {
        stepId: "s-b",
        attemptId: "at-2",
        payload: { attempt_status: "failed", step_status: "failed", reason: "runtime_error" },
      }),
    ]);
    const [a, b] = tree.steps;
    expect(a.stepId).toBe("s-a");
    expect(a.status).toBe("running");
    expect(a.attempts.map((attempt) => attempt.attemptId)).toEqual(["at-1"]);
    expect(b.stepId).toBe("s-b");
    expect(b.status).toBe("failed");
    expect(b.attempts[0].status).toBe("failed");
    expect(b.attempts[0].markers).toContain("reason=runtime_error");
    // Neither step absorbed the other's events.
    expect(a.eventCount).toBe(2);
    expect(b.eventCount).toBe(2);
  });

  it("nests attempts under their step in first-seen order and counts retries", () => {
    const tree = buildRunTree([
      RUN_STARTED,
      event(2, "attempt.updated", { stepId: "s-1", attemptId: "at-1", payload: { status: "queued" } }),
      event(3, "attempt.updated", { stepId: "s-1", attemptId: "at-1", payload: { status: "running" } }),
      event(4, "task.updated", { stepId: "s-1", payload: { change: "retry_requested" } }),
      event(5, "attempt.updated", { stepId: "s-1", attemptId: "at-2", payload: { status: "queued" } }),
      event(6, "attempt.updated", { stepId: "s-1", attemptId: "at-2", payload: { attempt_status: "completed", step_status: "completed" } }),
    ]);
    const step = tree.steps[0];
    expect(step.attempts.map((attempt) => attempt.attemptId)).toEqual(["at-1", "at-2"]);
    expect(step.retries).toBe(1);
    expect(step.status).toBe("completed");
    expect(step.attempts[0].status).toBe("running");
    expect(step.attempts[1].status).toBe("completed");
    expect(step.attempts[0].firstSequence).toBe(2);
    expect(step.attempts[0].lastSequence).toBe(3);
  });

  it("does not mistake an attempt status for a step status", () => {
    const tree = buildRunTree([
      event(1, "attempt.updated", { stepId: "s-1", attemptId: "at-1", payload: { status: "queued" } }),
    ]);
    expect(tree.steps[0].status).toBeNull();
    expect(tree.steps[0].attempts[0].status).toBe("queued");
  });

  it("records the step kind from control attempts", () => {
    const tree = buildRunTree([
      event(1, "attempt.updated", { stepId: "s-1", attemptId: "at-1", payload: { status: "running", control: "plan" } }),
    ]);
    expect(tree.steps[0].kind).toBe("plan");
  });

  it("tracks conversation effects and whether they were delivered", () => {
    const tree = buildRunTree([
      event(1, "task.updated", {
        stepId: "s-1",
        attemptId: "at-1",
        payload: { change: "conversation_effect_requested", effect: "steer", operation_id: "op-1" },
      }),
      event(2, "task.updated", {
        stepId: "s-1",
        attemptId: "at-1",
        payload: { change: "conversation_effect_delivered", effect: "steer", operation_id: "op-1" },
      }),
      event(3, "task.updated", {
        stepId: "s-1",
        attemptId: "at-1",
        payload: { change: "conversation_effect_requested", effect: "stop_turn", operation_id: "op-2" },
      }),
    ]);
    expect(tree.steps[0].effects).toEqual([
      { sequence: 1, effect: "steer", delivered: false },
      { sequence: 2, effect: "steer", delivered: true },
      { sequence: 3, effect: "stop_turn", delivered: false },
    ]);
  });

  it("attaches approvals to their attempt and marks the answered one", () => {
    const tree = buildRunTree([
      event(1, "approval.requested", {
        stepId: "s-1",
        attemptId: "at-1",
        payload: { question: "Allow the write?", stop_turn_operation_id: "op-1" },
        cas: true,
      }),
      event(2, "approval.responded", { stepId: "s-1", attemptId: "at-1", payload: { answered: true } }),
    ]);
    const approval = tree.steps[0].attempts[0].approval;
    expect(approval?.question).toBe("Allow the write?");
    expect(approval?.answered).toBe(true);
    expect(approval?.answerable).toBe(true);
  });

  it("reports an approval without CAS tokens as unanswerable", () => {
    const tree = buildRunTree([
      event(1, "approval.requested", {
        stepId: "s-1",
        attemptId: "at-1",
        payload: { question: "Allow the write?" },
      }),
    ]);
    expect(tree.steps[0].attempts[0].approval?.answerable).toBe(false);
  });
});

describe("buildRunTree · robustness", () => {
  it("counts only the events that ended up in no node at all", () => {
    const tree = buildRunTree([
      RUN_STARTED,
      event(2, "run.status_changed", { payload: { status: "running" } }),
      event(3, "attempt.updated", { attemptId: "at-9", payload: { status: "queued" } }),
      event(4, "run.deleted", { payload: { deleted: true } }),
    ]);
    // run.started / run.status_changed feed the header; the step-less attempt
    // event and run.deleted land nowhere.
    expect(tree.unattributedEventCount).toBe(2);
    expect(tree.steps).toHaveLength(0);
    expect(tree.header.status).toBe("running");
  });

  it("never throws on unknown event types or payload shapes", () => {
    const tree = buildRunTree([
      event(1, "future.event", { stepId: "s-1", payload: {} }),
      event(2, "future.event", { stepId: "s-1", payload: { status: 42, question: null } }),
    ]);
    expect(tree.steps[0].status).toBeNull();
    expect(tree.steps[0].markers).toEqual([]);
  });

  it("is idempotent and order independent", () => {
    const events = [
      RUN_STARTED,
      event(2, "task.updated", { stepId: "s-1", payload: { change: "retry_requested" } }),
      event(3, "attempt.updated", { stepId: "s-1", attemptId: "at-2", payload: { status: "running" } }),
    ];
    const forward = buildRunTree(events);
    const shuffled = buildRunTree([events[1], events[2], events[0], events[1]]);
    expect(shuffled).toEqual(forward);
  });

  it("handles an empty stream", () => {
    const tree = buildRunTree([]);
    expect(tree.header.status).toBeNull();
    expect(tree.header.terminal).toBe(false);
    expect(tree.steps).toEqual([]);
  });
});

describe("runStatusTone", () => {
  it("maps every protocol status onto the five badge tones", () => {
    expect(runStatusTone("planning")).toBe("active");
    expect(runStatusTone("running")).toBe("active");
    expect(runStatusTone("paused")).toBe("attention");
    expect(runStatusTone("waiting_input")).toBe("attention");
    expect(runStatusTone("awaiting_approval")).toBe("attention");
    expect(runStatusTone("recovery_required")).toBe("attention");
    expect(runStatusTone("completed")).toBe("ok");
    expect(runStatusTone("completed_with_failures")).toBe("bad");
    expect(runStatusTone("failed")).toBe("bad");
    expect(runStatusTone("cancelled")).toBe("pending");
    expect(runStatusTone(null)).toBe("pending");
    expect(runStatusTone("something-new")).toBe("pending");
  });
});
