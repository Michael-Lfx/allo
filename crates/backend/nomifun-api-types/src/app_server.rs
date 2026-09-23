//! Agent Store App Server public contract types — Skill / Connector catalog.
//!
//! These are the *public* Agent Store shapes consumed over the versioned App
//! Server Protocol (`docs/agent-store/05-flowy-agent-store-app-server-protocol.md`), mapped
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
    /// Marketplace icon URL (`/api/app-server/store/{mkt}/entries/{e}/assets/…`)
    /// for a product installed from a marketplace. `None` for builtin/user
    /// skills or markets that ship no icon for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
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

/// One file inside a skill directory (`skill/files`).
///
/// A Skill is a *directory*, not a single document: `SKILL.md` plus whatever
/// it ships alongside (`references/`, `scripts/`, `templates/`, `assets/` —
/// `02` §5, `17` §5). `skill/get` can only ever return a bounded summary of the
/// manifest, so this is the read face for the rest of the tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerSkillFile {
    /// Path relative to the skill directory, POSIX-separated, deterministic order.
    pub path: String,
    pub size: u64,
    /// Single-file sha256, lowercase hex.
    pub digest: String,
}

/// The readable file inventory of one Skill (`skill/files`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerSkillFileList {
    pub skill_id: String,
    /// Every readable file, sorted by `path`. Directories are not listed.
    pub files: Vec<AppServerSkillFile>,
    /// Tree digest of **this skill directory**, computed with the same
    /// `tree_digest` rule the snapshot digest uses (sorted relative paths +
    /// per-file sha256) so a caller can pin the exact version it read.
    ///
    /// Deliberately **not** the snapshot's `content_digest`, and not
    /// interchangeable with it: the snapshot digest covers the whole imported
    /// source tree (`import.rs`), while this one is scoped to the one skill
    /// directory. The two coincide only when the snapshot holds exactly this
    /// directory and nothing else — do not assume equality.
    pub content_digest: String,
    /// The inventory hit its entry ceiling and is incomplete. Reported rather
    /// than silently truncated: a partial list that claims to be whole is the
    /// failure mode this field exists to prevent.
    #[serde(default)]
    pub truncated: bool,
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

/// One namespaced tool exposed by a Connector.
///
/// The public (namespaced) name, the upstream description and the upstream
/// parameter schema are returned; the connection and its credentials stay on
/// the host. The schema is here because a caller that has to *name* a tool in
/// order to be granted it should be able to read what that tool takes — a grant
/// to a name nobody can inspect is a blind signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The upstream `tools/list` `inputSchema`, **verbatim**; `None` when the
    /// server published none, or when it was omitted to stay inside the tools
    /// budget (see [`AppServerConnectorDetail::tools_truncated`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

/// One localized string, both languages already resolved by the host.
///
/// The marketplace ships `title` / `title_en` (and friends) with holes in every
/// combination; the host applies the fallback once (`zh → en → key`,
/// `en → zh → key`) so the WebUI and the SDK cannot disagree about it (34 §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerLocalizedString {
    pub zh: String,
    pub en: String,
}

/// A connector's request to have the user fill something in (`34` §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerCredentialMode {
    /// Nothing to fill.
    None,
    /// The OAuth flow (`connector/auth/*`).
    Oauth,
    /// A key / token the user supplies.
    Token,
}

/// What the caller has to do about a connector's credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerCredentialStatus {
    NotRequired,
    /// At least one required field has no value.
    RequiresInput,
    Configured,
    /// Configured, and the server rejected it (401/403 on the last probe).
    Error,
}

/// One field of a connector's credential form.
///
/// **Never carries a secret**: `value` is present only for a `plain` field, whose
/// value belongs to the connector (a `HOST`, a `PORT`) rather than to the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerCredentialField {
    pub key: String,
    /// `secret` (the credential store) or `plain` (the connector's own values).
    pub kind: String,
    pub required: bool,
    pub label: AppServerLocalizedString,
    pub placeholder: AppServerLocalizedString,
    pub description: AppServerLocalizedString,
    /// Only for `plain`: the value in effect (declared default, or what the user
    /// set). Never a secret's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub doc_url: AppServerLocalizedString,
    pub doc_label: AppServerLocalizedString,
}

