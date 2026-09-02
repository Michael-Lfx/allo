use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};

/// Row mapping for the `plugin_snapshots` table.
///
/// One immutable import result (roadmap Phase 1). The same
/// `(plugin_id, declared_version, content_digest)` triple is idempotent; a
/// digest conflict for an existing identity+version is blocked by the
/// importer and never overwrites this row.
///
/// `source_uri` is internal traceability only: it records where the content
/// came from and must never cross into the public App Server protocol.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
pub struct PluginSnapshotRow {
    /// SQLite row identity (internal).
    pub id: i64,
    /// Public opaque snapshot id (`snapshot_id`).
    pub snapshot_id: String,
    pub name: String,
    pub version: String,
    /// `codebuddy-plugin` | `workbuddy-skill-market` | `workbuddy-connector-market`.
    pub source_kind: String,
    /// Internal source location (never exposed publicly).
    pub source_uri: Option<String>,
    pub plugin_id: String,
    pub declared_version: String,
    pub resolved_revision: Option<String>,
    pub content_digest: String,
    /// `completed` | `completed-with-warnings` | `blocked` | `failed`.
    pub status: String,
    pub imported_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Row mapping for the `plugin_snapshot_components` table.
///
/// One standardized definition produced by an import (Agent / Team / Skill /
/// Connector / Command / Hook / LSP / CredentialSchema / Dependency / Script).
/// `payload_json` holds the normalized definition; it never contains
/// credential values. `compatibility_json` holds the three-dimensional
/// compatibility report of `docs/agent-store/03-...-compatibility-matrix.md`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
pub struct PluginSnapshotComponentRow {
    pub id: i64,
    /// Logical link to `plugin_snapshots.snapshot_id` (no physical FK).
    pub snapshot_id: String,
    /// Public opaque component id (e.g. `wb-<plugin_id>-<slug>`).
    pub component_id: String,
    /// `agent` | `team` | `skill` | `connector` | `command` | `hook` | `lsp` |
    /// `credential` | `dependency` | `script`.
    pub kind: String,
    pub name: String,
    /// Snapshot-relative source path (internal traceability).
    pub relative_path: Option<String>,
    pub compatibility_json: String,
    pub payload_json: String,
}