//! `/api/video-canvas/*` HTTP surface.

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Extension, Json, Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::Value;
use tower_http::limit::RequestBodyLimitLayer;

use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::MAX_MEDIA_BYTES;
use crate::dto::{CanvasMediaMeta, CanvasProjectMeta, GenerationTaskView};
use crate::service::NewGenerationRequest;
use crate::state::CanvasRouterState;

const SERVE_CACHE_CONTROL: &str = "private, max-age=3600";

pub fn video_canvas_routes(state: CanvasRouterState) -> Router {
    let upload = Router::new()
        .route("/api/video-canvas/media/upload", post(upload_media))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(MAX_MEDIA_BYTES))
        .with_state(state.clone());

    Router::new()
        .route(
            "/api/video-canvas/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/video-canvas/projects/{project_id}",
            get(get_project)
                .patch(patch_project)
                .delete(delete_project),
        )
        .route(
            "/api/video-canvas/projects/{project_id}/doc",
            axum::routing::put(put_doc),
        )
        .route("/api/video-canvas/media", get(list_media))
        .route("/api/video-canvas/media/concat", post(concat_media))
        .route(
            "/api/video-canvas/media/{media_id}",
            axum::routing::delete(delete_media),
        )
        .route(
            "/api/video-canvas/media/{media_id}/path",
            get(get_media_path),
        )
        .route("/api/video-canvas/tasks", post(create_task).get(list_tasks))
        .route("/api/video-canvas/tasks/{task_id}", get(get_task).delete(delete_task))
        .route(
            "/api/video-canvas/tasks/{task_id}/cancel",
            post(cancel_task),
        )
        .route(
            "/api/video-canvas/llm/v1/chat/completions",
            post(crate::llm_proxy::proxy_chat_completions),
        )
        .with_state(state)
        .merge(upload)
}

/// Auth-exempt binary serve (same rationale as workshop public files).
pub fn video_canvas_public_routes(state: CanvasRouterState) -> Router {
    Router::new()
        .route(
            "/api/video-canvas/media/{media_id}",
            get(serve_media).head(head_media),
        )
        .with_state(state)
}

#[derive(serde::Serialize)]
struct ProjectListResponse {
    projects: Vec<CanvasProjectMeta>,
}

async fn list_projects(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<ProjectListResponse>>, AppError> {
    let projects = state.service.list_projects().await?;
    Ok(Json(ApiResponse::ok(ProjectListResponse { projects })))
}

#[derive(Deserialize)]
struct CreateProjectBody {
    #[serde(default)]
    title: Option<String>,
}

async fn create_project(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<CreateProjectBody>, JsonRejection>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let title = body.ok().and_then(|Json(b)| b.title);
    let meta = state.service.create_project(title).await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(meta))))
}

#[derive(serde::Serialize)]
struct ProjectDetail {
    meta: CanvasProjectMeta,
    doc: Value,
}

async fn get_project(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<ProjectDetail>>, AppError> {
    let project = state.service.get_project(&project_id).await?;
    Ok(Json(ApiResponse::ok(ProjectDetail {
        meta: project.meta,
        doc: project.doc,
    })))
}

#[derive(Deserialize)]
struct PatchProjectBody {
    title: String,
}

async fn patch_project(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(project_id): Path<String>,
    body: Result<Json<PatchProjectBody>, JsonRejection>,
) -> Result<Json<ApiResponse<CanvasProjectMeta>>, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let meta = state
        .service
        .patch_project_title(&project_id, body.title)
        .await?;
    Ok(Json(ApiResponse::ok(meta)))
}

async fn put_doc(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(project_id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<ApiResponse<CanvasProjectMeta>>, AppError> {
    let Json(doc) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let meta = state.service.put_doc(&project_id, doc).await?;
    Ok(Json(ApiResponse::ok(meta)))
}

async fn delete_project(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_project(&project_id).await?;
    Ok(Json(ApiResponse::ok(())))
}

#[derive(serde::Serialize)]
struct MediaListResponse {
    items: Vec<CanvasMediaMeta>,
}

async fn list_media(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<MediaListResponse>>, AppError> {
    let items = state.service.list_media().await?;
    Ok(Json(ApiResponse::ok(MediaListResponse { items })))
}

async fn upload_media(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    mut multipart: Multipart,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let mut file_name = String::from("upload.bin");
    let mut content_type: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;
    let mut title: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                if let Some(n) = field.file_name() {
                    file_name = n.to_string();
                }
                content_type = field.content_type().map(|m| m.to_string());
                bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("read file: {e}")))?
                        .to_vec(),
                );
            }
            "title" => {
                title = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("title: {e}")))?,
                );
            }
            _ => {}
        }
    }

    let bytes = bytes.ok_or_else(|| AppError::BadRequest("file field required".into()))?;
    let meta = state
        .service
        .upload_media(file_name, content_type, bytes, title)
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(meta))))
}

