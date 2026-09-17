/**
 * Agent Store App Server Protocol types.
 *
 * The chat surface uses only opaque public conversation/message IDs. Internal
 * execution/session/step/attempt IDs never appear in this module.
 */

/**
 * A contract **fingerprint**, not a version number: it must differ from the
 * previous value on any wire change at all, additive included. The shape is
 * `fp-<n>` — a plain counter, so a bump just increments it and no value is ever
 * reused by accident.
 *
 * So it is a **label, not a version**: neither a release number nor a date.
 * Until `fp-1` the values were date stamps (kept below as history), and those
 * dates were **not** the day of the change: consecutive changes advanced the
 * stamp a day each, so they ran ahead of the calendar. The list below is keyed
 * by *value* — read it as "which wire change is this?", never as "when did this
 * ship".
 *
 * `2026-09-16` carried `StoreItem.published_at`; `2026-09-17` added the two MCP
 * declaration write methods (`config/set-mcp`, `config/set-mcp-enabled`);
 * `2026-09-18` adds `config/get-mcp`, the file editor's read of the same file;
 * `2026-09-19` adds the `conversation/list-changed` notification; `2026-09-20`
 * adds the Skill **file tree** read face (`skill/files`, `skill/file`) so a
 * Skill's companion files are readable, not just its bounded manifest summary;
 * `2026-09-21` adds the connector **call proxy** (`connector/call`), so a third
 * party can run an installed MCP tool while the connection and its credentials
 * stay on the host. **`fp-1` changes the shape only** (date stamp → counter): a
 * `2026-…` value invites being read as a release date, and no wire behaviour
 * changed with the rename. **`fp-2` carries the tools' parameters**
 * (`ConnectorTool.input_schema`, plus `tools_truncated` on `ConnectorDetail`
 * and `ConnectorProbeResult`): a caller that must *name* a tool to be granted it
 * should be able to read what that tool takes. It is the counterpart of the
 * host's `[connector_proxy]` grant moving from one tool at a time to the
 * connector (doc `26`). **`fp-3` makes a Skill selectable per turn**:
 * `conversation/send` gains an optional `mentions` list whose only honoured kind
 * is `skill`, so one turn can mount a Skill's instructions without rewriting the
 * conversation's create-time snapshot (doc `27` 阶段 1). **`fp-4` lets a
 * conversation be created as an installed expert**: `conversation/create` gains
 * an optional `agent_id`, whose Definition supplies the chat's preset identity
 * plus its own Skill and Connector fences — frozen at creation, since none of
 * those keys is mutable afterwards (doc `27` 阶段 2a). **`fp-5` opens a Team's
 * Leader the same way**: an optional `team_id` runs the `team/run` orchestration
 * (members, template, fences) but stops before the goal turn, so the client
 * speaks first; `agent_id` and `team_id` are mutually exclusive (阶段 2b).
 * **`fp-6` makes the model and the reasoning level selectable per call**:
 * `conversation/send` and `agent/run` each gain an optional `model` and
 * `reasoning_effort`. The scope is deliberately the **conversation** (send) and
 * the **run** (agent/run), not "just this turn": the Nomi runtime is built from
 * the persisted row, so a send-carried value is a sticky switch that takes
 * effect on that very turn (doc `29` §4). `ConversationView` also gains
 * `reasoning_effort`, because a setting that can be written by three methods but
 * read back by none is not a setting a client can honour (doc `29` §5.5).
 */
export const APP_SERVER_PROTOCOL_VERSION = "fp-6";

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number | string;
  method: string;
  params?: unknown;
}

export interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: WireError;
}

export interface JsonRpcEventNotification {
  jsonrpc: "2.0";
  method: "event";
  params: RunEvent;
}

export interface ResyncRequiredNotification {
  jsonrpc: "2.0";
  method: "run/resync-required";
  params: { run_ids: string[]; reason: string };
}

export interface ConversationEventNotification {
  jsonrpc: "2.0";
  method: "conversation/event";
  params: ConversationEvent;
}

export interface ConversationResyncRequiredNotification {
  jsonrpc: "2.0";
  method: "conversation/resync-required";
  params: { conversation_ids: string[]; reason: string };
}

/**
 * 会话**列表**投影变更（`conversation/list-changed`，2026-09-19 加入）。
 *
 * 与 `conversation/event` 是两件事：那些是某条被订阅会话的转写帧、带 `sequence`；
 * 这一条改的是侧栏那一整份列表（自动标题、重命名、删除），所以：
 *   - 不要求订阅该会话（用户此刻可能正看着另一个会话）；
 *   - **不带 `sequence`**，客户端不得据此推进 `lastSeenSequence`；
 *   - 尽力而为：丢一条只是让界面晚一步刷新，`conversation/list` 始终是权威。
 */
export interface ConversationListChangedNotification {
  jsonrpc: "2.0";
  method: "conversation/list-changed";
  params: { conversation_id: string; action: "created" | "updated" | "deleted" };
}

export type ServerNotification =
  | JsonRpcEventNotification
  | ResyncRequiredNotification
  | ConversationEventNotification
  | ConversationResyncRequiredNotification
  | ConversationListChangedNotification;

export interface WireError {
  code: string;
  message: string;
  retryable: boolean;
  details: Record<string, unknown>;
  request_id?: string | number | null;
}

