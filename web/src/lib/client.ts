/**
 * Web host's `AppServerClient`: the transport-agnostic base from
 * `@flowy-agent-store/client` plus the Web-only helpers that don't belong in the
 * published package — asset `<img>` URL derivation, the independent host file
 * service (`/api/fs/*`, doc 19 §3 W5), and the one-shot HTTP workspace
 * registration used by the smoke/dev script.
 */

import {
  AppServerClient as BaseClient,
  HttpTransport,
  WebSocketTransport,
  appServerErrorFromWire,
  type AppServerClientOptions as BaseOptions,
  type Transport,
} from "@flowy-agent-store/client";
import { TransportError } from "@flowy-agent-store/protocol";
import {
  APP_SERVER_PROTOCOL_VERSION,
  type BrowseDirectoryResult,
  type FileMetadata,
  type SkillDetail,
  type WorkspaceFlatFile,
  type WorkspaceRegistration,
} from "@flowy-agent-store/protocol";
import type { FileChangeOperation, SnapshotCompare, SnapshotInfo } from "./artifact-changes";

export * from "@flowy-agent-store/client";
// The skill write face addresses `skill/get`'s shape, so the store needs the
// type re-exported here (it is imported for the helper signatures above).
export type { SkillDetail } from "@flowy-agent-store/protocol";

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

/**
 * `config/get` / `config/set` view of the host's `~/.agent-store/config.toml`.
 *
 * Host management surface (`16` §6): the shape exists so the Web UI can render
 * and write the provider default, and it deliberately has **no** field for an
 * `api_key` or `base_url`, so a credential cannot reach the front end through
 * this face.
 */
export interface AgentStoreConfigView {
  /** The file exists on this host; a save creates it when it does not. */
  exists: boolean;
  /** Declared `default_model`; explicit `null` = none declared in the file. */
  default_model: string | null;
  /** `[providers.<name>]` tables as declared in the file. */
  providers: AgentStoreConfigProvider[];
  /**
   * `[memory]` table; `null` when the file declares no such table (so the UI
   * can say "not configured" instead of inventing "off"). Additive field.
   */
  memory: AgentStoreConfigMemory | null;
}

/** `[memory]` in the host settings file, as far as the wire exposes it. */
export interface AgentStoreConfigMemory {
  /** `null` = the table exists without the key (upstream default applies). */
  distill_enabled: boolean | null;
}

/** One provider table from the file (never a registered-provider row). */
export interface AgentStoreConfigProvider {
  /** `[providers.<name>]` key — the left half of a `default_model`. */
  name: string;
  /** `enabled = false` in the file; `true` when the key is absent. */
  enabled: boolean;
  /** Model names declared for this provider in the file. */
  models: string[];
}

/** The only keys `config/set` accepts (the server rejects anything else). */
export interface AgentStoreConfigPatch {
  default_model?: string;
  /** Writes `[memory] distill_enabled` — the switch the host reads at startup. */
  memory?: { distill_enabled: boolean };
}

/** `skill/create` — structured fields; the server assembles the frontmatter. */
export interface SkillCreateInput {
  /** Becomes the skill's public id and its directory name. */
  name: string;
  description: string;
  when_to_use?: string;
  allowed_tools?: string;
  paths?: string;
  body?: string;
}

/**
 * `skill/update` — a field-level patch.
 *
 * An absent field is left alone (`undefined`, not empty string). An **empty
 * string** on one of the optional keys clears that key; `description` may not
 * be emptied (`invalid_request`). There is deliberately no `name`.
 */
export interface SkillUpdateInput {
  skill_id: string;
  description?: string;
  when_to_use?: string;
  allowed_tools?: string;
  paths?: string;
  /** Replaces the body wholesale — the read face never returned it, so an edit
   * can only ever *replace* prose, not append to what it never saw. */
  body?: string;
}

/** `skill/delete` — what the id resolves to after the delete. */
export interface SkillDeleteResult {
  skill_id: string;
  deleted: boolean;
  /** Present when the id now resolves to another origin (e.g. a built-in the
   * user skill was shadowing); absent when nothing is visible there any more. */
  revealed_origin?: SkillOriginWire | null;
}

