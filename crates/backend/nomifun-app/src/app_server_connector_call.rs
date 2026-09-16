//! Connector call proxy (`connector/call`, doc 24 §5).
//!
//! Implements [`ConnectorCallProvider`] over the host's MCP configuration. The
//! point of this module is what it **does not** expose: a caller names a
//! registered connector id and a tool, and everything else — the transport, its
//! headers, its `secret:` env values, the OAuth token, the child process —
//! stays here. That is why there is no `connector/export`: the connection and
//! its credentials never cross the wire, the call does.
//!
//! ## Three gates, in this order
//!
//! 1. **Host policy** (`[connector_proxy]`): off unless the host's own
//!    operator turned it on. Checked before any lookup, so a host that never
//!    opted in does no work at all on a caller's behalf.
//! 2. **Allowlist**: the connector/tool pair must be named explicitly. MCP
//!    tools carry no danger annotation (`DangerTier` is the *gateway's*
//!    vocabulary, for capabilities this codebase wrote), so there is nothing to
//!    infer a default from — the answer has to be written down.
//! 3. **Enabled**: a registered-but-switched-off connector is not callable,
//!    exactly as `agent/run` refuses one.
//!
//! ## What is deliberately absent
//!
//! **No argument logging.** Tool arguments are caller data — potentially user
//! content — so the audit line records the connector, the tool, the outcome and
//! the size, never the payload. **No result redaction** either: an MCP server's
//! result is passed through as-is and only size-capped. Redacting it would mean
//! guessing at an arbitrary schema, and a half-redacted payload is worse than
//! an audited one.

use async_trait::async_trait;
use std::time::Instant;

use nomifun_app_server::{
    ConnectorCallError, ConnectorCallProvider, MAX_CONNECTOR_CALL_RESULT_BYTES,
    agent_store::ConnectorProxyPolicy,
};
use nomifun_api_types::{AppServerConnectorCallResult, McpServerId};
use nomifun_mcp::{McpConfigService, McpConnectionTestService, McpServerTransport, McpToolCallError, McpToolCallPool};

const AUDIT_TARGET: &str = "agent_store_connector_proxy";

/// Read-side proxy that executes one MCP tool call on a caller's behalf.
pub struct AppServerConnectorCall {
    config: McpConfigService,
    calls: McpToolCallPool,
    policy: ConnectorProxyPolicy,
}

impl AppServerConnectorCall {
    pub fn new(
        config: McpConfigService,
        calls: McpConnectionTestService,
        policy: ConnectorProxyPolicy,
    ) -> Self {
        Self { config, calls: McpToolCallPool::new(calls), policy }
    }
}

#[async_trait]
impl ConnectorCallProvider for AppServerConnectorCall {
    async fn call(
        &self,
        connector_id: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<AppServerConnectorCallResult, ConnectorCallError> {
        if tool.trim().is_empty() {
            return Err(ConnectorCallError::InvalidRequest("tool must not be empty".to_owned()));
        }

        // Gate 1, before any lookup: an opted-out host does no work for a caller.
        if !self.policy.is_enabled() {
            return Err(ConnectorCallError::PolicyDenied(
                "the connector call proxy is disabled on this host; \
                 declare [connector_proxy] enabled = true in the host config to turn it on"
                    .to_owned(),
            ));
        }

        let parsed = McpServerId::parse(connector_id).map_err(|error| {
            // A malformed id is "no such connector" from a caller's standpoint;
            // echoing the parser error would only explain our id format.
            ConnectorCallError::NotFound(format!("connector {connector_id} not found: {error}"))
        })?;
        let server = self.config.get_server(&parsed).await.map_err(|error| match error {
            nomifun_mcp::McpError::NotFound(_) => {
                ConnectorCallError::NotFound(format!("connector {connector_id} not found"))
            }
            other => ConnectorCallError::Failed(format!("connector lookup failed: {other}")),
        })?;

        // Gate 2: explicit allowlist. `decide` accepts the id (precise) or the
        // registered name (convenient) — see its doc for why both are offered.
        if let Err(reason) = self.policy.decide(connector_id, &server.name, tool) {
            tracing::warn!(
                target: AUDIT_TARGET,
                connector = %server.name,
                tool = %tool,
                reason = %reason,
                "connector call refused by policy"
            );
            return Err(ConnectorCallError::PolicyDenied(reason));
        }

        // Gate 3: matching `agent/run`'s rule for a disabled MCP server.
        if !server.enabled {
            return Err(ConnectorCallError::Unavailable(format!(
                "connector {} is disabled",
                server.name
            )));
        }

        let started = Instant::now();
        let transport = McpServerTransport::from(server.transport);
        // Keyed by the connector id, not its name: MCP servers upsert by name,
        // so a later install can take a name over and would otherwise inherit
        // the session its predecessor left behind.
        let outcome = self
            .calls
            .call(connector_id, &transport, tool, arguments)
            .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        match outcome {
            Ok(outcome) => {
                // Measured on the serialized result, because that is what would
                // travel; a refusal beats a silently shortened JSON document.
                let encoded = serde_json::to_vec(&outcome.result).unwrap_or_default();
                if encoded.len() > MAX_CONNECTOR_CALL_RESULT_BYTES {
                    tracing::warn!(
                        target: AUDIT_TARGET,
                        connector = %server.name,
                        tool = %tool,
                        bytes = encoded.len(),
                        elapsed_ms,
                        "connector call result exceeds the response budget"
                    );
                    return Err(ConnectorCallError::TooLarge {
                        size: encoded.len(),
                        limit: MAX_CONNECTOR_CALL_RESULT_BYTES,
                    });
                }
                tracing::info!(
                    target: AUDIT_TARGET,
                    connector = %server.name,
                    tool = %tool,
                    is_error = outcome.is_error,
                    bytes = encoded.len(),
                    elapsed_ms,
                    "connector call completed"
                );
                Ok(AppServerConnectorCallResult {
                    is_error: outcome.is_error,
                    result: outcome.result,
                })
            }
            Err(McpToolCallError::Timeout(budget)) => {
                tracing::warn!(
                    target: AUDIT_TARGET,
                    connector = %server.name,
                    tool = %tool,
                    elapsed_ms,
                    "connector call timed out"
                );
                Err(ConnectorCallError::Timeout { seconds: budget.as_secs() })
            }
            Err(McpToolCallError::Failed(message)) => {
                tracing::warn!(
                    target: AUDIT_TARGET,
                    connector = %server.name,
                    tool = %tool,
                    elapsed_ms,
                    error = %message,
                    "connector call failed"
                );
                Err(ConnectorCallError::Failed(message))
            }
        }
    }
}
