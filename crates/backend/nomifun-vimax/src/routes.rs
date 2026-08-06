//! `/api/vimax/*` routes matching the frontend `videoGeneration/api.ts` client.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Extension, Json, Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, patch, post};
use serde::Deserialize;
use serde_json::json;
use tower_http::limit::RequestBodyLimitLayer;

use nomi_vimax::CameoUpdate;
use nomifun_api_types::{ApiResponse, TvShowPublishSessionRequest};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::state::VimaxRouterState;

/// Cameo upload body limit (matches nomi-vimax `CAMEO_MAX_BYTES` + multipart overhead).
const CAMEO_UPLOAD_BODY_LIMIT: usize = 11 * 1024 * 1024;
/// Artifact binary replace body limit (matches nomi-vimax `MAX_BINARY_BYTES` + overhead).
const ARTIFACT_UPLOAD_BODY_LIMIT: usize = 11 * 1024 * 1024;

pub fn vimax_routes(state: VimaxRouterState) -> Router {
    let cameo_upload = Router::new()
        .route("/api/vimax/sessions/{id}/cameos", post(upload_cameo))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(CAMEO_UPLOAD_BODY_LIMIT))
        .with_state(state.clone());

    let artifact_upload = Router::new()
        .route(
            "/api/vimax/sessions/{id}/artifact-replace",
            post(replace_artifact),
        )
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(ARTIFACT_UPLOAD_BODY_LIMIT))
        .with_state(state.clone());

    Router::new()
        .route("/api/vimax/sessions", get(list_sessions).post(create_session))
        .route("/api/vimax/sessions/import", post(import_session))
        .route(
            "/api/vimax/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/vimax/sessions/{id}/plan", post(plan_session))
        .route("/api/vimax/sessions/{id}/revise", post(revise_session))
        .route("/api/vimax/sessions/{id}/render", post(render_session))
        .route("/api/vimax/sessions/{id}/status", get(session_status))
        .route("/api/vimax/sessions/{id}/cancel", post(cancel_session))
        .route("/api/vimax/sessions/{id}/export", post(export_session))
        .route("/api/vimax/sessions/{id}/artifacts", get(list_artifacts))
        .route(
            "/api/vimax/sessions/{id}/artifacts/{*path}",
            get(get_artifact).put(put_artifact_text),
        )
        .route(
            "/api/vimax/sessions/{id}/artifact-prompt",
            get(get_artifact_prompt).put(put_artifact_prompt),
        )
        .route("/api/vimax/sessions/{id}/cameos", get(list_cameos))
        .route(
            "/api/vimax/sessions/{id}/cameos/{cameo_id}",
            patch(update_cameo).delete(delete_cameo),
        )
        .route(
            "/api/vimax/sessions/{id}/cameos/{cameo_id}/file",
            get(get_cameo_file),
        )
        .route(
            "/api/vimax/sessions/{id}/tv-show/publish",
            post(publish_session_to_tv_show),
        )
        .route("/api/vimax/tv-show/list", get(tv_show_list))
        .route("/api/vimax/tv-show/mine", get(tv_show_mine))
        .route(
            "/api/vimax/tv-show/{id}",
            get(tv_show_detail).delete(tv_show_delete),
        )
        .route(
            "/api/vimax/tv-show/{id}/like",
            post(tv_show_like).delete(tv_show_unlike),
        )
        .route("/api/vimax/tv-show/{id}/import", post(import_tv_show))
        .with_state(state)
        .merge(cameo_upload)
        .merge(artifact_upload)
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
    /// Seedance / poster aspect ratio (`16:9`, `9:16`, …).
    #[serde(default)]
    aspect_ratio: Option<String>,
    /// Output resolution (`480p` / `720p` / `1080p`); clamped per video model.
    #[serde(default)]
    resolution: Option<String>,
    /// Output fps (Seedance fixed at 24).
    #[serde(default)]
    fps: Option<u32>,
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
            body.aspect_ratio,
            body.resolution,
            body.fps,
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
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    fps: Option<u32>,
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
        .render(
            &id,
            body.llm_model,
            body.image_model,
            body.video_model,
            body.resolution,
            body.fps,
        )
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