/// Everything a client needs to render one connector's credential form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConnectorCredential {
    pub connector_id: String,
    pub mode: AppServerCredentialMode,
    pub status: AppServerCredentialStatus,
    /// **Key names only** — what is still missing, never a value.
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub fields: Vec<AppServerCredentialField>,
    /// Form-level text from the marketplace declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<AppServerLocalizedString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<AppServerLocalizedString>,
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
    /// Marketplace icon URL for a connector installed from a marketplace.
    /// `None` for builtin hosts or markets that ship no icon for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// The credential state, when the host can describe one (`34` §6.1).
    ///
    /// Optional so an older provider — and every projection that has no
    /// declaration to read — stays valid on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<AppServerConnectorCredential>,
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
    /// Some `input_schema` values were omitted to stay inside the tools budget.
    ///
    /// Reported rather than silently dropped: names and descriptions are always
    /// kept (they are what a caller chooses by), while a schema is only ever
    /// included whole — a half-truncated JSON Schema would be parsed and
    /// believed. `false` also means "nothing was omitted".
    #[serde(default)]
    pub tools_truncated: bool,
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
    /// Some `input_schema` values were omitted to stay inside the tools budget
    /// (same rule as [`AppServerConnectorDetail::tools_truncated`]).
    #[serde(default)]
    pub tools_truncated: bool,
}

/// Result of one MCP tool call through the connector call proxy
/// (`connector/call`, doc `24` §5.2).
///
/// **A tool-level failure is a result, not an error.** When an MCP server
/// answers with `isError: true` the call still succeeded, so it comes back here
/// with [`Self::is_error`] set; only transport, protocol and budget failures
/// become wire errors. Collapsing the two would leave a caller unable to tell
/// "the tool said no" from "we never reached the tool".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConnectorCallResult {
    /// The upstream `isError` flag (`false` when the server omitted it).
    pub is_error: bool,
    /// The upstream `tools/call` **result object, verbatim** — `content`,
    /// `structuredContent` and anything a newer server adds all survive, because
    /// this layer has no business reshaping what an MCP server returned.
    ///
    /// It carries no transport, header or env value: the connection and its
    /// credentials stay on the host (that is the entire point of a proxy).
    pub result: serde_json::Value,
}

/// OAuth state view. Never contains tokens, authorization codes or secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerOAuthStatusView {
    /// `authenticated` | `not_authenticated` | `reauthorization_required`.
    pub state: String,
    /// Last sanitized browser-flow failure: why an authorization this client
    /// started never finished. Set when the flow failed after the browser was
    /// opened (the pre-browser failures come back on `auth/start` instead), and
    /// cleared once a flow succeeds or the credential is forgotten. Never
    /// contains tokens, authorization codes or the full authorization URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `connector/auth/start` result. The browser flow is owned by the trusted host:
/// `started` means the authorization URL reached the browser and the callback is
/// being awaited; `error` means the flow failed *before* that, so nothing is
/// worth waiting for. A failure after the browser opens surfaces through
/// `auth/status` instead.
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
    /// Canonical MCP server ids this Team's own snapshot installed and enabled on
    /// this host — the only Connector surface a Team Run may bind (`20` §8.1).
    ///
    /// Deliberately *not* the member Agents' `mcpServers`: plugin-level Agents'
    /// Connector declarations are recorded rather than mapped to grants
    /// (`02` §5.1), so treating them as bindable ids would invent authority the
    /// import never established. A snapshot-installed Connector, by contrast, is
    /// an exact id the installer already validated.
    #[serde(default)]
    pub connectors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Expert export / ExpertPack (docs/agent-store/32-expert-pack-export.zh.md)
// ---------------------------------------------------------------------------

/// Format version of [`AppServerExpertPack`].
///
/// **Deliberately independent of the protocol fingerprint** (`fp-<n>`): the
/// fingerprint versions *our* App Server wire compatibility, while this value
/// versions the **artifact contract for third-party runtimes**. Binding the two
/// would make every unrelated wire change force every external runtime to
/// re-adapt; omitting it would let a field change break them silently. Bump it
/// only when the pack's shape changes (doc `32` §4.4).
pub const APP_SERVER_EXPERT_PACK_FORMAT: u32 = 1;

