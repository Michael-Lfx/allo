//! HTTP API for Flowy cloud account (email OTP login, whoami, server settings).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use nomi_config::{
    GatewayConfig, config_yaml_path, load_user_config_file, save_config_yaml,
};
use crate::config_defaults::ensure_gateway_defaults;
use crate::{
    AuthManager, AuthPollResult, AuthUserInput, LoginMethod, PendingLogin, ServerClientError,
};
use crate::activation::DeviceActivation;
use crate::flowy::FlowyApiClient;
use crate::session::ServerSession;
use nomifun_api_types::{
    CloudImConversation, CloudImLogUploadResponse, CloudImMessage, CloudImMessageList,
    CloudImSendMessageRequest,
};
use nomifun_common::AppError;

#[derive(Clone)]
struct PendingEntry {
    pending: PendingLogin,
}

pub struct CloudService {
    data_dir: PathBuf,
    config: Arc<Mutex<GatewayConfig>>,
    pending: Arc<DashMap<String, PendingEntry>>,
}

impl CloudService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let path = config_yaml_path(Some(&data_dir));
        let mut config = load_user_config_file(&path).map_err(|e| AppError::Internal(e))?;
        // Persist when the file is missing OR when base_url/provider were empty —
        // otherwise other services that read yaml from disk (media/vimax) stay
        // gated off by `api_ready() == false` for the whole process lifetime.
        let should_persist = !path.exists()
            || config.server.base_url.trim().is_empty()
            || config.media.provider.trim().is_empty();
        ensure_gateway_defaults(&mut config);
        if should_persist {
            save_config_yaml(&path, &config).map_err(|e| AppError::Internal(e))?;
        }
        Ok(Self {
            data_dir,
            config: Arc::new(Mutex::new(config)),
            pending: Arc::new(DashMap::new()),
        })
    }

    fn gateway_config(&self) -> GatewayConfig {
        self.config.lock().expect("cloud config lock").clone()
    }

    fn config_path(&self) -> PathBuf {
        config_yaml_path(Some(&self.data_dir))
    }

    pub(crate) fn auth_manager(&self) -> Result<AuthManager, AppError> {
        let cfg = self.gateway_config();
        AuthManager::new(cfg.server.clone(), &self.data_dir)
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn server_settings(&self) -> nomifun_api_types::CloudServerSettingsResponse {
        let cfg = self.gateway_config();
        nomifun_api_types::CloudServerSettingsResponse {
            enabled: cfg.server.enabled,
            base_url: cfg.server.base_url.clone(),
            channel: cfg.server.channel.clone(),
            app: cfg.server.app.clone(),
        }
    }

    pub fn update_server_settings(
        &self,
        req: nomifun_api_types::UpdateCloudServerSettingsRequest,
    ) -> Result<nomifun_api_types::CloudServerSettingsResponse, AppError> {
        {
            let mut cfg = self.config.lock().expect("cloud config lock");
            if let Some(enabled) = req.enabled {
                cfg.server.enabled = enabled;
            }
            if let Some(base_url) = req.base_url {
                cfg.server.base_url = base_url;
            }
            if let Some(channel) = req.channel {
                cfg.server.channel = channel;
            }
            if let Some(app) = req.app {
                cfg.server.app = app;
            }
            save_config_yaml(&self.config_path(), &cfg).map_err(|e| AppError::Internal(e))?;
        }
        Ok(self.server_settings())
    }

    pub async fn start_login(
        &self,
        method: &str,
    ) -> Result<nomifun_api_types::CloudLoginStartResponse, AppError> {
        let login_method = LoginMethod::parse(method).unwrap_or(LoginMethod::EmailOtp);
        let mgr = self.auth_manager()?;
        let pending = mgr
            .start_login(login_method)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let pending_id = uuid::Uuid::new_v4().to_string();
        let expires_at = pending.expires_at.map(|t| t.to_rfc3339());
        let message = pending.message.clone();
        let method_str = pending.method.as_str().to_string();
        self.pending.insert(pending_id.clone(), PendingEntry { pending });
        Ok(nomifun_api_types::CloudLoginStartResponse {
            pending_id,
            method: method_str,
            message,
            expires_at,
        })
    }

    pub async fn continue_login(
        &self,
        pending_id: &str,
        input: nomifun_api_types::CloudLoginInput,
    ) -> Result<serde_json::Value, AppError> {
        let entry = self
            .pending
            .get(pending_id)
            .ok_or_else(|| AppError::BadRequest("login session expired or invalid".into()))?;
        let pending = entry.pending.clone();
        drop(entry);

        let auth_input = match input {
            nomifun_api_types::CloudLoginInput::Email { address } => {
                AuthUserInput::Email { address }
            }
            nomifun_api_types::CloudLoginInput::OtpCode { code } => AuthUserInput::OtpCode { code },
            nomifun_api_types::CloudLoginInput::Poll => AuthUserInput::Poll,
        };

        let mgr = self.auth_manager()?;
        let result = mgr
            .continue_login(&pending, auth_input)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        match result {
            AuthPollResult::Pending(next) => {
                self.pending
                    .insert(pending_id.to_string(), PendingEntry { pending: next.clone() });
                Ok(serde_json::to_value(nomifun_api_types::CloudLoginPendingResponse {
                    status: "pending".into(),
                    pending_id: pending_id.to_string(),
                    method: next.method.as_str().to_string(),
                    message: next.message,
                    expires_at: next.expires_at.map(|t| t.to_rfc3339()),
                })
                .unwrap())
            }
            AuthPollResult::Success(_) => {
                self.pending.remove(pending_id);
                let whoami = self.whoami().await?;
                Ok(serde_json::to_value(nomifun_api_types::CloudLoginSuccessResponse {
                    status: "success".into(),
                    authenticated: whoami.authenticated,
                    user_id: whoami.user_id,
                    username: whoami.username,
                    email: whoami.email,
                })
                .unwrap())
            }
            AuthPollResult::InvalidCode => Err(AppError::CloudOtpInvalidCode),
            AuthPollResult::Failed(err) => {
                self.pending.remove(pending_id);
                Ok(serde_json::to_value(nomifun_api_types::CloudLoginFailedResponse {
                    status: "failed".into(),
                    error: err,
                })
                .unwrap())
            }
        }
    }

    pub async fn logout(&self) -> Result<bool, AppError> {
        let mgr = self.auth_manager()?;
        mgr.logout()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub async fn whoami(&self) -> Result<nomifun_api_types::CloudWhoamiResponse, AppError> {
        let cfg = self.gateway_config();
        let mgr = match AuthManager::new(cfg.server.clone(), &self.data_dir) {
            Ok(m) => m,
            Err(ServerClientError::MissingBaseUrl) => {
                return Ok(nomifun_api_types::CloudWhoamiResponse {
                    authenticated: false,
                    user_id: None,
                    username: None,
                    email: None,
                    server_base_url: None,
                    plan: None,
                    plan_code: None,
                });
            }
            Err(e) => return Err(AppError::Internal(e.to_string())),
        };
        let status = mgr
            .whoami()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let authenticated = status.is_logged_in();
        let profile = status.cached_profile;
        let plan = profile
            .as_ref()
            .and_then(|p| p.current_plan.as_ref())
            .and_then(|p| p.display_label());
        let plan_code = profile
            .as_ref()
            .and_then(|p| p.current_plan.as_ref())
            .and_then(|p| p.code.clone().or_else(|| p.name_en.clone()));
        Ok(nomifun_api_types::CloudWhoamiResponse {
            authenticated,
            user_id: profile.as_ref().map(|p| p.id.to_string()),
            username: profile.as_ref().map(|p| p.display_name()),
            email: profile.as_ref().and_then(|p| p.email.clone()),
            server_base_url: if cfg.server.base_url.is_empty() {
                None
            } else {
                Some(cfg.server.base_url.clone())
            },
            plan,
            plan_code,
        })
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    pub fn gateway_config_snapshot(&self) -> GatewayConfig {
        self.gateway_config()
    }

    pub async fn is_authenticated(&self) -> bool {
        self.whoami().await.map(|w| w.authenticated).unwrap_or(false)
    }

    pub async fn device_activation_status(
        &self,
    ) -> Result<nomifun_api_types::CloudDeviceActivationStatusResponse, AppError> {
        let mgr = self.auth_manager()?;
        let status = mgr
            .whoami()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if !status.is_logged_in() {
            return Ok(nomifun_api_types::CloudDeviceActivationStatusResponse {
                authenticated: false,
                serial_number: None,
                app_version: None,
                activated_for_version: false,
                last_reported_ip: None,
            });
        }

        let user_id = if let Some(profile) = status.cached_profile.as_ref() {
            profile.id
        } else {
            mgr.fetch_profile()
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
                .id
        };

        let activation = DeviceActivation::new(&self.data_dir);
        let local = activation
            .status_for_user(user_id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(nomifun_api_types::CloudDeviceActivationStatusResponse {
            authenticated: true,
            serial_number: if local.serial_number.is_empty() {
                None
            } else {
                Some(local.serial_number)
            },
            app_version: Some(local.app_version),
            activated_for_version: local.activated_for_version,
            last_reported_ip: local.last_reported_ip,
        })
    }

    pub async fn retry_device_activation(
        &self,
    ) -> Result<nomifun_api_types::CloudDeviceActivationRetryResponse, AppError> {
        let mgr = self.auth_manager()?;
        let reported = mgr
            .ensure_device_activation()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(nomifun_api_types::CloudDeviceActivationRetryResponse { reported })
    }

    /// Build an authenticated Flowy IM client. Fails closed when JWT is missing.
    pub(crate) async fn im_client_and_session(
        &self,
    ) -> Result<(FlowyApiClient, ServerSession), AppError> {
        let cfg = self.gateway_config();
        if !cfg.server.api_ready() {
            return Err(AppError::BadRequest(
                "server base_url not configured".into(),
            ));
        }
        let session = ServerSession::from_config(&cfg.server, &self.data_dir);
        let token = require_im_access_token(
            session
                .access_token()
                .await
                .map_err(map_im_client_error)?,
        )?;
        let _ = token;
        let client = FlowyApiClient::new(&cfg.server).map_err(map_im_client_error)?;
        Ok((client, session))
    }

    pub async fn get_im_conversation(
        &self,
        app: Option<&str>,
    ) -> Result<CloudImConversation, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .get_im_conversation(&session, app)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn list_im_messages(
        &self,
        after_seq: Option<i64>,
        before_seq: Option<i64>,
        limit: i64,
    ) -> Result<CloudImMessageList, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .list_im_messages(&session, after_seq, before_seq, limit)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn send_im_message(
        &self,
        request: CloudImSendMessageRequest,
    ) -> Result<CloudImMessage, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .send_im_message(&session, &request)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn upload_im_log(
        &self,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<CloudImLogUploadResponse, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        let result = client
            .upload_feedback_log(&session, file_bytes, file_name, content_type)
            .await
            .map_err(map_im_client_error)?;
        Ok(result)
    }

    pub async fn upload_im_screenshot(
        &self,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<CloudImLogUploadResponse, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        let result = client
            .upload_feedback_screenshot(&session, file_bytes, file_name, content_type)
            .await
            .map_err(map_im_client_error)?;
        Ok(result)
    }

    pub async fn mark_im_read(&self, last_read_seq: i64) -> Result<CloudImConversation, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .mark_im_read(&session, last_read_seq)
            .await
            .map_err(map_im_client_error)
    }
}

fn require_im_access_token(token: Option<String>) -> Result<String, AppError> {
    token
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| AppError::Unauthorized("not logged in to Flowy server".into()))
}

fn map_im_client_error(err: ServerClientError) -> AppError {
    match err {
        ServerClientError::AuthRequired(msg) => AppError::Unauthorized(msg),
        ServerClientError::MissingBaseUrl => {
            AppError::BadRequest("server base_url not configured".into())
        }
        ServerClientError::Disabled => AppError::BadRequest("server client disabled".into()),
        ServerClientError::NotConfigured(msg) => AppError::BadRequest(msg),
        ServerClientError::Api { code, msg } if code == 401 || code == 403 => {
            AppError::Unauthorized(msg)
        }
        ServerClientError::Api { code, msg } if code == 400 => AppError::BadRequest(msg),
        ServerClientError::Server {
            status: 401 | 403,
            body,
            ..
        } => AppError::Unauthorized(truncate_diag(&body)),
        ServerClientError::Server {
            status: 400,
            body,
            ..
        } => AppError::BadRequest(truncate_diag(&body)),
        other => AppError::BadGateway(truncate_diag(&other.to_string())),
    }
}

fn truncate_diag(message: &str) -> String {
    const MAX: usize = 500;
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(MAX).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::DEFAULT_WECHAT_FLOWY_SERVER_BASE;

    #[test]
    fn im_client_and_session_requires_auth() {
        let err = require_im_access_token(None).expect_err("missing token");
        assert!(matches!(err, AppError::Unauthorized(_)));

        let err = require_im_access_token(Some("   ".into())).expect_err("blank token");
        assert!(matches!(err, AppError::Unauthorized(_)));

        let token = require_im_access_token(Some("jwt-abc".into())).expect("token");
        assert_eq!(token, "jwt-abc");
    }

    #[test]
    fn new_persists_default_base_url_when_yaml_has_empty_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_yaml_path(Some(dir.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "server:\n  enabled: false\n  base_url: \"\"\nmedia:\n  provider: \"\"\n",
        )
        .unwrap();

        let svc = CloudService::new(dir.path().to_path_buf()).unwrap();
        let settings = svc.server_settings();
        assert_eq!(settings.base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);
        assert!(settings.enabled);

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(DEFAULT_WECHAT_FLOWY_SERVER_BASE));
        assert!(raw.contains("provider: flowy") || raw.contains("provider: \"flowy\""));
    }
}
