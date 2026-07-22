//! `/api/vimax/*` routes matching the frontend `videoGeneration/api.ts` client.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::state::VimaxRouterState;

pub fn vimax_routes(state: VimaxRouterState) -> Router {
    Router::new()
        .route("/api/vimax/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/vimax/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/vimax/sessions/{id}/plan", post(plan_session))
        .route("/api/vimax/sessions/{id}/revise", post(revise_session))
        .route("/api/vimax/sessions/{id}/render", post(render_session))
        .route("/api/vimax/sessions/{id}/status", get(session_status))
        .route("/api/vimax/sessions/{id}/cancel", post(cancel_session))
        .route("/api/vimax/sessions/{id}/artifacts", get(list_artifacts))
        .route(
            "/api/vimax/sessions/{id}/artifacts/{*path}",
            get(get_artifact),
        )
        .with_state(state)
}

async fn list_sessions(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let sessions = state.service.list_sessions()?;
    Ok(Json(ApiResponse::ok(json!({ "sessions": sessions }))))
}

#[derive(Deserialize)]
struct CreateBody {
    workflow: String,
    #[serde(default)]
    title: Option<String>,
}

async fn create_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<CreateBody>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_vimax::SessionRecord>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let session = state.service.create_session(&body.workflow, body.title)?;
    Ok(Json(ApiResponse::ok(session)))
}

async fn get_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_vimax::SessionRecord>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get_session(&id)?)))
}

async fn delete_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_session(&id).await?;
    Ok(Json(ApiResponse::ok(())))
}

#[derive(Deserialize)]
struct PlanBody {
    #[serde(default)]
    idea: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    novel_text: Option<String>,
    #[serde(default)]
    user_requirement: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    llm_model: Option<String>,
    #[serde(default)]
    image_model: Option<String>,
    #[serde(default)]
    video_model: Option<String>,
    /// Target finished video length in seconds (planning + clip duration).
    #[serde(default)]
    target_duration_secs: Option<u32>,
}

async fn plan_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<PlanBody>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .service
        .plan(
            &id,
            body.idea,
            body.script,
            body.novel_text,
            body.user_requirement,
            body.style,
            body.llm_model,
            body.image_model,
            body.video_model,
            body.target_duration_secs,
        )
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

#[derive(Deserialize)]
struct ReviseBody {
    revision_target: String,
    revision_instruction: String,
}

async fn revise_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<ReviseBody>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .service
        .revise(&id, body.revision_target, body.revision_instruction)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

#[derive(Deserialize, Default)]
struct RenderBody {
    #[serde(default)]
    llm_model: Option<String>,
    #[serde(default)]
    image_model: Option<String>,
    #[serde(default)]
    video_model: Option<String>,
}

async fn render_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Option<Json<RenderBody>>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    state
        .service
        .render(&id, body.llm_model, body.image_model, body.video_model)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn session_status(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_vimax::RenderStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.status(&id).await?)))
}

async fn cancel_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.cancel(&id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn list_artifacts(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let tree = state.service.list_artifacts(&id)?;
    Ok(Json(ApiResponse::ok(json!({ "tree": tree }))))
}

async fn get_artifact(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let abs: PathBuf = state.service.artifact_path(&id, &path)?;
    if !abs.is_file() {
        return Err(AppError::NotFound(format!("artifact {path}")));
    }
    let bytes = tokio::fs::read(&abs)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let mime = mime_guess::from_path(&abs)
        .first_or_octet_stream()
        .to_string();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "private, max-age=60")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.to_string()))?)
}
