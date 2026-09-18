/**
 * JSON-RPC 2.0 transport over WebSocket for the App Server protocol.
 *
 * Works in browsers and in Node 22+/Bun (global `WebSocket`). The browser
 * WebSocket API cannot set custom headers, so an optional bearer token is
 * appended as a query parameter (`?token=...`) when provided; HTTP calls in
 * the SDK use the `Authorization` header instead.
 */

import { AppServerError, RequestTimeoutError, TransportError } from "@flowy-agent-store/protocol";
import type {
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
  ServerNotification,
} from "@flowy-agent-store/protocol";

export type NotificationListener = (notification: ServerNotification) => void;

/**
 * True for loopback-only URLs (`127.0.0.0/8`, `::1`, `localhost`).
 * SDK spawn contract (docs/agent-store/12-sdk-packaging.md P0-4): an SDK
 * must only speak the local-trusted mode over loopback; anything else is
 * rejected before connecting.
 */
export function isLoopbackUrl(raw: string): boolean {
  try {
    const host = new URL(raw).hostname.toLowerCase();
    if (host === "localhost" || host === "::1") return true;
    if (host.startsWith("[") && host.endsWith("]")) return host === "[::1]";
    const parts = host.split(".").map(Number);
    return parts.length === 4 && parts[0] === 127 && parts.every((n) => Number.isInteger(n) && n >= 0 && n <= 255);
  } catch {
    return false;
  }
}

/**
 * Transport abstraction (docs/agent-store/07-typescript-sdk.md §2.3).
 * A transport only frames JSON-RPC over some channel; the
 * initialize → initialized → ready lifecycle lives in `AppServerClient`,
 * so business code never knows whether it runs over WebSocket (browser),
 * stdio (Node/CLI, future) or a one-shot HTTP binding.
 */
/**
 * Lifecycle of the underlying channel. `open` is emitted on every successful
 * dial (the first one included), `closed` only when an established connection
 * is lost — so `closed` → `open` is exactly a reconnect.
 */
export type TransportLifecycle = "open" | "closed";

export type TransportLifecycleListener = (state: TransportLifecycle) => void;

export interface Transport {
  connect(): Promise<void>;
  request<T>(method: string, params: unknown): Promise<T>;
  notify(method: string, params: unknown): void;
  onNotification(listener: NotificationListener): () => void;
  close(): void;
  /**
   * Optional: observe the channel lifecycle so a host can show a disconnect
   * banner and re-arm subscriptions on reconnect (docs/agent-store/16 T8/W8).
   * Custom transports may omit it.
   */
  onLifecycle?(listener: TransportLifecycleListener): () => void;
}

/** How long an unopened `connect()` waits before it fails. */
const DEFAULT_CONNECT_TIMEOUT_MS = 10_000;

export interface TransportOptions {
  requestTimeoutMs?: number;
  /** Max time `connect()` may stay unopened before failing (default 10s). */
  connectTimeoutMs?: number;
  token?: string;
}

export class WebSocketTransport implements Transport {
  private socket: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<
    number,
    {
      resolve: (value: unknown) => void;
      reject: (reason: unknown) => void;
      timer: ReturnType<typeof setTimeout>;
      method: string;
    }
  >();
  private listeners = new Set<NotificationListener>();
  private lifecycleListeners = new Set<TransportLifecycleListener>();
  private lifecycle: TransportLifecycle | null = null;
  private closedByCaller = false;
  /** Single in-flight connect shared by concurrent callers; null once settled. */
  private connecting: {
    promise: Promise<void>;
    resolve: () => void;
    reject: (error: unknown) => void;
    timer: ReturnType<typeof setTimeout>;
  } | null = null;

  readonly requestTimeoutMs: number;
  readonly connectTimeoutMs: number;
  readonly url: string;
  readonly token?: string;

  constructor(url: string, options: TransportOptions = {}) {
    this.url = url;
    this.token = options.token;
    this.requestTimeoutMs = options.requestTimeoutMs ?? 30_000;
    this.connectTimeoutMs = options.connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS;
  }

  get connected(): boolean {
    return this.socket?.readyState === WebSocket.OPEN;
  }

  connect(): Promise<void> {
    if (this.connected) {
      return Promise.resolve();
    }
    // Concurrent callers share one socket: a second `new WebSocket` here would
    // orphan the first one and let both race over `this.socket`.
    if (this.connecting) {
      return this.connecting.promise;
    }
    this.closedByCaller = false;

    const target = this.token ? `${this.url}${this.url.includes("?") ? "&" : "?"}token=${encodeURIComponent(this.token)}` : this.url;
    const socket = new WebSocket(target);
    this.socket = socket;

    let resolveConnect!: () => void;
    let rejectConnect!: (error: unknown) => void;
    const promise = new Promise<void>((resolve, reject) => {
      resolveConnect = resolve;
      rejectConnect = reject;
    });
    const timer = setTimeout(() => {
      // Still unopened past the deadline: detach first so this socket's later
      // `close` event can never touch the next connection, then fail the caller.
      this.detachSocket(socket);
      this.rejectConnect(
        new TransportError("connect", `app-server connect timed out after ${this.connectTimeoutMs}ms`, {
          retryable: true,
        }),
      );
    }, this.connectTimeoutMs);
    this.connecting = { promise, resolve: resolveConnect, reject: rejectConnect, timer };

    socket.onopen = () => {
      if (this.socket !== socket) return; // superseded: ignore the stale socket
      this.resolveConnect();
      this.setLifecycle("open");
    };
    socket.onmessage = (event: MessageEvent) => {
      if (this.socket !== socket) return; // superseded: ignore the stale socket
      this.handleMessage(event.data);
    };
    socket.onclose = (event: CloseEvent) => {
      // A superseded socket must never tear down the live one or reject the
      // connect promise that now belongs to its replacement.
      if (this.socket !== socket) return;
      this.failAllPending(
        new TransportError("close", `app-server connection closed (code ${event.code})`, {
          retryable: !this.closedByCaller,
        }),
      );
      this.socket = null;
      this.setLifecycle("closed");
      this.rejectConnect(
        new TransportError("connect", "app-server connection closed before opening"),
      );
    };
    socket.onerror = () => {
      // The close event carries the terminal state; nothing else to do here.
      if (this.socket === socket) socket.close();
    };

    return promise;
  }

