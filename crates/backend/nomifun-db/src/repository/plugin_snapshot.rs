use crate::error::DbError;
use crate::models::{PluginSnapshotComponentRow, PluginSnapshotListRow, PluginSnapshotRow};

/// One component to persist with a snapshot.
#[derive(Debug, Clone)]
pub struct NewPluginSnapshotComponent<'a> {
    pub component_id: &'a str,
    pub kind: &'a str,
    pub name: &'a str,
    pub relative_path: Option<&'a str>,
    pub compatibility_json: &'a str,
    pub payload_json: &'a str,
}

/// Parameters for inserting a snapshot with all of its components in one
/// transaction. A snapshot is immutable once stored.
#[derive(Debug, Clone)]
pub struct NewPluginSnapshot<'a> {
    pub snapshot_id: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub source_kind: &'a str,
    pub source_uri: Option<&'a str>,
    pub plugin_id: &'a str,
    pub declared_version: &'a str,
    pub resolved_revision: Option<&'a str>,
    pub content_digest: &'a str,
    pub status: &'a str,
    /// Marketplace provenance (roadmap Phase 2): set when the snapshot was
    /// imported from a marketplace entry.
    pub marketplace_id: Option<&'a str>,
    pub entry_name: Option<&'a str>,
    pub source_revision: Option<&'a str>,
    pub components: Vec<NewPluginSnapshotComponent<'a>>,
}

/// Runtime registration for one installed component. `location` is the
/// on-disk path (skills) / connector name (mcp_servers) / preset id.
#[derive(Debug, Clone)]
pub struct ComponentRuntimeRef<'a> {
    pub component_id: &'a str,
    /// `skill` | `connector` | `preset`.
    pub runtime_type: &'a str,
    /// On-disk path or logical runtime target identifier.
    pub location: &'a str,
    /// MCP server row id when the component became a configured connector.
    pub mcp_server_id: Option<&'a str>,
}

/// Importer Catalog data access (`docs/agent-store/02-...-import-spec.md` §2
/// step 9). Object-safe via `async_trait` for `Arc<dyn IPluginSnapshotRepository>`.
#[async_trait::async_trait]
pub trait IPluginSnapshotRepository: Send + Sync {
    /// Looks up an existing snapshot by its public opaque id.
    async fn get_by_snapshot_id(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<PluginSnapshotRow>, DbError>;

    /// All components of a snapshot, ordered by row id.
    async fn get_components(
        &self,
        snapshot_id: &str,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError>;

    /// Most recently imported snapshots first (history list), with each
    /// snapshot's component count resolved in the same query.
    async fn list_snapshots(&self, limit: u32) -> Result<Vec<PluginSnapshotListRow>, DbError>;

    /// Idempotency probe: an identical `(plugin_id, declared_version,
    /// content_digest)` import reuses this snapshot instead of inserting.
    async fn find_by_identity_digest(
        &self,
        plugin_id: &str,
        declared_version: &str,
        content_digest: &str,
    ) -> Result<Option<PluginSnapshotRow>, DbError>;

    /// Digest-conflict probe: same identity+version with a *different* digest
    /// must block the import (never overwrite).
    async fn list_by_identity(
        &self,
        plugin_id: &str,
        declared_version: &str,
    ) -> Result<Vec<PluginSnapshotRow>, DbError>;

    /// Persists a snapshot and its components atomically.
    async fn insert_snapshot_with_components(
        &self,
        params: NewPluginSnapshot<'_>,
    ) -> Result<PluginSnapshotRow, DbError>;

    /// All components of a given kind across snapshots (catalog projection),
    /// ordered by snapshot import recency then row id.
    async fn list_components_by_kind(
        &self,
        kind: &str,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError>;

    // --- installer state (roadmap Phase 2) ---------------------------------

    /// Marks components as installed (enabled) with their runtime references.
    /// One transaction; unknown component ids are skipped.
    async fn mark_components_installed(
        &self,
        refs: &[ComponentRuntimeRef<'_>],
        installed_at: i64,
    ) -> Result<(), DbError>;

    /// Sets `disabled` for the given components. Idempotent.
    async fn set_components_disabled(
        &self,
        component_ids: &[&str],
        disabled: bool,
    ) -> Result<(), DbError>;

    /// Clears the installation state for the given components (uninstall):
    /// `installed=0`, `disabled=0`, staggered refs nulled. The snapshot rows
    /// themselves are kept.
    async fn clear_components_installed(
        &self,
        component_ids: &[&str],
    ) -> Result<(), DbError>;

    /// Installation state projection for a snapshot's components, or for all
    /// installed components when `snapshot_id` is `None`.
    async fn list_installation_state(
        &self,
        snapshot_id: Option<&str>,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError>;
}