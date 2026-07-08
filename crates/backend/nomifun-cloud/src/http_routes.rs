//! Axum routes for Flowy cloud account API.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Extension, Json, State};
use axum::routing::{get, post};

use nomifun_api_types::{
    ApiResponse, CloudDeviceActivationRetryResponse, CloudDeviceActivationStatusResponse,
    CloudLoginContinueRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudServerSettingsResponse, CloudWhoamiResponse, UpdateCloudServerSettingsRequest,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use nomifun_db::IProviderRepository;

use crate::http_service::CloudService;
use crate::provider_sync::{disable_flowy_builtin_provider, sync_flowy_builtin_provider};

#[derive(Clone)]
pub struct CloudRouterState {
    pub service: Arc<CloudService>,
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub encryption_key: [u8; 32],
}

impl CloudRouterState {
    pub fn new(
        service: Arc<CloudService>,
        provider_repo: Arc<dyn IProviderRepository>,
        encryption_key: [u8; 32],
    ) -> Self {
        Self {
            service,
            provider_repo,
            encryption_key,
        }
    }
}

pub fn cloud_routes(state: CloudRouterState) -> Router {
    Router::new()
        .route("/api/cloud/settings", get(get_settings).patch(patch_settings))
        .route("/api/cloud/whoami", get(whoami))
        .route("/api/cloud/device/status", get(device_activation_status))
        .route("/api/cloud/device/activate", post(retry_device_activation))
        .route("/api/cloud/login/start", post(login_start))
        .route("/api/cloud/login/continue", post(login_continue))
        .route("/api/cloud/logout", post(logout))
        .with_state(state)
}

async fn get_settings(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CloudServerSettingsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.server_settings())))
}

async fn patch_settings(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<UpdateCloudServerSettingsRequest>,
) -> Result<Json<ApiResponse<CloudServerSettingsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.update_server_settings(req)?,
    )))
}

async fn whoami(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CloudWhoamiResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.whoami().await?)))
}

async fn device_activation_status(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CloudDeviceActivationStatusResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.device_activation_status().await?,
    )))
}

async fn retry_device_activation(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CloudDeviceActivationRetryResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.retry_device_activation().await?,
    )))
}

async fn login_start(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<CloudLoginStartRequest>,
) -> Result<Json<ApiResponse<CloudLoginStartResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.start_login(&req.method).await?,
    )))
}

async fn login_continue(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<CloudLoginContinueRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let result = state
        .service
        .continue_login(&req.pending_id, req.input)
        .await?;

    if result.get("status").and_then(|v| v.as_str()) == Some("success") {
        let cfg = state.service.gateway_config_snapshot();
        sync_flowy_builtin_provider(
            &state.provider_repo,
            &state.encryption_key,
            &cfg.server,
            state.service.data_dir(),
        )
        .await
        .map_err(|e| AppError::Internal(format!("sync Flowy provider: {e}")))?;
    }

    Ok(Json(ApiResponse::ok(result)))
}

async fn logout(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<bool>>, AppError> {
    let removed = state.service.logout().await?;
    disable_flowy_builtin_provider(&state.provider_repo)
        .await
        .map_err(|e| AppError::Internal(e))?;
    Ok(Json(ApiResponse::ok(removed)))
}