export interface ClientInfo {
  name: string;
  version: string;
}

export interface ClientCapabilities {
  events?: boolean;
  approvals?: boolean;
  team_runtime?: boolean;
  artifacts?: boolean;
}

export interface InitializeRequest {
  protocol_version: string;
  client: ClientInfo;
  auth?: { mode?: string; credential?: string };
  capabilities?: ClientCapabilities;
}

export interface InitializeResult {
  protocol_version: string;
  server: { name: string; version: string };
  auth_context: { principal_id: string; issuer: string; audience: string; scopes: string[] };
  capabilities: Capabilities;
  connection_id: string;
}

export interface Capabilities {
  agents: boolean;
  teams: boolean;
  team_runtime: boolean;
  skills: boolean;
  /**
   * The Skill **file tree** read face (`skill/files` / `skill/file`).
   *
   * Separate from `skills` on purpose: a host can wire the catalog without the
   * file provider, so `skills: true` alone does not mean `skill/files` will
   * answer. Check this before offering file access.
   */
  skill_files: boolean;
  connectors: boolean;
  /**
   * The connector **call proxy** (`connector/call`, doc 24 §5).
   *
   * Separate from `connectors` on purpose: the catalog can be wired without the
   * proxy, and a host may wire the proxy while its `[connector_proxy]` table is
   * absent or disabled, in which case every call answers `policy_denied`. This
   * flag says the *method* exists, not that any tool is callable; the host
   * operator's own config decides that (doc `26` §4).
   */
  connector_calls: boolean;
  run_notifications: boolean;
  approvals: boolean;
  artifacts: boolean;
  oauth: boolean;
  imports: boolean;
  installs: boolean;
  marketplaces: boolean;
  store: boolean;
  models: boolean;
}

export interface WorkspaceRef { id: string }
export interface WorkspaceRegistration { id: string }

/** Owner-scoped workspace projection returned by `workspace/list` / `workspace/create`. */
export interface WorkspaceView {
  workspace_id: string;
  /** Display label derived server-side from the directory basename (not a path). */
  name: string;
  /** Absolute canonical path. Only present on the same owner's authenticated connection. */
  canonical_path: string;
  created_at: number;
  updated_at: number;
}

export interface WorkspaceCreateInput {
  /** Absolute local directory path chosen by the owner; validated server-side. */
  path: string;
}

/** Result of revoking (soft-deleting) an owner's active workspace. */
export interface WorkspaceRevokeResult {
  workspace_id: string;
  /** false when the workspace is foreign, missing, or already revoked. */
  revoked: boolean;
}

/** `GET /api/fs/browse` — one directory listing entry (directories only by default). */
export interface BrowseEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  isFile: boolean;
  size?: number | null;
  modified?: number | null;
}

/** `GET /api/fs/browse` response. */
export interface BrowseDirectoryResult {
  currentPath: string;
  parentPath?: string | null;
  items: BrowseEntry[];
  canGoUp: boolean;
  truncated: boolean;
  isRoot?: boolean | null;
}

/** `POST /api/fs/list` — one flat file entry under a workspace root (host file service). */
export interface WorkspaceFlatFile {
  name: string;
  full_path: string;
  relative_path: string;
}

/** `POST /api/fs/metadata` response (host file service). */
export interface FileMetadata {
  name: string;
  path: string;
  size: number;
  /** Server-reported MIME type. */
  type: string;
  last_modified: number;
  is_directory?: boolean | null;
}

export type RunStatus =
  | "planning" | "running" | "completed" | "completed_with_failures"
  | "failed" | "cancelled" | "paused" | "waiting_input"
  | "awaiting_approval" | "recovery_required";

export type MentionKind = "agent" | "skill" | "connector";

export interface MentionRef {
  kind: MentionKind;
  id: string;
}

export interface AgentRunInput {
  agentId: string;
  agentVersion?: string;
  goal?: string;
  input?: { text?: string } | Record<string, unknown>;
  workspaceId?: string;
  steps?: unknown[];
  commandId?: string;
  idempotencyKey?: string;
  /** Structured `@` mentions resolved by the composer (docs/agent-store/05 §4.7). */
  mentions?: MentionRef[];
  /**
   * Run-scoped model (doc `29` §6.1). Wins over the preset's own model **and**
   * over the host default: explicit > preset > `~/.agent-store/config.toml`.
   */
  model?: ProviderWithModel;
  /** Run-scoped OpenAI-style reasoning effort (doc `29` §6.1); applies to every attempt of the run. */
  reasoningEffort?: ReasoningEffort | string;
}

export interface AgentRunRequestWire {
  agent_id: string;
  agent_version?: string;
  goal?: string;
  input?: unknown;
  work_dir?: string;
  workspace?: WorkspaceRef;
  steps?: unknown[];
  command_id?: string;
  idempotency_key?: string;
  mentions?: MentionRef[];
  model?: ProviderWithModel;
  reasoning_effort?: string;
}

export interface RunReceipt {
  run_id: string;
  status: RunStatus;
  version: number;
  preset_revision: number;
  content_digest: string;
}

/**
 * `team/run` — Leader Conversation + planned delegation (docs/agent-store/05
 * §5.2, `16` §7 决策 3).
 *
 * There is deliberately **no** `planning` field: member pool, concurrency
 * ceiling, routing constraints and authority come from the bound Team template
 * and server policy. The wire DTO is `deny_unknown_fields`, so sending one is
 * `invalid_request` rather than a silently ignored parameter.
 */