  request<T>(method: string, params: unknown): Promise<T> {
    const socket = this.requireSocket();
    const id = this.nextId++;
    const payload: JsonRpcRequest = { jsonrpc: "2.0", id, method, params };

    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new RequestTimeoutError(method, this.requestTimeoutMs));
      }, this.requestTimeoutMs);
      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer,
        method,
      });
      try {
        socket.send(JSON.stringify(payload));
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(new TransportError("send", `failed to send ${method}`, { cause: error }));
      }
    });
  }

  notify(method: string, params: unknown): void {
    const socket = this.requireSocket();
    const payload: JsonRpcNotification = { jsonrpc: "2.0", method, params };
    try {
      socket.send(JSON.stringify(payload));
    } catch (error) {
      throw new TransportError("send", `failed to send notification ${method}`, {
        cause: error,
      });
    }
  }

  onNotification(listener: NotificationListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /**
   * Observe the channel lifecycle. `closed` is only reported when an
   * established connection is lost (a failed first dial is not a disconnect),
   * and `open` follows it on the next successful dial — that pair is what a
   * host should treat as a reconnect. Unlike `onNotification`, these listeners
   * survive `close()`, since they exist to drive the reconnection itself.
   */
  onLifecycle(listener: TransportLifecycleListener): () => void {
    this.lifecycleListeners.add(listener);
    return () => {
      this.lifecycleListeners.delete(listener);
    };
  }

  close(): void {
    this.closedByCaller = true;
    // Detach before closing: the socket's own `close` event then hits the stale
    // guard instead of rejecting the state of a later connection.
    this.detachSocket(this.socket);
    this.rejectConnect(new TransportError("close", "app-server transport closed by caller"));
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(
        new TransportError("close", "app-server transport closed by caller"),
      );
    }
    this.pending.clear();
    // Registrations do not survive a close: a reconnected transport must not
    // deliver into listeners bound to the previous connection.
    this.listeners.clear();
    // Caller-initiated: silent (no `closed` for a deliberate teardown), but the
    // next dial must be reported as `open` again.
    this.lifecycle = null;
  }

  private setLifecycle(state: TransportLifecycle): void {
    // A dial that never opened is not a lost connection.
    if (state === "closed" && this.lifecycle !== "open") return;
    if (this.lifecycle === state) return;
    this.lifecycle = state;
    for (const listener of [...this.lifecycleListeners]) {
      try {
        listener(state);
      } catch {
        // listener isolation
      }
    }
  }

  /** Detach the current socket (if any) and close it; its later events are ignored. */
  private detachSocket(socket: WebSocket | null): void {
    if (!socket) return;
    if (this.socket === socket) this.socket = null;
    try {
      socket.close();
    } catch {
      // already closing / closed
    }
  }

  /** Settle the in-flight `connect()` successfully, when there is one. */
  private resolveConnect(): void {
    const connecting = this.connecting;
    if (!connecting) return;
    this.connecting = null;
    clearTimeout(connecting.timer);
    connecting.resolve();
  }

  /** Fail the in-flight `connect()`, when there is one. */
  private rejectConnect(error: unknown): void {
    const connecting = this.connecting;
    if (!connecting) return;
    this.connecting = null;
    clearTimeout(connecting.timer);
    connecting.reject(error);
  }

  private requireSocket(): WebSocket {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new TransportError("send", "app-server transport is not connected", {
        retryable: true,
      });
    }
    return this.socket;
  }

  private handleMessage(raw: unknown): void {
    let message: JsonRpcResponse | ServerNotification;
    try {
      message = JSON.parse(String(raw)) as JsonRpcResponse | ServerNotification;
    } catch {
      return; // ignore non-JSON frames
    }

    if ("id" in message && (message as JsonRpcResponse).id !== undefined) {
      const response = message as JsonRpcResponse;
      const pending = response.id === null ? undefined : this.pending.get(Number(response.id));
      if (!pending) {
        return;
      }
      clearTimeout(pending.timer);
      this.pending.delete(Number(response.id));
      if (response.error) {
        pending.reject(new AppServerError(response.error));
      } else {
        pending.resolve(response.result);
      }
      return;
    }

    if ("method" in message) {
      const notification = message as ServerNotification;
      for (const listener of [...this.listeners]) {
        try {
          listener(notification);
        } catch {
          // A listener must never break delivery to other listeners.
        }
      }
      return;
    }

    // Unknown frame shapes (binary frames, malformed envelopes) are ignored
    // so a single bad message can never tear the connection down.
  }

  private failAllPending(error: TransportError): void {
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
}