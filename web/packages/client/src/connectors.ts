/**
 * Agent Store Connector (MCP) catalog client over the App Server WebSocket
 * transport: catalog, status, probe and OAuth pass-through.
 *
 * Tokens never cross this module — the OAuth browser flow is owned by the
 * trusted host; clients only start it and poll `authStatus`.
 */

import type { Transport } from "./transport";
import type {
  ConnectorCallResult,
  ConnectorDetail,
  ConnectorProbeResult,
  ConnectorStatusView,
  ConnectorSummary,
  OAuthStartResult,
  OAuthStatusView,
} from "@flowy-agent-store/protocol";

export class ConnectorClient {
  constructor(private readonly transport: Transport) {}

  /** List the Agent Store Connector catalog (public summaries only). */
  list(): Promise<ConnectorSummary[]> {
    return this.transport.request<ConnectorSummary[]>("connector/list", {});
  }

  /** Fetch one Connector detail: namespaced tools + auth state. */
  get(connectorId: string): Promise<ConnectorDetail> {
    return this.transport.request<ConnectorDetail>("connector/get", {
      connector_id: connectorId,
    });
  }

  /** Combined auth × probe status. `connected` only when auth is ready AND the last probe succeeded. */
  status(connectorId: string): Promise<ConnectorStatusView> {
    return this.transport.request<ConnectorStatusView>("connector/status", {
      connector_id: connectorId,
    });
  }

  /**
   * Run a connection probe and report what the connector currently offers —
   * including each tool's `input_schema`.
   *
   * It really connects: a stdio connector gets its child process spawned, an
   * HTTP/SSE one gets `initialize` + `tools/list`. The result is **persisted**,
   * so this is also what refreshes the tool list `get()` reads back.
   *
   * Named `test` and not `probe` on purpose: this repo's split is `test` for the
   * action (wire `connector/test`, `test_connection`) and `probe` for its
   * artifact (`ConnectorProbeResult`, `probe_status`), and every method here is
   * the last segment of its wire method.
   */
  test(connectorId: string): Promise<ConnectorProbeResult> {
    return this.transport.request<ConnectorProbeResult>("connector/test", {
      connector_id: connectorId,
    });
  }

  /** Check the current OAuth state for a connector. */
  authStatus(connectorId: string): Promise<OAuthStatusView> {
    return this.transport.request<OAuthStatusView>("connector/auth/status", {
      connector_id: connectorId,
    });
  }

  /**
   * Kick off the OAuth browser flow on the trusted host. Returns the start
   * acknowledgement immediately; poll `authStatus` until `authenticated`.
   */
  authStart(connectorId: string): Promise<OAuthStartResult> {
    return this.transport.request<OAuthStartResult>("connector/auth/start", {
      connector_id: connectorId,
    });
  }

  /** Revoke the OAuth token for a connector. */
  logout(connectorId: string): Promise<void> {
    return this.transport.request<{ logged_out: boolean }>("connector/auth/logout", {
      connector_id: connectorId,
    }).then(() => undefined);
  }

  /**
   * Run one tool on a registered connector, through the host's own connection.
   *
   * The host holds the transport, its headers and its OAuth token; this sends a
   * tool name and an argument object and nothing else — you cannot name a URL,
   * a command or a header. Whether the pair is callable at all is the host's
   * `[connector_proxy]` policy: once its operator turns the proxy on, the
   * enabled connectors are callable, and `allow` / `deny` are that operator's
   * optional narrowing and subtraction. So `policy_denied` means either that
   * the host never turned the proxy on, or that this pair was narrowed out or
   * explicitly denied — not that a tool name was misspelled in a list you were
   * supposed to maintain.
   *
   * Arguments are passed through verbatim (there is no client-side schema
   * check); `get()` / `test()` return each tool's `input_schema` so you can see
   * what it takes, and a parameter the server rejects comes back as
   * `is_error: true`, not as a rejected promise.
   *
   * Resolves even when the tool itself failed — check `is_error`. It rejects
   * only when the call never reached the tool: `connector_call_timeout`,
   * `connector_call_failed`, `response_too_large`, `connector_unavailable`,
   * `policy_denied`, `not_found`.
   */
  call(connectorId: string, tool: string, args: unknown = {}): Promise<ConnectorCallResult> {
    return this.transport.request<ConnectorCallResult>("connector/call", {
      connector_id: connectorId,
      tool,
      arguments: args,
    });
  }
}