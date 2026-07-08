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
        ensure_gateway_defaults(&mut config);
        if !path.exists() {
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
}
