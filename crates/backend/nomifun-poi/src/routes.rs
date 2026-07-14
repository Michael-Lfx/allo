//! `/api/poi/*` route handlers.

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{delete, get, post, put};

use nomifun_api_types::{
    ApiResponse, PoiPinRequest, PoiSettingsResponse, PoiStarterListQuery, PoiStarterListResponse,
    PoiStatusResponse, PoiTopicListResponse, PoiTopicResponse, PoiTopicStatusRequest,
    UpdatePoiSettingsRequest,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::state::PoiRouterState;

pub fn poi_routes(state: PoiRouterState) -> Router {
    Router::new()
        .route("/api/poi/topics", get(list_topics).delete(clear_topics))
        .route("/api/poi/topics/{id}", delete(delete_topic))
        .route("/api/poi/starters", get(list_starters))
        .route("/api/poi/status", get(status))
        .route("/api/poi/settings", get(get_settings).patch(patch_settings))
        .route("/api/poi/topics/{id}/pin", post(pin_topic))
        .route("/api/poi/topics/{id}/status", put(set_topic_status))
        .with_state(state)
}

async fn list_topics(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<PoiTopicListResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.list_topics()?)))
}

async fn list_starters(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<PoiStarterListQuery>,
) -> Result<Json<ApiResponse<PoiStarterListResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.list_starters(query).await?,
    )))
}

async fn status(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<PoiStatusResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.status()?)))
}

async fn get_settings(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<PoiSettingsResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get_settings()?)))
}

async fn patch_settings(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<UpdatePoiSettingsRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<PoiSettingsResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(ApiResponse::ok(state.service.update_settings(req)?)))
}

async fn pin_topic(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<PoiPinRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<PoiTopicResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let ok = state.service.pin_topic(&id, req.pinned)?;
    if !ok {
        return Err(AppError::NotFound(format!("POI topic not found: {id}")));
    }
    let topics = state.service.list_topics()?;
    let topic = topics
        .topics
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::NotFound(format!("POI topic not found: {id}")))?;
    Ok(Json(ApiResponse::ok(topic)))
}

async fn set_topic_status(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<PoiTopicStatusRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<PoiTopicResponse>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let ok = state.service.set_topic_status(&id, &req.status)?;
    if !ok {
        return Err(AppError::NotFound(format!("POI topic not found: {id}")));
    }
    let topics = state.service.list_topics()?;
    let topic = topics
        .topics
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::NotFound(format!("POI topic not found: {id}")))?;
    Ok(Json(ApiResponse::ok(topic)))
}

async fn delete_topic(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.delete_topic(&id)?;
    Ok(Json(ApiResponse::ok(())))
}

async fn clear_topics(
    State(state): State<PoiRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.service.clear_topics()?;
    Ok(Json(ApiResponse::ok(())))
}
