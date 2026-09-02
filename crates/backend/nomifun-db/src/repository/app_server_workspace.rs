use crate::error::DbError;
use crate::models::AppServerWorkspaceRow;

/// Durable owner-scoped registry for local App Server workspaces.
#[async_trait::async_trait]
pub trait IAppServerWorkspaceRepository: Send + Sync {
    /// Find a workspace only when it belongs to `owner_id`.
    async fn get(
        &self,
        owner_id: &str,
        workspace_id: &str,
    ) -> Result<Option<AppServerWorkspaceRow>, DbError>;

    /// Register or reactivate an owner/workspace pair at an existing directory.
    async fn register(
        &self,
        owner_id: &str,
        workspace_id: &str,
        root_path: &str,
    ) -> Result<AppServerWorkspaceRow, DbError>;

    /// Return the active workspace for this root, creating one when absent.
    async fn ensure_default(
        &self,
        owner_id: &str,
        root_path: &str,
    ) -> Result<AppServerWorkspaceRow, DbError>;

    /// List the active workspaces registered to `owner_id`, newest-updated
    /// first. Revoked (and foreign) rows never appear.
    async fn list_active(&self, owner_id: &str) -> Result<Vec<AppServerWorkspaceRow>, DbError>;

    /// Revoke (soft-delete) an active workspace owned by `owner_id`: flips its
    /// `status` to `revoked` so it disappears from `list_active`. Idempotent:
    /// returns `Ok(true)` when an active row was revoked, `Ok(false)` when the
    /// workspace does not exist, is foreign to `owner_id`, or is already
    /// revoked. Never deletes the workspace row, its conversations, or the
    /// underlying directory — `ensure_default` re-activates the same
    /// `workspace_id` on re-registration of the same root.
    async fn revoke(
        &self,
        owner_id: &str,
        workspace_id: &str,
    ) -> Result<bool, DbError>;
}
