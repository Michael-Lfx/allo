import { describe, expect, it } from "vitest";

import {
  contentDigest,
  failedTurns,
  lastUserTurn,
  messageTextOf,
  resolveTurnAction,
  unreceiptedUserTurns,
  TURN_ACTION_REFUSAL_KEYS,
  TURN_ACTION_TOAST_KEYS,
  type TurnMessageLike,
} from "./turn-actions";

function user(id: string, text: string, status?: string): TurnMessageLike {
  return { message_id: id, role: "user", message_type: "text", status: status ?? "sent", content: text };
}

function assistant(id: string, text: string): TurnMessageLike {
  return { message_id: id, role: "assistant", message_type: "text", status: "sent", content: text };
}

function errorRow(
  id: string,
  text: string,
  extra: { retryable?: boolean | null; code?: string | null } = {},
): TurnMessageLike {
  return {
    message_id: id,
    role: "assistant",
    message_type: "error",
    status: "error",
    content: { content: text, retryable: extra.retryable ?? null, code: extra.code ?? null },
  };
}

const FAILED_EXCHANGE: TurnMessageLike[] = [
  user("u1", "read the repo"),
  assistant("a1", "done"),
  user("u2", "now rewrite the config"),
  errorRow("e1", "provider refused the request", { retryable: true, code: "provider_error" }),
];

describe("turn projection", () => {
  it("pairs an error row with the user turn that produced it", () => {
    const failures = failedTurns(FAILED_EXCHANGE);
    expect(failures).toEqual([
      {
        errorMessageId: "e1",
        userMessageId: "u2",
        content: "now rewrite the config",
        retryable: true,
        code: "provider_error",
      },
    ]);
  });

  it("reports an unknown retryable flag instead of guessing it", () => {
    const failures = failedTurns([user("u1", "hi"), errorRow("e1", "boom")]);
    expect(failures[0].retryable).toBeNull();
    // A string on the wire is not a boolean: it stays unknown.
    const coerced = failedTurns([
      user("u1", "hi"),
      { ...errorRow("e1", "boom"), content: { content: "boom", retryable: "true" } },
    ]);
    expect(coerced[0].retryable).toBeNull();
  });

  it("finds the last user turn and the unreceipted ones", () => {
    expect(lastUserTurn(FAILED_EXCHANGE)).toEqual({ messageId: "u2", content: "now rewrite the config" });
    expect(unreceiptedUserTurns([...FAILED_EXCHANGE, user("u3", "hello again", "sending")])).toEqual([
      { messageId: "u3", content: "hello again" },
    ]);
    expect(lastUserTurn([])).toBeNull();
  });

  it("reads text from both plain and wrapped content", () => {
    expect(messageTextOf(user("u1", "plain"))).toBe("plain");
    expect(messageTextOf(errorRow("e1", "wrapped"))).toBe("wrapped");
    expect(messageTextOf(undefined)).toBe("");
  });
});

