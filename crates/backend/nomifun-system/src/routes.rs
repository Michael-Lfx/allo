use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use nomifun_api_types::{
    ApiResponse, ClientPreferencesResponse, CloneProviderRequest, CreateProviderModelRequest,
    CreateProviderRequest, DetectProtocolRequest,
    FetchModelsAnonymousRequest, FetchModelsRequest, FetchModelsResponse, ManagedModel,
    ManagedModelHealthBatchResult,
    ManagedModelHealthResult, ManagedModelServiceStatus, ModelProfile, ModelProfileKeyRequest,
    ModelProfileUpsertRequest, ProtocolDetectionResponse,
    ProviderConnectionResponse, ProviderModelKeyRequest, ProviderModelResponse, ProviderResponse,
    ResolveModelsRequest, ResolveModelsResponse, SetManagedModelEnabledRequest,
    SetManagedModelServiceEnabledRequest, PackSupportLogsRequest, SupportLogsPackResponse, SystemInfoResponse,
    SystemSettingsResponse, UpdateCheckRequest, UpdateCheckResult, UpdateClientPreferencesRequest,
    UpdateProviderModelRequest, UpdateProviderRequest, UpdateSettingsRequest, UpdateWorkDirRequest,
    UpdateWorkDirResponse, ReplaceWorkDirRelocationRequest, RuntimeCapabilities,
    WorkDirChangeState, WorkDirChangeStatus, WorkDirRelocationBackup,
    WorkDirRelocationCalculationMode, WorkDirRelocationErrorClass,
    WorkDirRelocationOperation, WorkDirRelocationOperationState, WorkDirRelocationResponse,
    UpsertProviderConnectionRequest,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use nomifun_db::IConversationRepository;

use crate::client_pref::ClientPrefService;
use crate::managed_model::ManagedModelService;
use crate::model_fetcher::ModelFetchService;
use crate::model_profile::ModelProfileService;
use crate::protocol::ProtocolDetectionService;
use crate::provider::ProviderService;
use crate::provider_connection::ProviderConnectionService;
use crate::provider_model::ProviderModelService;
use crate::settings::SettingsService;
use crate::version::VersionCheckService;

/// Shared state for system route handlers.
#[derive(Clone)]
pub struct SystemRouterState {
    pub settings_service: SettingsService,
    pub client_pref_service: ClientPrefService,
    pub provider_service: ProviderService,
    pub provider_connection_service: ProviderConnectionService,
    pub model_fetch_service: ModelFetchService,
    pub model_profile_service: ModelProfileService,
    pub provider_model_service: ProviderModelService,
    pub managed_model_service: Option<std::sync::Arc<ManagedModelService>>,
    pub protocol_detection_service: ProtocolDetectionService,
    pub version_check_service: VersionCheckService,
    /// Data directory root — owns the durable work-root relocation plan and
    /// the v3 dataset lifecycle markers.
    pub data_dir: PathBuf,
    /// Canonical work root used by the live dataset. Explicit reset requests
    /// are bound to this value so config/env changes cannot redirect them.
    pub work_dir: PathBuf,
    /// True when `--work-dir` has authoritative priority on every restart.
    pub work_dir_is_cli_override: bool,
    /// Capabilities supplied by the host composition. Web and Desktop use
    /// the same HTTP routes but expose different lifecycle operations.
    pub runtime_capabilities: RuntimeCapabilities,
    /// Used to confirm `conversation_id` on support-log packing belongs to
    /// the authenticated user before attaching observation JSONL.
    pub conversation_repo: Option<Arc<dyn IConversationRepository>>,
}

/// Build the system router (settings + client prefs + providers + system).
///
/// All routes require authentication (applied by the caller).
///
/// Endpoints:
/// - `GET  /api/settings`                    — get all backend settings
/// - `PATCH /api/settings`                   — partial update backend settings
/// - `GET  /api/settings/client`             — get client preferences
/// - `PUT  /api/settings/client`             — batch update client preferences
/// - `GET  /api/providers`                   — list all providers
/// - `POST /api/providers`                   — create a provider
/// - `PUT  /api/providers/:provider_id`      — update a provider
/// - `DELETE /api/providers/:provider_id`    — delete a provider
/// - `POST /api/providers/:provider_id/clone` — clone a provider (models + connections)
/// - `GET  /api/providers/:provider_id/connections` — list connection profiles
/// - `POST /api/providers/:provider_id/connections` — upsert a connection profile
/// - `DELETE /api/providers/:provider_id/connections/:role` — delete a connection profile
/// - `POST /api/providers/:provider_id/models` — fetch models from remote API
/// - `POST /api/providers/fetch-models`      — fetch models anonymously (pre-create preview)
/// - `POST /api/providers/detect-protocol`   — detect API protocol
/// - `GET  /api/provider-models`             — list model catalog rows (`?provider_id=` filter)
/// - `POST /api/provider-models`             — create a model catalog row
/// - `POST /api/provider-models/update`      — partially update a model catalog row
/// - `POST /api/provider-models/delete`      — delete a model catalog row
/// - `GET  /api/system/info`                 — system directory & platform info
/// - `POST /api/system/check-update`         — check GitHub for new versions
/// - `POST /api/system/factory-reset`        — arm a factory reset (wipes on next boot)
/// - `POST /api/system/work-dir`             — request a restart-time work-root change
pub fn system_routes(state: SystemRouterState) -> Router {
    Router::new()
        .route("/api/settings", get(get_settings).patch(update_settings))
        .route(
            "/api/settings/client",
            get(get_client_preferences).put(update_client_preferences),
        )
        .route("/api/providers", get(list_providers).post(create_provider))
        // Literal-segment routes must register BEFORE the provider routes so
        // axum matches the literals instead of treating "detect-protocol" /
        // "fetch-models" as a provider id.
        .route("/api/providers/detect-protocol", post(detect_protocol))
        .route("/api/providers/fetch-models", post(fetch_models_anonymous))
        .route("/api/model-services/free/status", get(get_free_model_status))
        .route("/api/model-services/free/models", get(get_free_models))
        .route("/api/model-services/free/refresh", post(refresh_free_models))
        .route(
            "/api/model-services/free/health",
            get(get_free_model_health).post(check_all_free_model_health),
        )
        .route("/api/model-services/free/activate", post(activate_free_models))
        .route(
            "/api/model-services/free/models/{model_id}/health",
            post(check_free_model_health),
        )
        .route(
            "/api/model-services/free/models/{model_id}",
            patch(set_free_model_enabled),
        )
        .route(
            "/api/providers/{provider_id}",
            delete(delete_provider).put(update_provider),
        )
        .route(
            "/api/providers/{provider_id}/clone",
            post(clone_provider),
        )
        .route(
            "/api/providers/{provider_id}/connections",
            get(list_provider_connections).post(upsert_provider_connection),
        )
        .route(
            "/api/providers/{provider_id}/connections/{role}",
            delete(delete_provider_connection),
        )
        .route(
            "/api/providers/{provider_id}/models",
            post(fetch_models),
        )
        // Multimodal model hub: authoritative per-model capability profiles.
        .route("/api/model-profiles", get(list_model_profiles).post(upsert_model_profile))
        .route("/api/model-profiles/delete", post(delete_model_profile))
        .route("/api/model-profiles/resolve", post(resolve_model_profiles))
        // Row-level model catalog CRUD over the authoritative provider_models
        // entity (`(provider_id, model)` composite natural key).
        .route(
            "/api/provider-models",
            get(list_provider_models).post(create_provider_model),
        )
        .route("/api/provider-models/update", post(update_provider_model))
        .route("/api/provider-models/delete", post(delete_provider_model))
        .route("/api/system/info", get(get_system_info))
        .route("/api/system/support-logs/pack", post(pack_support_logs))
        .route("/api/system/check-update", post(check_update))
        .route("/api/system/factory-reset", post(factory_reset))
        .route("/api/system/work-dir", post(set_work_dir))
        .route("/api/system/work-dir-relocation", get(get_work_dir_relocation))
        .route(
            "/api/system/work-dir-relocation/{operation_id}",
            delete(delete_work_dir_relocation),
        )
        .route(
            "/api/system/work-dir-relocation/{operation_id}/retry",
            post(retry_work_dir_relocation),
        )
        .route(
            "/api/system/work-dir-relocation/{operation_id}/replace",
            post(replace_work_dir_relocation),
        )
        .route(
            "/api/system/work-dir-relocation/{operation_id}/backup",
            delete(delete_work_dir_relocation_backup),
        )
        .with_state(state)
}

// ===========================================================================
// Settings handlers
// ===========================================================================

async fn get_settings(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<SystemSettingsResponse>>, AppError> {
    let settings = state.settings_service.get_settings().await?;
    Ok(Json(ApiResponse::ok(settings)))
}

async fn update_settings(
    State(state): State<SystemRouterState>,
    body: Result<Json<UpdateSettingsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<SystemSettingsResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let settings = state.settings_service.update_settings(req).await?;
    Ok(Json(ApiResponse::ok(settings)))
}

// ===========================================================================
// Client preferences handlers
// ===========================================================================

#[derive(Debug, serde::Deserialize, Default)]
struct ClientPrefQuery {
    keys: Option<String>,
}

async fn get_client_preferences(
    State(state): State<SystemRouterState>,
    Query(query): Query<ClientPrefQuery>,
) -> Result<Json<ApiResponse<ClientPreferencesResponse>>, AppError> {
    let keys_filter: Option<Vec<String>> = query.keys.map(|k| {
        k.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let key_refs: Option<Vec<&str>> = keys_filter.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());

    let prefs = state.client_pref_service.get_preferences(key_refs.as_deref()).await?;
    Ok(Json(ApiResponse::ok(prefs)))
}

async fn update_client_preferences(
    State(state): State<SystemRouterState>,
    body: Result<Json<UpdateClientPreferencesRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state.client_pref_service.update_preferences(req).await?;
    Ok(Json(ApiResponse::success()))
}

// ===========================================================================
// Provider handlers
// ===========================================================================

async fn list_providers(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<Vec<ProviderResponse>>>, AppError> {
    let providers = state.provider_service.list().await?;
    Ok(Json(ApiResponse::ok(providers)))
}

async fn create_provider(
    State(state): State<SystemRouterState>,
    body: Result<Json<CreateProviderRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<ProviderResponse>>), AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let provider = state.provider_service.create(req).await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(provider))))
}

