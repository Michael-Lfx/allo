use std::path::Path;

use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{APP_SERVER_WORKSPACE_STATUS_ACTIVE, AppServerWorkspaceRow};
use crate::repository::app_server_workspace::IAppServerWorkspaceRepository;

#[derive(Clone, Debug)]
pub struct SqliteAppServerWorkspaceRepository {
    pool: SqlitePool,
}

impl SqliteAppServerWorkspaceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn validate_owner_id(owner_id: &str) -> Result<(), DbError> {
    nomifun_common::UserId::parse(owner_id.to_owned()).map_err(|error| {
        DbError::Conflict(format!(
            "App Server workspace owner_id must be a canonical UUIDv7: {error}"
        ))
    })?;
    Ok(())
}

fn validate_workspace_id(workspace_id: &str) -> Result<(), DbError> {
    nomifun_common::validate_uuidv7(workspace_id).map_err(|error| {
        DbError::Conflict(format!(
            "App Server workspace_id must be a canonical UUIDv7: {error}"
        ))
    })?;
    Ok(())
}

fn canonical_directory(root_path: &str) -> Result<String, DbError> {
    if root_path.trim().is_empty() {
        return Err(DbError::Conflict(
            "App Server workspace root_path must not be empty".to_owned(),
        ));
    }
    let path = Path::new(root_path);
    if !path.is_absolute() {
        return Err(DbError::Conflict(
            "App Server workspace root_path must be absolute".to_owned(),
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        DbError::Conflict(format!(
            "App Server workspace root_path must name an existing directory: {error}"
        ))
    })?;
    if !canonical.is_dir() {
        return Err(DbError::Conflict(
            "App Server workspace root_path must name an existing directory".to_owned(),
        ));
    }
    canonical.into_os_string().into_string().map_err(|_| {
        DbError::Conflict("App Server workspace root_path must be valid UTF-8".to_owned())
    })
}

async fn load_by_owner_and_root(
    pool: &SqlitePool,
    owner_id: &str,
    root_path: &str,
) -> Result<Option<AppServerWorkspaceRow>, DbError> {
    sqlx::query_as::<_, AppServerWorkspaceRow>(
        "SELECT * FROM app_server_workspaces WHERE user_id = ? AND root_path = ?",
    )
    .bind(owner_id)
    .bind(root_path)
    .fetch_optional(pool)
    .await
    .map_err(DbError::Query)
}

#[async_trait::async_trait]
impl IAppServerWorkspaceRepository for SqliteAppServerWorkspaceRepository {
    async fn get(
        &self,
        owner_id: &str,
        workspace_id: &str,
    ) -> Result<Option<AppServerWorkspaceRow>, DbError> {
        validate_owner_id(owner_id)?;
        validate_workspace_id(workspace_id)?;
        sqlx::query_as::<_, AppServerWorkspaceRow>(
            "SELECT * FROM app_server_workspaces WHERE user_id = ? AND workspace_id = ?",
        )
        .bind(owner_id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(DbError::Query)
    }

    async fn register(
        &self,
        owner_id: &str,
        workspace_id: &str,
        root_path: &str,
    ) -> Result<AppServerWorkspaceRow, DbError> {
        validate_owner_id(owner_id)?;
        validate_workspace_id(workspace_id)?;
        let root_path = canonical_directory(root_path)?;
        let now = nomifun_common::now_ms();

        let row = sqlx::query_as::<_, AppServerWorkspaceRow>(
            "INSERT INTO app_server_workspaces (\
                 workspace_id, user_id, root_path, status, created_at, updated_at\
             ) VALUES (?, ?, ?, 'active', ?, ?) \
             ON CONFLICT(workspace_id) DO UPDATE SET \
                 root_path = excluded.root_path, status = 'active', updated_at = excluded.updated_at \
             WHERE app_server_workspaces.user_id = excluded.user_id \
             RETURNING *",
        )
        .bind(workspace_id)
        .bind(owner_id)
        .bind(&root_path)
        .bind(now)
        .bind(now)
        .fetch_optional(&self.pool)
        .await;

        match row {
            Ok(Some(row)) => Ok(row),
            Ok(None) => Err(DbError::Conflict(
                "App Server workspace_id is already registered to another owner".to_owned(),
            )),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(DbError::Conflict(
                    "App Server workspace root_path is already registered under another workspace_id"
                        .to_owned(),
                ))
            }
            Err(error) => Err(DbError::Query(error)),
        }
    }

    async fn ensure_default(
        &self,
        owner_id: &str,
        root_path: &str,
    ) -> Result<AppServerWorkspaceRow, DbError> {
        validate_owner_id(owner_id)?;
        let root_path = canonical_directory(root_path)?;
        if let Some(row) = load_by_owner_and_root(&self.pool, owner_id, &root_path).await? {
            if row.status == APP_SERVER_WORKSPACE_STATUS_ACTIVE {
                return Ok(row);
            }
            return self.register(owner_id, &row.workspace_id, &root_path).await;
        }

        let workspace_id = uuid::Uuid::now_v7().to_string();
        match self.register(owner_id, &workspace_id, &root_path).await {
            Ok(row) => Ok(row),
            Err(DbError::Conflict(_)) => load_by_owner_and_root(&self.pool, owner_id, &root_path)
                .await?
                .ok_or_else(|| {
                    DbError::Conflict(
                        "App Server workspace root_path could not be registered".to_owned(),
                    )
                }),
            Err(error) => Err(error),
        }
    }

    async fn list_active(
        &self,
        owner_id: &str,
    ) -> Result<Vec<AppServerWorkspaceRow>, DbError> {
        validate_owner_id(owner_id)?;
        sqlx::query_as::<_, AppServerWorkspaceRow>(
            "SELECT * FROM app_server_workspaces \
             WHERE user_id = ? AND status = 'active' \
             ORDER BY updated_at DESC, id DESC",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(DbError::Query)
    }

    async fn revoke(
        &self,
        owner_id: &str,
        workspace_id: &str,
    ) -> Result<bool, DbError> {
        validate_owner_id(owner_id)?;
        validate_workspace_id(workspace_id)?;
        let updated_at = nomifun_common::now_ms();
        let result = sqlx::query(
            "UPDATE app_server_workspaces \
             SET status = 'revoked', updated_at = ? \
             WHERE user_id = ? AND workspace_id = ? AND status = 'active'",
        )
        .bind(updated_at)
        .bind(owner_id)
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
