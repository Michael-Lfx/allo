use serde::{Deserialize, Serialize};

/// Durable owner-scoped mapping from an App Server public run ID to an internal execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppServerRunMappingRow {
    pub id: i64,
    pub public_run_id: String,
    pub execution_id: String,
    pub user_id: String,
    pub created_at: i64,
}