async fn update_provider(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
    body: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ProviderResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let provider = state.provider_service.update(&provider_id, req).await?;
    Ok(Json(ApiResponse::ok(provider)))
}

async fn delete_provider(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.provider_service.delete(&provider_id).await?;
    Ok(Json(ApiResponse::success()))
}

/// Server-side provider clone: copies the provider row (api-key ciphertext
/// as-is), every `provider_models` profile row (minus per-deployment health)
/// and every connection profile. Replaces the frontend clone, which lost the
/// per-model rows keyed by the old provider_id.
///
/// The JSON body is optional: a missing body (or one without a usable
/// `name`) falls back to the default `"{source name} copy"` clone name; a
/// trimmed non-empty `name` wins.
async fn clone_provider(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
    body: Option<Json<CloneProviderRequest>>,
) -> Result<(StatusCode, Json<ApiResponse<ProviderResponse>>), AppError> {
    let req = body.map(|Json(req)| req).unwrap_or_default();
    let connection_repo = state.provider_connection_service.repository();
    let provider = state
        .provider_service
        .clone_provider(&provider_id, req.name.as_deref(), &connection_repo)
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(provider))))
}

async fn fetch_models(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
    body: Result<Json<FetchModelsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<FetchModelsResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let result = state
        .model_fetch_service
        .fetch_models(&provider_id, &req)
        .await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn fetch_models_anonymous(
    State(state): State<SystemRouterState>,
    body: Result<Json<FetchModelsAnonymousRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<FetchModelsResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let result = state.model_fetch_service.fetch_models_anonymous(&req).await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn detect_protocol(
    State(state): State<SystemRouterState>,
    body: Result<Json<DetectProtocolRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ProtocolDetectionResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let result = state.protocol_detection_service.detect_protocol(&req).await?;
    Ok(Json(ApiResponse::ok(result)))
}

// ===========================================================================
// Provider connection handlers (non-default per-role connection profiles)
// ===========================================================================

async fn list_provider_connections(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<ProviderConnectionResponse>>>, AppError> {
    let connections = state.provider_connection_service.list(&provider_id).await?;
    Ok(Json(ApiResponse::ok(connections)))
}

async fn upsert_provider_connection(
    State(state): State<SystemRouterState>,
    Path(provider_id): Path<String>,
    body: Result<Json<UpsertProviderConnectionRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ProviderConnectionResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let connection = state
        .provider_connection_service
        .upsert(&provider_id, req)
        .await?;
    Ok(Json(ApiResponse::ok(connection)))
}

async fn delete_provider_connection(
    State(state): State<SystemRouterState>,
    Path((provider_id, role)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state
        .provider_connection_service
        .delete(&provider_id, &role)
        .await?;
    Ok(Json(ApiResponse::success()))
}

// ===========================================================================
// Managed model services
// ===========================================================================

fn managed_service(
    state: &SystemRouterState,
) -> Result<std::sync::Arc<ManagedModelService>, AppError> {
    state.managed_model_service.clone().ok_or_else(|| {
        if nomifun_common::free_models_enabled() {
            AppError::ProviderUnavailable("managed model service is not available in this process".into())
        } else {
            AppError::ManagedFreeModelsDisabled(
                "managed free models are disabled for this process".into(),
            )
        }
    })
}

async fn get_free_model_status(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<ManagedModelServiceStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(
        managed_service(&state)?.free_status().await,
    )))
}

async fn get_free_models(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<Vec<ManagedModel>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        managed_service(&state)?.free_models().await,
    )))
}