describe("resolveTurnAction · retry entry", () => {
  it("retries an error row with a NEW key (the old receipt would just replay)", () => {
    const resolved = resolveTurnAction(FAILED_EXCHANGE, { kind: "retry-entry", messageId: "e1" });
    expect(resolved.ok).toBe(true);
    if (!resolved.ok) return;
    expect(resolved.plan.kind).toBe("retry");
    expect(resolved.plan.content).toBe("now rewrite the config");
    expect(resolved.plan.idempotency.mode).toBe("fresh");
    expect(resolved.plan.userMessageId).toBe("u2");
    expect(resolved.plan.sourceMessageId).toBe("e1");
  });

  it("is deterministic: the same failure yields the same fresh key (double click safe)", () => {
    const first = resolveTurnAction(FAILED_EXCHANGE, { kind: "retry-entry", messageId: "e1" });
    const second = resolveTurnAction(FAILED_EXCHANGE, { kind: "retry-entry", messageId: "e1" });
    expect(first.ok && second.ok && first.plan.idempotency).toEqual(second.ok ? second.plan.idempotency : null);
  });

  it("refuses a turn the wire marked as not retryable", () => {
    const messages = [user("u1", "hi"), errorRow("e1", "quota exhausted", { retryable: false })];
    expect(resolveTurnAction(messages, { kind: "retry-entry", messageId: "e1" })).toEqual({
      ok: false,
      reason: "not-retryable",
    });
  });

  it("resends an unreceipted user turn with the ORIGINAL key", () => {
    const messages = [user("u1", "hi", "sending")];
    const resolved = resolveTurnAction(messages, { kind: "retry-entry", messageId: "u1" }, { rememberedKey: "chat-abc" });
    expect(resolved.ok).toBe(true);
    if (!resolved.ok) return;
    expect(resolved.plan.kind).toBe("resend");
    expect(resolved.plan.idempotency).toEqual({ mode: "reuse", key: "chat-abc" });
  });

  it("refuses to resend when the original key is gone (a fresh key could duplicate execution)", () => {
    const messages = [user("u1", "hi", "failed")];
    expect(resolveTurnAction(messages, { kind: "retry-entry", messageId: "u1" })).toEqual({
      ok: false,
      reason: "missing-key",
    });
    expect(resolveTurnAction(messages, { kind: "retry-entry", messageId: "u1" }, { rememberedKey: "  " })).toEqual({
      ok: false,
      reason: "missing-key",
    });
  });

  it("does not offer resend for a delivered user message", () => {
    expect(resolveTurnAction([user("u1", "hi")], { kind: "retry-entry", messageId: "u1" })).toEqual({
      ok: false,
      reason: "not-found",
    });
  });

  it("refuses unknown rows and error rows with no user turn", () => {
    expect(resolveTurnAction(FAILED_EXCHANGE, { kind: "retry-entry", messageId: "nope" })).toEqual({
      ok: false,
      reason: "not-found",
    });
    expect(resolveTurnAction([errorRow("e1", "boom")], { kind: "retry-entry", messageId: "e1" })).toEqual({
      ok: false,
      reason: "not-found",
    });
  });
});

describe("resolveTurnAction · regenerate and edit", () => {
  it("regenerates from the last user turn with a fresh key", () => {
    const resolved = resolveTurnAction(FAILED_EXCHANGE, { kind: "regenerate" });
    expect(resolved.ok).toBe(true);
    if (!resolved.ok) return;
    expect(resolved.plan).toMatchObject({ kind: "regenerate", content: "now rewrite the config", userMessageId: "u2" });
    expect(resolved.plan.idempotency.mode).toBe("fresh");
  });

  it("refuses regenerate without a user turn", () => {
    expect(resolveTurnAction([assistant("a1", "hi")], { kind: "regenerate" })).toEqual({ ok: false, reason: "not-found" });
  });

  it("edits a user turn: new content, new key, original untouched", () => {
    const resolved = resolveTurnAction(FAILED_EXCHANGE, { kind: "edit", messageId: "u2", text: "  rewrite only the logging config  " });
    expect(resolved.ok).toBe(true);
    if (!resolved.ok) return;
    expect(resolved.plan).toMatchObject({
      kind: "edit",
      content: "rewrite only the logging config",
      sourceMessageId: "u2",
      userMessageId: "u2",
    });
    // The edited text changes the key, so it cannot collide with the original attempt.
    const original = resolveTurnAction(FAILED_EXCHANGE, { kind: "edit", messageId: "u2", text: "now rewrite the config" });
    expect(resolved.plan.idempotency).not.toEqual(original.ok ? original.plan.idempotency : null);
    // Nothing in the message list was rewritten by resolving the plan.
    expect(FAILED_EXCHANGE[2].content).toBe("now rewrite the config");
  });

  it("refuses editing an assistant row or empty text", () => {
    expect(resolveTurnAction(FAILED_EXCHANGE, { kind: "edit", messageId: "a1", text: "x" })).toEqual({
      ok: false,
      reason: "not-found",
    });
    expect(resolveTurnAction(FAILED_EXCHANGE, { kind: "edit", messageId: "u2", text: "   " })).toEqual({
      ok: false,
      reason: "empty-content",
    });
  });

  it("digests content stably", () => {
    expect(contentDigest("abc")).toBe(contentDigest("abc"));
    expect(contentDigest("abc")).not.toBe(contentDigest("abd"));
    expect(contentDigest("abc")).toMatch(/^[0-9a-f]{8}$/);
  });

  it("covers every kind and refusal with a copy key", () => {
    expect(Object.keys(TURN_ACTION_TOAST_KEYS).sort()).toEqual(["edit", "regenerate", "resend", "retry"]);
    for (const key of Object.values(TURN_ACTION_TOAST_KEYS)) expect(key).toMatch(/^message\./);
    for (const key of Object.values(TURN_ACTION_REFUSAL_KEYS)) expect(key).toMatch(/^message\./);
  });
});
