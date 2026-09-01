//! Axum routes for Flowy cloud account API.

use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension, Json, Multipart, Path, Query, State};
use axum::routing::{get, post};
use serde::Deserialize;
use tower_http::limit::RequestBodyLimitLayer;

use nomifun_api_types::{
    ApiResponse, CloudBillingAirwallexSession, CloudBillingCouponList,
    CloudBillingCreateOrderRequest, CloudBillingCreditPack, CloudBillingOrder,
    CloudBillingPaymentChannel, CloudBillingPlan, CloudDeviceActivationRetryResponse,
    CloudDeviceActivationStatusResponse, CloudImAttachmentPayload, CloudImConversation,
    CloudImLogUploadResponse, CloudImMessage, CloudImMessageList, CloudImSendMessageRequest,
    CloudLoginContinueRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudServerSettingsResponse, CloudSyncModelsResponse, CloudWebsiteEntryResponse,
    CloudWhoamiResponse, UpdateCloudServerSettingsRequest, VideoGrowthEvent,
    VideoGrowthEventBatchRequest, VideoGrowthEventBatchResponse,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use nomifun_db::{IClientPreferenceRepository, IProviderModelRepository, IProviderRepository};
use tracing::warn;

use crate::http_service::CloudService;
use crate::provider_sync::{disable_flowy_builtin_provider, sync_flowy_builtin_provider};

const ALLOWED_IM_APP: &str = "flowymes";
const DEFAULT_IM_MESSAGE_LIMIT: i64 = 50;
const MIN_IM_MESSAGE_LIMIT: i64 = 1;
const MAX_IM_MESSAGE_LIMIT: i64 = 100;
const MAX_IM_CONTENT_CHARS: usize = 4000;
const MAX_CLIENT_MSG_ID_CHARS: usize = 64;
const MAX_LOG_PAYLOAD_BYTES: i64 = 50 * 1024 * 1024;
const MAX_LOG_UPLOAD_BODY_BYTES: usize = 52 * 1024 * 1024;
// FlowyClaw `/uploads/feedback/screenshot` limit (doc §5.2: image ≤ 10MiB).
const MAX_IMAGE_PAYLOAD_BYTES: i64 = 10 * 1024 * 1024;
const MAX_IMAGE_UPLOAD_BODY_BYTES: usize = 11 * 1024 * 1024;
const ALLOWED_IM_IMAGE_CONTENT_TYPES: [&str; 4] =
    ["image/jpeg", "image/png", "image/webp", "image/gif"];
const MAX_GROWTH_EVENTS_PER_BATCH: usize = 50;
const MAX_GROWTH_PROPERTIES: usize = 24;
const TELEMETRY_EVENT_NAMES: [&str; 15] = [
    "app_opened",
    "home_viewed",
    "task_drafted",
    "task_accepted",
    "first_task_started",
    "first_artifact_visible",
    "render_started",
    "film_succeeded",
    "film_failed",
    "film_cancelled",
    "value_confirmed",
    "project_exported",
    "tv_published",
    "resume_started",
    "resume_succeeded",
];

#[derive(Clone)]
pub struct CloudRouterState {
    pub service: Arc<CloudService>,
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub client_preference_repo: Arc<dyn IClientPreferenceRepository>,
    pub encryption_key: [u8; 32],
}

impl CloudRouterState {
    pub fn new(
        service: Arc<CloudService>,
        provider_repo: Arc<dyn IProviderRepository>,
        provider_model_repo: Arc<dyn IProviderModelRepository>,
        client_preference_repo: Arc<dyn IClientPreferenceRepository>,
        encryption_key: [u8; 32],
    ) -> Self {
        Self {
            service,
            provider_repo,
            provider_model_repo,
            client_preference_repo,
            encryption_key,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImConversationQuery {
    app: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImMessagesQuery {
    after_seq: Option<i64>,
    before_seq: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImReadRequest {
    last_read_seq: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadImLogFromPathRequest {
    zip_path: String,
    #[serde(default)]
    file_name: Option<String>,
}

pub fn cloud_routes(state: CloudRouterState) -> Router {
    let upload_routes = Router::new()
        .route("/api/cloud/im/logs/upload", post(upload_im_log))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(MAX_LOG_UPLOAD_BODY_BYTES))
        .with_state(state.clone());

    let screenshot_upload_routes = Router::new()
        .route(
            "/api/cloud/im/screenshots/upload",
            post(upload_im_screenshot),
        )
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(MAX_IMAGE_UPLOAD_BODY_BYTES))
        .with_state(state.clone());

    Router::new()
        .route("/api/cloud/settings", get(get_settings).patch(patch_settings))
        .route("/api/cloud/whoami", get(whoami))
        .route("/api/cloud/website-entry", get(website_entry))
        .route("/api/cloud/device/status", get(device_activation_status))
        .route("/api/cloud/device/activate", post(retry_device_activation))
        .route("/api/cloud/login/start", post(login_start))
        .route("/api/cloud/login/continue", post(login_continue))
        .route("/api/cloud/logout", post(logout))
        .route("/api/cloud/sync-models", post(sync_models))
        .route(
            "/api/cloud/telemetry/events",
            post(upload_video_growth_events),
        )
        .route(
            "/api/cloud/growth/video/events",
            post(upload_video_growth_events),
        )
        .route("/api/cloud/plans", get(list_billing_plans))
        .route("/api/cloud/credit-packs", get(list_billing_credit_packs))
        .route("/api/cloud/coupons", get(list_billing_coupons))
        .route("/api/cloud/payment-channels", get(list_billing_payment_channels))
        .route("/api/cloud/orders", post(create_billing_order))
        .route("/api/cloud/orders/by-order-no", get(get_billing_order_by_no))
        .route(
            "/api/cloud/orders/{order_no}/airwallex/init",
            post(init_billing_airwallex),
        )
        .route("/api/cloud/im/conversation", get(get_im_conversation))
        .route(
            "/api/cloud/im/messages",
            get(list_im_messages).post(send_im_message),
        )
        .route("/api/cloud/im/read", post(mark_im_read))
        .route(
            "/api/cloud/im/logs/upload-from-path",
            post(upload_im_log_from_path),
        )
        .with_state(state)
        .merge(upload_routes)
        .merge(screenshot_upload_routes)
}

fn validate_video_growth_event(event: &VideoGrowthEvent) -> Result<(), AppError> {
    if event.event_id.is_empty() || event.event_id.len() > 128 {
        return Err(AppError::BadRequest("growth event id is invalid".into()));
    }
    if !TELEMETRY_EVENT_NAMES.contains(&event.name.as_str()) {
        return Err(AppError::BadRequest("growth event name is invalid".into()));
    }
    if let Some(module) = event.module.as_deref() {
        let expected = match event.name.as_str() {
            "app_opened" => "platform",
            _ => "video_generation",
        };
        if module != expected {
            return Err(AppError::BadRequest("growth event module is invalid".into()));
        }
    }
    if event.cohort.as_deref().is_some_and(|value| value != "A" && value != "B") {
        return Err(AppError::BadRequest("growth event cohort is invalid".into()));
    }
    chrono::DateTime::parse_from_rfc3339(&event.occurred_at)
        .map_err(|_| AppError::BadRequest("growth event timestamp is invalid".into()))?;
    if event.properties.len() > MAX_GROWTH_PROPERTIES {
        return Err(AppError::BadRequest(
            "growth event has too many properties".into(),
        ));
    }
    for (key, value) in &event.properties {
        if key.is_empty() || key.len() > 64 {
            return Err(AppError::BadRequest(
                "growth event property key is invalid".into(),
            ));
        }
        match value {
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_) => {}
            serde_json::Value::String(value) if value.len() <= 256 => {}
            _ => {
                return Err(AppError::BadRequest(
                    "growth event property value is invalid".into(),
                ));
            }
        }
    }
    Ok(())
}

async fn upload_video_growth_events(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(request): Json<VideoGrowthEventBatchRequest>,
) -> Result<Json<ApiResponse<VideoGrowthEventBatchResponse>>, AppError> {
    if request.events.is_empty() || request.events.len() > MAX_GROWTH_EVENTS_PER_BATCH {
        return Err(AppError::BadRequest(
            "growth event batch size is invalid".into(),
        ));
    }
    for event in &request.events {
        validate_video_growth_event(event)?;
    }
    Ok(Json(ApiResponse::ok(
        state.service.upload_video_growth_events(&request).await?,
    )))
}

#[cfg(test)]
mod growth_tests {
    use super::*;

    fn event(name: &str) -> VideoGrowthEvent {
        VideoGrowthEvent {
            event_id: "video:film_succeeded:session-1".into(),
            name: name.into(),
            occurred_at: "2026-08-26T00:00:00Z".into(),
            module: None,
            properties: Default::default(),
            cohort: Some("A".into()),
        }
    }

    #[test]
    fn accepts_known_video_growth_event() {
        assert!(validate_video_growth_event(&event("film_succeeded")).is_ok());
    }

    #[test]
    fn rejects_unknown_video_growth_event() {
        assert!(validate_video_growth_event(&event("arbitrary_event")).is_err());
    }

    #[test]
    fn accepts_cancelled_and_platform_events() {
        assert!(validate_video_growth_event(&event("film_cancelled")).is_ok());
        let mut opened = event("app_opened");
        opened.module = Some("platform".into());
        assert!(validate_video_growth_event(&opened).is_ok());
    }

    #[test]
    fn rejects_nested_growth_property() {
        let mut event = event("film_succeeded");
        event
            .properties
            .insert("payload".into(), serde_json::json!({"prompt": "private"}));
        assert!(validate_video_growth_event(&event).is_err());
    }
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

#[derive(Debug, Deserialize)]
struct WebsiteEntryQuery {
    language: Option<String>,
}

async fn website_entry(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<WebsiteEntryQuery>,
) -> Result<Json<ApiResponse<CloudWebsiteEntryResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .service
            .website_entry(query.language.as_deref())
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingCouponsQuery {
    item_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingPaymentChannelsQuery {
    item_type: String,
    item_id: i64,
    plan_period: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingOrderByNoQuery {
    order_no: String,
}

fn validate_billing_item_type(item_type: &str) -> Result<&str, AppError> {
    match item_type.trim() {
        "plan" | "pack" => Ok(item_type.trim()),
        _ => Err(AppError::BadRequest(
            "itemType must be plan or pack".into(),
        )),
    }
}

fn validate_order_no(order_no: &str) -> Result<&str, AppError> {
    let trimmed = order_no.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(AppError::BadRequest("orderNo is invalid".into()));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AppError::BadRequest("orderNo is invalid".into()));
    }
    Ok(trimmed)
}

fn validate_create_order(
    mut request: CloudBillingCreateOrderRequest,
) -> Result<CloudBillingCreateOrderRequest, AppError> {
    request.item_type = validate_billing_item_type(&request.item_type)?.to_string();
    if request.item_id <= 0 {
        return Err(AppError::BadRequest("itemId must be positive".into()));
    }
    let key = request.idempotency_key.trim();
    if key.is_empty() || key.len() > 128 {
        return Err(AppError::BadRequest("idempotencyKey is invalid".into()));
    }
    request.idempotency_key = key.to_string();
    request.pay_channel = "airwallex".into();
    if let Some(coupon_id) = request.coupon_id {
        if coupon_id <= 0 {
            return Err(AppError::BadRequest("couponId must be positive".into()));
        }
    }
    if let Some(period) = request.plan_period.as_mut() {
        let normalized = period.trim().to_uppercase();
        if !matches!(normalized.as_str(), "MONTH" | "HALF_YEAR" | "YEAR") {
            return Err(AppError::BadRequest("planPeriod is invalid".into()));
        }
        *period = normalized;
    }
    Ok(request)
}

async fn list_billing_plans(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<CloudBillingPlan>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.list_billing_plans().await?)))
}

async fn list_billing_credit_packs(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<CloudBillingCreditPack>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.list_billing_credit_packs().await?,
    )))
}