/// Which catalog kind a pack describes.
///
/// The wire keeps `agent` / `team` as two kinds (cf. `MentionKind`). A pack is an
/// *artifact*, not a third kind, so `expert` never appears in a `kind` position
/// (doc `32` §4.1 — the word does exist elsewhere on the wire, e.g.
/// `AppServerAgentDetail::expert_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerExpertPackKind {
    Agent,
    Team,
}

/// A portable expert definition produced by `agent/export` / `team/export`.
///
/// This is the **only** public face that carries an expert's persona. It is
/// deliberately *not* a field on [`AppServerAgentDetail`]: the catalog face is
/// what every store UI calls, and doc `32` §2 keeps the two apart precisely so
/// the export gate can live on its own methods.
///
/// A pack carries a **definition, not execution semantics**. How a team plans and
/// schedules its steps, how tool and credential policy is applied, and the shape
/// of its events are all enforced by the runtime and are *not* here — doc `32` §5
/// (R1–R11) lists what a consumer has to implement itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertPack {
    pub pack_format: u32,
    pub kind: AppServerExpertPackKind,
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub persona: AppServerExpertPersona,
    pub model: AppServerExpertModel,
    #[serde(default)]
    pub skills: Vec<AppServerExpertSkillRef>,
    #[serde(default)]
    pub connectors: Vec<AppServerExpertConnectorRef>,
    pub tool_policy: AppServerExpertToolPolicy,
    /// Present exactly when `kind == Team`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<AppServerExpertTeamPack>,
    pub provenance: AppServerExpertProvenance,
    pub runtime_binding: AppServerExpertRuntimeBinding,
}

/// The expert's persona — the only body of text this face carries.
///
/// `instructions` is the Agent Markdown body **verbatim**: the importer projects
/// it into the snapshot payload and the installer copies it into the Preset. The
/// catalog face withholds exactly this field by design (`frontmatter.rs:114`,
/// `app_server.rs:435`), so a value here must never also appear there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertPersona {
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

/// Declared and resolved model hints. Neither is binding.
///
/// `declared` is the source frontmatter string (often a name this host cannot
/// resolve) and `resolved` is what *this* host resolved the preset to. A
/// consumer resolves its own model and its own precedence chain — doc `32` §5 R6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<AppServerExpertModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

/// `(provider_id, model)` as resolved on the host that produced the pack.
///
/// `provider_id` is **host-local** (a row id on that host) and is not portable;
/// `model` is the portable half. Both are reported so a consumer can tell "same
/// model, different host" from "different model".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertModelRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub model: String,
}

/// One skill the expert declares, **by reference**.
///
/// The bytes are deliberately not inlined (doc `32` §2): a skill is a directory
/// (auxiliary files included), `skill/files` / `skill/file` already serve it, and
/// inlining would create a second source of truth. `id` and `name` are the same
/// string — `skill/list` publishes the skill name as its id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertSkillRef {
    pub name: String,
    pub id: String,
}

/// One connector a **team's own snapshot** installed and left enabled.
///
/// Identity and switch only — no transport, env, headers or token ever crosses
/// this seam (`24` §2/§5.3); tool schemas stay on `connector/get`.
///
/// An **agent** pack's list is always empty, and that is faithful rather than a
/// gap: this format has no agent-level connector dependency (`02` §5.1 — a
/// plugin-level `mcpServers` is a plugin capability and explicitly *not* a
/// per-agent grant), so mapping it would invent authority the import never
/// established.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertConnectorRef {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// The expert's declared tool surface.
///
/// A **declaration, never a permission boundary**: tool / file / network /
/// credential authority is computed by runtime policy, not from persona text
/// (`04` §3.2; doc `32` §5 R9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertToolPolicy {
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
}

/// Team roster and policy.
///
/// The **roster is a hard boundary**; the policy fields are free-form *intent*
/// that a consumer must translate into its own enforceable rules (doc `32` §5
/// R2/R3/R4). The runtime's own planner is not part of the pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertTeamPack {
    pub lead_agent_id: String,
    pub member_agent_ids: Vec<String>,
    pub planner_policy: String,
    #[serde(default)]
    pub routing_constraints: Vec<String>,
    pub workflow_limits: serde_json::Value,
    #[serde(default)]
    pub team_runtime_capabilities: Vec<String>,
    /// Fully expanded member packs, **leader first**. `team/export` fails rather
    /// than emit a partial roster (doc `32` §6.4).
    pub members: Vec<AppServerExpertPack>,
}

