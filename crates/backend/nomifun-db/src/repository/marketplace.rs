//! Marketplace registry data access (roadmap Phase 2).
//!
//! Object-safe via `async_trait` for `Arc<dyn IMarketplaceRepository>`.
//! Snapshots' provenance columns (`marketplace_id` / `entry_name`) are read
//! and cleared here as well, so remove-cascade stays one module.

use crate::error::DbError;
use crate::models::{MarketplaceEntry, PluginMarketplaceRow, PluginSnapshotRow};

/// Parameters for adding a marketplace registry row.
#[derive(Debug, Clone)]
pub struct NewPluginMarketplace<'a> {
    pub marketplace_id: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub source_kind: &'a str,
    pub source_uri: &'a str,
    pub owner_json: Option<&'a str>,
    pub version: Option<&'a str>,
    pub content_digest: Option<&'a str>,
    pub entries: Vec<MarketplaceEntry>,
    pub auto_update: bool,
}

/// Marketplace registry seam (docs/agent-store/02 §8).
#[async_trait::async_trait]
pub trait IMarketplaceRepository: Send + Sync {
    /// Insert a marketplace registry row. `(source_kind, source_uri)` must be
    /// unique; an existing active row is an error the caller maps to conflict.
    async fn insert_marketplace(
        &self,
        params: NewPluginMarketplace<'_>,
    ) -> Result<PluginMarketplaceRow, DbError>;

    /// Look up one marketplace by its public opaque id (active or removed).
    async fn get_marketplace(
        &self,
        marketplace_id: &str,
    ) -> Result<Option<PluginMarketplaceRow>, DbError>;

    /// Look up one marketplace by its source (duplicate-add probe).
    async fn find_by_source(
        &self,
        source_kind: &str,
        source_uri: &str,
    ) -> Result<Option<PluginMarketplaceRow>, DbError>;

    /// All active marketplaces, most recently added first.
    async fn list_marketplaces(&self) -> Result<Vec<PluginMarketplaceRow>, DbError>;

    /// Replace the entries projection (+ digest + version + updated_at).
    async fn update_marketplace_entries(
        &self,
        marketplace_id: &str,
        entries: &[MarketplaceEntry],
        content_digest: &str,
        version: Option<&str>,
    ) -> Result<(), DbError>;

    /// Record the resolved source revision (git commit / HTTP marker) and the
    /// materialization root after a successful fetch. Internal traceability.
    async fn record_resolved_revision(
        &self,
        marketplace_id: &str,
        resolved_revision: &str,
        staging_root: &str,
    ) -> Result<(), DbError>;

    /// Toggle auto-update for one marketplace.
    async fn set_auto_update(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<(), DbError>;

    /// Toggle the enabled flag (enabled=0 hides the market from discovery).
    async fn set_enabled(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<(), DbError>;

    /// Soft-delete a marketplace (removed_at stamp). Snapshot provenance keeps
    /// pointing at it; callers decide whether to clear it.
    async fn soft_remove_marketplace(
        &self,
        marketplace_id: &str,
        removed_at: i64,
    ) -> Result<(), DbError>;

    /// Reactivate a previously soft-removed marketplace: clear the removal
    /// stamp, re-enable it, replace its entries projection and **update its
    /// source** — a re-add may point at a different source than the original
    /// registration, and the duplicate probe (`find_by_source`) must match the
    /// *current* source afterwards. The row keeps its opaque id (the id column
    /// is unique, so re-adding a source reuses the row instead of inserting a
    /// duplicate).
    async fn reactivate_marketplace(
        &self,
        marketplace_id: &str,
        source_kind: &str,
        source_uri: &str,
        entries: &[MarketplaceEntry],
        content_digest: &str,
        version: Option<&str>,
    ) -> Result<(), DbError>;

    /// Snapshots imported from this marketplace entry (provenance lookup),
    /// newest first.
    async fn list_snapshots_by_marketplace(
        &self,
        marketplace_id: &str,
    ) -> Result<Vec<PluginSnapshotRow>, DbError>;

    /// Clear provenance on a snapshot (the marketplace is gone / entry removed).
    async fn clear_snapshot_provenance(
        &self,
        snapshot_id: &str,
    ) -> Result<(), DbError>;
}