async fn list_billing_coupons(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<BillingCouponsQuery>,
) -> Result<Json<ApiResponse<CloudBillingCouponList>>, AppError> {
    let item_type = query
        .item_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(item_type) = item_type {
        validate_billing_item_type(item_type)?;
    }
    Ok(Json(ApiResponse::ok(
        state.service.list_billing_coupons(item_type).await?,
    )))
}

async fn list_billing_payment_channels(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<BillingPaymentChannelsQuery>,
) -> Result<Json<ApiResponse<Vec<CloudBillingPaymentChannel>>>, AppError> {
    let item_type = validate_billing_item_type(&query.item_type)?;
    if query.item_id <= 0 {
        return Err(AppError::BadRequest("itemId must be positive".into()));
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .list_billing_payment_channels(item_type, query.item_id, query.plan_period.as_deref())
            .await?,
    )))
}

async fn create_billing_order(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(request): Json<CloudBillingCreateOrderRequest>,
) -> Result<Json<ApiResponse<CloudBillingOrder>>, AppError> {
    let request = validate_create_order(request)?;
    Ok(Json(ApiResponse::ok(
        state.service.create_billing_order(request).await?,
    )))
}

async fn get_billing_order_by_no(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<BillingOrderByNoQuery>,
) -> Result<Json<ApiResponse<CloudBillingOrder>>, AppError> {
    let order_no = validate_order_no(&query.order_no)?;
    Ok(Json(ApiResponse::ok(
        state.service.get_billing_order_by_no(order_no).await?,
    )))
}

