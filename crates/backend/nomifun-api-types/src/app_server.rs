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

use nomifun_common::LocalizedVariant;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Finer on-disk owner than `source`: `user` | `shared` | `companion` |
    /// `draft` | `marketplace` | `builtin` | `unmanaged` (`16` R17 / W12).
    ///
    /// `source` alone cannot separate a user skill from an installed
    /// marketplace product — both are `custom`. This field is the fact the
    /// write face enforces, not a heuristic the UI has to re-derive.
    #[serde(default)]
    pub origin: String,
    /// `origin == "user"` — the single condition under which
    /// `skill/update` / `skill/delete` are accepted. Kept on the wire so a UI
    /// does not have to know the directory layout to decide whether to offer a
    /// write action. Always serialized (like `enabled`).
    #[serde(default)]
    pub writable: bool,
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

/// Result of `skill/delete` (`16` R17 / W12).
///
/// Deleting a user skill can *reveal* a skill that was shadowed by it: the
/// public id is the skill name, so removing a user skill that carried a
/// built-in's name makes the built-in addressable again. The server answers
/// with what is visible under the id **after** the delete instead of letting a
/// caller assume the id is gone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerSkillDeleteResult {
    pub skill_id: String,
    /// Always `true` on a successful response — the field exists so a caller
    /// never has to treat "request sent" as "skill deleted".
    pub deleted: bool,
    /// `origin` of the skill now visible under `skill_id` (e.g. `builtin`), or
    /// `null` when nothing resolves there any more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_origin: Option<String>,
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
    /// A single MCP connector directory (`connectors/<id>/` with `mcp.json`
    /// `mcpServers` + `skills/`), e.g. agent-earth / datayes-data.
    #[serde(rename = "workbuddy-mcp-connector")]
    WorkBuddyMcpConnector,
}

impl AppServerImportSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeBuddyPlugin => "codebuddy-plugin",
            Self::WorkBuddySkillMarket => "workbuddy-skill-market",
            Self::WorkBuddyConnectorMarket => "workbuddy-connector-market",
            Self::WorkBuddyCliConnector => "workbuddy-cli-connector",
            Self::WorkBuddyMcpConnector => "workbuddy-mcp-connector",
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
    /// Preset created by the installer for this agent definition (`install/*`
    /// of roadmap Phase 2). `None` until the snapshot was installed; only a
    /// preset-backed definition can be started through `agent/run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
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
    /// Localized display metadata preserved from `plugin.json`/frontmatter.
    /// `None` when the source did not carry the field (02 §5.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profession: Option<AppServerLocalizedText>,
    /// Public avatar URL (`/api/app-server/imports/{snapshot}/assets/{path}`).
    /// `None` when the source did not declare an avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// A localized display value (`{en, zh}` subset; both optional).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppServerLocalizedText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub en: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zh: Option<String>,
}

/// Agent Store AgentDefinition summary. Never a Runtime Agent instance; never
/// carries credentials, hidden system instructions or raw prompt bodies.
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
    /// Localized market description (`displayDescription`). Never the raw
    /// prompt body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_description: Option<AppServerLocalizedText>,
    /// `quickPrompts`: the "专家帮你做" entries shown on the expert card.
    #[serde(default)]
    pub quick_prompts: Vec<AppServerLocalizedText>,
    #[serde(default)]
    pub tags: Vec<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_init_prompt: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expert_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
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
    /// Last resolved source revision (git commit / HTTP freshness marker).
    /// Internal traceability, never a public identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_revision: Option<String>,
    /// Last successful freshness check (epoch ms); `None` before the first one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
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
    /// Localized `<field>_<lang>` variants from the market manifest
    /// (`description_zh` / `description_en`, `name_*`, `category_*`,
    /// `tags_*`, `legacy_tags_*`, `examples_*`), transported verbatim.
    ///
    /// The fallback chain `{field}_{lang}` → `{field}` is resolved by the
    /// reader, since only the client knows its UI language (doc `18` §4,
    /// decision D8=A). `tags_{lang}` outranks `legacy_tags_{lang}` — that
    /// ordering is part of the same decision and is applied client-side.
    /// Absent when the manifest declared no variants, so the field is purely
    /// additive on the wire.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub localized: BTreeMap<String, LocalizedVariant>,
    /// `strict` as declared by the **market entry** (`02` §8): `true` requires
    /// the plugin source to carry its own `.codebuddy-plugin/plugin.json`.
    /// Absent means `false` — the pre-existing behaviour.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
    /// Why this entry cannot be imported (a `02` §11.1 blocking rule), when
    /// discovery can already tell. Absent = importable as far as discovery
    /// knows. An entry carrying one stays listed (an invisible entry cannot
    /// explain itself) but is not installable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// Snapshot produced by importing this entry (`market/entry-import`), with
    /// its install tally. Absent until the entry has been imported — the same
    /// condition a cascade removal reports as "nothing to uninstall here"
    /// (doc 16 D-W13-1 ①).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<AppServerMarketplaceEntrySnapshot>,
}