export interface TeamRunInput {
  teamId: string;
  teamVersion?: string;
  goal?: string;
  input?: { text?: string } | Record<string, unknown>;
  workspaceId?: string;
  commandId?: string;
  idempotencyKey?: string;
}

export interface TeamRunRequestWire {
  team_id: string;
  team_version?: string;
  goal?: string;
  input?: unknown;
  workspace?: WorkspaceRef;
  command_id?: string;
  idempotency_key?: string;
}

/**
 * Deliberately **not** `RunReceipt`: a Team Run has no lead preset to report a
 * revision/digest for — its authority is the Team's mutable execution template.
 * Only what the server can honestly answer is on the wire.
 */
export interface TeamRunReceipt {
  run_id: string;
  status: RunStatus;
}

export interface RunView {
  run_id: string;
  status: RunStatus;
  version: number;
  summary?: string | null;
  output_files: string[];
  preset_revision?: number | null;
  content_digest?: string | null;
}
export type RunResult = RunView;
export interface CancelRunInput { runId: string; expectedVersion: number; commandId?: string; idempotencyKey?: string }
export interface SteerRunInput { runId: string; text: string; expectedVersion: number; commandId?: string; idempotencyKey?: string }
export type AnswerDecisionInput = {
  runId: string;
  stepId: string;
  attemptId: string;
  answer: string;
  /**
   * The engine's three-way CAS tokens. They are mandatory: an answer that does
   * not name the exact execution/step/attempt version it was written against is
   * refused with a `Conflict` instead of being applied. `run/events` projects
   * the current three onto every pending `approval.requested`, so a client
   * echoes those rather than inventing versions.
   */
  expectedExecutionVersion: number;
  expectedStepVersion: number;
  expectedAttemptVersion: number;
};
/**
 * One projected run event.
 *
 * `step_id` / `attempt_id` are present when the engine scoped the event to an
 * attempt (notably `approval.requested`). A pending decision additionally
 * carries the three CAS versions an accepted answer must echo; they are read
 * from the authoritative rows when the event is projected, so an answer built
 * from them races safely (a concurrent change turns it into a `Conflict`).
 *
 * There is deliberately no `always_allow` counterpart on the wire: answering a
 * decision never widens a tool policy.
 */
export interface RunEvent {
  run_id: string;
  sequence: number;
  event_type: string;
  payload: Record<string, unknown>;
  step_id?: string | null;
  attempt_id?: string | null;
  expected_execution_version?: number | null;
  expected_step_version?: number | null;
  expected_attempt_version?: number | null;
}
export interface RunEventsQuery { runId: string; afterSequence?: number; limit?: number }
export interface RunSubscriptionParams { run_id: string }

/**
 * W4 / W6 (doc 16 R10/R11, resolves D-W6-1): the authoritative plan snapshot.
 *
 * `run/events` is an append-only log of markers — step titles, failure reasons
 * and timings never appear there. This projection reads the engine's own rows,
 * so each step exists once with its current facts. Internal participant ids are
 * deliberately not on the wire: member attribution is `role` + `model`.
 */
export interface RunPlan {
  run_id: string;
  status: RunStatus;
  version: number;
  steps: RunPlanStep[];
  dependencies: RunPlanDependency[];
}
export interface RunPlanStep {
  step_id: string;
  title: string;
  kind: string;
  status: string;
  role?: string | null;
  model?: string | null;
  introduced_in_revision: number;
  superseded_in_revision?: number | null;
  created_at: number;
  updated_at: number;
  attempts: RunPlanAttempt[];
}
export interface RunPlanAttempt {
  attempt_id: string;
  attempt_no: number;
  status: string;
  trigger_reason: string;
  role?: string | null;
  model?: string | null;
  question?: string | null;
  error?: string | null;
  output_summary?: string | null;
  output_files: string[];
  tokens?: number | null;
  started_at?: number | null;
  finished_at?: number | null;
}
export interface RunPlanDependency { blocker_step_id: string; blocked_step_id: string }

export interface ProviderWithModel { provider_id: string; model: string; use_model?: string }
export interface ConversationCreateInput { name?: string; model?: ProviderWithModel; workspaceId?: string; reasoningEffort?: string; agentId?: string; teamId?: string }
export interface ConversationUpdateInput { name?: string; model?: ProviderWithModel; reasoningEffort?: string }
export type ReasoningEffort = "low" | "medium" | "high" | "xhigh" | "max";
export interface ConversationModelOption {
  name: string;
  display_name?: string | null;
  context_limit?: number | null;
  /**
   * models.dev catalog facts (W9 / R14). All of them are **absent** when the
   * registry has no entry for this provider+model (an unmapped platform, e.g.
   * the bundled `mimo`, never gets zeros standing in for "unknown").
   */
  cost_input?: number | null;
  cost_output?: number | null;
  catalog_context_window?: number | null;
  supports_vision?: boolean | null;
}
export interface ProviderModelOption { name: string; models: ConversationModelOption[] }
export interface ConversationModelSelection { provider: string; model: string }
export interface ConversationModelOptions {
  default?: ConversationModelSelection | null;
  providers: ProviderModelOption[];
  reasoning_efforts: ReasoningEffort[];
}
export interface ConversationView {
  conversation_id: string;
  name: string;
  model: ProviderWithModel;
  /**
   * The conversation's current reasoning effort (doc `29` §5.5). Absent = not
   * specified. `create` / `update` / `send` can all write it, so the view has to
   * say what it is — otherwise the setting is write-only.
   */
  reasoning_effort?: string | null;
  status: string;
  created_at: number;
  modified_at: number;
  is_processing: boolean;
  /** Opaque owner-scoped workspace id; absent for legacy chats (UI groups them under "未归类"). */
  workspace_id?: string | null;
  /** Last server-measured context occupancy; null/absent = unknown. */
  context_usage?: ContextUsage | null;
}

