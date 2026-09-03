//! Agent Store App Server public contract types — Skill / Connector catalog.
//!
//! These are the *public* Agent Store shapes consumed over the versioned App
//! Server Protocol (`docs/agent-store/05-allo-app-server-protocol.md`), mapped
//! from the system Skill / MCP services. They intentionally carry no internal
//! Allo IDs, credentials, filesystem paths or provider-private fields.
//!
//! Normalization notes:
//! - `source` / `compatibility_status` are **derived** until the Agent Store
//!   Importer/PluginSnapshot layer lands (roadmap Phase 1): builtin skills map
//!   to `compatible`, custom/extension content to `compatible-with-adapter`.
//! - Connector status follows `docs/agent-store/08-flowy-web-integration.md`
//!   §5.1: `connected` is only reported when auth is ready AND the last probe
//!   succeeded.

use serde::{Deserialize, Serialize};

/// Agent Store compatibility status (`10-public-contracts.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerCompatibilityStatus {
    Compatible,
    CompatibleWithAdapter,
    ManualReview,
    Unsupported,
    PendingLegalReview,
}

/// Public Skill summary (`01-domain-model.md` §6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerSkillSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Version label; derived (`builtin` / `custom` / `extension`) until the
    /// Importer provides real PluginSnapshot versions.
    pub version: String,
    pub source: String,
    pub compatibility_status: AppServerCompatibilityStatus,
    pub enabled: bool,
    /// Always serialized (empty when no connectors are required) — consumers
    /// treat this as a stable list, never an optional field.
    #[serde(default)]
    pub required_connectors: Vec<String>,
}

/// Public Skill detail: summary fields plus execution metadata. The raw
/// `SKILL.md` body and internal routing rules are never returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerSkillDetail {
    #[serde(flatten)]
    pub summary: AppServerSkillSummary,
    /// `client-instructions` | `store-agent` | `store-workflow` (derived).
    pub mode: String,
    /// `@invoke` | `/invoke` | `model-auto` (derived).
    pub invocation_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_summary: Option<String>,
}

/// Agent Store Connector status (`08-flowy-web-integration.md` §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerConnectorStatus {
    Installed,
    Configured,
    AuthorizationRequired,
    Authenticated,
    Connected,
    Degraded,
    Error,
    ReauthorizationRequired,
}

impl AppServerConnectorStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Configured => "configured",
            Self::AuthorizationRequired => "authorization_required",
            Self::Authenticated => "authenticated",
            Self::Connected => "connected",
            Self::Degraded => "degraded",
            Self::Error => "error",
            Self::ReauthorizationRequired => "reauthorization_required",
        }
    }
}

/// One namespaced tool exposed by a Connector. Only the public (namespaced)
/// name is returned; upstream tool structures and schemas stay internal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Public Connector summary (`01-domain-model.md` §7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `remote-mcp` | `stdio-mcp` | `http-api` | `cli` | `composite` (derived).
    pub kind: String,
    /// Display-only transport summary (URL or launch command). Never a raw
    /// shell command the client may execute.
    pub transport_summary: String,
    /// `none` | `apikey` | `env` | `oauth` | `cli-login` (derived).
    pub auth_mode: String,
    pub enabled: bool,
    pub status: AppServerConnectorStatus,
}

/// Public Connector detail: summary fields plus tools and auth state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorDetail {
    #[serde(flatten)]
    pub summary: AppServerConnectorSummary,
    /// Public namespacing rule, e.g. `connector__<name>__<tool>` (derived).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_filter: Option<String>,
    /// Always serialized (empty before the first successful probe).
    #[serde(default)]
    pub tools: Vec<AppServerConnectorTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_status: Option<AppServerOAuthStatusView>,
    pub source: String,
    pub compatibility_status: AppServerCompatibilityStatus,
}

