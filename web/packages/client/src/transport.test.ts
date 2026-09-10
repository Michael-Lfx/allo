import { TransportError } from "@flowy-agent-store/protocol";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebSocketTransport } from "./transport";

/**
 * Transport robustness (A2 / T7, `16` §5.2). The global `WebSocket` is stubbed
 * so each connection state (connecting / open / dropped) is driven explicitly,
 * including the states a real socket cannot be told to enter on demand.
 *
 * Note the fake keeps the real API's asynchrony: `close()` moves the state but
 * the `close` **event** is delivered separately (`drop()` / `emitClose()`), so
 * tests can reproduce a stale socket reporting in after a newer one is live.
 */

type Handler<T> = ((event: T) => void) | null;

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  readyState = FakeWebSocket.CONNECTING;
  readonly sent: string[] = [];
  onopen: Handler<unknown> = null;
  onmessage: Handler<{ data: string }> = null;
  onclose: Handler<{ code: number }> = null;
  onerror: Handler<unknown> = null;

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }

  open(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.(undefined);
  }

  deliver(payload: unknown): void {
    this.onmessage?.({ data: JSON.stringify(payload) });
  }

  /** State-only close: the `close` event is not delivered here (see `drop`). */
  close(): void {
    if (this.readyState === FakeWebSocket.CLOSED) return;
    this.readyState = FakeWebSocket.CLOSED;
  }

  /** Deliver the `close` event (a real socket emits it exactly once). */
  emitClose(code: number): void {
    this.onclose?.({ code });
  }

  /** Unexpected drop: state plus the `close` event. */
  drop(code: number): void {
    this.close();
    this.emitClose(code);
  }

  send(data: string): void {
    if (this.readyState !== FakeWebSocket.OPEN) throw new Error("socket is not open");
    this.sent.push(data);
  }
}

const RealWebSocket = (globalThis as { WebSocket?: unknown }).WebSocket;
const URL = "ws://127.0.0.1:8787/api/app-server/ws";

function notification(method: string, params: unknown): unknown {
  return { jsonrpc: "2.0", method, params };
}

function lastSocket(): FakeWebSocket {
  const socket = FakeWebSocket.instances.at(-1);
  if (!socket) throw new Error("no fake socket was created");
  return socket;
}

/** Open the newest socket and await the connect promise it belongs to. */
async function connect(transport: WebSocketTransport): Promise<FakeWebSocket> {
  const pending = transport.connect();
  const socket = lastSocket();
  socket.open();
  await pending;
  return socket;
}

beforeEach(() => {
  FakeWebSocket.instances = [];
  (globalThis as { WebSocket?: unknown }).WebSocket = FakeWebSocket;
});

afterEach(() => {
  (globalThis as { WebSocket?: unknown }).WebSocket = RealWebSocket;
  vi.useRealTimers();
});

describe("WebSocketTransport lifecycle", () => {
  it("reuses one socket for concurrent connect() calls", async () => {
    const transport = new WebSocketTransport(URL);

    const first = transport.connect();
    const second = transport.connect();

    expect(FakeWebSocket.instances).toHaveLength(1);
    lastSocket().open();
    await Promise.all([first, second]);
    expect(transport.connected).toBe(true);
  });

  it("times out a connect() that never opens and tears the socket down", async () => {
    vi.useFakeTimers();
    const transport = new WebSocketTransport(URL, { connectTimeoutMs: 500 });

    const outcome = transport.connect().then(
      () => "resolved",
      (error: unknown) => error,
    );
    await vi.advanceTimersByTimeAsync(600);

    const error = await outcome;
    expect(error).toBeInstanceOf(TransportError);
    expect((error as TransportError).phase).toBe("connect");
    expect((error as TransportError).message).toMatch(/timed out after 500ms/);
    expect(lastSocket().readyState).toBe(FakeWebSocket.CLOSED);
  });

  it("settles an in-flight connect() from close() even without a close event", async () => {
    const transport = new WebSocketTransport(URL);
    const pending = transport.connect();
    lastSocket().close();

    transport.close();

    const outcome = await Promise.race([
      pending.then(
        () => "resolved",
        () => "rejected",
      ),
      new Promise((resolve) => setTimeout(() => resolve("still pending"), 100)),
    ]);
    expect(outcome).toBe("rejected");
  });

  it("drops notification listeners on close()", async () => {
    const transport = new WebSocketTransport(URL);
    const seen: unknown[] = [];
    transport.onNotification((item) => seen.push(item));
    await connect(transport);

    transport.close();

    const socket = await connect(transport);
    socket.deliver(notification("event", { run_id: "r1", sequence: 1 }));
    expect(seen).toHaveLength(0);
  });

  it("ignores a late close event from a socket replaced by a newer connection", async () => {
    const transport = new WebSocketTransport(URL);
    const stale = await connect(transport);

    transport.close();
    await connect(transport);

    stale.emitClose(1006);

    expect(transport.connected).toBe(true);
    expect(transport.request("store/list", {})).toBeInstanceOf(Promise);
  });

  it("rejects pending requests when the socket drops unexpectedly", async () => {
    const transport = new WebSocketTransport(URL);
    const socket = await connect(transport);

    const pending = transport.request("store/list", {});
    socket.drop(1006);

    await expect(pending).rejects.toBeInstanceOf(TransportError);
  });
});

describe("WebSocketTransport lifecycle signal", () => {
  it("reports closed then open across a reconnect", async () => {
    const transport = new WebSocketTransport(URL);
    const states: string[] = [];
    transport.onLifecycle((state) => states.push(state));

    await connect(transport);
    expect(states).toEqual(["open"]);

    lastSocket().drop(1006);
    expect(states).toEqual(["open", "closed"]);

    await connect(transport);
    expect(states).toEqual(["open", "closed", "open"]);
  });

  it("does not call a failed first dial a disconnect", async () => {
    vi.useFakeTimers();
    const transport = new WebSocketTransport(URL, { connectTimeoutMs: 500 });
    const states: string[] = [];
    transport.onLifecycle((state) => states.push(state));

    const outcome = transport.connect().catch(() => undefined);
    await vi.advanceTimersByTimeAsync(600);
    await outcome;

    expect(states).toEqual([]);
  });

  it("survives close() and stays silent for a deliberate teardown", async () => {
    const transport = new WebSocketTransport(URL);
    const states: string[] = [];
    transport.onLifecycle((state) => states.push(state));

    await connect(transport);
    transport.close();
    expect(states).toEqual(["open"]);

    // The next dial is a fresh connection, reported as open again.
    await connect(transport);
    expect(states).toEqual(["open", "open"]);
  });
});