/** Measured context occupancy for one chat. `percent` is null when the engine reported no window. */
export interface ContextUsage {
  used_tokens: number;
  window_tokens: number;
  percent?: number | null;
  /**
   * W9 / R14 ③: the runtime's own per-turn report for the **most recent**
   * completed turn, persisted server-side next to the gauge so a reloaded
   * WebUI can still show last turn's tokens (and catalog-priced cost). Both are
   * absent when the runtime reported nothing — never a `0` standing in for
   * "unknown". They are not occupancy: `used_tokens` is the last request's
   * prompt size and must never be substituted for these.
   */
  last_turn_input_tokens?: number | null;
  last_turn_output_tokens?: number | null;
  updated_at: number;
  source: "measured" | string;
}

/**
 * Per-turn token accounting for ONE completed turn (W9 / R14, additive).
 *
 * The runtime's own report (`TurnCompleted`), projected onto the conversation
 * event stream — field names mirror the Run-side `TurnUsage` so one vocabulary
 * covers both surfaces. It is **not** context occupancy: `ContextUsage` is a
 * gauge (the last request's prompt size) and cannot express what one turn
 * cost. Absent whenever the runtime reported nothing (never a zero).
 */
export interface TurnUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

export interface ConversationDeleteResult {
  conversation_id: string;
  deleted: boolean;
}
export type ConversationMessageRole = "user" | "assistant" | "activity";
export interface ConversationMessage {
  message_id: string;
  conversation_id: string;
  role: ConversationMessageRole;
  content: unknown;
  message_type: string;
  status?: string | null;
  created_at: number;
}
export interface ConversationSendReceipt {
  conversation_id: string;
  message_id: string;
  turn_id?: string | null;
  accepted: boolean;
  replayed: boolean;
  completed: boolean;
  result_ok?: boolean | null;
  result_text?: string | null;
  result_error?: string | null;
  result_error_code?: string | null;
  result_error_retryable?: boolean | null;
}
/**
 * The closed set of conversation event kinds the server emits (`05` §4).
 *
 * Deliberately **not** widened with `| string` (doc `16` R1): the escape made
 * the union collapse to `string`, so exhaustive switches and type guards were
 * impossible. A newer server kind reaches a client as a runtime value the
 * switch does not match — which is handled by falling through — and widening
 * this union is the deliberate, reviewable act that changes the contract.
 */
export type ConversationEventType =
  | "message.created"
  | "message.delta"
  | "message.thinking"
  | "message.tips"
  | "message.tool"
  | "message.error"
  | "message.activity"
  | "turn.status"
  | "context.usage";

export interface ConversationEvent {
  conversation_id: string;
  sequence: number;
  event_type: ConversationEventType;
  payload: Record<string, unknown>;
}
export interface ConversationMessagesQuery { conversationId: string; page?: number; pageSize?: number; cursor?: string }
export interface ConversationMessagesPage {
  items: ConversationMessage[];
  /** Exact server-computed flag: `true` when an older page still exists. */
  has_more: boolean;
}
export interface ConversationSubscription { conversation_id: string }

// ---------------------------------------------------------------------------
// Agent Store Skill / Connector catalog (docs/agent-store/01-domain-model.md)
// ---------------------------------------------------------------------------

export type CompatibilityStatus =
  | "compatible"
  | "compatible-with-adapter"
  | "manual-review"
  | "unsupported"
  | "pending-legal-review";

export interface SkillSummary {
  id: string;
  name: string;
  description?: string | null;
  version: string;
  source: string;
  /**
   * Finer on-disk owner than `source` (`16` R17 / W12): `user` | `shared` |
   * `companion` | `draft` | `marketplace` | `builtin` | `unmanaged`.
   *
   * `source` alone cannot separate a user skill from an installed marketplace
   * product — both are `custom`.
   *
   * Optional so a client compiled against this package keeps working against a
   * host that predates the field; treat a missing value as "unknown owner".
   */
  origin?: SkillOrigin;
  /**
   * Whether the store's write face accepts `skill/update` / `skill/delete` for
   * this skill — `origin === "user"` and the directory is the canonical one.
   * Absent means unknown, so a UI must not offer a write action on `!== true`.
   */
  writable?: boolean;
  compatibility_status: CompatibilityStatus;
  enabled: boolean;
  required_connectors: string[];
  /**
   * Marketplace icon URL for a product installed from a marketplace
   * (`/api/app-server/store/{mkt}/entries/{entry}/assets/…`). Absent for
   * builtin / user skills and for markets that ship no icon for the entry.
   */
  avatar_url?: string | null;
}