#[derive(Deserialize)]
struct WriteArtifactBody {
    content: String,
}

/// PUT JSON `{ content }` to overwrite a text/JSON artifact in place.
async fn put_artifact_text(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, path)): Path<(String, String)>,
    body: Result<Json<WriteArtifactBody>, JsonRejection>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let result = state
        .service
        .write_artifact_text(&id, &path, &body.content)
        .await?;
    Ok(Json(ApiResponse::ok(json!({
        "revised_path": result.revised_path,
        "stale_keys": result.stale_keys,
        "invalidated": result.invalidated,
    }))))
}

/// Multipart upload that replaces an image artifact (`path` + `file` fields).
async fn replace_artifact(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let fields = extract_artifact_replace_fields(multipart).await?;
    let result = state
        .service
        .replace_artifact_file(&id, &fields.path, fields.bytes)
        .await?;
    Ok(Json(ApiResponse::ok(json!({
        "revised_path": result.revised_path,
        "stale_keys": result.stale_keys,
        "invalidated": result.invalidated,
    }))))
}

struct ArtifactReplaceFields {
    path: String,
    bytes: Vec<u8>,
}

async fn extract_artifact_replace_fields(
    mut multipart: Multipart,
) -> Result<ArtifactReplaceFields, AppError> {
    let mut path: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "path" => {
                path = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("read path: {e}")))?,
                );
            }
            "file" | "content" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("read file field: {e}")))?;
                bytes = Some(data.to_vec());
            }
            _ => {}
        }
    }
    let path = path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AppError::BadRequest("missing 'path' field".into()))?;
    let bytes = bytes.ok_or_else(|| AppError::BadRequest("missing 'file' field".into()))?;
    if bytes.is_empty() {
        return Err(AppError::BadRequest("uploaded file is empty".into()));
    }
    Ok(ArtifactReplaceFields { path, bytes })
}

#[derive(Deserialize)]
struct ArtifactPromptQuery {
    path: String,
}

#[derive(Deserialize)]
struct ArtifactPromptBody {
    path: String,
    prompt: String,
}

async fn get_artifact_prompt(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ArtifactPromptQuery>,
) -> Result<Json<ApiResponse<nomi_vimax::ImagePromptInfo>>, AppError> {
    let path = query.path.trim();
    if path.is_empty() {
        return Err(AppError::BadRequest("path is required".into()));
    }
    let info = state.service.get_artifact_image_prompt(&id, path).await?;
    Ok(Json(ApiResponse::ok(info)))
}

async fn put_artifact_prompt(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<ArtifactPromptBody>, JsonRejection>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let path = body.path.trim();
    if path.is_empty() {
        return Err(AppError::BadRequest("path is required".into()));
    }
    let result = state
        .service
        .update_artifact_image_prompt(&id, path, &body.prompt)
        .await?;
    Ok(Json(ApiResponse::ok(json!({
        "revised_path": result.revised_path,
        "stale_keys": result.stale_keys,
        "invalidated": result.invalidated,
    }))))
}

#[derive(Deserialize)]
struct ExportBody {
    dest_path: String,
}

async fn export_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<ExportBody>, JsonRejection>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let dest = body.dest_path.trim();
    if dest.is_empty() {
        return Err(AppError::BadRequest("dest_path is required".into()));
    }
    let path = state.service.export_session(&id, PathBuf::from(dest)).await?;
    Ok(Json(ApiResponse::ok(json!({
        "dest_path": path.to_string_lossy(),
    }))))
}

#[derive(Deserialize)]
struct ImportBody {
    source_path: String,
}

async fn import_session(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<ImportBody>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_vimax::SessionRecord>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let source = body.source_path.trim();
    if source.is_empty() {
        return Err(AppError::BadRequest("source_path is required".into()));
    }
    let session = state
        .service
        .import_session(PathBuf::from(source))
        .await?;
    Ok(Json(ApiResponse::ok(session)))
}

