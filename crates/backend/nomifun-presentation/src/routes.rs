//! `/api/presentation/capability/*`. Mounted behind instance-owner auth by
//! nomifun-app.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use nomifun_api_types::{
    ApiResponse, ErrorResponse, PresentationCancelResponse, PresentationCapabilityView,
    PresentationInstallRequest, PresentationInstallResponse, PresentationUninstallResponse,
};

use crate::service::{PresentationCapabilityService, UninstallError};

type ApiError = (StatusCode, Json<ErrorResponse>);

#[derive(Clone)]
pub struct PresentationRouterState {
    pub service: Arc<PresentationCapabilityService>,
}

pub fn presentation_routes(state: PresentationRouterState) -> Router {
    Router::new()
        .route("/api/presentation/capability", axum::routing::delete(uninstall))
        .route("/api/presentation/capability/status", get(status))
        .route("/api/presentation/capability/install", post(install))
        .route("/api/presentation/capability/cancel", post(cancel))
        .with_state(state)
}

async fn status(State(state): State<PresentationRouterState>) -> Json<ApiResponse<PresentationCapabilityView>> {
    Json(ApiResponse::ok(state.service.status()))
}

/// An empty body means `{}`. A malformed body is a 400, not a silent default.
/// Install failures happen in the background and surface through `status`.
async fn install(
    State(state): State<PresentationRouterState>,
    body: Bytes,
) -> Result<Json<ApiResponse<PresentationInstallResponse>>, ApiError> {
    let request = if body.iter().all(u8::is_ascii_whitespace) {
        PresentationInstallRequest::default()
    } else {
        serde_json::from_slice::<PresentationInstallRequest>(&body).map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid install request: {error}"),
                "PRESENTATION_INSTALL_BAD_REQUEST",
            )
        })?
    };
    Ok(Json(ApiResponse::ok(state.service.start_install(request.force))))
}

async fn cancel(State(state): State<PresentationRouterState>) -> Json<ApiResponse<PresentationCancelResponse>> {
    Json(ApiResponse::ok(state.service.cancel()))
}

async fn uninstall(
    State(state): State<PresentationRouterState>,
) -> Result<Json<ApiResponse<PresentationUninstallResponse>>, ApiError> {
    match state.service.uninstall() {
        Ok(capability) => Ok(Json(ApiResponse::ok(PresentationUninstallResponse { capability }))),
        Err(UninstallError::Busy) => Err(error_response(
            StatusCode::CONFLICT,
            UninstallError::Busy.to_string(),
            "PRESENTATION_CAPABILITY_BUSY",
        )),
        Err(UninstallError::Io(message)) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            message,
            "PRESENTATION_UNINSTALL_FAILED",
        )),
    }
}

fn error_response(status: StatusCode, message: String, code: &str) -> ApiError {
    (status, Json(ErrorResponse::new(message, code)))
}