/** Where a skill lives on the host (`SkillSummary.origin`). */
export type SkillOrigin =
  | "user"
  | "shared"
  | "companion"
  | "draft"
  | "marketplace"
  | "builtin"
  | "unmanaged";

/** `skill/get` — the raw SKILL.md body and internal routing rules are never returned. */
export interface SkillDetail extends SkillSummary {
  mode: string;
  invocation_policy: string;
  instructions_summary?: string | null;
}

/**
 * One file inside a Skill directory (`skill/files`, doc 24 §4).
 *
 * A Skill is a *directory*, not a single document: `SKILL.md` plus whatever it
 * ships alongside (`references/`, `scripts/`, `templates/`, `assets/`).
 * `SkillDetail.instructions_summary` is a bounded summary of the manifest, so
 * this is the read face for everything else.
 */
export interface SkillFile {
  /** Path relative to the skill directory, POSIX-separated. */
  path: string;
  size: number;
  /** Single-file sha256, lowercase hex. */
  digest: string;
}

/** `skill/files` response: the readable inventory of one Skill. */
export interface SkillFileList {
  skill_id: string;
  /** Sorted by `path`; directories are not listed. */
  files: SkillFile[];
  /**
   * Tree digest of **this skill directory** (sorted relative paths + per-file
   * sha256).
   *
   * Deliberately **not** the snapshot's `content_digest`, which covers the
   * whole imported source tree — the two coincide only when the snapshot holds
   * exactly this directory. Do not compare it against `import/get`.
   */
  content_digest: string;
  /** The inventory hit its ceiling and is incomplete. Never silently truncated. */
  truncated: boolean;
}

/**
 * `skill/file` over the WebSocket binding: base64, because JSON has no byte
 * string. The HTTP binding returns the raw body with a `content-type` header
 * instead, so prefer `skills.readFile()` (HTTP) for bytes and this shape only
 * when you are already on a socket.
 */
export interface SkillFileContent {
  skill_id: string;
  path: string;
  content_type: string;
  encoding: "base64";
  content: string;
}

// ---------------------------------------------------------------------------
// Host-management faces (docs/agent-store/16 R16 / R17, 05 §4.11)
// ---------------------------------------------------------------------------

/**
 * Request/response shapes for the six **host-management** methods:
 * `config/get` · `config/set` · `skill/create` · `skill/update` ·
 * `skill/delete` · `skill/copy`.
 *
 * They live here because this package is the wire contract's single source of
 * truth: a method that exists on the wire has a shape, and a caller that speaks
 * the protocol must be able to type-check it whether or not the curated
 * `@flowy-agent-store/client` surface offers a typed method for it. Declaring
 * them only in the host app made that claim false.
 *
 * **Not a stability promise.** These are the most volatile methods in the
 * protocol — they track the host's own settings file and its skills directory,
 * and are the ones most likely to change without notice during beta
 * (`16` D10=A: no backward-compatibility promise before release). The client
 * package deliberately ships no typed method for them; call them through
 * `transport.request` if you need them, and expect churn. What is *not* the
 * reason is access control: `transport` is public and the server enforces
 * writability from the on-disk origin, so this is a discoverability boundary,
 * not a security one.
 */

/**
 * `config/get` / `config/set` view of the host's `~/.agent-store/config.toml`.
 *
 * Never carries a credential: the file's `[providers.<name>]` tables hold
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
  /**
   * `~/.agent-store/mcp.json` as the host read it (`20` §7.9 / `21` D14);
   * `null` when there is no readable declaration file. Additive field, and the
   * **only** read surface for a declared server: declarations never become
   * `mcp_servers` rows, so they do not appear in `connector/*`.
   */
  mcp: AgentStoreConfigMcp | null;
}

/** One `mcpServers` entry the host accepted. */
export interface AgentStoreConfigMcpServer {
  /** The `mcpServers` key — the `<server>` segment of `mcp__<server>__*`. */
  name: string;
  /** `stdio` | `http` | `sse` (already a label, not a discriminant to branch on). */
  transport: string;
  /** `enabled = false` keeps the entry declared but out of every session. */
  enabled: boolean;
}

/** One `mcpServers` entry the host refused, and why. */
export interface AgentStoreConfigMcpRejection {
  name: string;
  /**
   * The parser's own reason ("a field on the wrong transport", "an out-of-range
   * timeout", …). Server-authored prose, not an i18n key.
   */
  reason: string;
}

/**
 * The `mcp.json` projection. A refusal is reported rather than silently
 * dropped: an ignored `enabledTools` would leave tools the user believes
 * excluded still callable, which is why this half has to be visible.
 */
export interface AgentStoreConfigMcp {
  /**
   * The declaration file is present. The host only sends this view after it has
   * read the file, so this is `true` whenever `mcp` is not `null`; it is kept
   * because the wire carries it, not because a branch should read it.
   */
  exists: boolean;
  /**
   * Whether **this host** feeds the file into agent sessions, as the launcher
   * reported at startup. Omitted when the host did not say — `undefined` is
   * "cannot tell", which is not the same answer as `false` ("this host does not
   * read the file"). `servers` describes the file; this describes the host.
   */
  adopted?: boolean;
  /** Accepted entries, ordered by server key. Never a credential value. */
  servers: AgentStoreConfigMcpServer[];
  /** Refused entries, with the reason the user has to fix. */
  rejected: AgentStoreConfigMcpRejection[];
  /**
   * Why the **whole file** could not be read as declarations (invalid JSON,
   * wrong top level). Omitted when the file parsed — without it a broken file
   * and an empty one look the same.
   */
  error?: string;
}

