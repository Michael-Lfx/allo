/**
 * `run/answer-decision` client binding (R8 · W2).
 *
 * The method is a thin, exact pass-through: the three CAS tokens reach the wire
 * verbatim under their protocol names, and no desktop-only approve-all switch is
 * ever added by the client.
 */
import { describe, expect, it } from "vitest";
import { AppServerError } from "@flowy-agent-store/protocol";
import { RunClient } from "./runs";
import type { NotificationListener, Transport } from "./transport";

const RUN_ID = "0190f5fe-7c00-7a00-8000-000000000010";
const STEP_ID = "0190f5fe-7c00-7a00-8000-000000000011";
const ATTEMPT_ID = "0190f5fe-7c00-7a00-8000-000000000012";

class RecordingTransport implements Transport {
  requests: { method: string; params: Record<string, unknown> }[] = [];

  async connect(): Promise<void> {}
  async request<T>(method: string, params: unknown): Promise<T> {
    this.requests.push({ method, params: params as Record<string, unknown> });
    return {
      run_id: RUN_ID,
      status: "running",
      version: 7,
      summary: null,
      output_files: [],
    } as unknown as T;
  }
  notify(): void {}
  onNotification(_listener: NotificationListener): () => void {
    return () => {};
  }
  close(): void {}
}

describe("RunClient.answerDecision", () => {
  it("sends the three CAS versions and the attempt scope verbatim", async () => {
    const transport = new RecordingTransport();
    const client = new RunClient(transport);

    const view = await client.answerDecision({
      runId: RUN_ID,
      stepId: STEP_ID,
      attemptId: ATTEMPT_ID,
      answer: "yes, deploy",
      expectedExecutionVersion: 4,
      expectedStepVersion: 5,
      expectedAttemptVersion: 6,
    });

    expect(transport.requests).toHaveLength(1);
    const [call] = transport.requests;
    expect(call.method).toBe("run/answer-decision");
    expect(call.params).toEqual({
      run_id: RUN_ID,
      step_id: STEP_ID,
      attempt_id: ATTEMPT_ID,
      answer: "yes, deploy",
      expected_execution_version: 4,
      expected_step_version: 5,
      expected_attempt_version: 6,
    });
    // Exactly the protocol contract: nothing additive, nothing missing.
    expect(Object.keys(call.params).sort()).toEqual([
      "answer",
      "attempt_id",
      "expected_attempt_version",
      "expected_execution_version",
      "expected_step_version",
      "run_id",
      "step_id",
    ]);
    // The desktop confirmation route's approve-all flag has no counterpart.
    expect(call.params).not.toHaveProperty("always_allow");
    expect(call.params).not.toHaveProperty("approve_all");
    expect(view.status).toBe("running");
  });

  it("keeps the answer text untouched (whitespace is not trimmed client-side)", async () => {
    const transport = new RecordingTransport();
    const client = new RunClient(transport);
    await client.answerDecision({
      runId: RUN_ID,
      stepId: STEP_ID,
      attemptId: ATTEMPT_ID,
      answer: "  approved  ",
      expectedExecutionVersion: 1,
      expectedStepVersion: 1,
      expectedAttemptVersion: 1,
    });
    expect(transport.requests[0].params.answer).toBe("  approved  ");
  });

  it("surfaces a wire conflict instead of retrying with fresh versions", async () => {
    class ConflictTransport implements Transport {
      calls = 0;
      async connect(): Promise<void> {}
      async request<T>(): Promise<T> {
        this.calls += 1;
        throw new AppServerError({
          code: "conflict",
          message: "waiting attempt changed before the answer",
          retryable: false,
          details: {},
        });
      }
      notify(): void {}
      onNotification(_listener: NotificationListener): () => void {
        return () => {};
      }
      close(): void {}
    }
    const transport = new ConflictTransport();
    const client = new RunClient(transport);
    await expect(
      client.answerDecision({
        runId: RUN_ID,
        stepId: STEP_ID,
        attemptId: ATTEMPT_ID,
        answer: "yes",
        expectedExecutionVersion: 1,
        expectedStepVersion: 1,
        expectedAttemptVersion: 1,
      }),
    ).rejects.toMatchObject({ code: "conflict" });
    expect(transport.calls).toBe(1);
  });
});