async fn refresh_free_models(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<ManagedModelServiceStatus>>, AppError> {
    let status = managed_service(&state)?.refresh_free_models().await?;
    if status.last_error.is_none() {
        let provider_id = status.provider_id.as_deref().ok_or_else(|| {
            AppError::Internal("managed free-model status is missing its provider id".into())
        })?;
        let models = status
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        match state
            .model_profile_service
            .seed_missing_inferred(
                provider_id,
                crate::managed_model::FREE_MODEL_PLATFORM,
                &models,
            )
            .await
        {
            Ok(seeded) if seeded > 0 => tracing::info!(
                seeded,
                "Manual managed free-model refresh seeded inferred profiles"
            ),
            Ok(_) => {}
            Err(error) => tracing::warn!(
                error = %error,
                "Manual managed free-model profile reconciliation failed"
            ),
        }
    }
    Ok(Json(ApiResponse::ok(status)))
}

async fn get_free_model_health(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<Vec<ManagedModelHealthResult>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        managed_service(&state)?.free_health_snapshot().await,
    )))
}

async fn check_free_model_health(
    State(state): State<SystemRouterState>,
    Path(model_id): Path<String>,
) -> Result<Json<ApiResponse<ManagedModelHealthResult>>, AppError> {
    let service = managed_service(&state)?;
    let result = service.check_free_model_health(&model_id).await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn check_all_free_model_health(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<ManagedModelHealthBatchResult>>, AppError> {
    let service = managed_service(&state)?;
    Ok(Json(ApiResponse::ok(
        service.check_all_free_model_health().await,
    )))
}

async fn activate_free_models(
    State(state): State<SystemRouterState>,
    body: Result<Json<SetManagedModelServiceEnabledRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ManagedModelServiceStatus>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let status = managed_service(&state)?
        .set_free_enabled(req.enabled)
        .await?;
    Ok(Json(ApiResponse::ok(status)))
}

async fn set_free_model_enabled(
    State(state): State<SystemRouterState>,
    Path(model_id): Path<String>,
    body: Result<Json<SetManagedModelEnabledRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ManagedModelServiceStatus>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let status = managed_service(&state)?
        .set_free_model_enabled(&model_id, req.enabled)
        .await?;
    Ok(Json(ApiResponse::ok(status)))
}

// ===========================================================================
// Model-profile handlers (multimodal model hub)
// ===========================================================================

async fn list_model_profiles(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<Vec<ModelProfile>>>, AppError> {
    // Model profiles are persisted independently from the provider projection.
    // Filter them against the active provider view so a hidden managed free
    // provider cannot leak through the profile endpoint while its rows remain
    // readable for history and future re-enablement.
    let visible_provider_ids = state
        .provider_service
        .list()
        .await?
        .into_iter()
        .map(|provider| provider.provider_id)
        .collect::<HashSet<_>>();
    let profiles = state
        .model_profile_service
        .list()
        .await?
        .into_iter()
        .filter(|profile| visible_provider_ids.contains(&profile.provider_id))
        .collect();
    Ok(Json(ApiResponse::ok(profiles)))
}

async fn upsert_model_profile(
    State(state): State<SystemRouterState>,
    body: Result<Json<ModelProfileUpsertRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ModelProfile>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let profile = state.model_profile_service.upsert(req).await?;
    Ok(Json(ApiResponse::ok(profile)))
}

async fn delete_model_profile(
    State(state): State<SystemRouterState>,
    body: Result<Json<ModelProfileKeyRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .model_profile_service
        .delete(&req.provider_id, &req.model)
        .await?;
    Ok(Json(ApiResponse::success()))
}

/// Resolve enabled models supporting a task (+ required traits) across all
/// providers. Composes the provider list with stored profiles via the pure
/// [`nomifun_api_types::resolve_models`] authority.
async fn resolve_model_profiles(
    State(state): State<SystemRouterState>,
    body: Result<Json<ResolveModelsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ResolveModelsResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let providers = state.provider_service.list().await?;
    let profiles = state.model_profile_service.list().await?;
    let models = nomifun_api_types::resolve_models(&providers, &profiles, req.task, &req.required_traits);
    Ok(Json(ApiResponse::ok(ResolveModelsResponse { models })))
}

// ===========================================================================
// Provider-model handlers (row-level model catalog CRUD)
// ===========================================================================

#[derive(Debug, serde::Deserialize, Default)]
struct ListProviderModelsQuery {
    provider_id: Option<String>,
}

async fn list_provider_models(
    State(state): State<SystemRouterState>,
    Query(query): Query<ListProviderModelsQuery>,
) -> Result<Json<ApiResponse<Vec<ProviderModelResponse>>>, AppError> {
    let models = state
        .provider_model_service
        .list(query.provider_id.as_deref())
        .await?;
    Ok(Json(ApiResponse::ok(models)))
}

async fn create_provider_model(
    State(state): State<SystemRouterState>,
    body: Result<Json<CreateProviderModelRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<ProviderModelResponse>>), AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let model = state.provider_model_service.create(req).await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(model))))
}