/**
 * `config/get-mcp`: the declaration file's own text, for the file editor.
 *
 * The only read that returns a declaration's values, and the reason it is a
 * method of its own: `AgentStoreConfigMcp` describes the file's *verdict* and
 * carries no `env` / `headers` value, while an editor cannot edit what it
 * cannot see. `exists: false` (with `source` absent) is the normal answer for a
 * host that never created the file; the text comes back whether or not it
 * parses, because editing a broken file is the point.
 */
export interface McpSourceView {
  exists: boolean;
  source?: string | null;
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
  revealed_origin?: SkillOrigin | null;
}

export type ConnectorStatus =
  | "installed"
  | "configured"
  | "authorization_required"
  | "authenticated"
  | "connected"
  | "degraded"
  | "error"
  | "reauthorization_required";

export interface ConnectorSummary {
  id: string;
  name: string;
  description?: string | null;
  kind: string;
  transport_summary: string;
  auth_mode: string;
  enabled: boolean;
  status: ConnectorStatus;
  /**
   * Marketplace icon URL for a connector installed from a marketplace.
   * Absent for builtin hosts and for markets that ship no icon for the entry.
   */
  avatar_url?: string | null;
}

export interface ConnectorTool {
  name: string;
  description?: string | null;
  /**
   * The upstream `tools/list` `inputSchema`, **verbatim** (doc `26` §5).
   *
   * Absent when the server published none, or when it was omitted to stay
   * inside the host's tools budget — see `tools_truncated` on the response. It
   * carries no transport, header or env value: only the tool's own parameters.
   */
  input_schema?: unknown;
}

export interface OAuthStatusView {
  state: "authenticated" | "not_authenticated" | "reauthorization_required" | string;
  error?: string | null;
}

/** `connector/get` — tools are namespaced public names only. */
export interface ConnectorDetail extends ConnectorSummary {
  tool_filter?: string | null;
  tools: ConnectorTool[];
  /**
   * Some `input_schema` values were omitted to stay inside the host's tools
   * budget. Names and descriptions are always kept, and a schema is only ever
   * carried whole — never truncated. `false` also means "nothing was omitted".
   */
  tools_truncated: boolean;
  auth_status?: OAuthStatusView | null;
  source: string;
  compatibility_status: CompatibilityStatus;
}

export interface ConnectorStatusView {
  connector_id: string;
  status: ConnectorStatus;
  auth_status?: OAuthStatusView | null;
  last_error?: string | null;
}

export interface ConnectorProbeResult {
  connector_id: string;
  success: boolean;
  tools?: ConnectorTool[] | null;
  /** Same rule as `ConnectorDetail.tools_truncated`: an honest omission. */
  tools_truncated: boolean;
  error?: string | null;
  code?: string | null;
}

export interface OAuthStartResult {
  connector_id: string;
  state: "started" | "error" | string;
  error?: string | null;
}

/**
 * `connector/call` (doc 24 §5.2): the result of running one MCP tool on the
 * host's connection.
 *
 * **A tool-level failure is a result, not a rejection.** When the server
 * answers `isError: true` the promise still resolves, with `is_error` set; only
 * transport, protocol and budget failures reject. Branch on `is_error` to tell
 * "the tool said no" from "we never reached the tool".
 */
export interface ConnectorCallResult {
  /** The upstream `isError` flag (`false` when the server omitted it). */
  is_error: boolean;
  /**
   * The upstream `tools/call` result object, **verbatim** — `content`,
   * `structuredContent` and anything a newer server adds all survive.
   *
   * It carries no transport, header or env value: the connection and its
   * credentials stay on the host. That is the whole point of a proxy — there is
   * deliberately no `connector/export`.
   */
  result: unknown;
}

export interface ConnectorQueryParams {
  connector_id: string;
}

export interface SkillQueryParams {
  skill_id: string;
}

// ---------------------------------------------------------------------------
// Agent Store Importer / PluginSnapshot (docs/agent-store/02, roadmap Phase 1)
// ---------------------------------------------------------------------------

export type CompatibilityTriple = {
  /** Wire values: compatible | compatible_with_adapter | manual_review | unsupported | pending_legal_review. */
  semantic_status: string;
  /** not-verified | adapter-verified | runtime-verified | release-eligible. */
  runtime_status: string;
  /** local-only (V1). */
  distribution_status: string;
  reasons: string[];
};

export type ImportSourceKind = "codebuddy-plugin" | "workbuddy-skill-market" | "workbuddy-connector-market" | "workbuddy-cli-connector";

export interface ImportRequest {
  /** Absolute local directory path on the trusted host; validated server-side. */
  source_path: string;
  source_kind: ImportSourceKind;
}

export type ImportStatus = "completed" | "completed-with-warnings" | "blocked" | "failed";

export interface ImportResult {
  snapshot_id: string;
  name: string;
  version: string;
  source_kind: string;
  status: ImportStatus;
  content_digest: string;
  component_status: CompatibilityTriple;
  component_count: number;
  imported_at: number;
  /** true when an identical digest was already imported (idempotent reuse). */
  reused: boolean;
  warnings: string[];
  errors: string[];
}