async fn delete_media(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(media_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_media(&media_id).await?;
    Ok(Json(ApiResponse::ok(())))
}

/// `GET /api/video-canvas/media/{media_id}/path` — returns the local filesystem path
/// so the renderer can open the containing folder via Tauri.
async fn get_media_path(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(media_id): Path<String>,
) -> Result<Json<ApiResponse<MediaPathResponse>>, AppError> {
    let path = state.service.media_file_path(&media_id).await?;
    Ok(Json(ApiResponse::ok(MediaPathResponse { path: path.to_string_lossy().into_owned() })))
}

#[derive(serde::Serialize)]
struct MediaPathResponse {
    path: String,
}

#[derive(Deserialize)]
struct ConcatMediaBody {
    media_ids: Vec<String>,
    #[serde(default)]
    title: Option<String>,
}

async fn concat_media(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<ConcatMediaBody>, JsonRejection>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let meta = state
        .service
        .concat_media(body.media_ids, body.title)
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(meta))))
}

async fn serve_media(
    State(state): State<CanvasRouterState>,
    Path(media_id): Path<String>,
) -> Result<Response, AppError> {
    let (mime, bytes, file) = state.service.open_media(&media_id).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.to_string())
        .header(header::CACHE_CONTROL, SERVE_CACHE_CONTROL)
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .map_err(|e| AppError::Internal(format!("build response: {e}")))
}

/// `HEAD /media/{id}` — index-only lookup; never opens or reads the media file.
async fn head_media(
    State(state): State<CanvasRouterState>,
    Path(media_id): Path<String>,
) -> Result<Response, AppError> {
    let head = state.service.head_media(&media_id).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, head.mime)
        .header(header::CONTENT_LENGTH, head.bytes.to_string())
        .header(header::CACHE_CONTROL, SERVE_CACHE_CONTROL)
        .body(Body::empty())
        .map_err(|e| AppError::Internal(format!("build response: {e}")))
}

#[derive(Deserialize)]
struct CreateTaskBody {
    mode: String,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    aspect_ratio: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    duration_secs: Option<u32>,
    #[serde(default)]
    reference_media_ids: Vec<String>,
    #[serde(default)]
    first_frame_media_id: Option<String>,
    #[serde(default)]
    last_frame_media_id: Option<String>,
}

async fn create_task(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<CreateTaskBody>, JsonRejection>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let view = state
        .service
        .create_generation_task(NewGenerationRequest {
            mode: body.mode,
            prompt: body.prompt,
            model: body.model,
            aspect_ratio: body.aspect_ratio,
            resolution: body.resolution,
            duration_secs: body.duration_secs,
            reference_media_ids: body.reference_media_ids,
            first_frame_media_id: body.first_frame_media_id,
            last_frame_media_id: body.last_frame_media_id,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(view))))
}

async fn get_task(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(task_id): Path<String>,
) -> Result<Json<ApiResponse<GenerationTaskView>>, AppError> {
    let view = state.service.get_task(&task_id).await?;
    Ok(Json(ApiResponse::ok(view)))
}

#[derive(serde::Deserialize)]
struct ListTasksQuery {
    #[serde(default = "default_task_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_task_limit() -> usize {
    30
}

#[derive(serde::Serialize)]
struct TaskListResponse {
    tasks: Vec<GenerationTaskView>,
    total: usize,
}

async fn list_tasks(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<ApiResponse<TaskListResponse>>, AppError> {
    // Cap to a sane upper bound so a runaway client can't enumerate everything
    // in one request — the in-memory store is small today, but this keeps the
    // contract honest if the persistence layer changes later.
    let limit = params.limit.clamp(1, 200);
    let offset = params.offset.min(10_000);
    let tasks = state.service.list_tasks(limit, offset).await;
    let total = state.service.task_count().await;
    Ok(Json(ApiResponse::ok(TaskListResponse { tasks, total })))
}

async fn delete_task(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(task_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_task(&task_id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn cancel_task(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(task_id): Path<String>,
) -> Result<Json<ApiResponse<GenerationTaskView>>, AppError> {
    let view = state.service.cancel_task(&task_id).await?;
    Ok(Json(ApiResponse::ok(view)))
}