async fn update_provider_model(
    State(state): State<SystemRouterState>,
    body: Result<Json<UpdateProviderModelRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ProviderModelResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let model = state.provider_model_service.update(req).await?;
    Ok(Json(ApiResponse::ok(model)))
}

async fn delete_provider_model(
    State(state): State<SystemRouterState>,
    body: Result<Json<ProviderModelKeyRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let deleted = state
        .provider_model_service
        .delete(&req.provider_id, &req.model)
        .await?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "Provider model '{}' not found for provider '{}'",
            req.model, req.provider_id
        )));
    }
    Ok(Json(ApiResponse::success()))
}

// ===========================================================================
// System info & version check handlers
// ===========================================================================

async fn get_system_info(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<SystemInfoResponse>>, AppError> {
    let mut info = crate::sysinfo::get_system_info();
    info.runtime_capabilities = state.runtime_capabilities;
    info.managed_free_models_enabled = nomifun_common::free_models_enabled();
    info.work_dir_change = nomifun_common::work_dir_relocation::read_last_status_best_effort(
        &state.data_dir,
    )
        .map(|status| WorkDirChangeStatus {
        state: match status.state {
            nomifun_common::work_dir_relocation::WorkDirRelocationState::Completed => {
                WorkDirChangeState::Completed
            }
            nomifun_common::work_dir_relocation::WorkDirRelocationState::Failed => {
                WorkDirChangeState::Failed
            }
        },
        operation_id: status.operation_id,
        source_work_dir: status.source_work_dir,
        target_work_dir: status.target_work_dir,
        rollback_copy: status.rollback_copy,
        error: status.error,
    });
    Ok(Json(ApiResponse::ok(info)))
}

async fn pack_support_logs(
    State(state): State<SystemRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<PackSupportLogsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<SupportLogsPackResponse>>, AppError> {
    let Json(request) = body.map_err(|error| AppError::BadRequest(error.to_string()))?;
    let info = crate::sysinfo::get_system_info();

    let mut observation_paths = Vec::new();
    if let Some(conversation_id) = request
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        match state.conversation_repo.as_ref() {
            None => {
                tracing::warn!(
                    conversation_id,
                    "support pack skipped observation attach: conversation repository is not configured"
                );
            }
            Some(repo) => {
                let owned = repo
                    .get(conversation_id)
                    .await
                    .map_err(|error| {
                        AppError::Internal(format!("Failed to load conversation: {error}"))
                    })?
                    .filter(|row| row.user_id == user.id.as_str());
                if owned.is_none() {
                    return Err(AppError::NotFound(format!(
                        "Conversation {conversation_id} not found"
                    )));
                }
                let prefs = state
                    .client_pref_service
                    .get_preferences(Some(&[crate::client_pref::DEVELOPER_MODE_PREF_KEY]))
                    .await?;
                let developer_mode = prefs
                    .get(crate::client_pref::DEVELOPER_MODE_PREF_KEY)
                    .is_some_and(crate::client_pref::preference_value_is_true);
                if developer_mode {
                    observation_paths =
                        crate::support_logs::list_observation_files_for_conversation(
                            &state.data_dir,
                            conversation_id,
                        )?;
                }
            }
        }
    }

    let packed = crate::support_logs::pack_support_logs_with_failed_sse(
        std::path::Path::new(&info.log_dir),
        &state.data_dir,
        request.turn_id.as_deref(),
        &observation_paths,
    )?;
    Ok(Json(ApiResponse::ok(packed)))
}

async fn check_update(
    State(state): State<SystemRouterState>,
    body: Result<Json<UpdateCheckRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<UpdateCheckResult>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let result = state.version_check_service.check_update(&req).await?;
    Ok(Json(ApiResponse::ok(result)))
}

// ===========================================================================
// Factory reset handler
// ===========================================================================

/// Arm a factory reset: write the marker that the next boot consumes. The
/// actual database/derived-data wipe happens early on the next startup (see
/// `nomifun_common::factory_reset`); the client should restart the app right
/// after this returns. Nothing is deleted synchronously here — that would race
/// with the live connection pool and the background write loops.
async fn factory_reset(State(state): State<SystemRouterState>) -> Result<Json<ApiResponse<()>>, AppError> {
    nomifun_common::factory_reset::require_safe_data_work_root_layout(
        &state.data_dir,
        &state.work_dir,
    )
    .map_err(|_| {
        AppError::Conflict(
            "factory reset is unsafe because the active work directory overlaps \
             a product-managed data root; first change to a separate working directory"
                .into(),
        )
    })?;
    nomifun_common::factory_reset::request_v3_dataset_reset(
        &state.data_dir,
        &state.work_dir,
    )?;
    tracing::warn!(target: "factory_reset", "factory reset armed — will wipe database and derived data on next restart");
    Ok(Json(ApiResponse::success()))
}

// ===========================================================================
// Work directory handler
// ===========================================================================

/// Request a user-chosen working directory. This only takes effect on the next
/// boot: the backend resolves `work_dir` before the HTTP server exists.
///
/// Changing the root of a finalized v3 dataset writes a durable relocation
/// plan. The next boot moves only the product-managed `conversations` tree and
/// rebinds the existing v3 receipt to the same generation.
///
/// The new path is validated to be a non-empty, absolute, creatable directory so
/// the next boot does not fail on an unusable value.
async fn set_work_dir(
    State(state): State<SystemRouterState>,
    body: Result<Json<UpdateWorkDirRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<UpdateWorkDirResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    if state.work_dir_is_cli_override {
        return Err(AppError::Conflict(
            "the working directory is controlled by the --work-dir startup option; remove that option before changing it in Settings"
                .into(),
        ));
    }

    let trimmed = req.work_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("work_dir must not be empty".into()));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::BadRequest(format!("work_dir must be an absolute path: {trimmed}")));
    }
    // Reject paths with a leading/trailing-whitespace segment up front, with the
    // same dedicated error the conversation layer raises (service.rs) — otherwise
    // such a work_dir is accepted here only to make every later workspace
    // creation fail, and create_dir_all's behavior on these names is OS-specific.
    if nomifun_common::workspace_path_has_edge_whitespace_segment(&path) {
        return Err(AppError::WorkspacePathEdgeWhitespace(path.display().to_string()));
    }
    let current_work_dir =
        nomifun_common::factory_reset::finalized_v3_work_dir(
            &state.data_dir,
        )?
        .ok_or_else(|| {
            AppError::Conflict(
                "the current dataset has no finalized v3 work-root binding; preserving it without changing directories"
                .into(),
            )
        })?;
    // Reject nested source/target paths before target creation. The target can
    // be new, so the check is repeated after canonicalization by the durable
    // relocation-plan writer and again during startup consumption.
    if !nomifun_common::paths::paths_equivalent(&current_work_dir, &path) {
        nomifun_common::work_dir_relocation::validate_work_dir_relationship(
            &current_work_dir,
            &path,
        )?;
    }
    // Create it one component at a time so a symlink/junction in any parent is
    // rejected before the request can mutate a location outside the selected
    // work root.
    let canonical = nomifun_common::work_dir_relocation::prepare_work_dir_target(&path)?;
    if nomifun_common::paths::paths_equivalent(&current_work_dir, &canonical) {
            // Repair or refresh only the host-local control pointer; the
            // dataset binding is already correct, so no reset is needed.
            nomifun_common::dir_config::set_work_dir(
                &state.data_dir,
                &canonical,
            )?;
            tracing::info!(
                target: "system",
                work_dir = %canonical.display(),
                "work dir override refreshed without changing dataset generation"
            );
            Ok(Json(ApiResponse::ok(UpdateWorkDirResponse {
                operation_id: None,
                restart_required: false,
            })))
    } else {
        let plan = nomifun_common::work_dir_relocation::request_work_dir_relocation(
            &state.data_dir,
            &current_work_dir,
            &canonical,
        )?;
        tracing::info!(
            target: "system",
            work_dir = %canonical.display(),
            operation_id = %plan.operation_id,
            "work dir relocation armed; existing dataset generation will be preserved"
        );
        Ok(Json(ApiResponse::ok(UpdateWorkDirResponse {
            operation_id: Some(plan.operation_id),
            restart_required: true,
        })))
    }
}