export interface ImportSummary {
  snapshot_id: string;
  name: string;
  version: string;
  source_kind: string;
  status: ImportStatus;
  component_count: number;
  imported_at: number;
}

export interface ImportComponent {
  id: string;
  kind: string;
  name: string;
  compatibility: CompatibilityTriple;
  warnings: string[];
}

export interface ImportDetail extends ImportSummary {
  content_digest: string;
  component_status: CompatibilityTriple;
  warnings: string[];
  errors: string[];
  components: ImportComponent[];
}

// ---------------------------------------------------------------------------
// Installer / runtime registration (docs/agent-store/05 §4.5, roadmap Phase 2)
// ---------------------------------------------------------------------------

export interface InstallRequest {
  snapshot_id: string;
}

export type InstallState = "not-installed" | "installed" | "disabled";

export interface InstallComponent {
  id: string;
  kind: string;
  name: string;
  state: InstallState;
  runtime_location?: string | null;
  preset_id?: string | null;
}

export interface InstallResult {
  snapshot_id: string;
  name: string;
  version: string;
  installed_count: number;
  skipped: string[];
  warnings: string[];
  errors: string[];
  /** Per-component outcome. Absent on a host that predates the field — read a
   *  missing value as "no detail available", never as "nothing ran". */
  outcomes?: InstallOutcome[];
}

export interface InstallStatus {
  snapshot_id: string;
  components: InstallComponent[];
  /** Per-component outcome of the mutation that produced this projection
   *  (`install/uninstall` · `install/disable` · `install/enable`). Empty for a
   *  plain `install/status` read. */
  outcomes?: InstallOutcome[];
  /** Human-readable failures; mirrors the `ok: false` outcomes. */
  errors?: string[];
}

/** What happened to one component during install / uninstall / enable / disable. */
export type InstallOutcomeAction =
  | "created"
  | "reused"
  | "enabled"
  | "disabled"
  /** The flag moved but the runtime did not: the documented `skill` case, since
   *  the skill corpus has no enable state (docs `05` §4.5). */
  | "marked"
  | "removed"
  | "skipped"
  | "failed";

export interface InstallOutcome {
  component_id: string;
  kind: string;
  action: InstallOutcomeAction;
  /** `false` only when the requested state was **not** reached. A component
   *  that was already in the requested state reports `ok: true`. */
  ok: boolean;
  /**
   * Stable, documented token to branch on (`docs/agent-store/05` §4.5 lists the
   * closed set). `message` is for humans and nothing parses it — the field
   * exists so callers never have to match on prose.
   */
  code?: string;
  message?: string;
}

// ---------------------------------------------------------------------------
// Marketplace (docs/agent-store/05 §4.6, roadmap Phase 2)
// ---------------------------------------------------------------------------

/** Wire-marketplace source kinds (Phase A: directory only). */
export type MarketplaceSourceKind = "directory" | "github" | "git" | "url";

export interface MarketplaceAddRequest {
  name?: string;
  source_kind: MarketplaceSourceKind;
  /** Local directory path / owner/repo / Git URL / HTTP URL. */
  source: string;
}

export interface MarketplaceSummary {
  marketplace_id: string;
  name: string;
  description?: string | null;
  source_kind: string;
  version?: string | null;
  auto_update: boolean;
  enabled: boolean;
  entry_count: number;
  added_at: number;
  /** Last resolved source revision (git commit / freshness marker). */
  resolved_revision?: string | null;
  /** Last successful freshness check (epoch ms); absent before the first one. */
  last_checked_at?: number | null;
}

/**
 * Snapshot produced by importing this entry, plus how much of it is installed.
 * Projected server-side on `market/get` (doc 16 D-W13-1 ①) so a client never
 * has to re-derive "what would a cascade removal take out?" from the store
 * listing.
 */
export interface MarketplaceEntrySnapshot {
  snapshot_id: string;
  name: string;
  version: string;
  status: string;
  component_count: number;
  /** Components installed from this snapshot; `0` = imported but untouched. */
  installed_count: number;
  imported_at: number;
}

export interface MarketplaceEntry {
  name: string;
  source_kind: string;
  source: string;
  version?: string | null;
  description?: string | null;
  keywords: string[];
  category?: string | null;
  /**
   * Localized `<field>_<lang>` variants carried verbatim by the market
   * manifest (`description_zh`, `name_en`, `tags_zh`, `legacy_tags_en`, …).
   * Resolve with `pickEntryText` / `pickEntryTags` (doc 18 §4, D8=A);
   * absent when the manifest declared none.
   */
  localized?: Record<string, string | string[]> | null;
  /**
   * The entry's declared `strict` (doc `02` §8): `true` requires the plugin
   * source to carry its own `.codebuddy-plugin/plugin.json`. Absent = `false`.
   */
  strict?: boolean | null;
  /**
   * Why this entry cannot be imported (doc `02` §11.1), when discovery can
   * already tell. Present = listed for transparency, but not installable.
   */
  blocked_reason?: string | null;
  /** Absent until the entry has been imported from this marketplace. */
  snapshot?: MarketplaceEntrySnapshot | null;
}

export interface MarketplaceDetail extends MarketplaceSummary {
  entries: MarketplaceEntry[];
}

