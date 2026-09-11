use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};
use sqlx::Row as _;

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
    /// Marketplace provenance (roadmap Phase 2): set when the snapshot was
    /// imported from a marketplace entry; `None` for manual imports.
    pub marketplace_id: Option<String>,
    pub entry_name: Option<String>,
    /// Source revision at the time of import (git commit / HTTP marker);
    /// internal traceability only.
    pub source_revision: Option<String>,
}

/// History-list projection: a snapshot row plus its component count,
/// produced by `IPluginSnapshotRepository::list_snapshots` in one JOIN.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginSnapshotListRow {
    pub snapshot: PluginSnapshotRow,
    pub component_count: i64,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for PluginSnapshotListRow {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        let snapshot = PluginSnapshotRow::from_row(row)?;
        let component_count: i64 = row.try_get("component_count")?;
        Ok(Self { snapshot, component_count })
    }
}

/// Marketplace-scoped provenance projection: the snapshot row plus how many of
/// its components are installed.
///
/// `market/get` needs this to answer two client questions without a second
/// round-trip — "does this entry have an imported snapshot?" and "is anything
/// installed from it?" The second one *is* the impact set a cascade removal has
/// to show **before** it runs, which is why it must come from the server rather
/// than be re-derived from the aggregate store listing (doc 16 D-W13-1 ①).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginSnapshotProvenanceRow {
    pub snapshot: PluginSnapshotRow,
    pub component_count: i64,
    /// Components with `installed = 1` (a `disabled` component still counts:
    /// removal deletes its runtime artifacts too).
    pub installed_count: i64,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for PluginSnapshotProvenanceRow {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        let snapshot = PluginSnapshotRow::from_row(row)?;
        let component_count: i64 = row.try_get("component_count")?;
        let installed_count: i64 = row.try_get("installed_count")?;
        Ok(Self {
            snapshot,
            component_count,
            installed_count,
        })
    }
}

/// Row mapping for the `plugin_snapshot_components` table.
///
/// One standardized definition produced by an import (Agent / Team / Skill /
/// Connector / Command / Hook / LSP / CredentialSchema / Dependency / Script).
/// `payload_json` holds the normalized definition; it never contains
/// credential values. `compatibility_json` holds the three-dimensional
/// compatibility report of `docs/agent-store/03-...-compatibility-matrix.md`.
///
/// Installation state (roadmap Phase 2) lives on the row: `installed` /
/// `disabled` flags plus the runtime registration (`preset_id` for agent/team
/// components, `runtime_ref` JSON `{type, location, mcp_server_id}`).
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
    /// 1 when the installer registered this component into the runtime.
    pub installed: i64,
    /// 1 when installed but disabled; runtime artifacts stay in place.
    pub disabled: i64,
    /// When the component was installed (ms epoch).
    pub installed_at: Option<i64>,
    /// Preset created for agent/team components (nullable for other kinds).
    pub preset_id: Option<String>,
    /// JSON `{ "type": "skill"|"connector"|"preset",
    /// "location": "...", "mcp_server_id": "..." }` (internal traceability).
    pub runtime_ref: Option<String>,
}