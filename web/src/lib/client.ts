/**
 * Web host's `AppServerClient`: the transport-agnostic base from
 * `@flowy-agent-store/client` plus the Web-only helpers that don't belong in the
 * published package — asset `<img>` URL derivation, the independent host file
 * service (`/api/fs/*`, doc 19 §3 W5), and the one-shot HTTP workspace
 * registration used by the smoke/dev script.
 */

import {
  AppServerClient as BaseClient,
  WebSocketTransport,
  type AppServerClientOptions as BaseOptions,
  type Transport,
} from "@flowy-agent-store/client";
import { TransportError } from "@flowy-agent-store/protocol";
import { AppServerError } from "@flowy-agent-store/protocol";
import {
  APP_SERVER_PROTOCOL_VERSION,
  type BrowseDirectoryResult,
  type FileMetadata,
  type WorkspaceFlatFile,
  type WorkspaceRegistration,
} from "@flowy-agent-store/protocol";

export * from "@flowy-agent-store/client";

export interface AppServerClientOptions extends Omit<BaseOptions, "transport"> {
  /** Ready-made transport; defaults to a `WebSocketTransport` over `wsUrl`. */
  transport?: Transport;
  /**
   * `ws://host/api/app-server/ws` — required unless `transport` is provided.
   * Only used for URL derivation and the default transport; never sent.
   */
  wsUrl?: string;
  /** `http://host/api/app-server` — one-shot HTTP helpers + URL derivation. */
  httpBaseUrl?: string;
  token?: string;
  requestTimeoutMs?: number;
}

const CONNECTION_HEADER = "x-app-server-connection-id";

/** `{ success, data }` envelope used by the standalone host file service. */
interface ApiResponse<T> {
  success?: boolean;
  data?: T;
}

/** Derive the HTTP helper base URL from the WebSocket URL. */
function deriveHttpBaseUrl(wsUrl: string): string | undefined {
  try {
    const url = new URL(wsUrl);
    const protocol = url.protocol === "wss:" ? "https:" : "http:";
    const path = url.pathname.replace(/\/ws\/?$/, "") || "";
    return `${protocol}//${url.host}${path}`;
  } catch {
    return undefined;
  }
}

export class AppServerClient extends BaseClient {
  readonly httpBaseUrl?: string;
  readonly token?: string;
  /** WS endpoint for URL derivation (asset URLs, fs/browse). */
  private readonly wsEndpoint?: string;

  constructor(options: AppServerClientOptions) {
    const { wsUrl, httpBaseUrl, token, requestTimeoutMs, ...rest } = options;
    if (!options.transport && !wsUrl) {
      throw new TransportError("connect", "either transport or wsUrl is required");
    }
    super({
      ...rest,
      transport:
        options.transport ??
        new WebSocketTransport(wsUrl as string, {
          requestTimeoutMs,
          token,
        }),
    });
    // HTTP helpers (workspace registration helper, fs/browse) share the App
    // Server base URL with the WebSocket. When the caller only configured
    // `wsUrl`, derive the HTTP base
    // (`ws://host/api/app-server/ws` → `http://host/api/app-server`).
    this.httpBaseUrl = httpBaseUrl ?? (wsUrl ? deriveHttpBaseUrl(wsUrl) : undefined);
    this.token = token;
    this.wsEndpoint = wsUrl;
  }

  /**
   * Register a server-created workspace directory (no local path involved).
   * Kept on the one-shot HTTP binding: only the smoke/dev script uses it;
   * the UI registers user-chosen paths via `workspaces.create`
   * (`workspace/create` over the transport).
   */
  async registerWorkspace(): Promise<WorkspaceRegistration> {
    if (!this.httpBaseUrl) {
      throw new TransportError("send", "httpBaseUrl is required for workspace registration");
    }
    const { connectionId } = await this.httpHandshake();
    return this.httpPost<WorkspaceRegistration>("/workspaces", undefined, connectionId);
  }

