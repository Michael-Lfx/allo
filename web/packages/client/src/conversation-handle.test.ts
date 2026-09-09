/** ConversationHandle (REQ-PAR-05d): open/attach, send-await-turn, cancel, transcript. */
import { describe, expect, it } from "vitest";
import type { ServerNotification } from "@flowy-agent-store/protocol";
import { ConversationClient } from "./conversations";
import { ConversationHandle } from "./conversation-handle";
import type { NotificationListener, Transport } from "./transport";

const CONV = "0190f5fe-conversation-0000-000000000001";

function convEvent(sequence: number, event_type: string, payload: Record<string, unknown>): ServerNotification {
  return {
    jsonrpc: "2.0",
    method: "conversation/event",
    params: { conversation_id: CONV, sequence, event_type, payload },
  } as unknown as ServerNotification;
}

const completedReceipt = {
  conversation_id: CONV,
  message_id: "msg-user-1",
  turn_id: "turn-abc",
  accepted: true,
  replayed: false,
  completed: true,
  result_ok: true,
  result_text: "hello from the assistant",
  result_error: null,
  result_error_code: null,
};

class FakeTransport implements Transport {
  sent: string[] = [];
  receipt: Record<string, unknown> = completedReceipt;
  messages: unknown[] = [];
  private listeners = new Set<NotificationListener>();

  async connect(): Promise<void> {}
  async request<T>(method: string, params: unknown): Promise<T> {
    this.sent.push(method);
    if (method === "conversation/create") {
      return { conversation_id: CONV, name: "new", model: {} as never, status: "idle", created_at: 1, modified_at: 1, is_processing: false } as unknown as T;
    }
    if (method === "conversation/subscribe" || method === "conversation/unsubscribe") {
      return { subscribed: true } as unknown as T;
    }
    if (method === "conversation/send") {
      return this.receipt as T;
    }
    if (method === "conversation/cancel") {
      return { conversation_id: CONV } as unknown as T;
    }
    if (method === "conversation/messages") {
      return { items: this.messages, has_more: false } as unknown as T;
    }
    throw new Error(`unexpected method ${method}`);
  }
  notify(): void {}
  onNotification(listener: NotificationListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  close(): void {}
  emit(notification: ServerNotification): void {
    for (const listener of [...this.listeners]) listener(notification);
  }
}

describe("ConversationHandle", () => {
  it("open creates a conversation and send returns the aggregated completed turn", async () => {
    const transport = new FakeTransport();
    const client = new ConversationClient(transport);
    const handle = await ConversationHandle.open(client, { name: "task" });
    expect(handle.conversationId).toBe(CONV);
    const turn = await handle.send("do it");
    expect(turn.completed).toBe(true);
    expect(turn.message_id).toBe("msg-user-1");
    expect(turn.turn_id).toBe("turn-abc");
    expect(turn.assistant_text).toBe("hello from the assistant");
    expect(turn.isError).toBe(false);
    await handle.close();
  });

  it("send waits for the terminal turn.status event before resolving", async () => {
    const transport = new FakeTransport();
    transport.receipt = { ...completedReceipt, completed: false, result_text: null };
    const client = new ConversationClient(transport);
    const handle = await ConversationHandle.open(client);

    const pending = handle.send("stream it");
    // Emit the turn lifecycle after send is acknowledged.
    transport.emit(convEvent(1, "turn.status", { turn_id: "turn-abc", status: "running" }));
    transport.emit(convEvent(2, "message.delta", { message_id: "msg-asst-1", content: "hel" }));
    transport.emit(convEvent(3, "message.delta", { message_id: "msg-asst-1", content: "lo" }));
    transport.emit(convEvent(4, "context.usage", { context_usage: { used_tokens: 10, window_tokens: 100 } }));
    transport.emit(convEvent(5, "turn.status", { turn_id: "turn-abc", status: "completed" }));

    const turn = await pending;
    expect(turn.completed).toBe(true);
    expect(turn.assistant_text).toBe("hello"); // delta aggregation fallback
    expect(turn.usage).toEqual({ used_tokens: 10, window_tokens: 100 });
    expect(turn.events.map((event) => event.sequence)).toEqual([1, 2, 3, 4, 5]);
    expect(turn.isError).toBe(false);
    await handle.close();
  });

  it("send marks the turn as error on a message.error event", async () => {
    const transport = new FakeTransport();
    transport.receipt = { ...completedReceipt, completed: false, result_error: "boom", result_error_code: "rate_limited" };
    const client = new ConversationClient(transport);
    const handle = await ConversationHandle.open(client);
    const pending = handle.send("risky");
    transport.emit(convEvent(1, "message.error", { message_id: "m", message: "boom", code: "rate_limited" }));
    const turn = await pending;
    expect(turn.isError).toBe(true);
    expect(turn.result_error_code).toBe("rate_limited");
    await handle.close();
  });

  it("attach binds an existing conversation and cancel resolves", async () => {
    const transport = new FakeTransport();
    const client = new ConversationClient(transport);
    const handle = await ConversationHandle.attach(client, CONV);
    expect(handle.conversationId).toBe(CONV);
    await handle.cancel();
    expect(transport.sent).toContain("conversation/cancel");
    await handle.close();
  });

  it("messages queries the transcript for this conversation", async () => {
    const transport = new FakeTransport();
    transport.messages = [{ message_id: "m1", conversation_id: CONV, role: "user", content: "hi", message_type: "text", created_at: 1 }];
    const client = new ConversationClient(transport);
    const handle = await ConversationHandle.open(client);
    const page = await handle.messages();
    expect(page.items).toHaveLength(1);
    await handle.close();
  });
});