async fn init_billing_airwallex(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(order_no): Path<String>,
) -> Result<Json<ApiResponse<CloudBillingAirwallexSession>>, AppError> {
    let order_no = validate_order_no(&order_no)?;
    Ok(Json(ApiResponse::ok(
        state.service.init_billing_airwallex(order_no).await?,
    )))
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

/// Re-fetch the Flowy chat model catalog and upsert the builtin provider row.
/// Used when entering pages with a model selector so the UI sees the latest list.
/// Not-logged-in is a soft no-op (`synced: false`) rather than an error.
async fn sync_models(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CloudSyncModelsResponse>>, AppError> {
    let cfg = state.service.gateway_config_snapshot();
    match sync_flowy_builtin_provider(
        &state.provider_repo,
        &state.provider_model_repo,
        &state.encryption_key,
        &cfg.server,
        state.service.data_dir(),
    )
    .await
    {
        Ok(synced) => {
            if synced {
                if let Err(error) = crate::migrate_free_model_preferences(
                    &state.provider_repo,
                    &state.provider_model_repo,
                    &state.client_preference_repo,
                )
                .await
                {
                    warn!(error = %error, "Failed to migrate active free-model preferences to Flowy Cloud");
                }
            }
            Ok(Json(ApiResponse::ok(CloudSyncModelsResponse { synced })))
        }
        Err(e) if e.contains("not logged in") => Ok(Json(ApiResponse::ok(CloudSyncModelsResponse {
            synced: false,
        }))),
        Err(e) => {
            warn!(error = %e, "Failed to sync Flowy chat model catalog");
            Err(AppError::Internal(format!("sync Flowy provider: {e}")))
        }
    }
}

fn validate_im_app(app: Option<&str>) -> Result<Option<&str>, AppError> {
    match app.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(value) if value == ALLOWED_IM_APP => Ok(Some(ALLOWED_IM_APP)),
        Some(_) => Err(AppError::BadRequest(format!(
            "unsupported app; only `{ALLOWED_IM_APP}` is allowed"
        ))),
    }
}

fn validate_im_limit(limit: Option<i64>) -> Result<i64, AppError> {
    let limit = limit.unwrap_or(DEFAULT_IM_MESSAGE_LIMIT);
    if (MIN_IM_MESSAGE_LIMIT..=MAX_IM_MESSAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(AppError::BadRequest(format!(
            "limit must be between {MIN_IM_MESSAGE_LIMIT} and {MAX_IM_MESSAGE_LIMIT}"
        )))
    }
}

fn validate_send_im_message(
    mut request: CloudImSendMessageRequest,
) -> Result<CloudImSendMessageRequest, AppError> {
    let client_msg_id = request.client_msg_id.trim();
    if client_msg_id.is_empty() || client_msg_id.chars().count() > MAX_CLIENT_MSG_ID_CHARS {
        return Err(AppError::BadRequest(format!(
            "clientMsgId must be 1-{MAX_CLIENT_MSG_ID_CHARS} characters"
        )));
    }
    request.client_msg_id = client_msg_id.to_string();

    let msg_type = request.msg_type.trim();
    if msg_type != "text" && msg_type != "image" {
        return Err(AppError::BadRequest(
            "only msgType=text or msgType=image is supported".into(),
        ));
    }
    request.msg_type = msg_type.to_string();

    let content_len = request.content.chars().count();
    if content_len > MAX_IM_CONTENT_CHARS {
        return Err(AppError::BadRequest(format!(
            "content must be at most {MAX_IM_CONTENT_CHARS} characters"
        )));
    }
    // Doc §5: content is required for text, optional caption for attachments.
    if request.msg_type == "text" && content_len == 0 {
        return Err(AppError::BadRequest("content is required".into()));
    }

    if request.msg_type == "image" {
        let payload = request
            .payload
            .as_mut()
            .ok_or_else(|| AppError::BadRequest("payload is required for msgType=image".into()))?;
        validate_im_attachment_payload(payload, "payload", MAX_IMAGE_PAYLOAD_BYTES)?;
        if !ALLOWED_IM_IMAGE_CONTENT_TYPES.contains(&payload.content_type.as_str()) {
            return Err(AppError::BadRequest(
                "payload.contentType must be image/jpeg, image/png, image/webp or image/gif"
                    .into(),
            ));
        }
    } else {
        request.payload = None;
    }

    if let Some(payload) = request.log_payload.as_mut() {
        validate_im_attachment_payload(payload, "logPayload", MAX_LOG_PAYLOAD_BYTES)?;
    }

    request.app = validate_im_app(request.app.as_deref())?.map(str::to_string);
    Ok(request)
}

fn validate_im_attachment_payload(
    payload: &mut CloudImAttachmentPayload,
    field: &str,
    max_bytes: i64,
) -> Result<(), AppError> {
    let url = payload
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let object_key = payload
        .object_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if url.is_none() && object_key.is_none() {
        return Err(AppError::BadRequest(format!(
            "{field} requires url or objectKey"
        )));
    }
    payload.url = url;
    payload.object_key = object_key;

    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(format!("{field}.name is required")));
    }
    payload.name = name.to_string();

    let content_type = payload.content_type.trim();
    if content_type.is_empty() {
        return Err(AppError::BadRequest(format!(
            "{field}.contentType is required"
        )));
    }
    payload.content_type = content_type.to_string();

    if payload.byte_size <= 0 || payload.byte_size > max_bytes {
        return Err(AppError::BadRequest(format!(
            "{field}.byteSize must be between 1 and {max_bytes}"
        )));
    }
    Ok(())
}