  /** Absolute server root (without `/api/app-server`), derived from the
   *  websocket URL — used for root-level routes and asset URLs. */
  get serverRootUrl(): string | undefined {
    if (!this.httpBaseUrl) return undefined;
    try {
      if (!this.wsEndpoint) throw new Error("no ws endpoint");
      const url = new URL(this.wsEndpoint);
      return `${url.protocol === "wss:" ? "https:" : "http:"}//${url.host}`;
    } catch {
      return this.httpBaseUrl.replace(/\/api\/app-server(\/)?$/, "");
    }
  }

  async browseDirectory(path?: string, showFiles?: boolean): Promise<BrowseDirectoryResult> {
    if (!this.httpBaseUrl) {
      throw new TransportError("send", "httpBaseUrl is required for file browsing");
    }
    const wsUrl = this.wsEndpoint;
    const rootBase = (() => {
      try {
        if (!wsUrl) return undefined;
        const url = new URL(wsUrl);
        return `${url.protocol === "wss:" ? "https:" : "http:"}//${url.host}`;
      } catch {
        return undefined;
      }
    })() || this.httpBaseUrl.replace(/\/api\/app-server(\/)?$/, "");
    const params = new URLSearchParams();
    if (path) params.set("path", path);
    if (showFiles) params.set("showFiles", "true");
    const query = params.size > 0 ? `?${params.toString()}` : "";
    const { connectionId } = await this.httpHandshake();
    const payload = await this.httpGet<{ success?: boolean; data?: BrowseDirectoryResult } & BrowseDirectoryResult>(`${rootBase}/api/fs/browse${query}`, connectionId);
    return payload.data ?? payload;
  }

  /**
   * List every file under a workspace root (host file service, `POST /api/fs/list`).
   *
   * This is the WebUI-side stand-in for the deferred Artifact protocol: doc `05`
   * §8 defines `artifact/list` / `artifact/get` but hard-codes
   * `capabilities.artifacts=false` and forbids arbitrary path reads
   * (`TC-AS-008`), so per-Run attribution is unavailable and the panel scopes to
   * a workspace instead (doc `19` §3 W5, deviation D-W5-1).
   */
  async listWorkspaceFiles(root: string): Promise<WorkspaceFlatFile[]> {
    const payload = await this.httpPostRoot<WorkspaceFlatFile[]>("/api/fs/list", { root });
    return payload.data ?? [];
  }

  /**
   * Read one text file (host file service, `POST /api/fs/read`). Resolves `null`
   * when the server cannot hand the file back as text (binary or too large).
   */
  async readFileContent(path: string, workspace?: string): Promise<string | null> {
    const payload = await this.httpPostRoot<string | null>("/api/fs/read", workspace ? { path, workspace } : { path });
    return payload.data ?? null;
  }

  /** Size / MIME type / mtime for one path (host file service, `POST /api/fs/metadata`). */
  async getFileMetadata(path: string, workspace?: string): Promise<FileMetadata | null> {
    const payload = await this.httpPostRoot<FileMetadata>("/api/fs/metadata", workspace ? { path, workspace } : { path });
    return payload.data ?? null;
  }

  /**
   * POST to the server **root** file service. `/api/fs/*` is served outside the
   * `/api/app-server` prefix that `httpPost` targets, exactly like `/api/fs/browse`.
   */
  private async httpPostRoot<T>(path: string, body: unknown): Promise<ApiResponse<T>> {
    const base = this.serverRootUrl;
    if (!base) {
      throw new TransportError("send", "serverRootUrl is required for host file-service calls");
    }
    const { connectionId } = await this.httpHandshake();
    const response = await fetch(`${base}${path}`, {
      method: "POST",
      headers: this.httpHeaders(connectionId),
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      throw await this.httpError(response);
    }
    return (await response.json()) as ApiResponse<T>;
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

  private async httpGet<T>(path: string, connectionId: string): Promise<T> {
    const url = path.startsWith("http://") || path.startsWith("https://") ? path : `${this.httpBaseUrl}${path}`;
    const response = await fetch(url, {
      method: "GET",
      headers: this.httpHeaders(connectionId),
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
