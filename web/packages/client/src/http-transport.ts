/**
 * HTTP transport binding (doc `16` R2 / `05` §2.1.1).
 *
 * ## What this is
 *
 * The App Server exposes one method semantics over two bindings. This class is
 * the **request/response** binding: it maps a protocol method name onto the
 * server's hand-written REST route and returns the same result body the
 * WebSocket binding would.
 *
 * ## What this is not
 *
 * - **No server push.** HTTP has no notification channel, so `onNotification`
 *   hands back a no-op unsubscribe and `notify` throws. Live conversation / run
 *   events and subscription methods (`conversation/subscribe`, `run/subscribe`)
 *   exist **only** over `WebSocketTransport`; an `HttpTransport` is not a
 *   drop-in replacement for it. Consumers that need events must connect over
 *   WS (or run both).
 * - **One-shot handshake per call.** `05` §2.1.1: *each* HTTP call performs its
 *   own `POST /initialize` → `POST /initialized` → business call, after which
 *   the connection id expires. `connect()` therefore has nothing to hold open
 *   and is a no-op; it does not warm anything up.
 * - **Host file services stay out.** `/api/fs/*` (browse / list / read /
 *   metadata) is a host file service on the server *root*, explicitly not a
 *   protocol method (`05` §2.1.1), so it is not reachable through this class.
 *
 * ## The route table
 *
 * Every entry was read out of the handler bodies in
 * `crates/backend/nomifun-app-server/src/lib.rs` — the `source` field records
 * which handler (and, where one exists, which shared `*_impl`) proves the two
 * bindings run the same operation. Methods with no entry throw
 * `TransportError("send", …)` instead of silently degrading.
 */

import { AppServerError, TransportError } from "@flowy-agent-store/protocol";
import type { Transport, NotificationListener } from "./transport";

/** Header the server mints the per-call connection id under. */
const CONNECTION_HEADER = "x-app-server-connection-id";

/** Protocol version this binding announces on the one-shot handshake. */
const PROTOCOL_VERSION = "2026-08-26";

type Verb = "GET" | "POST" | "DELETE";

interface HttpRoute {
  verb: Verb;
  /** `:name` segments are filled from the same-named param and dropped from the body/query. */
  path: string;
  /** Params carried in the query string (GET, and the query-configured POSTs). */
  query?: readonly string[];
  /** Params carried in the JSON body; every other param goes here for POST. */
  body?: readonly string[];
  /** Where this mapping was verified in the server source. */
  source: string;
}

/**
 * Protocol method → REST route. Verified from handler bodies, not from the
 * route list (a route existing does not prove which method it mirrors).
 */