async fn list_cameos(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let photos = state.service.list_cameos(&id)?;
    Ok(Json(ApiResponse::ok(json!({ "photos": photos }))))
}

struct CameoUploadFields {
    bytes: Vec<u8>,
    character_name: String,
    description: String,
}

async fn extract_cameo_multipart(mut multipart: Multipart) -> Result<CameoUploadFields, AppError> {
    let mut bytes: Option<Vec<u8>> = None;
    let mut character_name = String::new();
    let mut description = String::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("read file field: {e}")))?;
                bytes = Some(data.to_vec());
            }
            "character_name" | "name" => {
                character_name = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("read character_name: {e}")))?;
            }
            "description" => {
                description = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("read description: {e}")))?;
            }
            _ => {
                // Drain unknown fields.
                let _ = field.bytes().await;
            }
        }
    }

    let bytes = bytes.ok_or_else(|| AppError::BadRequest("missing 'file' field".into()))?;
    if character_name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "character_name is required".into(),
        ));
    }
    Ok(CameoUploadFields {
        bytes,
        character_name,
        description,
    })
}

async fn upload_cameo(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<ApiResponse<nomi_vimax::CameoPhotoEntry>>), AppError> {
    let fields = extract_cameo_multipart(multipart).await?;
    let entry = state
        .service
        .upload_cameo(
            &id,
            fields.bytes,
            fields.character_name,
            fields.description,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(entry))))
}

#[derive(Deserialize, Default)]
struct UpdateCameoBody {
    #[serde(default)]
    character_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

async fn update_cameo(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, cameo_id)): Path<(String, String)>,
    body: Result<Json<UpdateCameoBody>, JsonRejection>,
) -> Result<Json<ApiResponse<nomi_vimax::CameoPhotoEntry>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let entry = state
        .service
        .update_cameo(
            &id,
            &cameo_id,
            CameoUpdate {
                character_name: body.character_name,
                description: body.description,
                bound_identifier: None,
            },
        )
        .await?;
    Ok(Json(ApiResponse::ok(entry)))
}

async fn delete_cameo(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, cameo_id)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_cameo(&id, &cameo_id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn get_cameo_file(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((id, cameo_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let abs = state.service.cameo_photo_path(&id, &cameo_id)?;
    if !abs.is_file() {
        return Err(AppError::NotFound(format!("cameo photo {cameo_id}")));
    }
    let bytes = tokio::fs::read(&abs)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "private, max-age=60")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.to_string()))?)
}

async fn publish_session_to_tv_show(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Option<Json<TvShowPublishSessionRequest>>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowPublishResponse>>, AppError> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let result = state.service.publish_session_to_tv_show(&id, req).await?;
    Ok(Json(ApiResponse::ok(result)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TvShowListQuery {
    page: Option<i32>,
    page_size: Option<i32>,
    workflow: Option<String>,
    keyword: Option<String>,
    sort: Option<String>,
    status: Option<String>,
}

async fn tv_show_list(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    axum::extract::Query(query): axum::extract::Query<TvShowListQuery>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowListResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .service
            .tv_show_list(
                query.page,
                query.page_size,
                query.workflow,
                query.keyword,
                query.sort,
            )
            .await?,
    )))
}

async fn tv_show_mine(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    axum::extract::Query(query): axum::extract::Query<TvShowListQuery>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowListResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .service
            .tv_show_mine(query.page, query.page_size, query.status)
            .await?,
    )))
}

async fn tv_show_detail(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowVideo>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.tv_show_detail(id).await?)))
}

async fn tv_show_like(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowLikeResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.tv_show_like(id).await?)))
}

async fn tv_show_unlike(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<nomifun_api_types::TvShowLikeResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.tv_show_unlike(id).await?,
    )))
}

async fn tv_show_delete(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.tv_show_delete(id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn import_tv_show(
    State(state): State<VimaxRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<nomi_vimax::SessionRecord>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.import_tv_show(id).await?)))
}
