import { describe, expect, it } from "vitest";

import { decodeConversationEvent } from "./conversation-events";
import type { ConversationEvent } from "./protocol";

function frame(event_type: string, payload: Record<string, unknown>, sequence = 1): ConversationEvent {
  return { conversation_id: "c1", sequence, event_type, payload } as unknown as ConversationEvent;
}

describe("decodeConversationEvent", () => {
  it("normalizes an activity frame whose kind is thinking into the thinking body", () => {
    const decoded = decodeConversationEvent(
      frame("message.activity", {
        kind: "thinking",
        content: "第一步",
        subject: "分析",
        status: "running",
        duration_ms: 12,
        message_id: "m1",
        created_at: 7,
      }),
    );
    expect(decoded.kind).toBe("message.thinking");
    if (decoded.kind !== "message.thinking") throw new Error("unreachable");
    expect(decoded.thinking).toEqual({ content: "第一步", subject: "分析", status: "running", duration: 12 });
    expect(decoded.messageId).toBe("m1");
    expect(decoded.createdAt).toBe(7);
    expect(decoded.replace).toBe(false);
  });

  it("carries the server id and timestamp, leaving defaults to the consumer", () => {
    const decoded = decodeConversationEvent(frame("message.created", { content: "hi" }));
    expect(decoded.kind).toBe("message.created");
    if (decoded.kind !== "message.created") throw new Error("unreachable");
    // No `message_id` / `created_at` on the wire → both stay null, so the
    // consumer owns the row identity and the clock.
    expect(decoded.messageId).toBeNull();
    expect(decoded.createdAt).toBeNull();
    expect(decoded.content).toBe("hi");
  });

  it("decodes the error code and the three-valued retryable flag", () => {
    const retryable = decodeConversationEvent(frame("message.error", { message_id: "m", message: "boom", code: "provider_error", retryable: true }));
    expect(retryable.kind === "message.error" && retryable.code).toBe("provider_error");
    expect(retryable.kind === "message.error" && retryable.retryable).toBe(true);

    const terminal = decodeConversationEvent(frame("message.error", { message_id: "m", message: "quota", retryable: false }));
    expect(terminal.kind === "message.error" && terminal.retryable).toBe(false);

    // An absent flag is `null` (unknown), never coerced to a boolean.
    const unknown = decodeConversationEvent(frame("message.error", { message_id: "m", message: "?" }));
    expect(unknown.kind === "message.error" && unknown.retryable).toBeNull();
    expect(unknown.kind === "message.error" && unknown.code).toBeNull();
    // A truthy non-boolean must not be read as "retryable".
    const sloppy = decodeConversationEvent(frame("message.error", { message_id: "m", retryable: "yes" }));
    expect(sloppy.kind === "message.error" && sloppy.retryable).toBeNull();
  });

  it("reads the replace flag and defaults a missing delta body to an empty string", () => {
    const replaced = decodeConversationEvent(frame("message.delta", { message_id: "m", replace: true, content: "全文" }));
    expect(replaced.kind === "message.delta" && replaced.delta).toBe("全文");
    expect(replaced.kind === "message.delta" && replaced.replace).toBe(true);
    const empty = decodeConversationEvent(frame("message.delta", { message_id: "m" }));
    expect(empty.kind === "message.delta" && empty.delta).toBe("");
  });

  it("shapes a JSON-encoded tool body and accepts the field aliases", () => {
    const decoded = decodeConversationEvent(
      frame("message.tool", { content: JSON.stringify({ tool_name: "read_file", input: { path: "a" }, status: "completed" }) }),
    );
    expect(decoded.kind).toBe("message.tool");
    if (decoded.kind !== "message.tool") throw new Error("unreachable");
    expect(decoded.tool).toEqual({ name: "read_file", args: { path: "a" }, output: undefined, status: "completed" });
  });

  it("returns a null tool body when nothing tool-shaped was sent", () => {
    const decoded = decodeConversationEvent(frame("message.tool", { message_id: "m" }));
    expect(decoded.kind === "message.tool" && decoded.tool).toBeNull();
  });

  it("accepts `type` as an alias of `tip_type` in a tips body", () => {
    const decoded = decodeConversationEvent(frame("message.tips", { content: "试试这个", type: "suggestion" }));
    expect(decoded.kind === "message.tips" && decoded.tips).toEqual({ content: "试试这个", tipType: "suggestion" });
  });

  it("reports context usage as null when nothing was measured, and clamps percent", () => {
    const missing = decodeConversationEvent(frame("context.usage", { context_usage: { used_tokens: 0, window_tokens: 0 } }));
    expect(missing.kind === "context.usage" && missing.usage).toBeNull();
    const over = decodeConversationEvent(frame("context.usage", { context_usage: { used_tokens: 300, window_tokens: 100, source: "estimated" } }));
    expect(over.kind === "context.usage" && over.usage?.percent).toBe(100);
    expect(over.kind === "context.usage" && over.usage?.source).toBe("estimated");
  });

  it("maps turn.status to a boolean running flag", () => {
    const running = decodeConversationEvent(frame("turn.status", { status: "running" }));
    expect(running.kind === "turn.status" && running.running).toBe(true);
    const idle = decodeConversationEvent(frame("turn.status", { status: "idle" }));
    expect(idle.kind === "turn.status" && idle.running).toBe(false);
  });

  it("preserves a newer server kind as `unknown` instead of dropping the frame", () => {
    // Bypasses the closed `event_type` union on purpose: a newer server can
    // send a kind this build does not know, and that must stay observable.
    const decoded = decodeConversationEvent(frame("message.mystery", { message_id: "m" }));
    expect(decoded.kind).toBe("unknown");
    expect(decoded.kind === "unknown" && decoded.eventType).toBe("message.mystery");
  });

  /**
   * W9（R14）：`turn_completed` 活动带**本轮** token 用量。它是「按 turn 的费用」的
   * 唯一数字来源——上下文占用（水位）算不出单轮花费，所以这里的 null 语义很重要。
   */
  it("carries the per-turn usage a turn_completed activity reports", () => {
    const decoded = decodeConversationEvent(
      frame("message.activity", { kind: "turn_completed", message_id: "m", usage: { input_tokens: 1_200, output_tokens: 340, total_tokens: 1_540 } }),
    );
    expect(decoded.kind === "message.activity" && decoded.usage).toEqual({
      input_tokens: 1_200,
      output_tokens: 340,
      total_tokens: 1_540,
    });
  });

  it("keeps the usage unknown when the runtime reported nothing usable", () => {
    // No usage key at all (an older server / a turn that reported no tokens).
    const missing = decodeConversationEvent(frame("message.activity", { kind: "turn_completed", message_id: "m" }));
    expect(missing.kind === "message.activity" && missing.usage).toBeNull();
    // All-zero report: "no tokens reported" must not read as "this turn was free".
    const zeros = decodeConversationEvent(frame("message.activity", { kind: "turn_completed", usage: { input_tokens: 0, output_tokens: 0 } }));
    expect(zeros.kind === "message.activity" && zeros.usage).toBeNull();
    // Half a bill is not a bill: a missing side stays unknown instead of being
    // totalled as if the absent direction were zero.
    const partial = decodeConversationEvent(frame("message.activity", { kind: "turn_completed", usage: { input_tokens: 1_200 } }));
    expect(partial.kind === "message.activity" && partial.usage).toBeNull();
    // Derives the total when the server only sent the two sides.
    const derived = decodeConversationEvent(frame("message.activity", { kind: "turn_completed", usage: { input_tokens: 10, output_tokens: 5 } }));
    expect(derived.kind === "message.activity" && derived.usage?.total_tokens).toBe(15);
  });

  it("keeps usage off a non-turn activity (the field is not a general one)", () => {
    const decoded = decodeConversationEvent(frame("message.activity", { kind: "agent_status", usage: { input_tokens: 1, output_tokens: 1 } }));
    // Only `turn_completed` carries the per-turn bill; a stray `usage` on some
    // other activity must not be read as this turn's accounting.
    expect(decoded.kind).toBe("message.activity");
    expect(decoded.kind === "message.activity" && decoded.activityKind).toBe("agent_status");
    expect(decoded.kind === "message.activity" && decoded.usage).toBeNull();
  });
});