fn map_relocation_operation_state(
    state: nomifun_common::work_dir_relocation::RelocationManagementState,
) -> WorkDirRelocationOperationState {
    use nomifun_common::work_dir_relocation::RelocationManagementState as State;
    match state {
        State::Planned => WorkDirRelocationOperationState::Planned,
        State::Copying => WorkDirRelocationOperationState::Copying,
        State::Verified => WorkDirRelocationOperationState::Verified,
        State::Published => WorkDirRelocationOperationState::Published,
        State::SourcePreserved => WorkDirRelocationOperationState::SourcePreserved,
        State::Completed => WorkDirRelocationOperationState::Completed,
        State::Failed => WorkDirRelocationOperationState::Failed,
        State::Paused => WorkDirRelocationOperationState::Paused,
        State::Cancelled => WorkDirRelocationOperationState::Cancelled,
    }
}

fn map_relocation_error_class(
    class: Option<nomifun_common::work_dir_relocation::RelocationErrorClass>,
) -> Option<WorkDirRelocationErrorClass> {
    class.map(|class| match class {
        nomifun_common::work_dir_relocation::RelocationErrorClass::Transient => {
            WorkDirRelocationErrorClass::Transient
        }
        nomifun_common::work_dir_relocation::RelocationErrorClass::Deterministic => {
            WorkDirRelocationErrorClass::Deterministic
        }
        nomifun_common::work_dir_relocation::RelocationErrorClass::Unknown => {
            WorkDirRelocationErrorClass::Unknown
        }
    })
}

