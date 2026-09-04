/**
 * JSON-RPC 2.0 transport over WebSocket for the App Server protocol.
 *
 * Works in browsers and in Node 22+/Bun (global `WebSocket`). The browser
 * WebSocket API cannot set custom headers, so an optional bearer token is
 * appended as a query parameter (`?token=...`) when provided; HTTP calls in
 * the SDK use the `Authorization` header instead.
 */

import { AppServerError, RequestTimeoutError, TransportError } from "@agent-store/protocol";
import type {
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
  ServerNotification,
} from "@agent-store/protocol";

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
export interface Transport {
  connect(): Promise<void>;
  request<T>(method: string, params: unknown): Promise<T>;
  notify(method: string, params: unknown): void;
  onNotification(listener: NotificationListener): () => void;
  close(): void;
}

export interface TransportOptions {
  requestTimeoutMs?: number;
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
  private connectResolvers: Array<() => void> = [];
  private connectRejecters: Array<(reason: unknown) => void> = [];
  private closedByCaller = false;

  readonly requestTimeoutMs: number;
  readonly url: string;
  readonly token?: string;

  constructor(url: string, options: TransportOptions = {}) {
    this.url = url;
    this.token = options.token;
    this.requestTimeoutMs = options.requestTimeoutMs ?? 30_000;
  }

  get connected(): boolean {
    return this.socket?.readyState === WebSocket.OPEN;
  }

  connect(): Promise<void> {
    if (this.connected) {
      return Promise.resolve();
    }
    this.closedByCaller = false;

    const target = this.token ? `${this.url}${this.url.includes("?") ? "&" : "?"}token=${encodeURIComponent(this.token)}` : this.url;
    const socket = new WebSocket(target);
    this.socket = socket;

    socket.onopen = () => {
      for (const resolve of this.connectResolvers.splice(0)) {
        resolve();
      }
    };
    socket.onmessage = (event: MessageEvent) => {
      this.handleMessage(event.data);
    };
    socket.onclose = (event: CloseEvent) => {
      this.failAllPending(
        new TransportError("close", `app-server connection closed (code ${event.code})`, {
          retryable: !this.closedByCaller,
        }),
      );
      this.socket = null;
      for (const reject of this.connectRejecters.splice(0)) {
        reject(
          new TransportError("connect", "app-server connection closed before opening"),
        );
      }
    };
    socket.onerror = () => {
      // The close event carries the terminal state; nothing else to do here.
      this.socket?.close();
    };

    return new Promise<void>((resolve, reject) => {
      this.connectResolvers.push(resolve);
      this.connectRejecters.push(reject);
    });
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

  close(): void {
    this.closedByCaller = true;
    this.socket?.close();
    this.socket = null;
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(
        new TransportError("close", "app-server transport closed by caller"),
      );
    }
    this.pending.clear();
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