/// Full Connector status view combining auth state and the last probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorStatusView {
    pub connector_id: String,
    pub status: AppServerConnectorStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_status: Option<AppServerOAuthStatusView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Connector probe (`connector/test`) result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorProbeResult {
    pub connector_id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AppServerConnectorTool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Stable machine code (e.g. `MCP_CONNECTION_FAILED`); never raw internals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// OAuth state view. Never contains tokens, authorization codes or secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerOAuthStatusView {
    /// `authenticated` | `not_authenticated` | `reauthorization_required`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `connector/auth/start` result. The browser flow is owned by the trusted
/// host; clients only learn that the flow was started (and poll auth/status).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerOAuthStartResult {
    pub connector_id: String,
    /// `started` | `error`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Importer / PluginSnapshot (roadmap Phase 1)
// ---------------------------------------------------------------------------

/// V1 import source kinds (docs/agent-store/02 §1: local directories only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppServerImportSourceKind {
    /// `.codebuddy-plugin/plugin.json` plugin root.
    #[serde(rename = "codebuddy-plugin")]
    CodeBuddyPlugin,
    /// `.codebuddy-skill/marketplace.json` skill market directory.
    #[serde(rename = "workbuddy-skill-market")]
    WorkBuddySkillMarket,
    /// `.codebuddy-connector/connectors.json` connector market directory.
    #[serde(rename = "workbuddy-connector-market")]
    WorkBuddyConnectorMarket,
    /// A single CLI connector directory (`connectors/<id>/` with `cli.json`
    /// + `skills/`), e.g. wecom / feishu / tmeet.
    #[serde(rename = "workbuddy-cli-connector")]
    WorkBuddyCliConnector,
}

impl AppServerImportSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeBuddyPlugin => "codebuddy-plugin",
            Self::WorkBuddySkillMarket => "workbuddy-skill-market",
            Self::WorkBuddyConnectorMarket => "workbuddy-connector-market",
            Self::WorkBuddyCliConnector => "workbuddy-cli-connector",
        }
    }
}

/// `import/run` request. The source path is an operator-chosen local
/// directory on the trusted host; only relative/offending manifest entries are
/// echoed back, never the absolute source path (02 §9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerImportRequest {
    pub source_path: String,
    pub source_kind: AppServerImportSourceKind,
}

/// Three-dimensional compatibility report
/// (docs/agent-store/03 §1). `reasons` may carry reason codes such as
/// `ignored-by-source-runtime` (02 §11.2: reason codes are not statuses).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerCompatibilityTriple {
    /// `compatible` | `compatible_with_adapter` | `manual_review` |
    /// `unsupported` | `pending_legal_review` (serialized snake_case).
    pub semantic_status: String,
    /// `not-verified` | `adapter-verified` | `runtime-verified` |
    /// `release-eligible`.
    pub runtime_status: String,
    /// `local-only` (V1).
    pub distribution_status: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// Synchronous import result (docs/agent-store/02 §11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerImportResult {
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    pub source_kind: String,
    /// `completed` | `completed-with-warnings` | `blocked` | `failed`.
    pub status: String,
    pub content_digest: String,
    pub component_status: AppServerCompatibilityTriple,
    pub component_count: usize,
    pub imported_at: i64,
    /// `true` when an identical digest already existed and the immutable
    /// snapshot was reused (idempotent import).
    pub reused: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Import history row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerImportSummary {
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    pub source_kind: String,
    pub status: String,
    pub component_count: usize,
    pub imported_at: i64,
}

/// One standardized component produced by an import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerImportComponent {
    pub id: String,
    /// `agent` | `team` | `skill` | `connector` | `command` | `hook` |
    /// `lsp` | `credential` | `dependency` | `script`.
    pub kind: String,
    pub name: String,
    pub compatibility: AppServerCompatibilityTriple,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Full import detail (history + components).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerImportDetail {
    #[serde(flatten)]
    pub summary: AppServerImportSummary,
    pub content_digest: String,
    pub component_status: AppServerCompatibilityTriple,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub components: Vec<AppServerImportComponent>,
}

// ---------------------------------------------------------------------------
// Agent / Team catalog (docs/agent-store/05 §4.1 / §4.2)
// ---------------------------------------------------------------------------

/// Agent Store AgentDefinition summary. Never a Runtime Agent instance; never
/// carries credentials, hidden system instructions or raw prompt bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerAgentSummary {
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub connectors: Vec<String>,
    /// Derived from the frontmatter `model`/`effort` fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_summary: Option<String>,
    /// Derived from `tools`/`disallowedTools`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_policy_summary: Option<String>,
    pub source: String,
    pub compatibility_status: AppServerCompatibilityStatus,
}

/// AgentDefinition detail: summary plus the structured fields preserved from
/// `agents/*.md` frontmatter (02 §5.1). No raw prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerAgentDetail {
    #[serde(flatten)]
    pub summary: AppServerAgentSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Plugin-level Agents: source runtime ignores `mcpServers`/`permissionMode`
    /// (02 §5.1); importers record the fact instead of mapping it to grants.
    pub permission_mode_ignored: bool,
}