fn map_relocation_calculation_mode(
    mode: Option<nomifun_common::work_dir_relocation::RelocationCalculationMode>,
) -> Option<WorkDirRelocationCalculationMode> {
    mode.map(|mode| match mode {
        nomifun_common::work_dir_relocation::RelocationCalculationMode::SameVolumeRename => {
            WorkDirRelocationCalculationMode::SameVolumeRename
        }
        nomifun_common::work_dir_relocation::RelocationCalculationMode::CrossVolumeCopy => {
            WorkDirRelocationCalculationMode::CrossVolumeCopy
        }
    })
}

fn map_relocation_operation(
    operation: nomifun_common::work_dir_relocation::RelocationOperationSnapshot,
) -> WorkDirRelocationOperation {
    WorkDirRelocationOperation {
        operation_id: operation.operation_id,
        state: map_relocation_operation_state(operation.state),
        source_work_dir: operation.source_work_dir,
        target_work_dir: operation.target_work_dir,
        restart_required: operation.restart_required,
        attempt_count: operation.attempt_count,
        last_attempt_at: operation.last_attempt_at,
        last_error_class: map_relocation_error_class(operation.last_error_class),
        last_error_code: operation.last_error_code,
        error: operation.error,
        required_bytes: operation.required_bytes,
        available_bytes: operation.available_bytes,
        shortfall_bytes: operation.shortfall_bytes,
        calculation_mode: map_relocation_calculation_mode(operation.calculation_mode),
    }
}

