//! `/api/insights/*` route handlers.

use axum::Router;
use axum::extract::{Json, State};
use axum::routing::{get, post};

use nomifun_api_types::{
    ApiResponse, InsightsContributionStatusResponse, InsightsFlushResponse,
    InsightsResetOutboxRequest, InsightsResetOutboxResponse, UpdateInsightsContributionRequest,
};
use nomifun_common::AppError;

use crate::state::InsightsRouterState;

pub fn insights_routes(state: InsightsRouterState) -> Router {
    Router::new()
        .route("/api/insights/contribution/status", get(get_status))
        .route("/api/insights/contribution", post(update_contribution))
        .route("/api/insights/contribution/flush", post(flush_contribution))
        .route("/api/insights/contribution/reset", post(reset_outbox))
        .with_state(state)
}

async fn get_status(
    State(state): State<InsightsRouterState>,
) -> Result<Json<ApiResponse<InsightsContributionStatusResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.status().await?)))
}

async fn update_contribution(
    State(state): State<InsightsRouterState>,
    Json(req): Json<UpdateInsightsContributionRequest>,
) -> Result<Json<ApiResponse<InsightsContributionStatusResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.update_contribution(req).await?,
    )))
}

async fn flush_contribution(
    State(state): State<InsightsRouterState>,
) -> Result<Json<ApiResponse<InsightsFlushResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.flush().await?)))
}

async fn reset_outbox(
    State(state): State<InsightsRouterState>,
    Json(req): Json<InsightsResetOutboxRequest>,
) -> Result<Json<ApiResponse<InsightsResetOutboxResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.reset_outbox(req)?)))
}
