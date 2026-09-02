/**
 * Typed App Server client.
 *
 * Connection lifecycle: transport connect → `initialize` → protocol version
 * check → `initialized` notification → ready. Business methods require a
 * ready client; the server rejects them with `not_initialized` otherwise.
 */

import { AppServerError, ProtocolError, TransportError } from "./errors";
import {
  APP_SERVER_PROTOCOL_VERSION,
  type ClientCapabilities,
  type ClientInfo,
  type InitializeRequest,
  type InitializeResult,
  type WorkspaceRegistration,
} from "./protocol";
import { ConversationClient } from "./conversations";
import { ConnectorClient } from "./connectors";
import { RunClient } from "./runs";
import { SkillClient } from "./skills";
import { WorkspaceClient } from "./workspaces";
import { WebSocketTransport, type NotificationListener } from "./transport";

export interface AppServerClientOptions {
  /** `ws://host/api/app-server/ws` */
  wsUrl: string;
  /** `http://host/api/app-server` — used for helper HTTP endpoints. */
  httpBaseUrl?: string;
  client: ClientInfo;
  capabilities?: ClientCapabilities;
  token?: string;
  requestTimeoutMs?: number;
}

const CONNECTION_HEADER = "x-app-server-connection-id";

export class AppServerClient {
  readonly transport: WebSocketTransport;
  /** Legacy preset/execution workflow client. */
  readonly runs: RunClient;
  /** Persistent presetless Nomi chat client. */
  readonly conversations: ConversationClient;
  /** Owner-scoped workspace registry client (user-chosen paths). */
  readonly workspaces: WorkspaceClient;
  /** Agent Store Skill catalog client (`skill/list`, `skill/get`). */
  readonly skills: SkillClient;
  /** Agent Store Connector catalog/status/probe/OAuth client. */
  readonly connectors: ConnectorClient;
  readonly httpBaseUrl?: string;
  readonly token?: string;
  readonly clientInfo: ClientInfo;
  readonly capabilities?: ClientCapabilities;

  private initializeResult: InitializeResult | null = null;
  private notificationListeners = new Set<NotificationListener>();

  constructor(options: AppServerClientOptions) {
    this.clientInfo = options.client;
    this.capabilities = options.capabilities;
    this.httpBaseUrl = options.httpBaseUrl;
    this.token = options.token;
    this.transport = new WebSocketTransport(options.wsUrl, {
      requestTimeoutMs: options.requestTimeoutMs,
      token: options.token,
    });
    this.runs = new RunClient(this.transport);
    this.conversations = new ConversationClient(this.transport);
    this.workspaces = new WorkspaceClient(this.transport);
    this.skills = new SkillClient(this.transport);
    this.connectors = new ConnectorClient(this.transport);
    this.transport.onNotification((notification) => {
      for (const listener of [...this.notificationListeners]) {
        try {
          listener(notification);
        } catch {
          // listener isolation
        }
      }
    });
  }

  get ready(): boolean {
    return this.initializeResult !== null;
  }

  get initializeInfo(): InitializeResult | null {
    return this.initializeResult;
  }

  /** Connect and perform the initialize/initialized handshake. */
  async connect(): Promise<InitializeResult> {
    await this.transport.connect();
    const request: InitializeRequest = {
      protocol_version: APP_SERVER_PROTOCOL_VERSION,
      client: this.clientInfo,
      capabilities: this.capabilities,
    };
    const result = await this.transport.request<InitializeResult>("initialize", request);
    if (result.protocol_version !== APP_SERVER_PROTOCOL_VERSION) {
      throw new ProtocolError(
        "version_mismatch",
        `server protocol version ${result.protocol_version} is not supported (client ${APP_SERVER_PROTOCOL_VERSION})`,
      );
    }
    this.transport.notify("initialized", {});
    this.initializeResult = result;
    return result;
  }

  onNotification(listener: NotificationListener): () => void {
    this.notificationListeners.add(listener);
    return () => {
      this.notificationListeners.delete(listener);
    };
  }

  /** Close the transport; the server revokes the connection immediately. */
  close(): void {
    this.transport.close();
    this.initializeResult = null;
    this.notificationListeners.clear();
  }

  /**
   * Register an owner-scoped workspace through the HTTP helper endpoint.
   *
   * Each call performs its own short-lived HTTP handshake so the returned
   * `connection_id` belongs to the workspace registration call, not to the
   * long-lived WebSocket connection.
   */
  async registerWorkspace(): Promise<WorkspaceRegistration> {
    if (!this.httpBaseUrl) {
      throw new TransportError("send", "httpBaseUrl is required for workspace registration");
    }
    const { connectionId } = await this.httpHandshake();
    return this.httpPost<WorkspaceRegistration>("/workspaces", undefined, connectionId);
  }

  private async httpHandshake(): Promise<{ connectionId: string }> {
    const response = await fetch(`${this.httpBaseUrl}/initialize`, {
      method: "POST",
      headers: this.httpHeaders(),
      body: JSON.stringify({
        protocol_version: APP_SERVER_PROTOCOL_VERSION,
        client: this.clientInfo,
        capabilities: this.capabilities,
      }),
    });
    const connectionId = response.headers.get(CONNECTION_HEADER);
    if (!response.ok || !connectionId) {
      throw await this.httpError(response);
    }
    // Complete the ready transition before any business call, exactly like
    // the WebSocket lifecycle: initialize → initialized → ready.
    const ready = await fetch(`${this.httpBaseUrl}/initialized`, {
      method: "POST",
      headers: this.httpHeaders(connectionId),
    });
    if (!ready.ok) {
      throw await this.httpError(ready);
    }
    return { connectionId };
  }

  private async httpPost<T>(path: string, body: unknown, connectionId: string): Promise<T> {
    const response = await fetch(`${this.httpBaseUrl}${path}`, {
      method: "POST",
      headers: this.httpHeaders(connectionId),
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) {
      throw await this.httpError(response);
    }
    return (await response.json()) as T;
  }

  private httpHeaders(connectionId?: string): Record<string, string> {
    const headers: Record<string, string> = {
      "content-type": "application/json",
    };
    if (connectionId) {
      headers[CONNECTION_HEADER] = connectionId;
    }
    if (this.token) {
      headers.authorization = `Bearer ${this.token}`;
    }
    return headers;
  }

  private async httpError(response: Response): Promise<unknown> {
    interface HttpWireError {
      code?: string;
      message?: string;
      retryable?: boolean;
      details?: Record<string, unknown>;
    }
    let wire: HttpWireError | null = null;
    try {
      wire = (await response.json()) as HttpWireError;
    } catch {
      // fall through to a generic transport error
    }
    if (wire?.code) {
      return new AppServerError({
        code: wire.code,
        message: wire.message ?? `http ${response.status}`,
        retryable: wire.retryable ?? false,
        details: wire.details ?? {},
      });
    }
    return new TransportError("receive", `http ${response.status} from app-server`);
  }
}