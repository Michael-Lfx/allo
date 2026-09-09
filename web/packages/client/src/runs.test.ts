/** EventSubscription: dedupe, cursor, auto/manual resync (no server needed). */
import { describe, expect, it, vi } from "vitest";
import type { RunEvent, ServerNotification } from "@flowy-agent-store/protocol";
import { RunClient } from "./runs";
import type { NotificationListener, Transport } from "./transport";

function runEvent(run_id: string, sequence: number, event_type = "run.status"): RunEvent {
  return { run_id, sequence, event_type, payload: {} };
}

function liveEvent(run_id: string, sequence: number): ServerNotification {
  return { jsonrpc: "2.0", method: "event", params: runEvent(run_id, sequence) } as ServerNotification;
}

function resyncNotice(run_id: string): ServerNotification {
  return {
    jsonrpc: "2.0",
    method: "run/resync-required",
    params: { run_ids: [run_id], reason: "event_stream_lagged" },
  } as unknown as ServerNotification;
}

class FakeTransport implements Transport {
  requests: { method: string; params: unknown }[] = [];
  eventsByCursor: RunEvent[] = [];
  failEvents = false;
  private listeners = new Set<NotificationListener>();

  async connect(): Promise<void> {}
  async request<T>(method: string, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method === "run/subscribe" || method === "run/unsubscribe") {
      return { subscribed: true } as unknown as T;
    }
    if (method === "run/events") {
      if (this.failEvents) {
        throw new Error("events unavailable");
      }
      const after = (params as { after_sequence?: number }).after_sequence ?? 0;
      return this.eventsByCursor.filter((e) => e.sequence > after) as unknown as T;
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
  close(): void {}
  emit(notification: ServerNotification): void {
    for (const listener of [...this.listeners]) {
      listener(notification);
    }
  }
}

function methods(transport: FakeTransport): string[] {
  return transport.requests.map((request) => request.method);
}

describe("EventSubscription", () => {
  it("catches up silently on follow, then streams live events", async () => {
    const transport = new FakeTransport();
    transport.eventsByCursor = [runEvent("run-1", 1), runEvent("run-1", 2)];
    const sub = await new RunClient(transport).follow("run-1");
    // History establishes the cursor but never dispatches.
    expect(methods(transport)).toEqual(["run/subscribe", "run/events"]);
    expect(sub.lastSequence).toBe(2);

    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));
    transport.emit(liveEvent("run-1", 3));
    transport.emit(liveEvent("run-1", 3)); // duplicate dropped
    transport.emit(liveEvent("run-2", 9)); // foreign run ignored
    expect(seen).toEqual([3]);
    expect(sub.lastSequence).toBe(3);
    await sub.close();
  });

  it("delivers out-of-order live events once each, cursor tracks max", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));
    transport.emit(liveEvent("run-1", 5));
    transport.emit(liveEvent("run-1", 4));
    transport.emit(liveEvent("run-1", 5));
    expect(seen).toEqual([5, 4]);
    expect(sub.lastSequence).toBe(5);
    await sub.close();
  });

  it("auto-fills the hole on run/resync-required, sorted and deduped", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    const notices: string[] = [];
    sub.onEvent((event) => seen.push(event.sequence));
    sub.onResync((params) => notices.push(params.reason));
    transport.emit(liveEvent("run-1", 1));
    // Server persisted 2..4 while the socket was dark (unsorted page).
    transport.eventsByCursor = [runEvent("run-1", 2), runEvent("run-1", 4), runEvent("run-1", 3)];
    transport.emit(resyncNotice("run-1"));
    await vi.waitFor(() => expect(seen).toEqual([1, 2, 3, 4]));
    expect(notices).toEqual(["event_stream_lagged"]);
    expect(sub.lastSequence).toBe(4);
    const fetches = transport.requests.filter((r) => r.method === "run/events");
    expect(fetches[fetches.length - 1]?.params).toMatchObject({ after_sequence: 1 });
    await sub.close();
  });

  it("manual resync returns missed events without redelivery", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));
    transport.emit(liveEvent("run-1", 1));
    transport.eventsByCursor = [runEvent("run-1", 1), runEvent("run-1", 2)];
    const missed = await sub.resync();
    expect(missed.map((event) => event.sequence)).toEqual([2]);
    expect(seen).toEqual([1, 2]);
    await sub.close();
  });

  it("shares one flight for concurrent resync calls", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1");
    transport.eventsByCursor = [runEvent("run-1", 1)];
    const [first, second] = await Promise.all([sub.resync(), sub.resync()]);
    expect(first.map((event) => event.sequence)).toEqual([1]);
    expect(second.map((event) => event.sequence)).toEqual([1]);
    const fetches = transport.requests.filter((r) => r.method === "run/events");
    // One for catch-up on follow, one shared for both resync calls.
    expect(fetches).toHaveLength(2);
    await sub.close();
  });

  it("does not fetch when autoResync is off", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1", { autoResync: false });
    const notices: string[] = [];
    sub.onResync((params) => notices.push(params.reason));
    transport.emit(resyncNotice("run-1"));
    await vi.waitFor(() => expect(notices).toEqual(["event_stream_lagged"]));
    expect(methods(transport)).toEqual(["run/subscribe", "run/events"]);
    await sub.close();
  });

  it("routes resync failures to error listeners", async () => {
    const transport = new FakeTransport();
    transport.failEvents = true;
    const errors: unknown[] = [];
    // Catch-up on follow fails silently; the subscription still works.
    const sub = await new RunClient(transport).follow("run-1");
    sub.onError((error) => errors.push(error));
    transport.emit(resyncNotice("run-1"));
    await vi.waitFor(() => expect(errors).toHaveLength(1));
    await sub.close();
  });

  it("close unsubscribes and drops later notifications", async () => {
    const transport = new FakeTransport();
    const sub = await new RunClient(transport).follow("run-1");
    const seen: number[] = [];
    sub.onEvent((event) => seen.push(event.sequence));
    await sub.close();
    transport.emit(liveEvent("run-1", 1));
    expect(seen).toEqual([]);
    expect(methods(transport)).toEqual(["run/subscribe", "run/events", "run/unsubscribe"]);
  });
});