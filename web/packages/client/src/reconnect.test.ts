import { describe, expect, it } from "vitest";
import type { RunEvent, ServerNotification } from "@flowy-agent-store/protocol";
import { ConversationClient } from "./conversations";
import { RunClient } from "./runs";
import type { NotificationListener, Transport, TransportLifecycle } from "./transport";

/**
 * T8 (`16` §5.2): after a reconnect a subscription must re-register itself,
 * re-subscribe on the server and replay what it missed instead of silently
 * going dark. Two transport behaviours are covered because a lost socket and a
 * caller `close()` differ in whether notification listeners survive.
 */

function runEvent(runId: string, sequence: number): RunEvent {
  return { run_id: runId, sequence, event_type: "run.status", payload: {} };
}

function liveRunEvent(runId: string, sequence: number): ServerNotification {
  return { jsonrpc: "2.0", method: "event", params: runEvent(runId, sequence) } as ServerNotification;
}

function conversationEvent(conversationId: string, sequence: number): ServerNotification {
  return {
    jsonrpc: "2.0",
    method: "conversation/event",
    params: { conversation_id: conversationId, sequence, event_type: "message.delta", payload: {} },
  } as unknown as ServerNotification;
}

class ReconnectingTransport implements Transport {
  requests: { method: string; params: unknown }[] = [];
  runEvents: RunEvent[] = [];
  private listeners = new Set<NotificationListener>();
  private lifecycleListeners = new Set<(state: TransportLifecycle) => void>();

  async connect(): Promise<void> {}

  async request<T>(method: string, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (
      method === "run/subscribe" ||
      method === "run/unsubscribe" ||
      method === "conversation/subscribe" ||
      method === "conversation/unsubscribe"
    ) {
      return { subscribed: true } as unknown as T;
    }
    if (method === "run/events") {
      const after = (params as { after_sequence?: number }).after_sequence ?? 0;
      return this.runEvents.filter((event) => event.sequence > after) as unknown as T;
    }
    throw new Error(`unexpected method ${method}`);
  }

  notify(): void {}

  onNotification(listener: NotificationListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  onLifecycle(listener: (state: TransportLifecycle) => void): () => void {
    this.lifecycleListeners.add(listener);
    return () => {
      this.lifecycleListeners.delete(listener);
    };
  }

  /** Caller-initiated teardown: mirrors `WebSocketTransport.close()` (silent). */
  close(): void {
    this.listeners.clear();
  }

  /** Lost socket: server-side subscriptions gone, local listeners survive. */
  drop(): void {
    for (const listener of [...this.lifecycleListeners]) listener("closed");
  }

  /** `close()` path: the transport also drops its notification listeners. */
  dropHard(): void {
    this.close();
    this.drop();
  }

  reopen(): void {
    for (const listener of [...this.lifecycleListeners]) listener("open");
  }

  emit(notification: ServerNotification): void {
    for (const listener of [...this.listeners]) listener(notification);
  }

  calls(method: string): number {
    return this.requests.filter((request) => request.method === method).length;
  }
}

describe("run subscription rearm", () => {
  it("re-subscribes and replays the outage window after a lost socket", async () => {
    const transport = new ReconnectingTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));

    transport.emit(liveRunEvent("run-1", 1));
    expect(sub.lastSequence).toBe(1);

    transport.drop();
    // Persisted while the socket was dark.
    transport.runEvents = [runEvent("run-1", 1), runEvent("run-1", 2), runEvent("run-1", 3)];
    transport.reopen();

    const replayed = await sub.rearm();
    expect(replayed.map((event) => event.sequence)).toEqual([1, 2, 3]);
    // The reset cursor replays history on purpose; consumers dedupe by sequence.
    expect(seen).toEqual([1, 1, 2, 3]);
    expect(sub.lastSequence).toBe(3);
    expect(transport.calls("run/subscribe")).toBe(2);

    // Live delivery resumes, and exactly once (no duplicated listener).
    transport.emit(liveRunEvent("run-1", 4));
    expect(seen).toEqual([1, 1, 2, 3, 4]);
    await sub.close();
  });

  it("re-registers its listener when the transport dropped them (close path)", async () => {
    const transport = new ReconnectingTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));

    transport.dropHard();
    transport.runEvents = [runEvent("run-1", 7)];
    transport.reopen();
    await sub.rearm();

    transport.emit(liveRunEvent("run-1", 8));
    expect(seen).toEqual([7, 8]);
    await sub.close();
  });

  it("is a no-op once closed", async () => {
    const transport = new ReconnectingTransport();
    const sub = await new RunClient(transport).follow("run-1");
    await sub.close();

    expect(await sub.rearm()).toEqual([]);
    expect(transport.calls("run/subscribe")).toBe(1);
  });
});

describe("conversation subscription rearm", () => {
  it("re-subscribes, resets the cursor and keeps streaming", async () => {
    const transport = new ReconnectingTransport();
    const sub = await new ConversationClient(transport).follow("conv-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));

    transport.emit(conversationEvent("conv-1", 3));
    expect(sub.lastSequence).toBe(3);

    transport.dropHard();
    transport.reopen();
    await sub.rearm();

    expect(sub.lastSequence).toBe(0);
    expect(transport.calls("conversation/subscribe")).toBe(2);

    transport.emit(conversationEvent("conv-1", 4));
    expect(seen).toEqual([3, 4]);
    expect(sub.lastSequence).toBe(4);
    await sub.close();
  });
});
