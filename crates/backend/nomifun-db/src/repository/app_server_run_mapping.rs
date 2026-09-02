use crate::error::DbError;
use crate::models::AppServerRunMappingRow;

/// Durable owner-scoped App Server public run ID mappings.
#[async_trait::async_trait]
pub trait IAppServerRunMappingRepository: Send + Sync {
    /// Create or return the stable public mapping for an existing owned execution.
    async fn create_mapping(
        &self,
        execution_id: &str,
        owner_user_id: &str,
    ) -> Result<AppServerRunMappingRow, DbError>;

    /// Resolve a public run ID only when it belongs to the supplied owner.
    async fn get_by_public_id(
        &self,
        public_run_id: &str,
        owner_user_id: &str,
    ) -> Result<Option<AppServerRunMappingRow>, DbError>;

    /// Resolve an internal execution ID only when it belongs to the supplied owner.
    async fn get_by_execution_id(
        &self,
        execution_id: &str,
        owner_user_id: &str,
    ) -> Result<Option<AppServerRunMappingRow>, DbError>;
}