const HTTP_ROUTES: Record<string, HttpRoute> = {
  // -- connection -----------------------------------------------------------
  ping: { verb: "POST", path: "/ping", source: "ping() -> registry.require_ready" },

  // -- workspace ------------------------------------------------------------
  // NOTE: `POST /workspaces` (`workspace_register`) is deliberately absent: it
  // takes no body, allocates the id server-side and returns `{id}`, while the
  // WS `workspace/create` takes `{path}`, canonicalises it and returns a full
  // `WorkspaceView`. Same noun, different operation — see `16` R2 deviation.
  "workspace/list": {
    verb: "GET",
    path: "/workspaces",
    source: "workspace_list() -> owner-scoped active rows",
  },
  "workspace/revoke": {
    verb: "DELETE",
    path: "/workspaces/:workspace_id",
    source: "workspace_delete() -> workspace_revoke_impl (shared with the WS arm)",
  },

  // -- conversations --------------------------------------------------------
  "conversation/create": {
    verb: "POST",
    path: "/conversations",
    source: "conversation_create() -> create_conversation_for_user (shared)",
  },
  "conversation/list": {
    verb: "GET",
    path: "/conversations",
    query: ["limit"],
    source: "conversation_list() -> list_app_server_chats (shared); Query<limit>",
  },
  "conversation/get": {
    verb: "GET",
    path: "/conversations/:conversation_id",
    source: "conversation_get() -> project_conversation_view (shared)",
  },
  "conversation/delete": {
    verb: "DELETE",
    path: "/conversations/:conversation_id",
    source: "conversation_delete() -> conversation/delete arm semantics",
  },
  "conversation/messages": {
    verb: "GET",
    path: "/conversations/:conversation_id/messages",
    query: ["page", "page_size", "cursor"],
    source: "conversation_messages() -> list_conversation_messages_for_user (shared)",
  },
  "conversation/send": {
    verb: "POST",
    path: "/conversations/:conversation_id/messages",
    body: ["content", "idempotency_key"],
    source: "conversation_send() -> send_conversation_message_for_user (shared with the WS arm)",
  },
  "conversation/cancel": {
    verb: "POST",
    path: "/conversations/:conversation_id/cancel",
    source: "conversation_cancel() -> cancel arm semantics",
  },

  // -- runs -----------------------------------------------------------------
  "agent/run": {
    verb: "POST",
    path: "/agent/run",
    source: "agent_run() -> same request/receipt as the `agent/run` arm",
  },
  "run/get": { verb: "GET", path: "/run/:run_id", source: "run_get()" },
  "run/result": { verb: "GET", path: "/run/:run_id/result", source: "run_result()" },
  "run/plan": {
    verb: "GET",
    path: "/run/:run_id/plan",
    source: "run_plan() -> get_run_plan_for_user() (same as the WS arm)",
  },
  "run/events": {
    verb: "GET",
    path: "/run/:run_id/events",
    query: ["after_sequence", "limit"],
    source: "run_events() -> runtime.list_events(after_sequence, limit) (same as the WS arm)",
  },
  "run/cancel": {
    verb: "POST",
    path: "/run/:run_id/cancel",
    body: ["expected_version", "reason", "idempotency_key"],
    source: "run_cancel() -> same CancelRunRequest as the WS arm",
  },
  "run/steer": {
    verb: "POST",
    path: "/run/:run_id/steer",
    body: ["text", "expected_version", "command_id", "idempotency_key"],
    source: "run_steer() -> execute_steer_run (shared with the WS arm)",
  },
  "run/answer-decision": {
    verb: "POST",
    path: "/run/:run_id/answer-decision",
    body: [
      "step_id",
      "attempt_id",
      "answer",
      "expected_execution_version",
      "expected_step_version",
      "expected_attempt_version",
    ],
    source:
      "run_answer_decision() -> execute_answer_decision (shared with the WS arm; engine answer gate, no always_allow)",
  },

  // -- catalogs -------------------------------------------------------------
  "skill/list": { verb: "GET", path: "/skills", source: "list_skills_route() -> list_skills_impl" },
  "skill/get": { verb: "GET", path: "/skills/:skill_id", source: "get_skill_route() -> get_skill_impl" },
  "connector/list": {
    verb: "GET",
    path: "/connectors",
    source: "list_connectors_route() -> list_connectors_impl",
  },
  "models/list": { verb: "GET", path: "/models", source: "list_models_route() -> list_models_impl" },
  "connector/get": {
    verb: "GET",
    path: "/connectors/:connector_id",
    source: "get_connector_route() -> get_connector_impl",
  },
  "connector/status": {
    verb: "GET",
    path: "/connectors/:connector_id/status",
    source: "connector_status_route() -> connector_status_impl",
  },
  "connector/test": {
    verb: "POST",
    path: "/connectors/:connector_id/test",
    source: "connector_test_route() -> connector_test_impl",
  },
  "connector/auth/start": {
    verb: "POST",
    path: "/connectors/:connector_id/auth-start",
    source: "connector_auth_start_route() -> connector_auth_start_impl (WS name uses slashes)",
  },
  "connector/auth/status": {
    verb: "GET",
    path: "/connectors/:connector_id/auth-status",
    source: "connector_auth_status_route() -> connector_auth_status_impl",
  },
  "connector/auth/logout": {
    verb: "POST",
    path: "/connectors/:connector_id/auth-logout",
    source: "connector_auth_logout_route() -> connector_auth_logout_impl",
  },

  // -- importer / installer -------------------------------------------------
  "import/run": {
    verb: "POST",
    path: "/imports",
    source: "run_import_route() -> run_import_impl",
  },
  "import/list": { verb: "GET", path: "/imports", source: "list_imports_route() -> list_imports_impl" },
  "import/get": {
    verb: "GET",
    path: "/imports/:snapshot_id",
    source: "get_import_route() -> get_import_impl",
  },
  "install/run": {
    verb: "POST",
    path: "/installs",
    source: "run_install_route() -> install_impl",
  },
  "install/status": {
    verb: "GET",
    path: "/installs/:snapshot_id",
    source: "install_status_route() -> install_status_impl",
  },
  "install/disable": {
    verb: "POST",
    path: "/installs/:snapshot_id/disable",
    body: ["component_ids"],
    source: "install_disable_route() -> install_disable_impl; body `component_ids`",
  },
  "install/enable": {
    verb: "POST",
    path: "/installs/:snapshot_id/enable",
    body: ["component_ids"],
    source: "install_enable_route() -> install_enable_impl; body `component_ids`",
  },
  "install/uninstall": {
    verb: "POST",
    path: "/installs/:snapshot_id/uninstall",
    body: ["component_ids"],
    source: "install_uninstall_route() -> install_uninstall_impl; body `component_ids`",
  },

  // -- marketplaces ---------------------------------------------------------
  "market/add": { verb: "POST", path: "/markets", source: "market_add_route() -> market_add_impl" },
  "market/list": { verb: "GET", path: "/markets", source: "market_list_route() -> market_list_impl" },
  "market/get": {
    verb: "GET",
    path: "/markets/:marketplace_id",
    source: "market_get_route() -> market_get_impl",
  },
  "market/remove": {
    verb: "POST",
    path: "/markets/:marketplace_id/remove",
    body: ["cascade"],
    source: "market_remove_route() -> market_remove_impl; body `cascade` defaults to true",
  },
  "market/auto-update": {
    verb: "POST",
    path: "/markets/:marketplace_id/auto-update",
    body: ["enabled"],
    source: "market_auto_update_route() -> market_auto_update_impl; body `enabled` defaults to true",
  },
  "market/refresh": {
    verb: "POST",
    path: "/markets/:marketplace_id/refresh",
    source: "market_refresh_route() -> market_refresh_impl",
  },
  "market/entry-import": {
    verb: "POST",
    path: "/markets/:marketplace_id/entries/:entry_name/import",
    source: "market_entry_import_route() -> market_entry_import_impl",
  },

  // -- unified store --------------------------------------------------------
  "store/list": { verb: "GET", path: "/store", source: "store_list_route() -> store_list_impl" },
  "store/install-entry": {
    verb: "POST",
    path: "/store/:marketplace_id/entries/:entry_name/install",
    source: "store_install_entry_route() -> store_install_entry_impl",
  },
};

