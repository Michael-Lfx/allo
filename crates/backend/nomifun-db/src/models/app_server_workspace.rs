use serde::{Deserialize, Serialize};

pub const APP_SERVER_WORKSPACE_STATUS_ACTIVE: &str = "active";
pub const APP_SERVER_WORKSPACE_STATUS_REVOKED: &str = "revoked";

/// Durable owner-scoped App Server workspace registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppServerWorkspaceRow {
    pub id: i64,
    pub workspace_id: String,
    pub user_id: String,
    pub root_path: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}
