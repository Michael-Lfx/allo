use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{PluginSnapshotComponentRow, PluginSnapshotListRow, PluginSnapshotRow};
use crate::repository::plugin_snapshot::{
    ComponentRuntimeRef, IPluginSnapshotRepository, NewPluginSnapshot,
};

/// SQLite-backed implementation of [`IPluginSnapshotRepository`].
#[derive(Clone, Debug)]
pub struct SqlitePluginSnapshotRepository {
    pool: SqlitePool,
}

impl SqlitePluginSnapshotRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IPluginSnapshotRepository for SqlitePluginSnapshotRepository {
    async fn get_by_snapshot_id(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<PluginSnapshotRow>, DbError> {
        let row = sqlx::query_as::<_, PluginSnapshotRow>(
            "SELECT * FROM plugin_snapshots WHERE snapshot_id = ?",
        )
        .bind(snapshot_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_components(
        &self,
        snapshot_id: &str,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginSnapshotComponentRow>(
            "SELECT * FROM plugin_snapshot_components WHERE snapshot_id = ? \
             ORDER BY id ASC",
        )
        .bind(snapshot_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_snapshots(&self, limit: u32) -> Result<Vec<PluginSnapshotListRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginSnapshotListRow>(
            "SELECT snapshots.*, COUNT(components.id) AS component_count \
             FROM plugin_snapshots snapshots \
             LEFT JOIN plugin_snapshot_components components \
               ON components.snapshot_id = snapshots.snapshot_id \
             GROUP BY snapshots.id \
             ORDER BY snapshots.imported_at DESC, snapshots.id DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn find_by_identity_digest(
        &self,
        plugin_id: &str,
        declared_version: &str,
        content_digest: &str,
    ) -> Result<Option<PluginSnapshotRow>, DbError> {
        let row = sqlx::query_as::<_, PluginSnapshotRow>(
            "SELECT * FROM plugin_snapshots \
             WHERE plugin_id = ? AND declared_version = ? AND content_digest = ?",
        )
        .bind(plugin_id)
        .bind(declared_version)
        .bind(content_digest)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_by_identity(
        &self,
        plugin_id: &str,
        declared_version: &str,
    ) -> Result<Vec<PluginSnapshotRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginSnapshotRow>(
            "SELECT * FROM plugin_snapshots \
             WHERE plugin_id = ? AND declared_version = ?",
        )
        .bind(plugin_id)
        .bind(declared_version)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn find_snapshot_by_provenance(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<Option<PluginSnapshotRow>, DbError> {
        let row = sqlx::query_as::<_, PluginSnapshotRow>(
            "SELECT * FROM plugin_snapshots \
             WHERE marketplace_id = ? AND entry_name = ? \
             ORDER BY imported_at DESC, id DESC LIMIT 1",
        )
        .bind(marketplace_id)
        .bind(entry_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn insert_snapshot_with_components(
        &self,
        params: NewPluginSnapshot<'_>,
    ) -> Result<PluginSnapshotRow, DbError> {
        let mut tx = self.pool.begin().await?;
        let now = nomifun_common::now_ms();
        let result = sqlx::query(
            "INSERT INTO plugin_snapshots \
                (snapshot_id, name, version, source_kind, source_uri, plugin_id, \
                 declared_version, resolved_revision, content_digest, status, \
                 imported_at, updated_at, marketplace_id, entry_name, source_revision) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(params.snapshot_id)
        .bind(params.name)
        .bind(params.version)
        .bind(params.source_kind)
        .bind(params.source_uri)
        .bind(params.plugin_id)
        .bind(params.declared_version)
        .bind(params.resolved_revision)
        .bind(params.content_digest)
        .bind(params.status)
        .bind(now)
        .bind(now)
        .bind(params.marketplace_id)
        .bind(params.entry_name)
        .bind(params.source_revision)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::Init("plugin snapshot row not inserted".into()));
        }
        for component in &params.components {
            sqlx::query(
                "INSERT INTO plugin_snapshot_components \
                    (snapshot_id, component_id, kind, name, relative_path, \
                     compatibility_json, payload_json) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(params.snapshot_id)
            .bind(component.component_id)
            .bind(component.kind)
            .bind(component.name)
            .bind(component.relative_path)
            .bind(component.compatibility_json)
            .bind(component.payload_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        // Return the stored row (generated row id) back to the caller.
        let row = self
            .get_by_snapshot_id(params.snapshot_id)
            .await?
            .ok_or_else(|| DbError::Init("plugin snapshot missing after insert".into()))?;
        Ok(row)
    }

    async fn list_components_by_kind(
        &self,
        kind: &str,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError> {
        let rows = sqlx::query_as::<_, PluginSnapshotComponentRow>(
            "SELECT components.* FROM plugin_snapshot_components components \
             JOIN plugin_snapshots snapshots ON snapshots.snapshot_id = components.snapshot_id \
             WHERE components.kind = ? \
             ORDER BY snapshots.imported_at DESC, snapshots.id DESC, components.id ASC",
        )
        .bind(kind)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn mark_components_installed(
        &self,
        refs: &[ComponentRuntimeRef<'_>],
        installed_at: i64,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for iter_ref in refs {
            let runtime_json = serde_json::json!({
                "type": iter_ref.runtime_type,
                "location": iter_ref.location,
                "mcp_server_id": iter_ref.mcp_server_id,
            })
            .to_string();
            sqlx::query(
                "UPDATE plugin_snapshot_components SET \
                    installed = 1, disabled = 0, installed_at = ?, preset_id = ?, \
                    runtime_ref = ? \
                 WHERE component_id = ?",
            )
            .bind(installed_at)
            .bind(if iter_ref.runtime_type == "preset" { iter_ref.location } else { "" })
            .bind(runtime_json)
            .bind(iter_ref.component_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn set_components_disabled(
        &self,
        component_ids: &[&str],
        disabled: bool,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for component_id in component_ids {
            sqlx::query(
                "UPDATE plugin_snapshot_components SET disabled = ? WHERE component_id = ?",
            )
            .bind(if disabled { 1_i64 } else { 0_i64 })
            .bind(component_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn clear_components_installed(
        &self,
        component_ids: &[&str],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for component_id in component_ids {
            sqlx::query(
                "UPDATE plugin_snapshot_components SET \
                    installed = 0, disabled = 0, installed_at = NULL, \
                    preset_id = NULL, runtime_ref = NULL \
                 WHERE component_id = ?",
            )
            .bind(component_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_installation_state(
        &self,
        snapshot_id: Option<&str>,
    ) -> Result<Vec<PluginSnapshotComponentRow>, DbError> {
        let rows = match snapshot_id {
            Some(snapshot_id) => {
                sqlx::query_as::<_, PluginSnapshotComponentRow>(
                    "SELECT * FROM plugin_snapshot_components WHERE snapshot_id = ? \
                     ORDER BY id ASC",
                )
                .bind(snapshot_id)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, PluginSnapshotComponentRow>(
                    "SELECT * FROM plugin_snapshot_components \
                     WHERE installed = 1 OR disabled = 1 \
                     ORDER BY id ASC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_database_memory;
    use crate::repository::plugin_snapshot::NewPluginSnapshotComponent;

    async fn setup() -> (SqlitePluginSnapshotRepository, crate::Database) {
        let db = init_database_memory().await.unwrap();
        let repo = SqlitePluginSnapshotRepository::new(db.pool().clone());
        (repo, db)
    }

    fn sample<'a>(snapshot_id: &'a str, digest: &'a str) -> NewPluginSnapshot<'a> {
        NewPluginSnapshot {
            snapshot_id,
            name: "software-company",
            version: "1.0.0",
            source_kind: "codebuddy-plugin",
            source_uri: Some("/local/source/software-company"),
            plugin_id: "software-company",
            declared_version: "1.0.0",
            resolved_revision: None,
            content_digest: digest,
            status: "completed",
            marketplace_id: Some("company-tools"),
            entry_name: Some("formatter"),
            source_revision: None,
            components: vec![
                NewPluginSnapshotComponent {
                    component_id: "wb-software-company-software-team-lead",
                    kind: "agent",
                    name: "software-team-lead",
                    relative_path: Some("agents/software-team-lead.md"),
                    compatibility_json: r#"{"semantic_status":"compatible_with_adapter"}"#,
                    payload_json: r#"{"id":"wb-software-company-software-team-lead"}"#,
                },
                NewPluginSnapshotComponent {
                    component_id: "wb-software-company-team",
                    kind: "team",
                    name: "software-company",
                    relative_path: None,
                    compatibility_json: r#"{"semantic_status":"compatible_with_adapter"}"#,
                    payload_json: r#"{"id":"wb-software-company-team"}"#,
                },
            ],
        }
    }

    #[tokio::test]
    async fn insert_then_get_snapshot_and_components() {
        let (repo, _db) = setup().await;
        let snapshot_id = nomifun_common::generate_id();
        let row = repo
            .insert_snapshot_with_components(sample(&snapshot_id, "digest-a"))
            .await
            .unwrap();
        assert_eq!(row.snapshot_id, snapshot_id);
        assert_eq!(row.content_digest, "digest-a");
        assert!(row.id > 0);

        let found = repo.get_by_snapshot_id(&snapshot_id).await.unwrap().unwrap();
        assert_eq!(found.id, row.id);
        assert_eq!(found.status, "completed");

        let components = repo.get_components(&snapshot_id).await.unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].kind, "agent");
        assert_eq!(components[1].kind, "team");
        assert_eq!(components[0].snapshot_id, snapshot_id);
    }

    #[tokio::test]
    async fn duplicate_snapshot_id_is_rejected() {
        let (repo, _db) = setup().await;
        let snapshot_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&snapshot_id, "digest-a"))
            .await
            .unwrap();
        let conflict = repo
            .insert_snapshot_with_components(sample(&snapshot_id, "digest-b"))
            .await;
        assert!(conflict.is_err(), "same snapshot_id must not be inserted twice");
    }

    #[tokio::test]
    async fn identity_digest_probes_and_conflict_probe() {
        let (repo, _db) = setup().await;
        let snapshot_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&snapshot_id, "digest-a"))
            .await
            .unwrap();

        let same = repo
            .find_by_identity_digest("software-company", "1.0.0", "digest-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(same.snapshot_id, snapshot_id);
        assert!(repo
            .find_by_identity_digest("software-company", "1.0.0", "digest-other")
            .await
            .unwrap()
            .is_none());

        let by_identity = repo
            .list_by_identity("software-company", "1.0.0")
            .await
            .unwrap();
        assert_eq!(by_identity.len(), 1);
        assert!(repo
            .list_by_identity("software-company", "2.0.0")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn provenance_lookup_returns_newest_matching_snapshot() {
        let (repo, _db) = setup().await;
        let first_id = nomifun_common::generate_id();
        let second_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&first_id, "digest-a"))
            .await
            .unwrap();
        repo.insert_snapshot_with_components(sample(&second_id, "digest-b"))
            .await
            .unwrap();

        let found = repo
            .find_snapshot_by_provenance("company-tools", "formatter")
            .await
            .unwrap()
            .expect("provenance snapshot must be found");
        assert_eq!(found.snapshot_id, second_id, "newest import wins");
        assert_eq!(found.content_digest, "digest-b");
        assert!(repo
            .find_snapshot_by_provenance("company-tools", "missing")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_snapshots_orders_most_recent_first() {
        let (repo, _db) = setup().await;
        let first_id = nomifun_common::generate_id();
        let second_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&first_id, "digest-a"))
            .await
            .unwrap();
        repo.insert_snapshot_with_components(sample(&second_id, "digest-b"))
            .await
            .unwrap();
        let list = repo.list_snapshots(10).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].snapshot.snapshot_id, second_id);
        assert_eq!(list[1].snapshot.snapshot_id, first_id);
        // component counts come back from the JOIN, not placeholders
        assert_eq!(list[0].component_count, 2);
        assert_eq!(list[1].component_count, 2);
        let limited = repo.list_snapshots(1).await.unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].snapshot.snapshot_id, second_id);
    }

    #[tokio::test]
    async fn list_components_by_kind_across_snapshots() {
        let (repo, _db) = setup().await;
        let first_id = nomifun_common::generate_id();
        let second_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&first_id, "digest-a"))
            .await
            .unwrap();
        repo.insert_snapshot_with_components(sample(&second_id, "digest-b"))
            .await
            .unwrap();
        let agents = repo.list_components_by_kind("agent").await.unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].snapshot_id, second_id, "newest snapshot first");
        let teams = repo.list_components_by_kind("team").await.unwrap();
        assert_eq!(teams.len(), 2);
        assert!(repo
            .list_components_by_kind("nothing")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn installer_state_roundtrip_mark_disable_clear() {
        let (repo, _db) = setup().await;
        let snapshot_id = nomifun_common::generate_id();
        repo.insert_snapshot_with_components(sample(&snapshot_id, "digest-a"))
            .await
            .unwrap();
        let agent_id = "wb-software-company-software-team-lead";
        let team_id = "wb-software-company-team";

        // initial state: both not installed
        let before = repo.list_installation_state(Some(&snapshot_id)).await.unwrap();
        assert!(before.iter().all(|row| row.installed == 0 && row.disabled == 0));

        // mark agent installed (skill ref) and team installed (preset ref)
        let refs = [
            ComponentRuntimeRef {
                component_id: agent_id,
                runtime_type: "skill",
                location: "/data/skills/agent-store/software-company",
                mcp_server_id: None,
            },
            ComponentRuntimeRef {
                component_id: team_id,
                runtime_type: "preset",
                location: "preset-abc",
                mcp_server_id: None,
            },
        ];
        repo.mark_components_installed(&refs, 42).await.unwrap();

        let after = repo.list_installation_state(Some(&snapshot_id)).await.unwrap();
        let agent = after.iter().find(|row| row.component_id == agent_id).unwrap();
        assert_eq!(agent.installed, 1);
        assert_eq!(agent.disabled, 0);
        assert_eq!(agent.runtime_ref.as_deref().unwrap().contains("agent-store"), true);
        let team = after.iter().find(|row| row.component_id == team_id).unwrap();
        assert_eq!(team.installed, 1);
        assert_eq!(team.preset_id.as_deref(), Some("preset-abc"));

        // disable agent
        repo.set_components_disabled(&[agent_id], true).await.unwrap();
        let disabled = repo.list_installation_state(Some(&snapshot_id)).await.unwrap();
        let agent = disabled.iter().find(|row| row.component_id == agent_id).unwrap();
        assert_eq!(agent.disabled, 1);
        assert_eq!(agent.installed, 1, "disabled keeps installed flag");

        // uninstall both (clear state, keep rows)
        repo.clear_components_installed(&[agent_id, team_id]).await.unwrap();
        let cleared = repo.list_installation_state(Some(&snapshot_id)).await.unwrap();
        assert!(cleared.iter().all(|row| row.installed == 0 && row.disabled == 0));
        assert!(cleared.iter().all(|row| row.runtime_ref.is_none() && row.preset_id.is_none()));
        assert_eq!(cleared.len(), 2, "rows are kept after uninstall");

        // global installed projection is empty now
        let global = repo.list_installation_state(None).await.unwrap();
        assert!(global.is_empty());
    }
}