/// Where a pack came from, for attribution and drift detection.
///
/// There is deliberately **no export timestamp**: two exports of one snapshot are
/// byte-identical, so a consumer can key a cache on `content_digest` and diff two
/// exports to answer "what changed upstream" (doc `32` §4.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertProvenance {
    pub source: String,
    pub snapshot_id: String,
    pub content_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_revision: Option<i64>,
}

/// Which engine this definition was bound to on the host that produced it.
///
/// `portable: false` is the normal case, not an error: an installed expert is a
/// Preset pinned to that host's own runtime agent, which no other runtime has.
/// The field exists so a consumer can *state* that fact instead of discovering it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppServerExpertRuntimeBinding {
    pub runtime: String,
    pub portable: bool,
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

/// What happened to one component during install / uninstall / enable / disable.
///
/// `code` is a **stable, documented** token a client branches on
/// (`docs/agent-store/05` §4.5 lists the closed set). `message` is for humans
/// and nothing parses it — the whole point of the field is to stop callers from
/// matching on prose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallOutcome {
    pub component_id: String,
    pub kind: String,
    /// `created` | `reused` | `enabled` | `disabled` | `marked` | `removed` |
    /// `skipped` | `failed`.
    ///
    /// `marked` is the documented skill case: the flag moved but the runtime did
    /// not, because the skill corpus has no enable state (`05` §4.5).
    pub action: String,
    /// `false` only when the requested state was **not** reached. A component
    /// that was already in the requested state reports `ok: true`.
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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
    /// Per-component outcome. Absent from a host that predates the field —
    /// treat a missing value as "no detail available", never as "nothing ran".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<AppServerInstallOutcome>,
}

/// Install state projection for one snapshot (or empty when not installed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerInstallStatus {
    pub snapshot_id: String,
    pub components: Vec<AppServerInstallComponent>,
    /// Per-component outcome of the mutation that produced this projection
    /// (`uninstall` / `disable` / `enable`). Empty for a plain status read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<AppServerInstallOutcome>,
    /// Human-readable failures; mirrors the `ok: false` outcomes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
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
    /// HTTP(S) zip archive whose root **is** the market root (doc 30).
    ///
    /// One request instead of one-per-file: the official mirror serves
    /// `experts` as 14,714 individual files (611 MiB) that a `url` source
    /// mirrors one by one, versus a single 289 MiB archive here.
    #[serde(rename = "zip")]
    Zip,
}

