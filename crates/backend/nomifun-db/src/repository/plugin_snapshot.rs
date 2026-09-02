use crate::error::DbError;
use crate::models::{PluginSnapshotComponentRow, PluginSnapshotRow};

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
    pub components: Vec<NewPluginSnapshotComponent<'a>>,
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

    /// Most recently imported snapshots first (history list).
    async fn list_snapshots(&self, limit: u32) -> Result<Vec<PluginSnapshotRow>, DbError>;

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
}