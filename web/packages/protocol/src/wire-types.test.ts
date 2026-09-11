import { describe, expect, it } from "vitest";

import type { ConversationEvent, ConversationEventType, ServerNotification } from "./protocol";

/**
 * Wire-type guards (doc `16` R3).
 *
 * The `@ts-expect-error` lines below are the real assertions: they only compile
 * while the type stays as narrow as intended, and `bun run typecheck` fails
 * with "Unused '@ts-expect-error' directive" if someone widens it again. R1
 * removed the old `| string` escape from `event_type` for exactly this reason.
 */
describe("wire type guards", () => {
  it("keeps ConversationEventType a closed union", () => {
    const known: ConversationEventType[] = [
      "message.created",
      "message.delta",
      "message.thinking",
      "message.tips",
      "message.tool",
      "message.error",
      "message.activity",
      "turn.status",
      "context.usage",
    ];
    expect(new Set(known).size).toBe(known.length);

    // @ts-expect-error — an unknown kind must not be assignable to the union.
    const bogus: ConversationEventType = "message.mystery";
    expect(bogus).toBe("message.mystery");
  });

  it("keeps the payload an untyped JSON bag until a decoder reads it", () => {
    const event: ConversationEvent = {
      conversation_id: "conv-1",
      sequence: 3,
      event_type: "context.usage",
      payload: { context_usage: { used_tokens: 1, window_tokens: 2 } },
    };
    // `payload` is deliberately `Record<string, unknown>`: the typed bodies
    // arrive with the decoder layer (R1 remaining), not from the wire type.
    const bag: unknown = event.payload.context_usage;
    expect(bag).toEqual({ used_tokens: 1, window_tokens: 2 });
  });

  it("discriminates the notification union by method", () => {
    const notification: ServerNotification = {
      jsonrpc: "2.0",
      method: "conversation/resync-required",
      params: { conversation_ids: ["conv-1"], reason: "lagged" },
    };
    if (notification.method !== "conversation/resync-required") throw new Error("unreachable");
    expect(notification.params.reason).toBe("lagged");
    expect(notification.params.conversation_ids).toEqual(["conv-1"]);
  });
});