/** Methods that are WebSocket-only by design — named so the error explains itself. */
const WS_ONLY_METHODS = new Set([
  "initialize",
  "initialized",
  "conversation/subscribe",
  "conversation/unsubscribe",
  "run/subscribe",
  "run/unsubscribe",
]);

export interface HttpTransportOptions {
  /**
   * App Server base URL. Either the full `…/api/app-server` root or a
   * `ws(s)://…/api/app-server/ws` endpoint (the `/ws` segment is stripped).
   */
  baseUrl: string;
  /** Bearer token for the handshake (optional; hosts may inject auth instead). */
  token?: string;
  /** Injected `fetch` (defaults to the global); tests pass a stub. */
  fetch?: typeof fetch;
  /** Per-call budget covering handshake + business call (default 30s). */
  requestTimeoutMs?: number;
  /** Client name/version announced on `initialize`. */
  client?: { name: string; version: string };
  /** Client capabilities block echoed on `initialize` (hosts pass their own). */
  capabilities?: unknown;
}

export class HttpTransport implements Transport {
  private readonly routes: Record<string, HttpRoute>;
  private readonly options: HttpTransportOptions;
  private closed = false;

  constructor(options: HttpTransportOptions, routes: Record<string, HttpRoute> = HTTP_ROUTES) {
    this.options = options;
    this.routes = routes;
  }

