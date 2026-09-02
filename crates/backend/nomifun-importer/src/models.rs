//! Core import domain objects (docs/agent-store/01-domain-model.md §4–§9 and
//! 02-...-import-spec.md). These are the *importer-internal* shapes; the
//! public protocol projections live in `nomifun-api-types`.

use serde::{Deserialize, Serialize};

pub const KIND_AGENT: &str = "agent";
pub const KIND_TEAM: &str = "team";
pub const KIND_SKILL: &str = "skill";
pub const KIND_CONNECTOR: &str = "connector";
pub const KIND_COMMAND: &str = "command";
pub const KIND_HOOK: &str = "hook";
pub const KIND_LSP: &str = "lsp";
pub const KIND_CREDENTIAL: &str = "credential";
pub const KIND_DEPENDENCY: &str = "dependency";
pub const KIND_SCRIPT: &str = "script";

/// Runtime status vocabulary (`03-...-compatibility-matrix.md` §1).
pub const RUNTIME_NOT_VERIFIED: &str = "not-verified";
pub const RUNTIME_ADAPTER_VERIFIED: &str = "adapter-verified";
pub const RUNTIME_VERIFIED: &str = "runtime-verified";
pub const RUNTIME_RELEASE_ELIGIBLE: &str = "release-eligible";

/// Distribution status vocabulary (02 §11 `component_status`).
pub const DIST_LOCAL_ONLY: &str = "local-only";

/// Reason codes shared across components (02 §11.2 / 03 §1: reason codes are
/// never first-class statuses).
pub const REASON_IGNORED_BY_SOURCE_RUNTIME: &str = "ignored-by-source-runtime";
pub const REASON_UNSUPPORTED_AUTH: &str = "unsupported-auth";

/// Import source kinds supported by V1 (02 §1: local directories only;
/// GitHub/Git/HTTP market sources stay `compatible-with-adapter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// `.codebuddy-plugin/plugin.json` plugin root.
    CodeBuddyPlugin,
    /// `.codebuddy-skill/marketplace.json` skill market directory.
    WorkBuddySkillMarket,
    /// `.codebuddy-connector/connectors.json` connector market directory.
    WorkBuddyConnectorMarket,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeBuddyPlugin => "codebuddy-plugin",
            Self::WorkBuddySkillMarket => "workbuddy-skill-market",
            Self::WorkBuddyConnectorMarket => "workbuddy-connector-market",
        }
    }

    /// Manifest location relative to the import root.
    pub fn manifest_rel_path(self) -> &'static str {
        match self {
            Self::CodeBuddyPlugin => ".codebuddy-plugin/plugin.json",
            Self::WorkBuddySkillMarket => ".codebuddy-skill/marketplace.json",
            Self::WorkBuddyConnectorMarket => ".codebuddy-connector/connectors.json",
        }
    }
}

/// Three-dimensional compatibility report (03 §1). `reasons` uses stable
/// reason codes (`ignored-by-source-runtime`, `unsupported-auth`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatTriple {
    /// `compatible` | `compatible_with_adapter` | `manual_review` |
    /// `unsupported` | `pending_legal_review`.
    pub semantic_status: String,
    /// `not-verified` | `adapter-verified` | `runtime-verified` | `release-eligible`.
    pub runtime_status: String,
    /// `local-only` (V1; never distribution-ready).
    pub distribution_status: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

impl CompatTriple {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("compatibility triple serializes as JSON")
    }
}

/// Snapshot identity / provenance (02 §9). `source_uri` is internal only.
#[derive(Debug, Clone)]
pub struct SnapshotMeta {
    pub plugin_id: String,
    pub name: String,
    pub declared_version: String,
    pub resolved_revision: Option<String>,
    pub source_kind: SourceKind,
    pub source_uri: String,
}

/// One standardized definition produced by an import.
#[derive(Debug, Clone)]
pub struct Component {
    /// Public opaque id, `wb-<plugin_id>-<slug>` (sanitized).
    pub component_id: String,
    pub kind: String,
    pub name: String,
    pub relative_path: Option<String>,
    pub compatibility: CompatTriple,
    pub payload: serde_json::Value,
    /// Per-component problems (folded into `completed-with-warnings`).
    pub warnings: Vec<String>,
}

impl Component {
    pub fn new(
        kind: &str,
        component_id: String,
        name: String,
        relative_path: Option<String>,
        compatibility: CompatTriple,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            component_id,
            kind: kind.to_owned(),
            name,
            relative_path,
            compatibility,
            payload,
            warnings: Vec::new(),
        }
    }
}

/// Sanitize a slug for use inside an opaque component id: keep ASCII
/// alphanumerics, `-` and `_`; everything else becomes `-`.
pub fn sanitize_slug(slug: &str) -> String {
    let mut out = String::with_capacity(slug.len());
    for ch in slug.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_owned()
}

/// Build the documented `wb-<plugin_id>-<slug>` component id (02 §9).
pub fn component_id(plugin_id: &str, slug: &str) -> String {
    format!("wb-{}-{}", sanitize_slug(plugin_id), sanitize_slug(slug))
}