//! Auth flow coordination across login methods.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::ServerConfig;
use nomifun_api_types::RuntimeKind;
use tracing::debug;

use super::email_otp::EmailOtpAuthProvider;
use super::provider::{AuthContext, AuthProvider};
use super::types::{AuthPollResult, AuthUserInput, LoginMethod, PendingLogin};
use super::wechat_qr::WeChatQrAuthProvider;
use crate::activation::DeviceActivation;
use crate::error::ServerClientError;
use crate::flowy::{CreditsBalance, CreditsCheckinResponse, FlowyApiClient, UserMe};
use crate::profile::ProfileStore;
use crate::session::{ServerSession, ServerTokens, TokenSource};
use crate::telemetry::spawn_post_login_telemetry;

/// Coordinates remote server login flows.
pub struct AuthManager {
    config: ServerConfig,
    api: FlowyApiClient,
    session: ServerSession,
    data_dir: PathBuf,
    profile_store: ProfileStore,
    providers: Vec<Arc<dyn AuthProvider>>,
    host_runtime: RuntimeKind,
}

impl AuthManager {
    pub fn new(
        config: ServerConfig,
        data_dir: impl AsRef<std::path::Path>,
    ) -> Result<Self, ServerClientError> {
        Self::new_with_host(config, data_dir, RuntimeKind::Web)
    }

    pub fn new_with_host(
        config: ServerConfig,
        data_dir: impl AsRef<std::path::Path>,
        host_runtime: RuntimeKind,
    ) -> Result<Self, ServerClientError> {
        if !config.api_ready() {
            return Err(ServerClientError::MissingBaseUrl);
        }
        let data_dir = data_dir.as_ref().to_path_buf();
        let api = FlowyApiClient::new(&config)?;
        let session = ServerSession::from_config(&config, &data_dir);
        let profile_store = ProfileStore::new(&data_dir);
        let providers: Vec<Arc<dyn AuthProvider>> = vec![
            Arc::new(WeChatQrAuthProvider),
            Arc::new(EmailOtpAuthProvider),
        ];
        Ok(Self {
            config,
            api,
            session,
            data_dir,
            profile_store,
            providers,
            host_runtime,
        })
    }

    pub fn session(&self) -> &ServerSession {
        &self.session
    }

    pub fn api(&self) -> &FlowyApiClient {
        &self.api
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub fn host_runtime(&self) -> RuntimeKind {
        self.host_runtime
    }

    pub fn resolve_method(&self, override_method: Option<LoginMethod>) -> LoginMethod {
        override_method.unwrap_or_else(|| self.config.auth.preferred_method.into())
    }

    fn provider_for(&self, method: LoginMethod) -> Option<&Arc<dyn AuthProvider>> {
        self.providers.iter().find(|p| p.method() == method)
    }

    fn auth_context(&self) -> AuthContext<'_> {
        AuthContext {
            api: &self.api,
            config: &self.config,
        }
    }

    pub async fn start_login(
        &self,
        method: LoginMethod,
    ) -> Result<PendingLogin, ServerClientError> {
        let provider = self.provider_for(method).ok_or_else(|| {
            ServerClientError::NotConfigured(format!("login method {}", method.as_str()))
        })?;
        provider.start(&self.auth_context()).await
    }

    pub async fn continue_login(
        &self,
        pending: &PendingLogin,
        input: AuthUserInput,
    ) -> Result<AuthPollResult, ServerClientError> {
        let provider = self.provider_for(pending.method).ok_or_else(|| {
            ServerClientError::NotConfigured(format!("login method {}", pending.method.as_str()))
        })?;
        let result = provider
            .poll_or_submit(&self.auth_context(), pending, input)
            .await?;
        if let AuthPollResult::Success(tokens) = &result {
            self.finish_login(tokens.clone(), pending.method).await?;
        }
        Ok(result)
    }

    async fn finish_login(
        &self,
        tokens: ServerTokens,
        method: LoginMethod,
    ) -> Result<(), ServerClientError> {
        self.session.save_tokens(tokens).await?;
        let profile = self.api.get_user_me(&self.session).await?;
        self.profile_store.save(&profile).await?;
        debug!(user_id = profile.id, "cached user profile after login");

        // Activation and client-package reporting are best-effort and must not
        // hold the login HTTP response. Startup `ensure_device_telemetry`
        // backfills if the task loses the race with process exit.
        spawn_post_login_telemetry(
            self.config.clone(),
            self.data_dir.clone(),
            self.session.clone(),
            profile.id,
            self.host_runtime,
            Some(method.as_str().to_string()),
            Some(chrono::Utc::now().timestamp_millis()),
        );
        Ok(())
    }

