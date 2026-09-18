use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::AppServerRunMappingRow;
use crate::repository::app_server_run_mapping::IAppServerRunMappingRepository;

const COLUMNS: &str = "id, public_run_id, execution_id, user_id, created_at";

#[derive(Clone, Debug)]
pub struct SqliteAppServerRunMappingRepository {
    pool: SqlitePool,
}

impl SqliteAppServerRunMappingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn validate_id(kind: &str, value: &str) -> Result<(), DbError> {
    nomifun_common::validate_uuidv7(value)
        .map(|_| ())
        .map_err(|error| {
            DbError::Conflict(format!(
                "App Server run mapping {kind} '{value}' is not a canonical UUIDv7: {error}"
            ))
        })
}

#[async_trait::async_trait]
impl IAppServerRunMappingRepository for SqliteAppServerRunMappingRepository {
    async fn create_mapping(
        &self,
        execution_id: &str,
        owner_user_id: &str,
    ) -> Result<AppServerRunMappingRow, DbError> {
        validate_id("execution_id", execution_id)?;
        validate_id("owner_user_id", owner_user_id)?;

        let public_run_id = uuid::Uuid::now_v7().to_string();
        let created_at = nomifun_common::now_ms();
        let sql = format!(
            "INSERT INTO app_server_run_mappings (public_run_id, execution_id, user_id, created_at) \
             SELECT ?, execution_id, user_id, ? FROM agent_executions \
             WHERE execution_id = ? AND user_id = ? \
             ON CONFLICT(execution_id) DO NOTHING \
             RETURNING {COLUMNS}"
        );
        if let Some(row) = sqlx::query_as::<_, AppServerRunMappingRow>(&sql)
            .bind(&public_run_id)
            .bind(created_at)
            .bind(execution_id)
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?
        {
            return Ok(row);
        }

        if let Some(row) = self
            .get_by_execution_id(execution_id, owner_user_id)
            .await?
        {
            return Ok(row);
        }

        Err(DbError::NotFound(format!(
            "owned AgentExecution {execution_id}"
        )))
    }

    async fn get_by_public_id(
        &self,
        public_run_id: &str,
        owner_user_id: &str,
    ) -> Result<Option<AppServerRunMappingRow>, DbError> {
        validate_id("public_run_id", public_run_id)?;
        validate_id("owner_user_id", owner_user_id)?;
        let sql = format!(
            "SELECT {COLUMNS} FROM app_server_run_mappings \
             WHERE public_run_id = ? AND user_id = ?"
        );
        Ok(sqlx::query_as::<_, AppServerRunMappingRow>(&sql)
            .bind(public_run_id)
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    async fn get_by_execution_id(
        &self,
        execution_id: &str,
        owner_user_id: &str,
    ) -> Result<Option<AppServerRunMappingRow>, DbError> {
        validate_id("execution_id", execution_id)?;
        validate_id("owner_user_id", owner_user_id)?;
        let sql = format!(
            "SELECT {COLUMNS} FROM app_server_run_mappings \
             WHERE execution_id = ? AND user_id = ?"
        );
        Ok(sqlx::query_as::<_, AppServerRunMappingRow>(&sql)
            .bind(execution_id)
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?)
    }
}