export interface MarketplaceRemoveResult {
  marketplace_id: string;
  /** Snapshot ids installed from this marketplace, uninstalled by the cascade. */
  snapshots: string[];
  uninstalled_components: string[];
  warnings: string[];
}

export interface MarketplaceRefreshResult {
  marketplace_id: string;
  /** true when the remote revision changed and the projection was rebuilt. */
  changed: boolean;
  resolved_revision: string;
  entry_count: number;
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Store — winget-style unified catalog over all enabled marketplaces
// ---------------------------------------------------------------------------

export type StoreItemKind = "agent" | "team" | "skill" | "connector";

/** One store item: a marketplace entry with display fidelity + install state. */
export interface StoreItem {
  /** Stable composite id: `<marketplace_id>/<entry_name>`. */
  id: string;
  marketplace_id: string;
  marketplace_name: string;
  entry_name: string;
  kind: StoreItemKind;
  /** Display name: `displayName` (localized) when present, else entry name. */
  name: string;
  display_name?: LocalizedText | null;
  profession?: LocalizedText | null;
  description?: string | null;
  display_description?: LocalizedText | null;
  /** Empty arrays are omitted on the wire (`skip_serializing_if`).
   * Treat as optional: `item.tags ?? []`. */
  tags?: LocalizedText[] | null;
  quick_prompts?: LocalizedText[] | null;
  /**
   * The market entry's own `publishedAt`, `YYYY-MM-DD` (`18` §3).
   *
   * A calendar date the *market* declared, never a derived one: absent means
   * "this market declares no date", so a client must not substitute the import
   * time or render a placeholder. The host normalizes it on the way in and
   * drops anything that is not a calendar date.
   */
  published_at?: string | null;
  /** Public avatar URL (store asset endpoint); relative to the API host. */
  avatar_url?: string | null;
  version: string;
  source_kind: string;
  installed: boolean;
  update_available: boolean;
  snapshot_id?: string | null;
  installed_version?: string | null;
  /**
   * Why this item cannot be installed (doc `02` §11.1), re-derived from the
   * live source tree. Present = the item stays listed so the reason can be
   * read, but install must not be offered.
   */
  blocked_reason?: string | null;
}

export interface StoreList {
  items: StoreItem[];
  /** `true` while the builtin default marketplaces are still registering in the
   *  background (D-SDK-1 ①) — the catalog may be incomplete. */
  markets_pending?: boolean;
}

/** One model in the public catalog (`models/list`, REQ-PAR-05b). Provider
 *  credentials, base URLs and health internals never cross this projection. */
export interface ModelSummary {
  provider_id: string;
  provider_name: string;
  model: string;
  /** Provider-supplied display label when configured. */
  display_name?: string | null;
  /** True when an unbound preset resolves to this provider/model. */
  is_default: boolean;
}

/** `models/list` response: the public model directory over enabled providers. */
export interface ModelList {
  items: ModelSummary[];
}

export interface StoreInstallResult {
  marketplace_id: string;
  entry_name: string;
  snapshot_id: string;
  version: string;
  /** true when the components were already registered (no-op install). */
  reused: boolean;
  installed_count: number;
  warnings: string[];
  errors: string[];
  /** Per-component outcome, forwarded from the installer's own report so a
   *  store install is as branchable as a direct `install/run`. Absent on a host
   *  that predates the field. */
  outcomes?: InstallOutcome[];
}

// ---------------------------------------------------------------------------
// Agent / Team catalog (docs/agent-store/05 §4.1 / §4.2)
// ---------------------------------------------------------------------------

export interface LocalizedText {
  en?: string | null;
  zh?: string | null;
}

export interface AgentSummary {
  id: string;
  version: string;
  name: string;
  /** Installed preset id; present after `install/*` of the snapshot (Phase 2). */
  preset_id?: string | null;
  description?: string | null;
  skills: string[];
  connectors: string[];
  model_summary?: string | null;
  tool_policy_summary?: string | null;
  source: string;
  compatibility_status: CompatibilityStatus;
  display_name?: LocalizedText | null;
  profession?: LocalizedText | null;
  /** Public avatar URL (server-asset endpoint); relative to the API host. */
  avatar_url?: string | null;
}

/** `agent/get` — structured frontmatter fields only; raw prompts never cross. */
export interface AgentDetail extends AgentSummary {
  effort?: string | null;
  max_turns?: number | null;
  disallowed_tools: string[];
  memory?: string | null;
  background?: string | null;
  isolation?: string | null;
  permission_mode_ignored: boolean;
  display_description?: LocalizedText | null;
  quick_prompts?: LocalizedText[];
  tags?: LocalizedText[];
  default_init_prompt?: LocalizedText | null;
  expert_type?: string | null;
  category_id?: string | null;
}

export interface TeamSummary {
  id: string;
  version: string;
  name: string;
  description?: string | null;
  lead_agent_id: string;
  member_agent_ids: string[];
  source: string;
  compatibility_status: CompatibilityStatus;
}

export interface TeamDetail extends TeamSummary {
  planner_policy: string;
  routing_constraints: string[];
  workflow_limits: Record<string, unknown>;
  team_runtime_capabilities: string[];
  /**
   * MCP server ids this Team's own snapshot installed **and** left enabled on
   * this host — the only Connector surface a Team Run may bind. Member Agents'
   * `mcpServers` are recorded, not mapped to grants (docs/agent-store/02 §5.1).
   */
  connectors: string[];
}