    pub async fn logout(&self) -> Result<bool, ServerClientError> {
        let removed = self.session.logout().await?;
        if removed {
            let _ = self.profile_store.clear().await;
        }
        Ok(removed)
    }

    pub async fn whoami(&self) -> Result<WhoamiStatus, ServerClientError> {
        let source = self.session.token_source().await;
        let tokens = self.session.load_tokens().await?;
        let cached_profile = self.profile_store.load().await?;
        Ok(WhoamiStatus {
            source,
            tokens,
            cached_profile,
            server_enabled: self.config.enabled,
            base_url: self.config.base_url.clone(),
        })
    }

    pub async fn fetch_profile(&self) -> Result<UserMe, ServerClientError> {
        let profile = self.api.get_user_me(&self.session).await?;
        self.profile_store.save(&profile).await?;
        Ok(profile)
    }

    pub async fn update_nickname(&self, nickname: &str) -> Result<UserMe, ServerClientError> {
        let status = self.whoami().await?;
        if !status.is_logged_in() {
            return Err(ServerClientError::AuthRequired(
                "cloud login required".into(),
            ));
        }
        let nickname = self.api.update_nickname(&self.session, nickname).await?;
        match self.fetch_profile().await {
            Ok(profile) => Ok(profile),
            Err(err) => {
                tracing::warn!(error = %err, "refresh profile after nickname update failed");
                let mut profile = self.profile_store.load().await?.unwrap_or_default();
                profile.nickname = Some(nickname);
                self.profile_store.save(&profile).await?;
                Ok(profile)
            }
        }
    }

    /// Best-effort activation for the current user and app version (no-op if already reported).
    pub async fn ensure_device_activation(&self) -> Result<bool, ServerClientError> {
        let status = self.whoami().await?;
        if !status.is_logged_in() {
            return Ok(false);
        }
        let profile = self.fetch_profile().await?;
        DeviceActivation::new(&self.data_dir)
            .try_activate_for_user(
                &self.api,
                &self.session,
                profile.id,
                host_runtime_label(self.host_runtime),
                None,
                None,
            )
            .await
    }

    pub async fn cached_profile(&self) -> Result<Option<UserMe>, ServerClientError> {
        self.profile_store.load().await
    }

    pub async fn credits_balance(&self) -> Result<CreditsBalance, ServerClientError> {
        self.api.get_credits_balance(&self.session).await
    }

    pub async fn credits_checkin(
        &self,
        time_zone: &str,
    ) -> Result<CreditsCheckinResponse, ServerClientError> {
        self.api.credits_checkin(&self.session, time_zone).await
    }

    pub async fn send_bind_email_code(&self, email: &str) -> Result<String, ServerClientError> {
        self.api.send_bind_email_code(&self.session, email).await
    }

    pub async fn bind_email(
        &self,
        email: &str,
        valid_code: &str,
        valid_code_req_no: &str,
    ) -> Result<String, ServerClientError> {
        let jwt = self
            .api
            .bind_email(&self.session, email, valid_code, valid_code_req_no)
            .await?;
        let tokens = ServerTokens::from_jwt(jwt);
        self.session.save_tokens(tokens).await?;
        let profile = self.api.get_user_me(&self.session).await?;
        self.profile_store.save(&profile).await?;
        Ok(profile.display_name())
    }

    pub async fn list_claw_models(
        &self,
        category: Option<i32>,
    ) -> Result<Vec<crate::flowy::ClawModelEntry>, ServerClientError> {
        let models = self
            .api
            .get_available_models_claw(&self.session, category)
            .await?;
        Ok(models.cloud)
    }
}

#[derive(Debug, Clone)]
pub struct WhoamiStatus {
    pub source: TokenSource,
    pub tokens: Option<ServerTokens>,
    pub cached_profile: Option<UserMe>,
    pub server_enabled: bool,
    pub base_url: String,
}

impl WhoamiStatus {
    pub fn is_logged_in(&self) -> bool {
        self.tokens
            .as_ref()
            .map(|t| !t.access_token.is_empty())
            .unwrap_or(false)
    }

    pub fn token_expired(&self) -> bool {
        self.tokens
            .as_ref()
            .map(|t| t.is_expired(0))
            .unwrap_or(false)
    }
}

fn host_runtime_label(runtime: RuntimeKind) -> &'static str {
    match runtime {
        RuntimeKind::Desktop => "desktop",
        RuntimeKind::Web => "web",
    }
}

