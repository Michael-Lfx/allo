//! Resolve insights contribution settings from server login (no manual endpoint/token).

use nomi_config::{InsightsContributionConfig, ServerConfig};

use crate::config_defaults::insights_batch_endpoint;
use crate::session::ServerSession;

/// Build effective insights config: endpoint from server root, token from encrypted session store.
pub async fn effective_insights_contribution_config(
    mut config: InsightsContributionConfig,
    server: &ServerConfig,
    data_dir: &std::path::Path,
) -> InsightsContributionConfig {
    if config.endpoint.trim().is_empty() && server.api_ready() {
        config.endpoint = insights_batch_endpoint(server);
    }

    if config.auth_token.as_ref().is_none_or(|t| t.trim().is_empty()) {
        if let Ok(Some(token)) = ServerSession::from_config(server, data_dir)
            .access_token()
            .await
        {
            if !token.trim().is_empty() {
                config.auth_token = Some(token);
            }
        }
    }

    config
}
