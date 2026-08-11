//! `/api/montage/*` routes for the Agent Montage runtime.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use nomi_montage::orchestrator::ApprovalRequest;
use nomi_montage::project::CreateProjectRequest;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::state::MontageRouterState;

const SERVE_CACHE_CONTROL: &str = "private, max-age=3600";

pub fn montage_routes(state: MontageRouterState) -> Router {
    Router::new()
        .route("/api/montage/pipelines", get(list_pipelines))
        .route("/api/montage/pipelines/{name}", get(get_pipeline))
        .route("/api/montage/provider-menu", get(provider_menu))
        .route(
            "/api/montage/projects",
            get(list_projects).post(create_project),
        )
        .route("/api/montage/projects/import", post(import_project))
        .route(
            "/api/montage/projects/{id}",
            get(get_project).delete(delete_project),
        )
        .route("/api/montage/projects/{id}/start", post(start_project))
        .route("/api/montage/projects/{id}/cancel", post(cancel_project))
        .route("/api/montage/projects/{id}/status", get(project_status))
        .route(
            "/api/montage/projects/{id}/board-state",
            get(board_state),
        )
        .route("/api/montage/projects/{id}/events", get(project_events))
        .route(
            "/api/montage/projects/{id}/artifacts",
            get(list_artifacts),
        )
        .route(
            "/api/montage/projects/{id}/artifacts/{name}",
            get(get_artifact).put(put_artifact),
        )
        .route(
            "/api/montage/projects/{id}/approvals",
            post(approve_project),
        )
        .route("/api/montage/projects/{id}/export", post(export_project))
        .route(
            "/api/montage/projects/{id}/files/{*path}",
            get(serve_project_file),
        )
        .route(
            "/api/montage/projects/{id}/materialize-to-canvas",
            post(materialize_to_canvas),
        )
        .route(
            "/api/montage/projects/{id}/sync-from-canvas",
            post(sync_from_canvas),
        )
        .with_state(state)
}

async fn list_pipelines(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let pipelines = state.service.list_pipelines();
    Ok(Json(ApiResponse::ok(json!({ "pipelines": pipelines }))))
}

async fn get_pipeline(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(name): Path<String>,
) -> Result<Json<ApiResponse<nomi_montage::pipeline::PipelineManifest>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get_pipeline(&name)?)))
}

async fn provider_menu(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<nomi_montage::orchestrator::ProviderMenu>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.provider_menu())))
}

async fn list_projects(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let projects = state.service.list_projects().await?;
    Ok(Json(ApiResponse::ok(json!({ "projects": projects }))))
}

async fn create_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_montage::project::ProjectRecord>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if req.pipeline.trim().is_empty() {
        return Err(AppError::BadRequest("pipeline is required".into()));
    }
    Ok(Json(ApiResponse::ok(
        state.service.create_project(req).await?,
    )))
}

async fn get_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_montage::service::ProjectDetail>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get_project(&id).await?)))
}

async fn delete_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_project(&id).await?;
    Ok(Json(ApiResponse::ok(())))
}

#[derive(Deserialize)]
struct ImportBody {
    source_path: String,
}

async fn import_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<ImportBody>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_montage::project::ProjectRecord>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let source = body.source_path.trim();
    if source.is_empty() {
        return Err(AppError::BadRequest("source_path is required".into()));
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .import_project(PathBuf::from(source).as_path())
            .await?,
    )))
}

async fn start_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.start(&id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn cancel_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.cancel(&id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn project_status(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_montage::service::RunStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.status(&id).await?)))
}

async fn board_state(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<nomi_montage::project::BoardState>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.board_state(&id).await?,
    )))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default = "default_events_limit")]
    limit: usize,
}

fn default_events_limit() -> usize {
    100
}

async fn project_events(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let limit = query.limit.clamp(1, 500);
    let events = state.service.recent_events(&id, limit)?;
    Ok(Json(ApiResponse::ok(json!({ "events": events }))))
}

async fn list_artifacts(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let names = state.service.list_artifacts(&id)?;
    Ok(Json(ApiResponse::ok(json!({ "artifacts": names }))))
}

async fn get_artifact(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.get_artifact(&id, &name).await?,
    )))
}

#[derive(Deserialize)]
struct PutArtifactBody {
    content: serde_json::Value,
}

async fn put_artifact(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, name)): Path<(String, String)>,
    body: Result<Json<PutArtifactBody>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state
        .service
        .put_artifact(&id, &name, body.content)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn approve_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<ApprovalRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_montage::project::BoardState>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(ApiResponse::ok(
        state.service.approve(&id, req).await?,
    )))
}

#[derive(Deserialize)]
struct ExportBody {
    dest_path: String,
}

async fn export_project(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<ExportBody>, JsonRejection>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let dest = body.dest_path.trim();
    if dest.is_empty() {
        return Err(AppError::BadRequest("dest_path is required".into()));
    }
    let path = state
        .service
        .export_zip(&id, PathBuf::from(dest).as_path())
        .await?;
    Ok(Json(ApiResponse::ok(json!({
        "dest_path": path.to_string_lossy(),
    }))))
}

async fn serve_project_file(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let file_path = state.service.resolve_project_file(&id, &path)?;
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| AppError::Internal(format!("read file: {e}")))?;
    let mime = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, SERVE_CACHE_CONTROL)
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(format!("build response: {e}")))
}

async fn materialize_to_canvas(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::materialize::MaterializeToCanvasResult>>, AppError> {
    let result =
        crate::materialize::materialize_project_to_canvas(&state.service, &state.canvas, &id)
            .await?;
    Ok(Json(ApiResponse::ok(result)))
}

async fn sync_from_canvas(
    State(state): State<MontageRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<crate::materialize::SyncFromCanvasRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<crate::materialize::SyncFromCanvasResult>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    if req.project_id.trim().is_empty() {
        return Err(AppError::BadRequest("project_id is required".into()));
    }
    let result =
        crate::materialize::sync_canvas_shots_to_project(&state.service, &state.canvas, &id, req)
            .await?;
    Ok(Json(ApiResponse::ok(result)))
}
