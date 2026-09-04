/**
 * Agent Store Connector (MCP) catalog client over the App Server WebSocket
 * transport: catalog, status, probe and OAuth pass-through.
 *
 * Tokens never cross this module — the OAuth browser flow is owned by the
 * trusted host; clients only start it and poll `authStatus`.
 */

import type { Transport } from "./transport";
import type {
  ConnectorDetail,
  ConnectorProbeResult,
  ConnectorStatusView,
  ConnectorSummary,
  OAuthStartResult,
  OAuthStatusView,
} from "@agent-store/protocol";

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

  /** Run a connection probe; the server persists the result. */
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
}