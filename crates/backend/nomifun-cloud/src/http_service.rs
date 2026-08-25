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
    CloudImSendMessageRequest, CloudBillingAirwallexSession, CloudBillingCouponList,
    CloudBillingCreateOrderRequest, CloudBillingCreditPack, CloudBillingOrder,
    CloudBillingPaymentChannel, CloudBillingPlan, VideoGrowthEventBatchRequest,
    VideoGrowthEventBatchResponse, VideoGrowthMetricsResponse,
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
        // Also persist when rewriting the retired `.cn` production host so login
        // stops targeting the old domain after a restart.
        let should_persist = !path.exists()
            || config.server.base_url.trim().is_empty()
            || config.server.website_url.trim().is_empty()
            || config.media.provider.trim().is_empty()
            || config.server.rewrite_legacy_cn_hosts();
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
            website_url: cfg.server.effective_website_url(),
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
            if let Some(website_url) = req.website_url {
                cfg.server.website_url = website_url;
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

    /// Official website URL with FlowyClaw `?token=&language=` auto-login params.
    pub async fn website_entry(
        &self,
        language: Option<&str>,
    ) -> Result<nomifun_api_types::CloudWebsiteEntryResponse, AppError> {
        let cfg = self.gateway_config();
        let website_url = cfg.server.effective_website_url();
        let token = match AuthManager::new(cfg.server.clone(), &self.data_dir) {
            Ok(mgr) => mgr
                .session()
                .access_token()
                .await
                .ok()
                .flatten()
                .filter(|value| !value.trim().is_empty()),
            Err(ServerClientError::MissingBaseUrl) => None,
            Err(e) => return Err(AppError::Internal(e.to_string())),
        };
        Ok(nomifun_api_types::CloudWebsiteEntryResponse {
            url: crate::website::build_website_entry_url(
                &website_url,
                token.as_deref(),
                language.unwrap_or("en"),
            ),
        })
    }

    pub async fn list_billing_plans(&self) -> Result<Vec<CloudBillingPlan>, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        let channel = self.gateway_config().server.channel;
        client
            .list_billing_plans(&session, &channel)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn list_billing_credit_packs(&self) -> Result<Vec<CloudBillingCreditPack>, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .list_billing_credit_packs(&session)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn list_billing_coupons(
        &self,
        item_type: Option<&str>,
    ) -> Result<CloudBillingCouponList, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        match client.list_billing_coupons(&session, item_type).await {
            Ok(list) => Ok(list),
            Err(ServerClientError::AuthRequired(msg)) => Err(AppError::Unauthorized(msg)),
            Err(ServerClientError::Api { code, msg }) if code == 401 || code == 403 => {
                Err(AppError::Unauthorized(msg))
            }
            Err(_) => Ok(CloudBillingCouponList { list: Vec::new() }),
        }
    }

    pub async fn list_billing_payment_channels(
        &self,
        item_type: &str,
        item_id: i64,
        plan_period: Option<&str>,
    ) -> Result<Vec<CloudBillingPaymentChannel>, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .list_billing_payment_channels(&session, item_type, item_id, plan_period)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn create_billing_order(
        &self,
        mut request: CloudBillingCreateOrderRequest,
    ) -> Result<CloudBillingOrder, AppError> {
        request.pay_channel = "airwallex".into();
        let (client, session) = self.im_client_and_session().await?;
        client
            .create_billing_order(&session, &request)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn get_billing_order_by_no(
        &self,
        order_no: &str,
    ) -> Result<CloudBillingOrder, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .get_billing_order_by_no(&session, order_no)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn init_billing_airwallex(
        &self,
        order_no: &str,
    ) -> Result<CloudBillingAirwallexSession, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .init_billing_airwallex(&session, order_no)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn upload_video_growth_events(
        &self,
        request: &VideoGrowthEventBatchRequest,
    ) -> Result<VideoGrowthEventBatchResponse, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .upload_video_growth_events(&session, request)
            .await
            .map_err(map_im_client_error)
    }

    pub async fn get_video_growth_metrics(
        &self,
        days: u16,
    ) -> Result<VideoGrowthMetricsResponse, AppError> {
        let (client, session) = self.im_client_and_session().await?;
        client
            .get_video_growth_metrics(&session, days)
            .await
            .map_err(map_im_client_error)
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
    err.into_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::{
        DEFAULT_FLOWY_WEBSITE_URL, DEFAULT_WECHAT_FLOWY_SERVER_BASE, FLOWY_WEBSITE_HOST,
        LEGACY_FLOWY_SERVER_HOST, LEGACY_WECHAT_FLOWY_SERVER_BASE,
    };

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
        assert_eq!(settings.website_url, DEFAULT_FLOWY_WEBSITE_URL);
        assert!(settings.enabled);

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(DEFAULT_WECHAT_FLOWY_SERVER_BASE));
        assert!(raw.contains(DEFAULT_FLOWY_WEBSITE_URL));
        assert!(raw.contains("provider: flowy") || raw.contains("provider: \"flowy\""));
    }

    #[test]
    fn new_rewrites_and_persists_legacy_cn_base_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_yaml_path(Some(dir.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(
                "server:\n  enabled: true\n  base_url: \"{LEGACY_WECHAT_FLOWY_SERVER_BASE}\"\nmedia:\n  provider: flowy\n"
            ),
        )
        .unwrap();

        let svc = CloudService::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(svc.server_settings().base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(DEFAULT_WECHAT_FLOWY_SERVER_BASE));
        assert!(!raw.contains(LEGACY_FLOWY_SERVER_HOST));
    }

    #[tokio::test]
    async fn website_entry_uses_default_host_and_language() {
        let dir = tempfile::tempdir().unwrap();
        let svc = CloudService::new(dir.path().to_path_buf()).unwrap();
        let entry = svc.website_entry(Some("zh-CN")).await.unwrap();
        let parsed = url::Url::parse(&entry.url).unwrap();
        assert_eq!(parsed.host_str(), Some(FLOWY_WEBSITE_HOST));
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(k, _)| k == "language")
                .map(|(_, v)| v.into_owned()),
            Some("zh".into())
        );
    }
}