async fn upload_im_log(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    multipart: Multipart,
) -> Result<Json<ApiResponse<CloudImLogUploadResponse>>, AppError> {
    let (file_name, content_type, bytes) = extract_log_upload_file(multipart).await?;
    if bytes.is_empty() {
        return Err(AppError::BadRequest("empty log file".into()));
    }
    if bytes.len() as i64 > MAX_LOG_PAYLOAD_BYTES {
        return Err(AppError::BadRequest(format!(
            "log file exceeds {MAX_LOG_PAYLOAD_BYTES} bytes"
        )));
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .upload_im_log(bytes, &file_name, &content_type)
            .await?,
    )))
}

/// Proxies FlowyClaw `POST /uploads/feedback/screenshot` (multipart `file`).
async fn upload_im_screenshot(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    multipart: Multipart,
) -> Result<Json<ApiResponse<CloudImLogUploadResponse>>, AppError> {
    let (file_name, content_type, bytes) = extract_log_upload_file(multipart).await?;
    if bytes.is_empty() {
        return Err(AppError::BadRequest("empty image file".into()));
    }
    if bytes.len() as i64 > MAX_IMAGE_PAYLOAD_BYTES {
        return Err(AppError::BadRequest(format!(
            "image file exceeds {MAX_IMAGE_PAYLOAD_BYTES} bytes"
        )));
    }
    if !ALLOWED_IM_IMAGE_CONTENT_TYPES.contains(&content_type.as_str()) {
        return Err(AppError::BadRequest(
            "image contentType must be image/jpeg, image/png, image/webp or image/gif".into(),
        ));
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .upload_im_screenshot(bytes, &file_name, &content_type)
            .await?,
    )))
}

