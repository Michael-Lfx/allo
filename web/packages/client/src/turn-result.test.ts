/** aggregateTurnResult: event-derived items, ordering and usage projection. */
import { describe, expect, it } from "vitest";
import type { RunEvent, RunView } from "@agent-store/protocol";
import { aggregateTurnResult } from "./turn-result";

const RUN_ID = "0190f5fe-turn-result-0000-000000000001";

function event(sequence: number, event_type: string, payload: Record<string, unknown> = {}): RunEvent {
  return { run_id: RUN_ID, sequence, event_type, payload };
}

function view(overrides: Partial<RunView> = {}): RunView {
  return {
    run_id: RUN_ID,
    status: "completed",
    version: 6,
    summary: "final answer",
    output_files: ["out.md"],
    preset_revision: 2,
    content_digest: "sha256:abc",
    ...overrides,
  };
}

describe("aggregateTurnResult", () => {
  it("orders events, derives items and carries the terminal fields", () => {
    const result = aggregateTurnResult(view(), [
      event(3, "attempt.updated", { attempt_id: "att-1", status: "completed" }),
      event(1, "run.started"),
      event(2, "run.plan_changed", { change: "initial_plan" }),
    ]);

    expect(result.status).toBe("completed");
    expect(result.final_response).toBe("final answer");
    expect(result.output_files).toEqual(["out.md"]);
    expect(result.preset_revision).toBe(2);
    expect(result.content_digest).toBe("sha256:abc");
    expect(result.events.map((item) => item.sequence)).toEqual([1, 2, 3]);
    expect(result.items).toEqual([
      { sequence: 1, event_type: "run.started", kind: "other", ref_id: undefined, status: undefined, reason: undefined },
      {
        sequence: 2,
        event_type: "run.plan_changed",
        kind: "plan",
        ref_id: undefined,
        status: undefined,
        reason: "initial_plan",
      },
      {
        sequence: 3,
        event_type: "attempt.updated",
        kind: "attempt",
        ref_id: "att-1",
        status: "completed",
        reason: undefined,
      },
    ]);
    expect(result.usage).toBeUndefined();
  });

  it("projects token usage when the runtime publishes it", () => {
    const result = aggregateTurnResult(view(), [
      event(1, "attempt.updated", { usage: { input_tokens: 120, output_tokens: 40, total_tokens: 160 } }),
    ]);

    expect(result.usage).toEqual({ input_tokens: 120, output_tokens: 40, total_tokens: 160 });
  });

  it("accepts context_usage payloads and null summaries", () => {
    const result = aggregateTurnResult(view({ summary: null }), [
      event(1, "run.status_changed", { context_usage: { inputTokens: 10, outputTokens: 5 } }),
    ]);

    expect(result.final_response).toBeNull();
    expect(result.usage).toEqual({ input_tokens: 10, output_tokens: 5 });
  });
});
