use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{MarketplaceEntry, PluginMarketplaceRow, PluginSnapshotRow};
use crate::repository::marketplace::{IMarketplaceRepository, NewPluginMarketplace};

/// SQLite-backed implementation of [`IMarketplaceRepository`].
#[derive(Clone, Debug)]
pub struct SqliteMarketplaceRepository {
    pool: SqlitePool,
}

impl SqliteMarketplaceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IMarketplaceRepository for SqliteMarketplaceRepository {
    async fn insert_marketplace(
        &self,
        params: NewPluginMarketplace<'_>,
    ) -> Result<PluginMarketplaceRow, DbError> {
        let entries_json = serde_json::to_string(&params.entries)
            .map_err(|error| DbError::Init(format!("encode entries: {error}")))?;
        let now = nomifun_common::now_ms();
        sqlx::query(
            "INSERT INTO plugin_marketplaces \
                (marketplace_id, name, description, source_kind, source_uri, \
                 owner_json, version, content_digest, entries_json, auto_update, \
                 enabled, last_checked_at, added_at, updated_at, removed_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, NULL)",
        )
        .bind(params.marketplace_id)
        .bind(params.name)
        .bind(params.description)
        .bind(params.source_kind)
        .bind(params.source_uri)
        .bind(params.owner_json)
        .bind(params.version)
        .bind(params.content_digest)
        .bind(entries_json)
        .bind(if params.auto_update { 1_i64 } else { 0_i64 })
        .bind(1_i64) // enabled
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_marketplace(params.marketplace_id)
            .await?
            .ok_or_else(|| DbError::Init("marketplace missing after insert".into()))
    }

