//! Thin activation orchestration over `McpConfigService` + a connection tester.
//!
//! This is the single seam for the "明确添加并启用" flow: import stays disabled,
//! the test reads its transport **only** from the persisted DB row (clients and
//! agents never re-submit transport for a saved server), and enabling happens
//! server-side only after a successful test against an unchanged configuration.

use std::sync::Arc;

use nomifun_api_types::{McpActivationResponse, McpConnectionTestResult, McpServerId, McpTestByIdResponse};

use crate::connection_test::McpConnectionTestService;
use crate::error::McpError;
use crate::service::McpConfigService;
use crate::types::{McpServer, McpServerTransport};

/// Async connection tester abstraction, so activation orchestration can be
/// integration-tested without spawning real MCP servers.
#[async_trait::async_trait]
pub trait McpConnectionTester: Send + Sync {
    async fn test_connection(&self, name: &str, transport: &McpServerTransport) -> McpConnectionTestResult;
}

#[async_trait::async_trait]
impl McpConnectionTester for McpConnectionTestService {
    async fn test_connection(&self, name: &str, transport: &McpServerTransport) -> McpConnectionTestResult {
        McpConnectionTestService::test_connection(self, name, transport).await
    }
}

/// Orchestrates test-by-id and the explicit "test and enable" activation flow.
///
/// Both actions persist the test result (status, last_connected, discovered
/// tools) through [`McpConfigService::persist_test_result`], so the DB row
/// stays the single source of truth for the UI, the conversation catalog, and
/// the Agent factories.
#[derive(Clone)]
pub struct McpActivationService {
    config: McpConfigService,
    tester: Arc<dyn McpConnectionTester>,
}

impl McpActivationService {
    pub fn new(config: McpConfigService, tester: Arc<dyn McpConnectionTester>) -> Self {
        Self { config, tester }
    }

    /// Test a saved MCP server by ID.
    ///
    /// The transport comes from the persisted row; the result (success or
    /// failure) is persisted before returning. This never fails the request
    /// because of a test outcome — failures are data.
    pub async fn test_server_by_id(&self, mcp_server_id: &McpServerId) -> Result<McpTestByIdResponse, McpError> {
        let server = self.load_server(mcp_server_id).await?;
        let revision = self.config.config_revision(mcp_server_id).await?;
        let result = self
            .tester
            .test_connection(&server.name, &server.transport)
            .await;
        let committed = self
            .config
            .persist_test_result_at_revision(mcp_server_id, revision, &result, server.enabled)
            .await?;
        let server = self.load_server(mcp_server_id).await?;
        Ok(McpTestByIdResponse {
            server: server.into_response(),
            test: result,
            config_changed: !committed,
        })
    }

    /// Test a saved MCP server and, only on success, enable it.
    ///
    /// Enabling is refused (the server stays disabled) when the test failed,
    /// the server requires authentication, or the persisted configuration
    /// changed while the test was running — a stale `connected` status must
    /// never flip the enabled flag. Results from a drifted run are not
    /// persisted: they describe a configuration that no longer exists.
    pub async fn test_and_enable(&self, mcp_server_id: &McpServerId) -> Result<McpActivationResponse, McpError> {
        let tested = self.load_server(mcp_server_id).await?;
        let config_snapshot = transport_snapshot(&tested.transport);
        let expected_revision = self.config.config_revision(mcp_server_id).await?;

        let result = self
            .tester
            .test_connection(&tested.name, &tested.transport)
            .await;
        let needs_auth = result.needs_auth == Some(true);

        let mut rejection: Option<String> = None;
        if !result.success {
            rejection = Some(
                result
                    .error
                    .clone()
                    .unwrap_or_else(|| "MCP connection test failed".to_owned()),
            );
        } else if needs_auth {
            rejection = Some("MCP server requires authentication before it can be enabled".to_owned());
        }

        // The persisted row remains the authority. A successful activation
        // commits the test result and enabled flag in one conditional update;
        // a failed test preserves the current enabled state so an already
        // enabled server is not unexpectedly disabled by a manual retry.
        let should_enable = rejection.is_none();
        let committed = self
            .config
            .persist_test_result_at_revision(
                mcp_server_id,
                expected_revision,
                &result,
                if should_enable { true } else { tested.enabled },
            )
            .await?;

        let current = self.load_server(mcp_server_id).await?;
        if !committed || transport_snapshot(&current.transport) != config_snapshot {
            return Ok(McpActivationResponse {
                server: current.into_response(),
                test: result,
                enabled: false,
                needs_auth,
                enable_rejected_reason: Some(
                    "MCP server configuration changed while the test was running; test again before enabling"
                        .to_owned(),
                ),
                config_changed: true,
            });
        }

        let enabled = current.enabled;
        if !enabled && rejection.is_none() {
            rejection = Some("MCP server could not be enabled after a successful test".to_owned());
        }

        Ok(McpActivationResponse {
            server: current.into_response(),
            test: result,
            enabled,
            needs_auth,
            enable_rejected_reason: rejection,
            config_changed: false,
        })
    }

    async fn load_server(&self, mcp_server_id: &McpServerId) -> Result<McpServer, McpError> {
        self.config.get_server_model(mcp_server_id).await
    }
}

/// Canonical transport snapshot used to detect config changes during a test.
fn transport_snapshot(transport: &McpServerTransport) -> String {
    transport.to_config_json().unwrap_or_default()
}
