use axum::Router;
use axum::extract::{Json, Query, State};
use axum::routing::{get, patch};

use nomifun_api_types::{
    ApiResponse, MediaCreditsResponse, MediaModelListResponse, MediaSettingsResponse,
    MediaWorkflowHistoryResponse, UpdateMediaSettingsRequest,
};
use nomifun_common::AppError;
use serde::Deserialize;

use crate::state::MediaRouterState;

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

pub fn media_routes(state: MediaRouterState) -> Router {
    Router::new()
        .route("/api/media/settings", get(get_settings).patch(update_settings))
        .route("/api/media/credits", get(get_credits))
        .route("/api/media/models", get(list_models))
        .route("/api/media/workflows/history", get(workflow_history))
        .with_state(state)
}

async fn get_settings(
    State(state): State<MediaRouterState>,
) -> Result<Json<ApiResponse<MediaSettingsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.settings())))
}

async fn update_settings(
    State(state): State<MediaRouterState>,
    Json(req): Json<UpdateMediaSettingsRequest>,
) -> Result<Json<ApiResponse<MediaSettingsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.update_settings(req)?)))
}

async fn get_credits(
    State(state): State<MediaRouterState>,
) -> Result<Json<ApiResponse<MediaCreditsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.credits().await?)))
}

async fn list_models(
    State(state): State<MediaRouterState>,
) -> Result<Json<ApiResponse<MediaModelListResponse>>, AppError> {
    let (image_models, video_models) = state.service.list_models().await?;
    Ok(Json(ApiResponse::ok(MediaModelListResponse {
        image_models,
        video_models,
    })))
}

async fn workflow_history(
    State(state): State<MediaRouterState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ApiResponse<MediaWorkflowHistoryResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.workflow_history(query.limit),
    )))
}