async fn upload_im_log_from_path(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<UploadImLogFromPathRequest>,
) -> Result<Json<ApiResponse<CloudImLogUploadResponse>>, AppError> {
    let zip_path = req.zip_path.trim();
    if zip_path.is_empty() {
        return Err(AppError::BadRequest("zipPath is required".into()));
    }
    let path = std::path::Path::new(zip_path);
    if !path.is_file() {
        return Err(AppError::BadRequest("zipPath is not a file".into()));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::Internal(format!("failed to read zipPath: {e}")))?;
    if bytes.is_empty() {
        let _ = std::fs::remove_file(path);
        return Err(AppError::BadRequest("empty log file".into()));
    }
    if bytes.len() as i64 > MAX_LOG_PAYLOAD_BYTES {
        let _ = std::fs::remove_file(path);
        return Err(AppError::BadRequest(format!(
            "log file exceeds {MAX_LOG_PAYLOAD_BYTES} bytes"
        )));
    }
    let file_name = req
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "app-logs.zip".to_string());

    let result = state
        .service
        .upload_im_log(bytes, &file_name, "application/zip")
        .await;
    let _ = std::fs::remove_file(path);
    Ok(Json(ApiResponse::ok(result?)))
}

async fn extract_log_upload_file(
    mut multipart: Multipart,
) -> Result<(String, String, Vec<u8>), AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("invalid multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field
            .file_name()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "app-logs.zip".to_string());
        let content_type = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/zip".to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("failed to read upload bytes: {e}")))?
            .to_vec();
        return Ok((file_name, content_type, bytes));
    }
    Err(AppError::BadRequest(
        "multipart field `file` is required".into(),
    ))
}