fn map_relocation_backup(
    backup: nomifun_common::work_dir_relocation::RelocationBackupDescriptor,
) -> WorkDirRelocationBackup {
    WorkDirRelocationBackup {
        operation_id: backup.operation_id,
        generation: backup.generation,
        source_work_dir: backup.source_work_dir,
        target_work_dir: backup.target_work_dir,
        backup_path: backup.backup_path,
        byte_size: backup.byte_size,
        created_at: backup.created_at,
    }
}

async fn get_work_dir_relocation(
    State(state): State<SystemRouterState>,
) -> Result<Json<ApiResponse<WorkDirRelocationResponse>>, AppError> {
    let operation = nomifun_common::work_dir_relocation::read_relocation_operation(&state.data_dir)?
        .map(map_relocation_operation);
    let backups = nomifun_common::work_dir_relocation::read_relocation_backups(&state.data_dir)?
        .into_iter()
        .map(map_relocation_backup)
        .collect();
    Ok(Json(ApiResponse::ok(WorkDirRelocationResponse { operation, backups })))
}

async fn delete_work_dir_relocation(
    State(state): State<SystemRouterState>,
    Path(operation_id): Path<String>,
) -> Result<Json<ApiResponse<UpdateWorkDirResponse>>, AppError> {
    nomifun_common::work_dir_relocation::cancel_relocation(&state.data_dir, &operation_id)?;
    Ok(Json(ApiResponse::ok(UpdateWorkDirResponse {
        operation_id: Some(operation_id),
        restart_required: false,
    })))
}