/// Snapshot provenance on an entry (what import/install produced).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMarketplaceEntrySnapshot {
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    pub status: String,
    pub component_count: usize,
    /// Components installed from this snapshot. `0` = imported but never
    /// installed, so a cascade removal leaves this entry untouched.
    #[serde(default)]
    pub installed_count: usize,
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

// ---------------------------------------------------------------------------
// Store (winget-style aggregated catalog over all enabled marketplaces)
// ---------------------------------------------------------------------------

/// One store item: a marketplace entry projected into the unified store
/// catalog with its display metadata (plugin.json fidelity) plus the current
/// local installation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerStoreItem {
    /// Stable composite id: `<marketplace_id>/<entry_name>`.
    pub id: String,
    pub marketplace_id: String,
    pub marketplace_name: String,
    pub entry_name: String,
    /// `agent` | `team` | `skill` | `connector` (entry kind derived from the
    /// entry's manifest shape).
    pub kind: String,
    /// Display name: `displayName` (localized) when present, else entry name.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profession: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_description: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quick_prompts: Vec<AppServerLocalizedText>,
    /// Public avatar URL (store asset endpoint); relative to the API host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// Available version from the marketplace entry / plugin manifest.
    pub version: String,
    /// Entry source kind (`directory` / `external` …).
    pub source_kind: String,
    /// `true` when a snapshot from this entry exists and at least one of its
    /// components is installed into the runtime.
    pub installed: bool,
    /// `true` when the entry's available version differs from the installed
    /// snapshot version.
    pub update_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// Why this item cannot be installed (a `02` §11.1 blocking rule), when the
    /// projection can tell from the live source tree. Absent = installable.
    /// The store still lists it (so the reason can be read) but the UI must not
    /// offer install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// `store/list` response: the unified catalog over all enabled marketplaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerStoreList {
    pub items: Vec<AppServerStoreItem>,
    /// `true` while the builtin default marketplaces are still registering in
    /// the background (D-SDK-1 ①): the catalog may be incomplete. Omitted when
    /// `false` so the addition stays wire-compatible.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub markets_pending: bool,
}

/// `market install-entry` result: import (when missing) + runtime
/// registration in one idempotent call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerStoreInstallResult {
    pub marketplace_id: String,
    pub entry_name: String,
    pub snapshot_id: String,
    pub version: String,
    /// `true` when the components were already registered (no-op install).
    pub reused: bool,
    pub installed_count: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// One model in the public catalog (`models/list`). The projection carries
/// only provider identity and model names — credentials, endpoints and health
/// internals never cross this seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerModelSummary {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    /// Provider-supplied display label when configured (`model_descriptions`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// `true` when an unbound preset resolves to this provider/model
    /// (mirrors the `agent/run` default-model fallback).
    #[serde(default)]
    pub is_default: bool,
}

/// `models/list` response: the public model directory over enabled providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerModelList {
    pub items: Vec<AppServerModelSummary>,
}

/// One provider as **declared in `~/.agent-store/config.toml`** (`config/get`).
///
/// The same facts the config-only projection of `models/list` uses, and nothing
/// else: no `api_key`, no `base_url`, no registered-Provider row. Credentials
/// have no field here, so they cannot leak through a read of this shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigProviderView {
    /// `[providers.<name>]` key — the left half of a `default_model`.
    pub name: String,
    /// `enabled = false` in the file (`true` when the key is absent).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Model names declared under `[models."<name>/<model>"]`, sorted.
    #[serde(default)]
    pub models: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// `config/get` / `config/set` response: the host's agent-store settings file
/// as a **read-back of what is actually on disk**.
///
/// The view is the write path's landing spot: `config/set` returns this shape
/// re-read after the write, so a client can never mistake "request sent" for
/// "value stored". `exists = false` with empty defaults is a normal answer on a
/// host that has not created the file yet (never an error); an unreadable or
/// unparseable file is an error rather than a fabricated default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigView {
    /// `~/.agent-store/config.toml` present on this host.
    pub exists: bool,
    /// Declared `default_model`, canonicalised. **Absent is reported as
    /// explicit `null`**, not omitted: the client distinguishes "no default
    /// declared" from "not loaded yet".
    pub default_model: Option<String>,
    /// Providers declared in the file, sorted by name.
    #[serde(default)]
    pub providers: Vec<AppServerConfigProviderView>,
    /// `[memory]` — absent when the file declares no memory table at all, so a
    /// client can tell "not configured" from "explicitly off" (`distill_enabled
    /// = false`). Additive: hosts that never wrote the table keep answering
    /// exactly what they answered before.
    #[serde(default)]
    pub memory: Option<AppServerConfigMemoryView>,
}

/// `[memory]` in the settings file, as far as the wire needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigMemoryView {
    /// `None` when the key is absent (upstream default: distillation ON).
    #[serde(default)]
    pub distill_enabled: Option<bool>,
}