async fn get_im_conversation(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ImConversationQuery>,
) -> Result<Json<ApiResponse<CloudImConversation>>, AppError> {
    let app = validate_im_app(query.app.as_deref())?;
    Ok(Json(ApiResponse::ok(
        state.service.get_im_conversation(app).await?,
    )))
}

async fn list_im_messages(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ImMessagesQuery>,
) -> Result<Json<ApiResponse<CloudImMessageList>>, AppError> {
    let limit = validate_im_limit(query.limit)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .list_im_messages(query.after_seq, query.before_seq, limit)
            .await?,
    )))
}

async fn send_im_message(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<CloudImSendMessageRequest>,
) -> Result<Json<ApiResponse<CloudImMessage>>, AppError> {
    let request = validate_send_im_message(req)?;
    Ok(Json(ApiResponse::ok(
        state.service.send_im_message(request).await?,
    )))
}

async fn mark_im_read(
    State(state): State<CloudRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(req): Json<ImReadRequest>,
) -> Result<Json<ApiResponse<CloudImConversation>>, AppError> {
    if req.last_read_seq < 0 {
        return Err(AppError::BadRequest(
            "lastReadSeq must be greater than or equal to 0".into(),
        ));
    }
    Ok(Json(ApiResponse::ok(
        state.service.mark_im_read(req.last_read_seq).await?,
    )))
}
