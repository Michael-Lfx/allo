import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ConversationSendReceipt } from "@flowy-agent-store/protocol";

/**
 * Store-level wiring of W7 重试 / 编辑 / 重新生成（R12）.
 *
 * The decision table (which action is allowed, which idempotency key it must use)
 * is covered by `lib/turn-actions.test.ts`; this file pins the store's half:
 * which HTTP-shaped call goes out, what lands in the transcript, and what the
 * user is told when an action is refused **without a request being sent**.
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });
vi.stubGlobal("requestAnimationFrame", (run: () => void) => {
  run();
  return 0;
});

const { useAppStore } = await import("./appStore");
const { initialConversationStream } = await import("../lib/conversation-events");

interface SendCall {
  conversationId: string;
  content: string;
  key: string;
}

function receipt(overrides: Partial<ConversationSendReceipt> = {}): ConversationSendReceipt {
  return {
    conversation_id: "conv-1",
    message_id: "u-sent",
    accepted: true,
    replayed: false,
    completed: false,
    ...overrides,
  } as ConversationSendReceipt;
}

function fakeClient(options: { failFirstSend?: boolean } = {}) {
  const sends: SendCall[] = [];
  let shouldFail = options.failFirstSend === true;
  return {
    sends,
    client: {
      conversations: {
        send: async (conversationId: string, content: string, key: string) => {
          sends.push({ conversationId, content, key });
          if (shouldFail) {
            shouldFail = false;
            throw Object.assign(new Error("socket closed"), { code: "transport_error" });
          }
          return receipt({ message_id: `u-${sends.length}` });
        },
      },
    },
  };
}

function message(id: string, role: string, content: unknown, extra: Record<string, unknown> = {}) {
  return {
    message_id: id,
    conversation_id: "conv-1",
    role,
    content,
    message_type: extra.message_type ?? "text",
    status: extra.status ?? "sent",
    created_at: 1,
  };
}

const FAILED_TRANSCRIPT = [
  message("u1", "user", "read the repo"),
  message("a1", "assistant", "done"),
  message("u2", "user", "rewrite the config"),
  message("e1", "assistant", { content: "provider refused", retryable: true }, { message_type: "error", status: "error" }),
];

function seed(messages: unknown[]) {
  useAppStore.setState({
    selectedConversationId: "conv-1",
    stream: { ...initialConversationStream, messages: messages as never },
    toasts: [],
    turnActionBusy: null,
    turnActionError: null,
  });
}

function seedClient(options: { failFirstSend?: boolean } = {}) {
  const fake = fakeClient(options);
  useAppStore.setState({ client: fake.client as never });
  return fake;
}

beforeEach(() => {
  useAppStore.setState({
    toasts: [],
    turnActionBusy: null,
    turnActionError: null,
    selectedConversationId: null,
    client: null,
  });
});

describe("runTurnAction · retry", () => {
  it("sends a fresh turn for a retryable failure and toasts the result", async () => {
    seed(FAILED_TRANSCRIPT);
    const fake = seedClient();

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: "e1" });

    expect(fake.sends).toEqual([
      { conversationId: "conv-1", content: "rewrite the config", key: expect.stringMatching(/^retry-u2-/) },
    ]);
    expect(useAppStore.getState().toasts.map((toast) => toast.messageKey)).toEqual(["message.actionRetryQueued"]);
    expect(useAppStore.getState().turnActionError).toBeNull();
    // The new turn appears in the transcript and the failed row is still there.
    const ids = useAppStore.getState().stream.messages.map((entry) => entry.message_id);
    expect(ids).toContain("u-1");
    expect(ids).toContain("e1");
  });

  it("refuses a not-retryable failure without touching the wire", async () => {
    seed([
      message("u2", "user", "rewrite the config"),
      message("e1", "assistant", { content: "quota", retryable: false }, { message_type: "error", status: "error" }),
    ]);
    const fake = seedClient();

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: "e1" });

    expect(fake.sends).toEqual([]);
    expect(useAppStore.getState().turnActionError).toBe("message.actionNotRetryable");
  });
});

describe("runTurnAction · resend after a lost receipt", () => {
  it("reuses the original idempotency key so the server cannot execute twice", async () => {
    seed([]);
    const fake = seedClient({ failFirstSend: true });
    useAppStore.setState({ draft: "hello", selectedConversationId: "conv-1" });

    await useAppStore.getState().send();
    const firstKey = fake.sends[0].key;
    // The failed send left the optimistic row behind: it is the resend target.
    const pending = useAppStore.getState().stream.messages.find((entry) => entry.status === "failed");
    expect(pending).toBeTruthy();

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: pending!.message_id });

    expect(fake.sends).toHaveLength(2);
    expect(fake.sends[1].key).toBe(firstKey);
    expect(fake.sends[1].content).toBe("hello");
    expect(useAppStore.getState().toasts.map((toast) => toast.messageKey)).toContain("message.actionResendQueued");
  });

  it("refuses to resend with a fresh key when the original key is gone", async () => {
    seed([message("pending:chat-gone", "user", "hello", { status: "failed" })]);
    const fake = seedClient();

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: "pending:chat-gone" });

    expect(fake.sends).toEqual([]);
    expect(useAppStore.getState().turnActionError).toBe("message.actionMissingKey");
  });
});

describe("runTurnAction · regenerate and edit", () => {
  it("regenerates from the last user turn", async () => {
    seed(FAILED_TRANSCRIPT);
    const fake = seedClient();

    await useAppStore.getState().runTurnAction({ kind: "regenerate" });

    expect(fake.sends).toEqual([
      { conversationId: "conv-1", content: "rewrite the config", key: expect.stringMatching(/^regen-u2-/) },
    ]);
    expect(useAppStore.getState().toasts.map((toast) => toast.messageKey)).toEqual(["message.actionRegenerateQueued"]);
  });

  it("sends the edited text as a new turn and never rewrites the original", async () => {
    seed(FAILED_TRANSCRIPT);
    const fake = seedClient();

    await useAppStore.getState().runTurnAction({ kind: "edit", messageId: "u2", text: "rewrite only the logging config" });

    expect(fake.sends).toEqual([
      {
        conversationId: "conv-1",
        content: "rewrite only the logging config",
        key: expect.stringMatching(/^edit-u2-/),
      },
    ]);
    const original = useAppStore.getState().stream.messages.find((entry) => entry.message_id === "u2");
    expect(original?.content).toBe("rewrite the config");
  });

  it("ignores a second action while one is in flight", async () => {
    seed(FAILED_TRANSCRIPT);
    const fake = seedClient();
    useAppStore.setState({ turnActionBusy: "e1" });

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: "e1" });

    expect(fake.sends).toEqual([]);
  });

  it("surfaces a transport failure instead of a silent no-op", async () => {
    seed(FAILED_TRANSCRIPT);
    useAppStore.setState({
      client: {
        conversations: {
          send: async () => {
            throw Object.assign(new Error("socket closed"), { code: "transport_error" });
          },
        },
      } as never,
    });

    await useAppStore.getState().runTurnAction({ kind: "retry-entry", messageId: "e1" });

    expect(useAppStore.getState().turnActionError).toBeTruthy();
    expect(useAppStore.getState().turnActionBusy).toBeNull();
  });
});