/** On-disk owner of a skill, as the read face reports it. */
export type SkillOriginWire =
  | "user"
  | "shared"
  | "companion"
  | "draft"
  | "marketplace"
  | "builtin"
  | "unmanaged";

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
   * R20b「接受 / 回退」的宿主面原语（`POST /api/fs/snapshot/*`，doc 19 §3 W5）。
   *
   * 与 `/api/fs/list` 同源——都是**宿主文件服务**，不是 App Server 协议。Artifact
   * 协议（`artifact/list` / `artifact/get`）仍然延后（`05` §8 / TC-AS-008），所以
   * 变更审查落在既有的 git 基线快照上：`compare` 出变更，`stage` = 接受，
   * `discard` = 回退。`file_path` 一律传**相对路径**（服务端 `workdir.join(...)`）。
   *
   * 回调契约：`/api/fs/*` 的 `{ success, data }` 之外，非 2xx 直接抛错——`compare`
   * 在「工作区未初始化」时抛 400，调用方据此先 `init` 一次（见 store 的
   * `refreshArtifactChanges`）。
   */

  /** Track a workspace against a fresh baseline (`POST /api/fs/snapshot/init`). */
  async snapshotInit(workspace: string): Promise<SnapshotInfo> {
    const payload = await this.httpPostRoot<SnapshotInfo>("/api/fs/snapshot/init", { workspace });
    if (!payload.data) throw new Error("snapshot/init returned no data");
    return payload.data;
  }

  /** Changes against the baseline, split into pending (`unstaged`) and accepted (`staged`). */
  async snapshotCompare(workspace: string): Promise<SnapshotCompare> {
    const payload = await this.httpPostRoot<SnapshotCompare>("/api/fs/snapshot/compare", { workspace });
    return payload.data ?? { staged: [], unstaged: [] };
  }

  /** Accept one change (`POST /api/fs/snapshot/stage`); file contents are untouched. */
  async snapshotStageFile(workspace: string, filePath: string): Promise<void> {
    await this.httpPostRoot<null>("/api/fs/snapshot/stage", { workspace, file_path: filePath });
  }

  /** Accept every pending change (`POST /api/fs/snapshot/stage-all`). */
  async snapshotStageAll(workspace: string): Promise<void> {
    await this.httpPostRoot<null>("/api/fs/snapshot/stage-all", { workspace });
  }

  /** Undo an acceptance without touching the file (`POST /api/fs/snapshot/unstage`). */
  async snapshotUnstageFile(workspace: string, filePath: string): Promise<void> {
    await this.httpPostRoot<null>("/api/fs/snapshot/unstage", { workspace, file_path: filePath });
  }

  /**
   * Revert one change (`POST /api/fs/snapshot/discard`).
   *
   * `operation` must be the value `compare` reported — the server does not
   * re-derive it. `create` deletes the new file; `modify` / `delete` restore it
   * from the baseline.
   */
  async snapshotDiscardFile(workspace: string, filePath: string, operation: FileChangeOperation): Promise<void> {
    await this.httpPostRoot<null>("/api/fs/snapshot/discard", { workspace, file_path: filePath, operation });
  }

  /**
   * R15：把一份本地文件（粘贴 / 拖拽来的 `File`）写进**会话工作区**，返回落点的
   * 绝对路径——正是附件载体（路径引用）需要的形态。
   *
   * 与 `/api/fs/upload` 的默认落点（宿主 tmp 沙箱）不同，这里带上 `workspace`：
   * 发送路径只准入会话工作区内的真实文件，落 tmp 会得到一个必然被拒的路径。工作区
   * 准入由服务端复用同一套 allowed-roots 校验（不是第二条路径权威）。
   */
  async uploadFileToWorkspace(workspace: string, file: File): Promise<string> {
    const form = new FormData();
    form.append("file", file, file.name);
    form.append("file_name", file.name);
    form.append("workspace", workspace);
    const payload = await this.httpPostRootForm<string>("/api/fs/upload", form);
    if (!payload.data) throw new Error("upload returned no path");
    return payload.data;
  }

  /**
   * Multipart POST to the server **root** file service. `Content-Type` is left
   * to the browser so the multipart boundary is correct — only the connection
   * header is sent.
   */
  private async httpPostRootForm<T>(path: string, form: FormData): Promise<ApiResponse<T>> {
    const base = this.serverRootUrl;
    if (!base) {
      throw new TransportError("send", "serverRootUrl is required for host file-service calls");
    }
    const { connectionId } = await this.httpHandshake();
    const response = await fetch(`${base}${path}`, {
      method: "POST",
      headers: { [CONNECTION_HEADER]: connectionId },
      body: form,
    });
    if (!response.ok) {
      throw await this.httpError(response);
    }
    return (await response.json()) as ApiResponse<T>;
  }

  /**
   * Read the host's agent-store settings file (`config/get`).
   *
   * Provider/default-model configuration is **host management surface**, not a
   * published SDK method (doc `16` §6: a third-party consumer has no business
   * reading this host's provider config), so it lives here with the other
   * Web-only helpers and rides the protocol transport directly. The result
   * carries no credential by construction, and the file location is the host's
   * own — no parameter can name a path.
   */
  async getAgentStoreConfig(): Promise<AgentStoreConfigView> {
    return this.transport.request<AgentStoreConfigView>("config/get", {});
  }

  /**
   * Write one whitelisted settings key (`config/set`) and return the file
   * **re-read from disk**, so a caller never has to treat "request sent" as
   * "value stored". `api_key` / `base_url` / arbitrary paths are not
   * expressible in `AgentStoreConfigPatch`; the server answers `invalid_request`
   * for anything outside the whitelist.
   */
  async setAgentStoreConfig(patch: AgentStoreConfigPatch): Promise<AgentStoreConfigView> {
    return this.transport.request<AgentStoreConfigView>("config/set", patch);
  }

  /**
   * Skill write face (`skill/create` · `skill/update` · `skill/delete` ·
   * `skill/copy`, doc `16` R17 / W12).
   *
   * Same reasoning as `config/*`: a third-party consumer must not be able to
   * write into this host's skill tree, so these are **wire-only** host
   * management methods with no counterpart in the published client package and
   * no HTTP binding. The Web UI is the host's own surface, so it calls them
   * through these helpers.
   *
   * Every one of them answers with the **re-read** shape: `skill/create`,
   * `skill/update` and `skill/copy` return `skill/get`'s view of the affected
   * skill, `skill/delete` returns what the id now resolves to. No field names a
   * path, and a name that would escape the user skills root is refused
   * server-side.
   */
  async createSkill(input: SkillCreateInput): Promise<SkillDetail> {
    return this.transport.request<SkillDetail>("skill/create", input);
  }

  /** Field-level edit: absent fields are left alone; `name` is not patchable. */
  async updateSkill(input: SkillUpdateInput): Promise<SkillDetail> {
    return this.transport.request<SkillDetail>("skill/update", input);
  }

  async deleteSkill(skillId: string): Promise<SkillDeleteResult> {
    return this.transport.request<SkillDeleteResult>("skill/delete", { skill_id: skillId });
  }

  /** Derive a new **user** skill from any origin (read-only sources included). */
  async copySkill(skillId: string, newName: string): Promise<SkillDetail> {
    return this.transport.request<SkillDetail>("skill/copy", { skill_id: skillId, new_name: newName });
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

  /**
   * Ready connection id for **host-side** calls.
   *
   * The handshake itself now lives in the package
   * (`@flowy-agent-store/client` `HttpTransport.openConnection`, doc 16 R2);
   * what stays here is the reason the webui needs one outside `request()`:
   * `/api/fs/*` is a host file service and `POST /workspaces` is an HTTP-only
   * register route — neither is a protocol method (`05` §2.1.1).
   */
  private async httpHandshake(): Promise<{ connectionId: string }> {
    const transport = new HttpTransport({
      baseUrl: this.httpBaseUrl ?? "",
      token: this.token,
      client: this.clientInfo,
      capabilities: this.capabilities,
    });
    return { connectionId: await transport.openConnection() };
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

  /** Wire-error → SDK error, via the one shared mapping (doc 16 R2). */
  private async httpError(response: Response): Promise<unknown> {
    let payload: unknown = null;
    try {
      payload = await response.json();
    } catch {
      // fall through: a body-less failure becomes a transport error
    }
    return appServerErrorFromWire(payload, response.status, "host file service");
  }
}
