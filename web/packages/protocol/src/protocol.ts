/**
 * Agent Store App Server Protocol types.
 *
 * The chat surface uses only opaque public conversation/message IDs. Internal
 * execution/session/step/attempt IDs never appear in this module.
 */

export const APP_SERVER_PROTOCOL_VERSION = "2026-08-26";

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

export type ServerNotification =
  | JsonRpcEventNotification
  | ResyncRequiredNotification
  | ConversationEventNotification
  | ConversationResyncRequiredNotification;

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
  connectors: boolean;
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
}

export interface RunReceipt {
  run_id: string;
  status: RunStatus;
  version: number;
  preset_revision: number;
  content_digest: string;
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
export interface ConversationCreateInput { name?: string; model?: ProviderWithModel; workspaceId?: string; reasoningEffort?: string }
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
}

export interface ConnectorTool {
  name: string;
  description?: string | null;
}

export interface OAuthStatusView {
  state: "authenticated" | "not_authenticated" | "reauthorization_required" | string;
  error?: string | null;
}

/** `connector/get` — tools are namespaced public names only. */
export interface ConnectorDetail extends ConnectorSummary {
  tool_filter?: string | null;
  tools: ConnectorTool[];
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
  error?: string | null;
  code?: string | null;
}

export interface OAuthStartResult {
  connector_id: string;
  state: "started" | "error" | string;
  error?: string | null;
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
}

export interface InstallStatus {
  snapshot_id: string;
  components: InstallComponent[];
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
  /** Public avatar URL (store asset endpoint); relative to the API host. */
  avatar_url?: string | null;
  version: string;
  source_kind: string;
  installed: boolean;
  update_available: boolean;
  snapshot_id?: string | null;
  installed_version?: string | null;
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
}