#[cfg(test)]
mod tests {
    /// `finish_login` must keep only session-critical work on the login request
    /// path; device activation and client-package reporting move to
    /// `spawn_post_login_telemetry` after the profile is saved.
    #[test]
    fn finish_login_spawns_telemetry_after_profile_save() {
        let source = include_str!("manager.rs");
        let start = source.find("async fn finish_login").expect("finish_login fn");
        let end = source.find("pub async fn logout").expect("logout fn");
        let body = &source[start..end];

        assert!(
            body.contains("self.session.save_tokens(tokens).await?;"),
            "tokens must be saved on the request path"
        );
        assert!(
            body.contains("self.api.get_user_me(&self.session).await?;"),
            "profile fetch stays on the request path"
        );
        assert!(
            body.contains("self.profile_store.save(&profile).await?;"),
            "profile save stays on the request path"
        );
        assert!(
            body.contains("spawn_post_login_telemetry("),
            "telemetry must be spawned inside finish_login"
        );

        let after_profile_save = body
            .split("self.profile_store.save(&profile).await?;")
            .nth(1)
            .expect("profile save line");
        assert!(
            after_profile_save.contains("spawn_post_login_telemetry("),
            "telemetry must be spawned after the profile save"
        );
        assert!(
            !after_profile_save.contains(".await"),
            "nothing may block the request path after the profile save"
        );

        assert!(
            !body.contains("try_activate_for_user(&self.api"),
            "activation must not run on the request path"
        );
        assert!(
            !body.contains("report_client_package(&self.session).await"),
            "client package report must not run on the request path"
        );
    }

    #[tokio::test]
    async fn update_nickname_refreshes_cached_profile() {
        use nomi_config::ServerConfig;
        use tempfile::tempdir;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::session::ServerTokens;

        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/user/nickname"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"nickname":"Alice"}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/user/me"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"id":2318038547,"nickname":"Alice","email":"user@example.com"}}"#,
            ))
            .mount(&server)
            .await;

        let data_dir = tempdir().expect("tmpdir");
        let config = ServerConfig {
            base_url: server.uri(),
            ..Default::default()
        };
        let mgr = super::AuthManager::new(config, data_dir.path()).expect("auth manager");
        mgr.session()
            .save_tokens(ServerTokens::from_jwt("jwt-nickname".into()))
            .await
            .expect("save token");

        let profile = mgr.update_nickname("Alice").await.expect("update");
        assert_eq!(profile.nickname.as_deref(), Some("Alice"));
        assert_eq!(profile.display_name(), "Alice");
        let cached = mgr.cached_profile().await.expect("cache").expect("profile");
        assert_eq!(cached.nickname.as_deref(), Some("Alice"));
        assert_eq!(cached.display_name(), "Alice");
    }

    #[tokio::test]
    async fn update_nickname_requires_login() {
        use nomi_config::ServerConfig;
        use tempfile::tempdir;

        let data_dir = tempdir().expect("tmpdir");
        let config = ServerConfig {
            base_url: "https://example.test/claw".into(),
            ..Default::default()
        };
        let mgr = super::AuthManager::new(config, data_dir.path()).expect("auth manager");
        let err = mgr
            .update_nickname("Alice")
            .await
            .expect_err("logged out");
        assert!(matches!(err, crate::error::ServerClientError::AuthRequired(_)));
    }

    #[tokio::test]
    async fn update_nickname_patches_cache_when_profile_refresh_fails() {
        use nomi_config::ServerConfig;
        use tempfile::tempdir;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::flowy::UserMe;
        use crate::profile::ProfileStore;
        use crate::session::ServerTokens;

        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/user/nickname"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"nickname":"Alice"}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/user/me"))
            .respond_with(ResponseTemplate::new(500).set_body_string(
                r#"{"code":500,"msg":"boom","data":null}"#,
            ))
            .mount(&server)
            .await;

        let data_dir = tempdir().expect("tmpdir");
        ProfileStore::new(data_dir.path())
            .save(&UserMe {
                id: 2318038547,
                nickname: Some("old".into()),
                email: Some("user@example.com".into()),
                ..Default::default()
            })
            .await
            .expect("seed profile");
        let config = ServerConfig {
            base_url: server.uri(),
            ..Default::default()
        };
        let mgr = super::AuthManager::new(config, data_dir.path()).expect("auth manager");
        mgr.session()
            .save_tokens(ServerTokens::from_jwt("jwt-nickname".into()))
            .await
            .expect("save token");

        let profile = mgr.update_nickname("Alice").await.expect("update");
        assert_eq!(profile.nickname.as_deref(), Some("Alice"));
        assert_eq!(profile.email.as_deref(), Some("user@example.com"));
        assert_eq!(profile.display_name(), "Alice");
    }
}
