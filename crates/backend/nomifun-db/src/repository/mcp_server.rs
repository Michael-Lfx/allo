use crate::error::DbError;
use crate::models::McpServerRow;

/// MCP server configuration data access abstraction.
///
/// Provides CRUD operations, batch upsert, and name-based lookup
/// on the `mcp_servers` table. JSON fields (`transport_config`, `tools`)
/// are opaque strings at this layer; the service layer handles
/// serialization/deserialization.
///
/// Object-safe via `async_trait` to support `Arc<dyn IMcpServerRepository>`.
#[async_trait::async_trait]
pub trait IMcpServerRepository: Send + Sync {
    /// Returns all MCP servers, ordered by creation time ascending.
    async fn list(&self) -> Result<Vec<McpServerRow>, DbError>;

    /// Finds an MCP server by ID, or `None` if not found.
    async fn find_by_id(&self, mcp_server_id: &str) -> Result<Option<McpServerRow>, DbError>;

    /// Finds an MCP server by name, or `None` if not found.
    async fn find_by_name(&self, name: &str) -> Result<Option<McpServerRow>, DbError>;

    /// Finds an MCP server by ID, including soft-deleted rows.
    async fn find_by_id_any(&self, mcp_server_id: &str) -> Result<Option<McpServerRow>, DbError> {
        self.find_by_id(mcp_server_id).await
    }

    /// Finds an MCP server by name, including soft-deleted rows.
    async fn find_by_name_any(&self, name: &str) -> Result<Option<McpServerRow>, DbError> {
        self.find_by_name(name).await
    }

    /// Finds a set of MCP servers by ID, including soft-deleted rows.
    async fn list_by_ids_any(&self, mcp_server_ids: &[String]) -> Result<Vec<McpServerRow>, DbError> {
        let mut rows = Vec::with_capacity(mcp_server_ids.len());
        for mcp_server_id in mcp_server_ids {
            if let Some(row) = self.find_by_id_any(mcp_server_id).await? {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    /// Creates a new MCP server and returns the inserted row.
    /// Returns `DbError::Conflict` if the name already exists.
    async fn create(&self, params: CreateMcpServerParams<'_>) -> Result<McpServerRow, DbError>;

    /// Updates an existing MCP server. Returns `DbError::NotFound` if the ID
    /// doesn't exist, `DbError::Conflict` if the new name collides with another.
    async fn update(
        &self,
        mcp_server_id: &str,
        params: UpdateMcpServerParams<'_>,
    ) -> Result<McpServerRow, DbError>;

    /// Soft-deletes an MCP server by ID. Returns `DbError::NotFound` if the ID
    /// doesn't exist.
    async fn delete(&self, mcp_server_id: &str) -> Result<(), DbError>;

    /// Upserts multiple servers by name: existing names are updated,
    /// new names are inserted. Returns the count of affected rows.
    async fn batch_upsert(&self, servers: &[CreateMcpServerParams<'_>]) -> Result<Vec<McpServerRow>, DbError>;

    /// Updates only the latest connection-test result status
    /// (and optionally `last_connected`).
    /// Returns `DbError::NotFound` if the ID doesn't exist.
    async fn update_status(
        &self,
        mcp_server_id: &str,
        status: &str,
        last_connected: Option<nomifun_common::TimestampMs>,
    ) -> Result<(), DbError>;

    /// Updates only the tools JSON for a server.
    /// Returns `DbError::NotFound` if the ID doesn't exist.
    async fn update_tools(&self, mcp_server_id: &str, tools: Option<&str>) -> Result<(), DbError>;

    /// Returns the revision of the persisted MCP configuration.
    ///
    /// Implementations that predate the revision column use zero so existing
    /// in-memory repositories remain usable in unit tests.
    async fn config_revision(&self, mcp_server_id: &str) -> Result<i64, DbError> {
        self.find_by_id(mcp_server_id)
            .await?
            .map(|_| 0)
            .ok_or_else(|| DbError::NotFound(format!("MCP server '{mcp_server_id}' not found")))
    }

    /// Atomically persists a test result and the requested enabled state when
    /// the saved configuration still has `expected_revision`.
    ///
    /// Returns `Ok(false)` when the configuration changed while the external
    /// connection test was running. SQLite overrides this with one conditional
    /// UPDATE; the default exists for lightweight test repositories.
    async fn complete_activation(
        &self,
        mcp_server_id: &str,
        expected_revision: i64,
        status: &str,
        last_connected: Option<nomifun_common::TimestampMs>,
        tools: Option<&str>,
        enabled: bool,
    ) -> Result<bool, DbError> {
        if self.config_revision(mcp_server_id).await? != expected_revision {
            return Ok(false);
        }
        self.update_status(mcp_server_id, status, last_connected).await?;
        self.update_tools(mcp_server_id, tools).await?;
        self.update(
            mcp_server_id,
            UpdateMcpServerParams {
                enabled: Some(enabled),
                ..Default::default()
            },
        )
        .await?;
        Ok(true)
    }
}

/// Parameters for creating a new MCP server.
#[derive(Debug, Clone)]
pub struct CreateMcpServerParams<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub enabled: bool,
    pub transport_type: &'a str,
    pub transport_config: &'a str,
    pub tools: Option<&'a str>,
    pub original_json: Option<&'a str>,
    pub builtin: bool,
}

/// Parameters for updating an existing MCP server.
///
/// All fields are optional; `None` means "keep the current value".
/// For nullable fields, `Some(None)` means "clear the value" and
/// `Some(Some(v))` means "set to v".
#[derive(Debug, Default)]
pub struct UpdateMcpServerParams<'a> {
    pub name: Option<&'a str>,
    pub description: Option<Option<&'a str>>,
    pub enabled: Option<bool>,
    pub transport_type: Option<&'a str>,
    pub transport_config: Option<&'a str>,
    pub tools: Option<Option<&'a str>>,
    pub last_test_status: Option<&'a str>,
    pub last_connected: Option<Option<nomifun_common::TimestampMs>>,
    pub original_json: Option<Option<&'a str>>,
    pub builtin: Option<bool>,
    pub deleted_at: Option<Option<nomifun_common::TimestampMs>>,
}