  /**
   * No-op: the HTTP binding handshakes per call (`05` §2.1.1), so there is no
   * connection to open. Kept for `Transport` compatibility.
   */
  async connect(): Promise<void> {
    this.closed = false;
  }

  /**
   * Perform one business call over its own handshake.
   *
   * Throws `TransportError("send", …)` for a method with no HTTP binding —
   * never a silent fallback to another binding.
   */
  async request<T>(method: string, params: unknown): Promise<T> {
    if (this.closed) {
      throw new TransportError("connect", "http transport is closed");
    }
    const route = this.routes[method];
    if (!route) {
      const reason = WS_ONLY_METHODS.has(method)
        ? `"${method}" is WebSocket-only (doc 05 §2.1.1); use WebSocketTransport for subscriptions and live events`
        : `"${method}" has no HTTP binding in this SDK (doc 16 R2 route table)`;
      throw new TransportError("send", reason);
    }

    const { path, query, body } = this.splitParams(route, params);
    const connectionId = await this.openConnection();
    const response = await this.fetchWithTimeout(route.verb, path, { query, body, connectionId });
    return (await this.readResult<T>(response, method)) as T;
  }

  /**
   * Not supported: HTTP carries no server-push. Throws instead of pretending.
   */
  notify(method: string, _params: unknown): void {
    throw new TransportError(
      "send",
      `HTTP binding cannot notify ("${method}"): notifications are WebSocket-only (doc 05 §2.1.1)`,
    );
  }

  /**
   * Returns a no-op unsubscribe: this binding receives **no** notifications.
   *
   * Not equivalent to the WebSocket binding — callers that need live events
   * must use `WebSocketTransport` (or run both).
   */
  onNotification(_listener: NotificationListener): () => void {
    return () => undefined;
  }

  /** No-op: every call is independent, so nothing is held open. */
  close(): void {
    this.closed = true;
  }

  // -- internals ------------------------------------------------------------

  /**
   * One-shot handshake: `POST /initialize` (connection id header) →
   * `POST /initialized`. Returns the connection id.
   *
   * Public because host-side services that sit *outside* the protocol surface
   * (`/api/fs/*`, the HTTP-only `POST /workspaces` register route) still need a
   * ready connection id; they are not protocol methods, so they cannot go
   * through `request()`.
   */
  async openConnection(): Promise<string> {
    const init = await this.fetchWithTimeout("POST", "/initialize", {
      body: {
        protocol_version: PROTOCOL_VERSION,
        client: this.options.client ?? { name: "flowy-agent-store-client", version: "0.1.0" },
        ...(this.options.capabilities === undefined ? {} : { capabilities: this.options.capabilities }),
      },
    });
    const connectionId = init.headers.get(CONNECTION_HEADER);
    if (!connectionId) {
      throw new TransportError(
        "connect",
        `handshake response is missing the ${CONNECTION_HEADER} header`,
        { retryable: true },
      );
    }
    await this.fetchWithTimeout("POST", "/initialized", { connectionId });
    return connectionId;
  }

  private splitParams(
    route: HttpRoute,
    params: unknown,
  ): { path: string; query: Record<string, string>; body: Record<string, unknown> | undefined } {
    const source = isRecord(params) ? params : {};
    const pathParams = [...route.path.matchAll(/:([A-Za-z0-9_]+)/g)].map((match) => match[1]);
    const path = route.path.replace(/:([A-Za-z0-9_]+)/g, (_full, name: string) => {
      const value = source[name];
      if (value === undefined || value === null) {
        throw new TransportError("send", `missing required path param "${name}" for ${route.verb} ${route.path}`);
      }
      return encodeURIComponent(String(value));
    });

    const query: Record<string, string> = {};
    for (const key of route.query ?? []) {
      const value = source[key];
      if (value !== undefined && value !== null) query[key] = String(value);
    }

    if (route.verb === "GET" || route.verb === "DELETE") {
      return { path, query, body: undefined };
    }

    const carried = route.body ?? [];
    const body: Record<string, unknown> = {};
    for (const key of Object.keys(source)) {
      if (pathParams.includes(key) || (route.query ?? []).includes(key)) continue;
      if (carried.length > 0 && !carried.includes(key)) continue;
      body[key] = source[key];
    }
    return { path, query, body };
  }

