//! `/api/local-models/*` route handlers.
//!
//! All routes are owner-scoped and mounted behind the app's instance-owner
//! auth gate (same as the terminal/office/shell surfaces) because they spawn a
//! local OS process and download files as the backend account.

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, Path, State};
use axum::routing::{get, post};

use nomifun_api_types::{
    ApiResponse, LocalModelLaunchOptions, LocalModelResponse, LocalModelsResponse,
};
use nomifun_common::AppError;

use crate::state::LocalModelsRouterState;

/// Routes for the local model runtime.
pub fn local_models_routes(state: LocalModelsRouterState) -> Router {
    Router::new()
        .route("/api/local-models", get(list_models))
        .route("/api/local-models/{id}/install", post(install_model))
        .route("/api/local-models/{id}/cancel", post(cancel_model))
        .route("/api/local-models/{id}/start", post(start_model))
        .route("/api/local-models/{id}/stop", post(stop_model))
        .route("/api/local-models/{id}/uninstall", post(uninstall_model))
        .with_state(state)
}

async fn list_models(
    State(state): State<LocalModelsRouterState>,
) -> Result<Json<ApiResponse<LocalModelsResponse>>, AppError> {
    let listed = state.service.list().await;
    Ok(Json(ApiResponse::ok(listed)))
}

async fn install_model(
    State(state): State<LocalModelsRouterState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<LocalModelResponse>>, AppError> {
    let model = state.service.install(&id).await?;
    Ok(Json(ApiResponse::ok(LocalModelResponse { model })))
}

async fn cancel_model(
    State(state): State<LocalModelsRouterState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<LocalModelResponse>>, AppError> {
    let model = state.service.cancel(&id).await?;
    Ok(Json(ApiResponse::ok(LocalModelResponse { model })))
}

async fn start_model(
    State(state): State<LocalModelsRouterState>,
    Path(id): Path<String>,
    body: Result<Json<LocalModelLaunchOptions>, JsonRejection>,
) -> Result<Json<ApiResponse<LocalModelResponse>>, AppError> {
    let Json(options) = body.unwrap_or(Json(LocalModelLaunchOptions::default()));
    let model = state.service.start(&id, options).await?;
    Ok(Json(ApiResponse::ok(LocalModelResponse { model })))
}

async fn stop_model(
    State(state): State<LocalModelsRouterState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<LocalModelResponse>>, AppError> {
    let model = state.service.stop(&id).await?;
    Ok(Json(ApiResponse::ok(LocalModelResponse { model })))
}

async fn uninstall_model(
    State(state): State<LocalModelsRouterState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<LocalModelResponse>>, AppError> {
    let model = state.service.uninstall(&id).await?;
    Ok(Json(ApiResponse::ok(LocalModelResponse { model })))
}