impl AppServerMarketplaceSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Github => "github",
            Self::Git => "git",
            Self::Url => "url",
            Self::Zip => "zip",
        }
    }

    /// Parse the wire string back into the enum.
    ///
    /// Lives next to [`Self::as_str`] on purpose: the host resolves configured
    /// source kinds by string, and the previous hand-written `match` in that
    /// resolver ended in `_ => Url`, so adding a variant here silently mapped
    /// the new kind onto `url` instead of failing. `None` (never a guess) is
    /// what callers must handle.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "directory" => Some(Self::Directory),
            "github" => Some(Self::Github),
            "git" => Some(Self::Git),
            "url" => Some(Self::Url),
            "zip" => Some(Self::Zip),
            _ => None,
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
/// `config/get-mcp` response: the declaration file's **raw text**.
///
/// The one read face that returns the file verbatim, and it exists for exactly
/// one purpose — the settings dialog's file editor. It is deliberately a
/// separate method rather than another field on `config/get`: that view
/// describes the file's *verdict* (`servers` / `rejected` / `error`) and carries
/// no `env` / `headers` value, and every caller of `config/get` — the whole
/// settings dialog, on open — would otherwise receive the text whether or not it
/// asked.
///
/// `exists: false` + `source: None` is the normal answer for a host that never
/// created the file. The text comes back whether or not it parses: editing a
/// broken file is the point of the editor, and `config/get` already reports the
/// parse verdict next to it.
///
/// **This is the one place a declaration's own values reach a client** (`21`
/// D17, superseding the blanket "values never cross" clause of `05` §4.10): the
/// operator is editing their own file on their own machine, and a read-only
/// viewer cannot edit. The read-modify-write loop here is therefore explicit
/// rather than accidental, and it is gated exactly like the write face.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerMcpSourceView {
    pub exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

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
    /// The market entry's own `publishedAt`, `YYYY-MM-DD` (`18` §3).
    ///
    /// A *calendar date*, not an instant, and never a derived one: a host that
    /// has no date for this entry omits the field rather than guessing from the
    /// import time or the snapshot's `added_at`. Absent therefore means "this
    /// market did not declare a date", which is why the client must not render
    /// a placeholder for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
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
    /// Per-component outcome, forwarded from the installer's own report so a
    /// store install is as branchable as a direct `install/run`. Absent from a
    /// host that predates the field — read it as "no detail available".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<AppServerInstallOutcome>,
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
    /// `[tools]` — absent when the file declares no tools table at all.
    ///
    /// A present table is reported with its defaults filled in, so
    /// `Some(policy)` with every switch `true` and both lists empty means "the
    /// file declares `[tools]` but constrains nothing" — the same
    /// absent-versus-explicit distinction the `memory` field makes.
    #[serde(default)]
    pub tools: Option<crate::NomiToolPolicy>,
    /// `~/.agent-store/mcp.json` — the host's MCP server declarations (`20` §7.9
    /// / `21` D14), projected read-only.
    ///
    /// Absent when the file does not exist (or the host cannot resolve it), so a
    /// client can tell "no declarations" from "declared nothing". This is the
    /// **only** read surface for a declared server: declarations deliberately do
    /// not become `mcp_servers` rows, so they never appear in `connector/*`.
    /// Credential *values* have no field here — only key names ever cross.
    #[serde(default)]
    pub mcp: Option<AppServerConfigMcpView>,
}

/// The `mcp.json` projection inside [`AppServerConfigView`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigMcpView {
    /// The declaration file is present on this host.
    pub exists: bool,
    /// Whether **this host** actually feeds the file into agent sessions, as the
    /// launcher reported it at startup (`--adopt-store-mcp-declarations`, `20`
    /// §7.9).
    ///
    /// Three states on purpose: `Some(..)` is the host's own answer, `None` means
    /// the host did not report one (a build older than this field). A plain
    /// `false` for "did not report" would turn "I cannot tell" into "this file is
    /// inert here" — and `apps/agent-store` has been adopting declarations since
    /// before this field existed, so that lie is reachable during a version skew.
    ///
    /// `exists: true, adopted: Some(false)` is the pair this field exists for:
    /// the file is real and parses cleanly, and still changes nothing on the host
    /// serving this screen. `servers` / `rejected` describe the **file**; this
    /// one describes the **host**, and without it the two are indistinguishable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted: Option<bool>,
    /// Accepted entries, ordered by server key.
    #[serde(default)]
    pub servers: Vec<AppServerConfigMcpServerView>,
    /// Entries this host refused, with the reason the user has to fix. Refused
    /// entries are reported rather than silently dropped: a `disabledTools` a
    /// user believes is in force is a security problem, not a cosmetic one.
    #[serde(default)]
    pub rejected: Vec<AppServerConfigMcpRejectionView>,
    /// Why the whole file was unreadable as declarations (invalid JSON, wrong
    /// top level) — reported instead of quietly answering "nothing declared",
    /// which would look identical to an empty file. Omitted when the file
    /// parsed; entries that were refused individually appear in `rejected`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One accepted entry of `mcp.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigMcpServerView {
    /// The `mcpServers` key, i.e. the `<server>` segment of `mcp__<server>__*`.
    pub name: String,
    /// `stdio` | `http` | `sse`.
    pub transport: String,
    /// `enabled = false` keeps the entry declared but out of every session.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// One refused entry of `mcp.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigMcpRejectionView {
    pub name: String,
    pub reason: String,
}

/// `[memory]` in the settings file, as far as the wire needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppServerConfigMemoryView {
    /// `None` when the key is absent (upstream default: distillation ON).
    #[serde(default)]
    pub distill_enabled: Option<bool>,
}