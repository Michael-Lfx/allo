/**
 * Agent Store Connector (MCP) catalog client over the App Server WebSocket
 * transport: catalog, status, probe and OAuth pass-through.
 *
 * Tokens never cross this module — the OAuth browser flow is owned by the
 * trusted host; clients only start it and wait on `waitForAuth`.
 */

import type { Transport } from "./transport";
import type {
  ConnectorCallResult,
  ConnectorCredential,
  ConnectorDetail,
  ConnectorProbeResult,
  ConnectorStatusView,
  ConnectorSummary,
  OAuthStartResult,
  OAuthStatusView,
} from "@flowy-agent-store/protocol";

/**
 * How long {@link ConnectorClient.waitForAuth} waits by default: the host's own
 * callback window is 120s, so a shorter client budget would give up on flows the
 * host is still willing to finish.
 */
const DEFAULT_AUTH_TIMEOUT_MS = 120_000;
const DEFAULT_AUTH_POLL_MS = 500;

export interface WaitForAuthOptions {
  /** Overall budget in ms; defaults to `DEFAULT_AUTH_TIMEOUT_MS` (120s). */
  timeoutMs?: number;
  /** Poll interval in ms; defaults to 500. The read is a local DB projection. */
  pollMs?: number;
}

/**
 * How a completed wait ended.
 *
 * `error` carries the host's own sentence. It is the shape of every failure that
 * happens *after* `authStart` acknowledged the browser step — the state stays
 * `not_authenticated` (there is no "failed" state on the wire), so the reason is
 * the only signal there is.
 */
export type WaitForAuthOutcome =
  | { state: "authenticated" }
  | { state: "error"; error: string }
  | { state: "timeout" };

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

  /**
   * Check the current OAuth state for a connector.
   *
   * Read `error` too: a flow that failed after the browser step is reported as
   * `state: "not_authenticated"` **plus** `error`, because the wire has no
   * failing state to flip to. {@link waitForAuth} is the loop that does this.
   */
  authStatus(connectorId: string): Promise<OAuthStatusView> {
    return this.transport.request<OAuthStatusView>("connector/auth/status", {
      connector_id: connectorId,
    });
  }

  /**
   * The credential form a connector needs filled in, as **this caller** sees it
   * (doc `34` §6.1).
   *
   * `mode` says which kind of authentication applies (`none` / `oauth` / `token`),
   * `status` whether anything is still missing, and `missing` names the keys.
   * `fields` is everything needed to render the form — labels, placeholders,
   * descriptions and the marketplace's "where do I get a key" link, in both
   * languages. No secret ever comes back: only `plain` fields carry a `value`.
   */
  credentials(connectorId: string): Promise<ConnectorCredential> {
    return this.transport.request<ConnectorCredential>("connector/credential/get", {
      connector_id: connectorId,
    });
  }

  /**
   * Store what the user typed and get the new state back.
   *
   * Only keys the connector's own declaration names are accepted — the form is
   * the whole write surface. `secret` fields go to the host's credential store
   * under the caller's own namespace, `plain` fields into the connector's
   * configuration, so a shared host never resolves one user's token for another.
   */
  setCredentials(
    connectorId: string,
    values: Record<string, string>,
  ): Promise<ConnectorCredential> {
    return this.transport.request<ConnectorCredential>("connector/credential/set", {
      connector_id: connectorId,
      values,
    });
  }

  /**
   * Forget stored credentials. `keys` omitted = every secret field of this
   * connector. Idempotent: clearing what is not there succeeds.
   */
  clearCredentials(connectorId: string, keys?: string[]): Promise<ConnectorCredential> {
    return this.transport.request<ConnectorCredential>("connector/credential/clear", {
      connector_id: connectorId,
      ...(keys ? { keys } : {}),
    });
  }

  /**
   * Kick off the OAuth browser flow on the trusted host. Returns the start
   * acknowledgement immediately; follow it with {@link waitForAuth}.
   *
   * `state: "error"` here is a failure that happened **before** the browser was
   * opened (endpoint discovery, client identity, binding the callback, launching
   * the browser): nothing was shown to the user, so `error` is the whole story
   * and there is nothing to wait for.
   */
  authStart(connectorId: string): Promise<OAuthStartResult> {
    return this.transport.request<OAuthStartResult>("connector/auth/start", {
      connector_id: connectorId,
    });
  }

  /**
   * Wait for an authorization that was started (in the browser, or by a
   * previous `authStart`) to finish — and **report why** when it does not.
   *
   * A hand-rolled `while (state !== "authenticated") poll()` loop ends in "it
   * never became authenticated", with the reason sitting unread in the status it
   * already fetched: a token exchange the authorization server refused (a
   * throttling gateway answers `slow_down`), a callback timeout, a CSRF/path
   * mismatch. Those land in `authStatus().error` *only*, and the state stays
   * `not_authenticated` — there is no failing state on the wire.
   *
   * Resolves for every domain outcome (`authenticated` / `error` / `timeout`);
   * rejects only when the status read itself fails (transport or protocol), the
   * same rule as `connector/call`. The wait is bounded by `timeoutMs`; a caller
   * that needs to stop earlier can race the returned promise.
   */
  async waitForAuth(
    connectorId: string,
    options: WaitForAuthOptions = {},
  ): Promise<WaitForAuthOutcome> {
    const deadline = Date.now() + (options.timeoutMs ?? DEFAULT_AUTH_TIMEOUT_MS);
    const pollMs = options.pollMs ?? DEFAULT_AUTH_POLL_MS;
    for (;;) {
      // Read before sleeping: the flow may already be over (the browser can
      // redirect before this call even starts).
      const status = await this.authStatus(connectorId);
      if (status.state === "authenticated") return { state: "authenticated" };
      if (status.error) return { state: "error", error: status.error };
      if (Date.now() >= deadline) return { state: "timeout" };
      await new Promise((resolve) => setTimeout(resolve, pollMs));
    }
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