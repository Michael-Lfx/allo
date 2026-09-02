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