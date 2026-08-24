//! Default Flowy server configuration applied on first launch.

use nomi_config::{
    GatewayConfig, InsightsConfig, MediaGenConfig, ServerConfig, ServerLoginMethod,
    DEFAULT_FLOWY_WEBSITE_URL, DEFAULT_WECHAT_FLOWY_SERVER_BASE, LEGACY_WECHAT_FLOWY_SERVER_BASE,
};

/// Built-in provider row id synced after cloud login.
pub use nomifun_common::FLOWY_BUILTIN_PROVIDER_ID;

/// Apply production defaults when server is not yet configured.
pub fn ensure_gateway_defaults(config: &mut GatewayConfig) {
    config.server.rewrite_legacy_cn_hosts();

    if config.server.base_url.trim().is_empty() {
        config.server = default_server_config();
    } else if !config.server.enabled {
        config.server.enabled = true;
    }

    if config.server.website_url.trim().is_empty() {
        config.server.website_url = DEFAULT_FLOWY_WEBSITE_URL.to_string();
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
        website_url: DEFAULT_FLOWY_WEBSITE_URL.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::ServerLoginMethod;

    #[test]
    fn ensure_gateway_defaults_fills_empty_base_url() {
        let mut cfg = GatewayConfig::default();
        assert!(cfg.server.base_url.is_empty());
        ensure_gateway_defaults(&mut cfg);
        assert_eq!(cfg.server.base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);
        assert_eq!(cfg.server.website_url, DEFAULT_FLOWY_WEBSITE_URL);
        assert!(cfg.server.enabled);
        assert_eq!(cfg.media.provider, "flowy");
        assert_eq!(cfg.server.auth.preferred_method, ServerLoginMethod::EmailOtp);
    }

    #[test]
    fn ensure_gateway_defaults_rewrites_legacy_cn_base_url() {
        let mut cfg = GatewayConfig {
            server: ServerConfig {
                enabled: true,
                base_url: LEGACY_WECHAT_FLOWY_SERVER_BASE.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        ensure_gateway_defaults(&mut cfg);
        assert_eq!(cfg.server.base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);
    }
}