  private async fetchWithTimeout(
    verb: Verb,
    path: string,
    request: { query?: Record<string, string>; body?: unknown; connectionId?: string },
  ): Promise<Response> {
    const url = new URL(`${this.baseUrl()}${path}`);
    for (const [key, value] of Object.entries(request.query ?? {})) {
      url.searchParams.set(key, value);
    }
    const headers: Record<string, string> = { accept: "application/json" };
    if (request.body !== undefined) headers["content-type"] = "application/json";
    if (request.connectionId) headers[CONNECTION_HEADER] = request.connectionId;
    if (this.options.token) headers.authorization = `Bearer ${this.options.token}`;

    const controller = new AbortController();
    const timeoutMs = this.options.requestTimeoutMs ?? 30_000;
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const fetchImpl = this.options.fetch ?? globalThis.fetch;
      if (!fetchImpl) throw new TransportError("connect", "no fetch implementation available");
      return await fetchImpl(url.toString(), {
        method: verb,
        headers,
        body: request.body === undefined ? undefined : JSON.stringify(request.body),
        signal: controller.signal,
      });
    } catch (error) {
      if (error instanceof TransportError) throw error;
      const aborted = (error as { name?: string } | null)?.name === "AbortError";
      throw new TransportError(
        "send",
        aborted ? `${verb} ${path} timed out after ${timeoutMs}ms` : `${verb} ${path} failed: ${String(error)}`,
        { retryable: true, cause: error },
      );
    } finally {
      clearTimeout(timer);
    }
  }

  /** 2xx → result body; otherwise the server's wire error becomes an `AppServerError`. */
  private async readResult<T>(response: Response, method: string): Promise<T | unknown> {
    let payload: unknown = undefined;
    const text = await response.text();
    if (text.length > 0) {
      try {
        payload = JSON.parse(text) as unknown;
      } catch {
        if (response.ok) {
          throw new TransportError("receive", `${method}: response is not JSON`, { retryable: true });
        }
      }
    }
    if (response.ok) return payload as T;

    throw appServerErrorFromWire(payload, response.status, method);
  }

  private baseUrl(): string {
    return this.options.baseUrl.replace(/\/+$/, "").replace(/\/ws$/, "");
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Turn an HTTP failure body into the SDK error model.
 *
 * Shared by this transport and by host-side helpers (webui `lib/client.ts`) so
 * a second spelling of the wire-error mapping cannot drift: the App Server
 * emits `{code, message, retryable, details, request_id}` (`into_wire_error`),
 * which is exactly the `AppServerError` shape. Anything else becomes a
 * retryable-on-5xx `TransportError`.
 */
export function appServerErrorFromWire(payload: unknown, status: number, context = "request"): unknown {
  if (isRecord(payload) && typeof payload.code === "string" && typeof payload.message === "string") {
    return new AppServerError({
      code: payload.code,
      message: payload.message,
      retryable: payload.retryable === true,
      request_id: (payload.request_id as string | null) ?? null,
      details: isRecord(payload.details) ? payload.details : {},
    });
  }
  return new TransportError("receive", `${context}: HTTP ${status} without a wire error body`, {
    retryable: status >= 500,
  });
}

/** Exposed for tests and docs (R5): the verified method → route table. */
export function httpRouteTable(): Readonly<Record<string, { verb: string; path: string; source: string }>> {
  return HTTP_ROUTES;
}
