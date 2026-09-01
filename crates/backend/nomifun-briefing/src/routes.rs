use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use nomi_briefing::{BeatScript, ResearchPlan, SessionRecord};
use nomifun_api_types::{
    ApiResponse, BriefingCreateRequest, BriefingModelsRequest, BriefingSessionListResponse,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::service::safe_artifact_path;
use crate::state::BriefingRouterState;

pub fn briefing_routes(state: BriefingRouterState) -> Router {
    Router::new()
        .route(
            "/api/briefing/sessions",
            get(list_sessions).post(create_session),
        )
        .route(
            "/api/briefing/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/briefing/sessions/{id}/models", post(update_models))
        .route("/api/briefing/sessions/{id}/plan", post(confirm_plan).get(get_plan))
        .route("/api/briefing/sessions/{id}/script", get(get_script).post(save_script))
        .route("/api/briefing/sessions/{id}/run", post(run_session))
        .route("/api/briefing/sessions/{id}/status", get(session_status))
        .route("/api/briefing/sessions/{id}/cancel", post(cancel_session))
        .route(
            "/api/briefing/sessions/{id}/artifacts/{*path}",
            get(get_artifact),
        )
        .with_state(state)
}

async fn list_sessions(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<BriefingSessionListResponse>>, AppError> {
    let service = state.service.clone();
    let data = tokio::task::spawn_blocking(move || service.list_response())
        .await
        .map_err(|e| AppError::Internal(format!("briefing list join: {e}")))??;
    Ok(Json(ApiResponse::ok(data)))
}

async fn create_session(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<BriefingCreateRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<SessionRecord>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let session = state.service.create(
        &body.intent,
        body.title,
        body.format_secs,
        &body.research_depth,
        body.time_window_hours,
        body.source_urls,
        body.tts_provider_id,
        body.tts_model,
        body.tts_voice,
        body.image_provider_id,
        body.image_model,
    )?;
    Ok(Json(ApiResponse::ok(session)))
}

async fn get_session(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<SessionRecord>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get(&id)?)))
}

async fn update_models(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<BriefingModelsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<SessionRecord>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(ApiResponse::ok(state.service.update_models(
        &id,
        body.tts_provider_id,
        body.tts_model,
        body.tts_voice,
        body.image_provider_id,
        body.image_model,
    )?)))
}

async fn delete_session(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    state.service.delete(&id)?;
    Ok(Json(ApiResponse::ok(json!({ "ok": true }))))
}

async fn get_plan(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<ResearchPlan>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.load_plan(&id)?)))
}

async fn confirm_plan(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<Option<ResearchPlan>>, JsonRejection>,
) -> Result<Json<ApiResponse<ResearchPlan>>, AppError> {
    let plan = match body {
        Ok(Json(plan)) => plan,
        Err(_) => None,
    };
    Ok(Json(ApiResponse::ok(state.service.confirm_plan(&id, plan)?)))
}

async fn get_script(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<BeatScript>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.load_script(&id)?)))
}

async fn save_script(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(script): Json<BeatScript>,
) -> Result<Json<ApiResponse<BeatScript>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.save_script(&id, script)?)))
}

#[derive(Deserialize)]
struct RunBody {
    #[serde(default)]
    confirm_plan: bool,
}

async fn run_session(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<RunBody>, JsonRejection>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let confirm = body.ok().map(|Json(b)| b.confirm_plan).unwrap_or(true);
    state.service.start_run(&id, confirm)?;
    Ok(Json(ApiResponse::ok(json!({ "started": true }))))
}

async fn session_status(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_briefing::RunSnapshot>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.status(&id)?)))
}

async fn cancel_session(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    state.service.cancel(&id);
    Ok(Json(ApiResponse::ok(json!({ "ok": true }))))
}

async fn get_artifact(
    State(state): State<BriefingRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, rel)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let root = state.service.working_dir(&id)?;
    let path = safe_artifact_path(&root, &rel)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound(rel))?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.to_string()))
}
