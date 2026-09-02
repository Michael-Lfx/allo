import { describe, expect, it } from "vitest";
import {
  conversationStreamReducer,
  initialConversationStream,
  mergeMessagesById,
  type ConversationStreamState,
} from "./conversation-events";
import type { ConversationMessage } from "./protocol";

function msg(id: string, created_at: number, role: ConversationMessage["role"] = "user"): ConversationMessage {
  return { message_id: id, conversation_id: "c", role, content: id, message_type: "text", created_at };
}

describe("mergeMessagesById", () => {
  it("dedups by message_id and sorts ascending by created_at", () => {
    const base = [msg("b", 2), msg("a", 1)];
    const merged = mergeMessagesById(base, [msg("a", 1, "assistant")]);
    expect(merged.map((m) => m.message_id)).toEqual(["a", "b"]);
    // Latest fields win on collision.
    expect(merged[0].role).toBe("assistant");
  });
});

describe("conversationStreamReducer", () => {
  it("prependHistory inserts an earlier page before the existing messages, in order", () => {
    let state = { ...initialConversationStream, messages: [5, 6, 7, 8].map((n) => msg(`m${n}`, n)) };
    state = conversationStreamReducer(state, {
      type: "prependHistory",
      messages: [1, 2, 3, 4].map((n) => msg(`m${n}`, n)),
    });
    expect(state.messages.map((m) => m.message_id)).toEqual(["m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8"]);
  });

  it("never duplicates an id a live event already inserted (history vs realtime consistency)", () => {
    let state = { ...initialConversationStream, messages: [msg("m5", 5), msg("m6", 6)] };
    // Live event streams in an older message before its history page arrives.
    state = conversationStreamReducer(state, {
      type: "event",
      event: { conversation_id: "c", sequence: 1, event_type: "message.created", payload: { message_id: "m3", created_at: 3, content: "x" } },
    });
    // Historical load returns the same m3 plus its neighbours; merge must dedupe.
    state = conversationStreamReducer(state, {
      type: "prependHistory",
      messages: [msg("m1", 1), msg("m2", 2), msg("m3", 3), msg("m4", 4)],
    });
    const ids = state.messages.map((m) => m.message_id);
    expect(ids).toEqual(["m1", "m2", "m3", "m4", "m5", "m6"]);
    expect(ids.filter((id) => id === "m3").length).toBe(1);
  });

  it("reset clears pagination state (no cross-conversation bleed)", () => {
    let state: ConversationStreamState = {
      ...initialConversationStream,
      historyCursor: "x",
      hasMore: true,
      loadingOlder: true,
      messages: [msg("m1", 1)],
    };
    state = conversationStreamReducer(state, { type: "reset", messages: [] });
    expect(state.historyCursor).toBeNull();
    expect(state.hasMore).toBe(false);
    expect(state.loadingOlder).toBe(false);
    expect(state.messages).toEqual([]);
  });
});
