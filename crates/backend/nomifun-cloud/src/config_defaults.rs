//! Default Flowy server configuration applied on first launch.

use nomi_config::{
    GatewayConfig, InsightsConfig, MediaGenConfig, ServerConfig, ServerLoginMethod,
    DEFAULT_WECHAT_FLOWY_SERVER_BASE,
};

/// Built-in provider row id synced after cloud login.
pub use nomifun_common::FLOWY_BUILTIN_PROVIDER_ID;

/// Apply production defaults when server is not yet configured.
pub fn ensure_gateway_defaults(config: &mut GatewayConfig) {
    if config.server.base_url.trim().is_empty() {
        config.server = default_server_config();
    } else if !config.server.enabled {
        config.server.enabled = true;
    }

    if config.server.auth.preferred_method == ServerLoginMethod::WechatQr
        && config.server.base_url == DEFAULT_WECHAT_FLOWY_SERVER_BASE
    {
        config.server.auth.preferred_method = ServerLoginMethod::EmailOtp;
    }

    if config.media.provider.trim().is_empty() {
        config.media.provider = "flowy".to_string();
    }

    // Insights endpoint is derived from server.base_url at runtime; leave empty in yaml.
    if config.insights.contribution.endpoint.trim().is_empty() {
        config.insights.contribution.on_session_end = true;
        config.insights.contribution.redacted_body = true;
    }
}

pub fn default_server_config() -> ServerConfig {
    ServerConfig {
        enabled: true,
        base_url: DEFAULT_WECHAT_FLOWY_SERVER_BASE.to_string(),
        channel: "flowy".to_string(),
        app: "flowymes".to_string(),
        auth: nomi_config::ServerAuthConfig {
            preferred_method: ServerLoginMethod::EmailOtp,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Derive insights batch upload URL from the Flowy server root.
pub fn insights_batch_endpoint(server: &ServerConfig) -> String {
    format!(
        "{}/v1/insights/batch",
        server.base_url.trim().trim_end_matches('/')
    )
}

pub fn default_gateway_config() -> GatewayConfig {
    GatewayConfig {
        server: default_server_config(),
        media: MediaGenConfig {
            provider: "flowy".to_string(),
            ..Default::default()
        },
        insights: InsightsConfig::default(),
        ..Default::default()
    }
}
