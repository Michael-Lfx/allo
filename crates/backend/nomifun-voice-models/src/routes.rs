//! Axum HTTP & SSE route handlers for voice models management.
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4, §5.1 (TC-AM-014)
//! - `execution/wave-0-skeleton.zh.md` §4 (VT-0-03)

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;

use nomifun_api_types::voice::{
    VoiceModelDeleteResponse, VoiceModelInstallRequest, VoiceModelStatusResponse,
};

use crate::manager::{ModelManager, ModelManagerError};

fn model_error_response(err: ModelManagerError) -> (StatusCode, Json<serde_json::Value>) {
    let status = if matches!(err, ModelManagerError::Conflict(_)) {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(serde_json::json!({ "error": err.to_string() })))
}

/// Builds the Axum router providing REST and SSE endpoints for voice models.
///
/// Supports both absolute `/api/voice/models/*` paths and relative paths
/// when mounted under a prefix.
pub fn voice_model_routes(manager: Arc<ModelManager>) -> Router {
    let sub_router = Router::new()
        .route("/status", get(get_status_handler))
        .route("/progress", get(get_progress_handler))
        .route("/install", post(install_handler))
        .route("/", delete(delete_handler));

    Router::new()
        .nest("/api/voice/models", sub_router)
        .with_state(manager)
}

/// GET /status -> Current lifecycle status, file breakdown, total size.
async fn get_status_handler(
    State(manager): State<Arc<ModelManager>>,
) -> Json<VoiceModelStatusResponse> {
    Json(manager.get_status().await)
}

/// GET /progress -> Server-Sent Events stream for lifecycle progress events.
async fn get_progress_handler(
    State(manager): State<Arc<ModelManager>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = manager.subscribe_progress();

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(data) = serde_json::to_string(&event) {
                        return Some((Ok(Event::default().data(data)), rx));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Receiver lagged behind, loop to get the latest event
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// POST /install -> Starts download and installation of voice models.
async fn install_handler(
    State(manager): State<Arc<ModelManager>>,
    payload: Option<Json<VoiceModelInstallRequest>>,
) -> impl IntoResponse {
    match manager.install(payload.map(|Json(req)| req)).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(err) => model_error_response(err).into_response(),
    }
}

/// DELETE / -> Deletes local voice model files and resets state to `NotInstalled`.
async fn delete_handler(
    State(manager): State<Arc<ModelManager>>,
) -> Result<Json<VoiceModelDeleteResponse>, (StatusCode, Json<serde_json::Value>)> {
    manager.remove().await.map(Json).map_err(model_error_response)
}
