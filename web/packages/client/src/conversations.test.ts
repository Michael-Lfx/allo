import { describe, expect, it } from "vitest";

import type { NotificationListener, Transport } from "./transport";
import { ConversationClient } from "./conversations";
import type { ConversationMessagesPage, ServerNotification } from "@flowy-agent-store/protocol";

const CONV = "conv-1";

class FakeTransport implements Transport {
  sent: string[] = [];
  page: ConversationMessagesPage = { items: [], has_more: false };
  fetchError: unknown = null;
  fetchCount = 0;
  private listeners = new Set<NotificationListener>();

  async connect(): Promise<void> {}
  async request<T>(method: string, _params: unknown): Promise<T> {
    this.sent.push(method);
    if (method === "conversation/subscribe" || method === "conversation/unsubscribe") {
      return { subscribed: true } as unknown as T;
    }
    if (method === "conversation/messages") {
      this.fetchCount += 1;
      if (this.fetchError) throw this.fetchError;
      return this.page as unknown as T;
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

function event(sequence: number, content = `d${sequence}`): ServerNotification {
  return {
    jsonrpc: "2.0",
    method: "conversation/event",
    params: {
      conversation_id: CONV,
      sequence,
      event_type: "message.delta",
      payload: { message_id: "m1", content },
    },
  } as ServerNotification;
}

function resyncRequired(reason: string): ServerNotification {
  return {
    jsonrpc: "2.0",
    method: "conversation/resync-required",
    params: { conversation_ids: [CONV], reason },
  } as ServerNotification;
}

/** Flush microtasks so the fire-and-forget catch-up promise settles. */
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe("ConversationSubscription (R1 — aligned with the Run subscription)", () => {
  it("buffers events that arrive before the first listener and flushes them in order", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV);

    // `follow()` acknowledged; the caller has not attached a listener yet.
    transport.emit(event(1, "a"));
    transport.emit(event(2, "b"));

    const seen: number[] = [];
    subscription.onEvent((item) => seen.push(item.sequence));
    expect(seen).toEqual([1, 2]);
    expect(subscription.droppedPendingCount).toBe(0);

    // Live delivery takes over once a listener exists.
    transport.emit(event(3, "c"));
    expect(seen).toEqual([1, 2, 3]);
    await subscription.close();
  });

  it("drops the oldest buffered events past the pending limit and counts the loss", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV, { pendingLimit: 2 });
    transport.emit(event(1));
    transport.emit(event(2));
    transport.emit(event(3));

    const seen: number[] = [];
    subscription.onEvent((item) => seen.push(item.sequence));
    expect(seen).toEqual([2, 3]);
    expect(subscription.droppedPendingCount).toBe(1);
    await subscription.close();
  });

  it("discards duplicates and out-of-order frames at or below the cursor", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV);
    const seen: number[] = [];
    subscription.onEvent((item) => seen.push(item.sequence));

    transport.emit(event(1));
    transport.emit(event(1));
    transport.emit(event(2));
    expect(seen).toEqual([1, 2]);
    expect(subscription.lastSequence).toBe(2);
    await subscription.close();
  });

  it("detects a sequence gap and backfills the transcript exactly once", async () => {
    const transport = new FakeTransport();
    transport.page = {
      items: [
        { message_id: "m1", conversation_id: CONV, role: "assistant", content: "a", message_type: "text", created_at: 1 },
      ],
      has_more: true,
    };
    const subscription = await new ConversationClient(transport).follow(CONV);

    const reasons: string[] = [];
    const backfills: Array<{ reason: string; count: number; hasMore: boolean }> = [];
    subscription.onResync((reason) => reasons.push(reason));
    subscription.onBackfill((backfill) => backfills.push({ reason: backfill.reason, count: backfill.messages.length, hasMore: backfill.hasMore }));
    const seen: number[] = [];
    subscription.onEvent((item) => seen.push(item.sequence));

    transport.emit(event(1));
    transport.emit(event(4)); // 2 and 3 never arrived → gap
    await settle();

    expect(seen).toEqual([1, 4]);
    expect(reasons).toEqual(["gap"]);
    expect(backfills).toEqual([{ reason: "gap", count: 1, hasMore: true }]);
    expect(transport.fetchCount).toBe(1);

    // A second gap while nothing else changed is coalesced while a fetch is in
    // flight, and the cursor keeps advancing either way.
    transport.emit(event(9));
    await settle();
    expect(transport.fetchCount).toBe(2);
    expect(subscription.lastSequence).toBe(9);
    await subscription.close();
  });

  it("backfills on conversation/resync-required with the server reason", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV);
    const reasons: string[] = [];
    subscription.onResync((reason) => reasons.push(reason));
    const backfilled: string[] = [];
    subscription.onBackfill((backfill) => backfilled.push(backfill.reason));

    transport.emit(resyncRequired("lagged"));
    await settle();

    expect(reasons).toEqual(["lagged"]);
    expect(backfilled).toEqual(["lagged"]);
    await subscription.close();
  });

  it("surfaces catch-up failures through onError instead of swallowing them", async () => {
    const transport = new FakeTransport();
    transport.fetchError = new Error("transcript unavailable");
    const subscription = await new ConversationClient(transport).follow(CONV);
    const errors: unknown[] = [];
    subscription.onError((error) => errors.push(error));

    transport.emit(resyncRequired("lagged"));
    await settle();

    expect(errors).toHaveLength(1);
    expect((errors[0] as Error).message).toBe("transcript unavailable");
    await subscription.close();
  });

  it("keeps only the resync notification when autoResync is off", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV, { autoResync: false });
    const reasons: string[] = [];
    subscription.onResync((reason) => reasons.push(reason));
    let backfilled = 0;
    subscription.onBackfill(() => { backfilled += 1; });

    transport.emit(resyncRequired("lagged"));
    await settle();

    expect(reasons).toEqual(["lagged"]);
    expect(transport.fetchCount).toBe(0);
    expect(backfilled).toBe(0);
    await subscription.close();
  });

  it("reports a failed rearm and stops delivering after close", async () => {
    const transport = new FakeTransport();
    const subscription = await new ConversationClient(transport).follow(CONV);
    const errors: unknown[] = [];
    subscription.onError((error) => errors.push(error));
    const seen: number[] = [];
    subscription.onEvent((item) => seen.push(item.sequence));

    const original = transport.request.bind(transport);
    transport.request = async (method: string, params: unknown) => {
      if (method === "conversation/subscribe") throw new Error("socket closed");
      return original(method, params);
    };
    await expect(subscription.rearm()).rejects.toThrow("socket closed");
    expect((errors[0] as Error).message).toBe("socket closed");

    transport.request = original;
    await subscription.rearm();
    expect(subscription.lastSequence).toBe(0);

    await subscription.close();
    transport.emit(event(1));
    expect(seen).toEqual([]);
  });
});
