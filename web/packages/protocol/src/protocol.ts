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
export interface RunEvent { run_id: string; sequence: number; event_type: string; payload: Record<string, unknown> }
export interface RunEventsQuery { runId: string; afterSequence?: number; limit?: number }
export interface RunSubscriptionParams { run_id: string }

export interface ProviderWithModel { provider_id: string; model: string; use_model?: string }
export interface ConversationCreateInput { name?: string; model?: ProviderWithModel; workspaceId?: string; reasoningEffort?: string }
export interface ConversationUpdateInput { name?: string; model?: ProviderWithModel; reasoningEffort?: string }
export type ReasoningEffort = "low" | "medium" | "high" | "xhigh" | "max";
export interface ConversationModelOption { name: string; display_name?: string | null; context_limit?: number | null }
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
export interface ConversationEvent {
  conversation_id: string;
  sequence: number;
  event_type: "message.created" | "message.delta" | "message.thinking" | "message.tips" | "message.tool" | "message.error" | "message.activity" | "turn.status" | "context.usage" | string;
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
  compatibility_status: CompatibilityStatus;
  enabled: boolean;
  required_connectors: string[];
}

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
}

export interface MarketplaceEntry {
  name: string;
  source_kind: string;
  source: string;
  version?: string | null;
  description?: string | null;
  keywords: string[];
  category?: string | null;
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
