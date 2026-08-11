//! `/api/video-generation/tv-show/*` — shared browse + publish endpoints.

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, State};
use axum::routing::{get, post};
use serde::Deserialize;

use nomi_montage::MontageService;
use nomifun_api_types::TvShowPublishSessionRequest;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::service::PublishPackageRequest;
use crate::state::TvShowRouterState;

pub fn tv_show_routes(state: TvShowRouterState) -> Router {
    Router::new()
        .route(
            "/api/video-generation/tv-show/list",
            get(tv_show_list),
        )
        .route(
            "/api/video-generation/tv-show/mine",
            get(tv_show_mine),
        )
        .route(
            "/api/video-generation/tv-show/publish",
            post(publish_package),
        )
        .route(
            "/api/video-generation/tv-show/publish-from-montage/{id}",
            post(publish_from_montage),
        )
        .route(
            "/api/video-generation/tv-show/{id}/import",
            post(import_to_montage),
        )
        .route(
            "/api/video-generation/tv-show/{id}",
            get(tv_show_detail).delete(tv_show_delete),
        )
        .route(
            "/api/video-generation/tv-show/{id}/like",
            post(tv_show_like).delete(tv_show_unlike),
        )
        .with_state(state)
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
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    axum::extract::Query(query): axum::extract::Query<TvShowListQuery>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowListResponse>>, AppError>
{
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state
            .service
            .list(
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
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    axum::extract::Query(query): axum::extract::Query<TvShowListQuery>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowListResponse>>, AppError>
{
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state
            .service
            .mine(query.page, query.page_size, query.status)
            .await?,
    )))
}

async fn tv_show_detail(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowVideo>>, AppError> {
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state.service.detail(id).await?,
    )))
}

async fn tv_show_like(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowLikeResponse>>, AppError>
{
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state.service.like(id).await?,
    )))
}

async fn tv_show_unlike(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowLikeResponse>>, AppError>
{
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state.service.unlike(id).await?,
    )))
}

async fn tv_show_delete(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<nomifun_api_types::ApiResponse<()>>, AppError> {
    state.service.delete(id).await?;
    Ok(Json(nomifun_api_types::ApiResponse::ok(())))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishPackageBody {
    package_path: String,
    cover_path: String,
    #[serde(default)]
    client_session_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    target_duration_secs: Option<i32>,
    #[serde(default)]
    package_name: Option<String>,
}

async fn publish_package(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<PublishPackageBody>, JsonRejection>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowPublishResponse>>, AppError>
{
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    if body.package_path.trim().is_empty() {
        return Err(AppError::BadRequest("package_path is required".into()));
    }
    if body.cover_path.trim().is_empty() {
        return Err(AppError::BadRequest("cover_path is required".into()));
    }
    let client_session_id = body
        .client_session_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let workflow = body
        .workflow
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "montage".into());
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state
            .service
            .publish_package(PublishPackageRequest {
                package_path: body.package_path,
                cover_path: body.cover_path,
                client_session_id,
                title: body.title,
                description: body.description,
                workflow,
                style: body.style,
                target_duration_secs: body.target_duration_secs,
                package_name: body.package_name,
            })
            .await?,
    )))
}

async fn publish_from_montage(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Option<Json<TvShowPublishSessionRequest>>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomifun_api_types::TvShowPublishResponse>>, AppError>
{
    let montage: &std::sync::Arc<MontageService> = state.montage.as_ref().ok_or_else(|| {
        AppError::BadRequest("montage runtime unavailable for TV Show publish".into())
    })?;
    let req = body.map(|Json(b)| b).unwrap_or_default();
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state
            .service
            .publish_from_montage(montage, &id, req)
            .await?,
    )))
}

async fn import_to_montage(
    State(state): State<TvShowRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Json<nomifun_api_types::ApiResponse<nomi_montage::project::ProjectRecord>>, AppError>
{
    let montage: &std::sync::Arc<MontageService> = state.montage.as_ref().ok_or_else(|| {
        AppError::BadRequest("montage runtime unavailable for TV Show import".into())
    })?;
    Ok(Json(nomifun_api_types::ApiResponse::ok(
        state.service.import_to_montage(montage, id).await?,
    )))
}
