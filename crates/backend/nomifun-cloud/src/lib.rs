//! Remote LLM server client — authentication and OpenAI-compatible inference gateway.
//!
//! Agent business logic (AgentLoop, tools, sessions) stays local; this crate only
//! talks to the server for login and LLM HTTP calls.

pub mod activation;
pub mod auth;
pub mod config_defaults;
pub mod doctor;
pub mod error;
pub mod flowy;
pub mod http_routes;
pub mod http_service;
pub mod insights_config;
pub mod llm;
pub mod paths;
pub mod platform;
pub mod profile;
pub mod provider_sync;
pub mod session;
pub mod telemetry;
pub mod token_store;
pub mod transport;

pub use activation::{DeviceActivation, DeviceActivationStatus};
pub use config_defaults::{
    FLOWY_BUILTIN_PROVIDER_ID, default_gateway_config, default_server_config,
    ensure_gateway_defaults, insights_batch_endpoint,
};
pub use insights_config::effective_insights_contribution_config;
pub use provider_sync::{disable_flowy_builtin_provider, sync_flowy_builtin_provider};
pub use auth::{
    AuthManager, AuthPollResult, AuthUserInput, LoginMethod, PendingLogin, WhoamiStatus,
};
pub use doctor::{DoctorReport, run_doctor};
pub use error::{CloudError, ServerClientError};
pub use flowy::{
    ClawModelEntry, CreateVideoTaskResponse, CreditsBalance, CreditsCheckinResponse,
    FlowyApiClient, ImageGenerationRequest, MODEL_CATEGORY_ASR, MODEL_CATEGORY_IMAGE,
    MODEL_CATEGORY_VIDEO, UserMe,
    VideoContentImage, VideoCreateParams, VideoTaskRecord, resolve_model_in_catalog,
};
pub use llm::ServerLlmProvider;
pub use profile::ProfileStore;
pub use session::{SERVER_TOKEN_PROVIDER, ServerSession, ServerTokens, TokenSource};
pub use http_routes::{CloudRouterState, cloud_routes};
pub use http_service::CloudService;
pub use telemetry::start_cloud_telemetry;
pub use transport::HttpTransport;

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::ServerConfig;

    #[test]
    fn login_method_parse_aliases() {
        assert_eq!(LoginMethod::parse("wechat"), Some(LoginMethod::WechatQr));
        assert_eq!(LoginMethod::parse("email_otp"), Some(LoginMethod::EmailOtp));
        assert!(LoginMethod::parse("unknown").is_none());
    }

    #[tokio::test]
    async fn auth_manager_missing_base_url() {
        let config = ServerConfig::default();
        let result = AuthManager::new(config, std::env::temp_dir());
        assert!(matches!(result, Err(ServerClientError::MissingBaseUrl)));
    }
}