    async fn get_marketplace(
        &self,
        marketplace_id: &str,
    ) -> Result<Option<PluginMarketplaceRow>, DbError> {
        let row = sqlx::query_as::<_, PluginMarketplaceRow>(
            "SELECT * FROM plugin_marketplaces WHERE marketplace_id = ?",
        )
        .bind(marketplace_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn find_by_source(
        &self,
        source_kind: &str,
        source_uri: &str,
    ) -> Result<Option<PluginMarketplaceRow>, DbError> {
        let row = sqlx::query_as::<_, PluginMarketplaceRow>(
            "SELECT * FROM plugin_marketplaces WHERE source_kind = ? AND source_uri = ?",
        )
        .bind(source_kind)
        .bind(source_uri)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_marketplaces(&self) -> Result<Vec<PluginMarketplaceRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginMarketplaceRow>(
            "SELECT * FROM plugin_marketplaces WHERE removed_at IS NULL \
             ORDER BY added_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn update_marketplace_entries(
        &self,
        marketplace_id: &str,
        entries: &[MarketplaceEntry],
        content_digest: &str,
        version: Option<&str>,
    ) -> Result<(), DbError> {
        let entries_json = serde_json::to_string(entries)
            .map_err(|error| DbError::Init(format!("encode entries: {error}")))?;
        let now = nomifun_common::now_ms();
        sqlx::query(
            "UPDATE plugin_marketplaces SET entries_json = ?, content_digest = ?, \
             version = ?, updated_at = ?, last_checked_at = ? \
             WHERE marketplace_id = ? AND removed_at IS NULL",
        )
        .bind(entries_json)
        .bind(content_digest)
        .bind(version)
        .bind(now)
        .bind(now)
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_resolved_revision(
        &self,
        marketplace_id: &str,
        resolved_revision: &str,
        staging_root: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE plugin_marketplaces SET resolved_revision = ?, staging_root = ?, \
             updated_at = ? WHERE marketplace_id = ? AND removed_at IS NULL",
        )
        .bind(resolved_revision)
        .bind(staging_root)
        .bind(nomifun_common::now_ms())
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_auto_update(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE plugin_marketplaces SET auto_update = ?, updated_at = ? \
             WHERE marketplace_id = ? AND removed_at IS NULL",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(nomifun_common::now_ms())
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_enabled(&self, marketplace_id: &str, enabled: bool) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE plugin_marketplaces SET enabled = ?, updated_at = ? \
             WHERE marketplace_id = ? AND removed_at IS NULL",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(nomifun_common::now_ms())
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn soft_remove_marketplace(
        &self,
        marketplace_id: &str,
        removed_at: i64,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE plugin_marketplaces SET removed_at = ?, enabled = 0, updated_at = ? \
             WHERE marketplace_id = ?",
        )
        .bind(removed_at)
        .bind(nomifun_common::now_ms())
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn reactivate_marketplace(
        &self,
        marketplace_id: &str,
        source_kind: &str,
        source_uri: &str,
        entries: &[MarketplaceEntry],
        content_digest: &str,
        version: Option<&str>,
    ) -> Result<(), DbError> {
        let entries_json = serde_json::to_string(entries)
            .map_err(|error| DbError::Init(format!("encode entries: {error}")))?;
        let now = nomifun_common::now_ms();
        sqlx::query(
            "UPDATE plugin_marketplaces \
             SET source_kind = ?, source_uri = ?, removed_at = NULL, enabled = 1, \
                 entries_json = ?, content_digest = ?, version = ?, updated_at = ?, \
                 last_checked_at = ? \
             WHERE marketplace_id = ?",
        )
        .bind(source_kind)
        .bind(source_uri)
        .bind(entries_json)
        .bind(content_digest)
        .bind(version)
        .bind(now)
        .bind(now)
        .bind(marketplace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_snapshots_by_marketplace(
        &self,
        marketplace_id: &str,
    ) -> Result<Vec<PluginSnapshotRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginSnapshotRow>(
            "SELECT * FROM plugin_snapshots WHERE marketplace_id = ? \
             ORDER BY imported_at DESC, id DESC",
        )
        .bind(marketplace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn clear_snapshot_provenance(&self, snapshot_id: &str) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE plugin_snapshots SET marketplace_id = NULL, entry_name = NULL \
             WHERE snapshot_id = ?",
        )
        .bind(snapshot_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_database_memory;
    use crate::repository::plugin_snapshot::IPluginSnapshotRepository;

    async fn setup() -> (SqliteMarketplaceRepository, crate::Database) {
        let db = init_database_memory().await.unwrap();
        let repo = SqliteMarketplaceRepository::new(db.pool().clone());
        (repo, db)
    }

    fn sample_entry(name: &str) -> MarketplaceEntry {
        MarketplaceEntry {
            name: name.to_owned(),
            source_kind: "directory".into(),
            source_uri: format!("./plugins/{name}"),
            version: Some("1.0.0".into()),
            description: Some(format!("{name} plugin")),
            keywords: vec!["demo".into()],
            category: Some("dev".into()),
        }
    }

    fn sample<'a>(id: &'a str, source: &'a str) -> NewPluginMarketplace<'a> {
        NewPluginMarketplace {
            marketplace_id: id,
            name: "company-tools",
            description: Some("team catalog"),
            source_kind: "directory",
            source_uri: source,
            owner_json: None,
            version: Some("1.0.0"),
            content_digest: Some("digest-abc"),
            entries: vec![sample_entry("formatter"), sample_entry("deploy")],
            auto_update: false,
        }
    }

    fn snapshot_params<'a>(
        snapshot_id: &'a str,
        marketplace_id: Option<&'a str>,
        entry_name: Option<&'a str>,
        digest: &'a str,
    ) -> crate::repository::NewPluginSnapshot<'a> {
        crate::repository::NewPluginSnapshot {
            snapshot_id,
            name: "formatter",
            version: "1.0.0",
            source_kind: "codebuddy-plugin",
            source_uri: Some("/tmp/market/plugins/formatter"),
            plugin_id: "formatter",
            declared_version: "1.0.0",
            resolved_revision: None,
            content_digest: digest,
            status: "completed",
            marketplace_id,
            entry_name,
            source_revision: None,
            components: vec![],
        }
    }

    #[tokio::test]
    async fn insert_then_get_and_list_active_only() {
        let (repo, _db) = setup().await;
        let row = repo
            .insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();
        assert_eq!(row.marketplace_id, "company-tools");
        assert_eq!(row.entries().len(), 2);
        assert_eq!(row.entries()[0].name, "formatter");

        let found = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(found.content_digest.as_deref(), Some("digest-abc"));

        let listed = repo.list_marketplaces().await.unwrap();
        assert_eq!(listed.len(), 1);

        repo.soft_remove_marketplace("company-tools", 99).await.unwrap();
        assert!(repo.list_marketplaces().await.unwrap().is_empty());
        let removed = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(removed.removed_at, Some(99));
        assert_eq!(removed.enabled, 0);
    }

    #[tokio::test]
    async fn same_source_twice_is_rejected() {
        let (repo, _db) = setup().await;
        repo.insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();
        let dup = repo
            .insert_marketplace(sample("other-name", "/tmp/company-tools"))
            .await;
        assert!(dup.is_err(), "same (source_kind, source_uri) must not be re-added");
    }

    #[tokio::test]
    async fn find_by_source_probes_duplicates() {
        let (repo, _db) = setup().await;
        repo.insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();
        let found = repo
            .find_by_source("directory", "/tmp/company-tools")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.marketplace_id, "company-tools");
        assert!(repo
            .find_by_source("directory", "/tmp/other")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn removed_row_is_found_and_then_marked_active_by_reactivate() {
        let (repo, _db) = setup().await;
        repo.insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();
        repo.soft_remove_marketplace("company-tools", 99).await.unwrap();
        // find_by_source still returns the removed row (it owns the source);
        // the caller decides whether to reactivate it.
        let found = repo
            .find_by_source("directory", "/tmp/company-tools")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.removed_at, Some(99));

        repo.reactivate_marketplace(
            "company-tools",
            "directory",
            "/tmp/company-tools-v2",
            &[sample_entry("formatter")],
            "digest-x",
            None,
        )
        .await
        .unwrap();
        let row = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(row.removed_at, None);
        assert_eq!(row.enabled, 1);
        assert_eq!(row.entries().len(), 1);
        // The re-add source is persisted, so the duplicate probe finds the
        // row under its *current* source.
        assert_eq!(row.source_uri, "/tmp/company-tools-v2");
        assert!(repo
            .find_by_source("directory", "/tmp/company-tools-v2")
            .await
            .unwrap()
            .is_some());
        let listed = repo.list_marketplaces().await.unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn entries_update_and_auto_update_toggle() {
        let (repo, _db) = setup().await;
        repo.insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();

        repo.update_marketplace_entries("company-tools", &[sample_entry("only")], "digest-new", Some("2.0.0"))
            .await
            .unwrap();
        let row = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(row.content_digest.as_deref(), Some("digest-new"));
        assert_eq!(row.version.as_deref(), Some("2.0.0"));
        assert_eq!(row.entries().len(), 1);
        assert_eq!(row.entries()[0].name, "only");

        repo.set_auto_update("company-tools", true).await.unwrap();
        let row = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(row.auto_update, 1);

        repo.set_enabled("company-tools", false).await.unwrap();
        let row = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(row.enabled, 0);
    }

    #[tokio::test]
    async fn snapshot_provenance_list_and_clear() {
        let (repo, db) = setup().await;
        let snapshot_repo = crate::repository::SqlitePluginSnapshotRepository::new(db.pool().clone());
        repo.insert_marketplace(sample("company-tools", "/tmp/company-tools"))
            .await
            .unwrap();

        let snap_a = nomifun_common::generate_id();
        let snap_b = nomifun_common::generate_id();
        snapshot_repo
            .insert_snapshot_with_components(snapshot_params(&snap_a, Some("company-tools"), Some("formatter"), "digest-a"))
            .await
            .unwrap();
        snapshot_repo
            .insert_snapshot_with_components(snapshot_params(&snap_b, None, None, "digest-b"))
            .await
            .unwrap();

        let snaps = repo
            .list_snapshots_by_marketplace("company-tools")
            .await
            .unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].snapshot_id, snap_a);
        assert_eq!(snaps[0].entry_name.as_deref(), Some("formatter"));

        repo.clear_snapshot_provenance(&snap_a).await.unwrap();
        assert!(repo
            .list_snapshots_by_marketplace("company-tools")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn resolved_revision_records_internal_traceability() {
        let (repo, _db) = setup().await;
        repo.insert_marketplace(sample("company-tools", "https://github.com/org/tools"))
            .await
            .unwrap();

        repo.record_resolved_revision("company-tools", "abc123def", "/tmp/staging/company-tools")
            .await
            .unwrap();
        let row = repo.get_marketplace("company-tools").await.unwrap().unwrap();
        assert_eq!(row.resolved_revision.as_deref(), Some("abc123def"));
        assert_eq!(row.staging_root.as_deref(), Some("/tmp/staging/company-tools"));
        assert!(row.last_checked_at.is_none(), "record_revision does not touch last_checked");
    }
}