async fn retry_work_dir_relocation(
    State(state): State<SystemRouterState>,
    Path(operation_id): Path<String>,
) -> Result<Json<ApiResponse<UpdateWorkDirResponse>>, AppError> {
    let plan = nomifun_common::work_dir_relocation::retry_relocation(
        &state.data_dir,
        &operation_id,
    )?;
    Ok(Json(ApiResponse::ok(UpdateWorkDirResponse {
        operation_id: Some(plan.operation_id),
        restart_required: true,
    })))
}

async fn replace_work_dir_relocation(
    State(state): State<SystemRouterState>,
    Path(operation_id): Path<String>,
    body: Result<Json<ReplaceWorkDirRelocationRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<UpdateWorkDirResponse>>, AppError> {
    let Json(request) = body.map_err(|error| AppError::BadRequest(error.to_string()))?;
    let target = PathBuf::from(request.work_dir.trim());
    if request.work_dir.trim().is_empty() || !target.is_absolute() {
        return Err(AppError::BadRequest(
            "work_dir must be a non-empty absolute path".into(),
        ));
    }
    if nomifun_common::workspace_path_has_edge_whitespace_segment(&target) {
        return Err(AppError::WorkspacePathEdgeWhitespace(target.display().to_string()));
    }
    let plan = nomifun_common::work_dir_relocation::replace_relocation(
        &state.data_dir,
        &operation_id,
        &target,
    )?;
    Ok(Json(ApiResponse::ok(UpdateWorkDirResponse {
        operation_id: Some(plan.operation_id),
        restart_required: true,
    })))
}

async fn delete_work_dir_relocation_backup(
    State(state): State<SystemRouterState>,
    Path(operation_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    nomifun_common::work_dir_relocation::delete_relocation_backup(
        &state.data_dir,
        &operation_id,
    )?;
    Ok(Json(ApiResponse::success()))
}