/// Agent Team definition summary (02 §6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerTeamSummary {
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `AgentDefinition` id of the planning role.
    pub lead_agent_id: String,
    pub member_agent_ids: Vec<String>,
    pub source: String,
    pub compatibility_status: AppServerCompatibilityStatus,
}

/// Team detail: summary plus team policy fields (01 §5). The planning
/// context body is never returned, only its capabilities list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerTeamDetail {
    #[serde(flatten)]
    pub summary: AppServerTeamSummary,
    pub planner_policy: String,
    #[serde(default)]
    pub routing_constraints: Vec<String>,
    #[serde(default)]
    pub workflow_limits: serde_json::Value,
    pub team_runtime_capabilities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Installer / runtime registration (roadmap Phase 2)
// ---------------------------------------------------------------------------

/// `install/run` request: which snapshot to install. The source kinds mirror
/// the import kinds — installation always acts on an existing snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallRequest {
    pub snapshot_id: String,
}

/// Installation state of one component (wire values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppServerInstallState {
    /// Never installed; only catalogued by an import.
    #[serde(rename = "not-installed")]
    NotInstalled,
    /// Installed and enabled (usable at runtime).
    #[serde(rename = "installed")]
    Installed,
    /// Installed but disabled (runtime artifacts remain, not usable).
    #[serde(rename = "disabled")]
    Disabled,
}

impl AppServerInstallState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not-installed",
            Self::Installed => "installed",
            Self::Disabled => "disabled",
        }
    }
}

/// Per-component installation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallComponent {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub state: AppServerInstallState,
    /// Runtime target summary: skill path / preset id / mcp server name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_location: Option<String>,
    /// Preset id created for agent/team components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
}

/// `install/run` result (one snapshot installation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallResult {
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    /// Number of components registered into the runtime.
    pub installed_count: usize,
    /// Components that could not be installed (skipped with a reason).
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Install state projection for one snapshot (or empty when not installed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallStatus {
    pub snapshot_id: String,
    pub components: Vec<AppServerInstallComponent>,
}

// ---------------------------------------------------------------------------
// Marketplace (roadmap Phase 2)
// ---------------------------------------------------------------------------

/// Marketplace source kinds (docs/agent-store/02 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppServerMarketplaceSourceKind {
    /// Local directory on the trusted host.
    #[serde(rename = "directory")]
    Directory,
    /// GitHub repository (`owner/repo`).
    #[serde(rename = "github")]
    Github,
    /// Any Git remote.
    #[serde(rename = "git")]
    Git,
    /// HTTP(S) `marketplace.json`.
    #[serde(rename = "url")]
    Url,
}

impl AppServerMarketplaceSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Github => "github",
            Self::Git => "git",
            Self::Url => "url",
        }
    }
}

/// `market/add` request. The raw source is resolved on the trusted host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceAddRequest {
    /// Optional stable name; derived from the source when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source_kind: AppServerMarketplaceSourceKind,
    /// Local directory path / `owner/repo` / Git URL / HTTP URL.
    pub source: String,
}

/// `market/list` row: registry projection, no entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceSummary {
    pub marketplace_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub auto_update: bool,
    pub enabled: bool,
    /// Entry count from the entries projection.
    pub entry_count: usize,
    pub added_at: i64,
}

/// One discovered entry (`market/get`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceEntry {
    pub name: String,
    pub source_kind: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Snapshot provenance on an entry (what import/install produced).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceEntrySnapshot {
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    pub status: String,
    pub component_count: usize,
    pub imported_at: i64,
}

/// `market/get` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceDetail {
    #[serde(flatten)]
    pub summary: AppServerMarketplaceSummary,
    #[serde(default)]
    pub entries: Vec<AppServerMarketplaceEntry>,
}

/// `market/remove` result: what was uninstalled by the cascade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceRemoveResult {
    pub marketplace_id: String,
    /// Snapshots that were installed from this marketplace.
    pub snapshots: Vec<String>,
    /// Component ids whose install state was cleared by cascade uninstall.
    pub uninstalled_components: Vec<String>,
    pub warnings: Vec<String>,
}

/// `market/refresh` result: one fetch + projection cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceRefreshResult {
    pub marketplace_id: String,
    /// `true` when the remote revision changed and the projection was rebuilt.
    pub changed: bool,
    /// Resolved revision for this fetch (git commit / freshness marker);
    /// internal traceability only, never a public identity.
    pub resolved_revision: String,
    pub entry_count: usize,
    pub warnings: Vec<String>,
}