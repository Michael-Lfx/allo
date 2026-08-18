//! Authenticated developer-mode session observation read APIs.

use axum::Extension;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use serde::Deserialize;

use nomifun_api_types::ApiResponse;
use nomifun_ai_agent::{
    ProjectedTurn, DEFAULT_SESSION_OBSERVATION_LIST_LIMIT, MAX_SESSION_OBSERVATION_LIST_LIMIT,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::state::ConversationRouterState;

#[derive(Debug, Deserialize)]
struct SessionObservationQuery {
    /// Prefer snake_case; accept camelCase for browser clients.
    #[serde(alias = "conversationId")]
    conversation_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

fn require_hub(
    state: &ConversationRouterState,
) -> Result<&std::sync::Arc<nomifun_ai_agent::AgentTraceHub>, AppError> {
    state.agent_trace_hub.as_ref().ok_or_else(|| {
        AppError::Internal("session observation hub is not configured on this host".into())
    })
}

fn clamp_list_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_SESSION_OBSERVATION_LIST_LIMIT)
        .clamp(1, MAX_SESSION_OBSERVATION_LIST_LIMIT)
}

/// Debug routes for inspecting persisted session observations.
///
/// All handlers require authentication (applied by the caller), conversation
/// ownership, and developer mode
/// (enforced by [`nomifun_ai_agent::AgentTraceHub::require_developer_mode`]).
pub fn conversation_trace_routes() -> Router<ConversationRouterState> {
    Router::new()
        .route(
            "/api/debug/session-observations",
            get(list_session_observations),
        )
        .route(
            "/api/debug/session-observations/turns/{root_turn_id}",
            get(get_session_observation_turn),
        )
}

async fn list_session_observations(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<SessionObservationQuery>,
) -> Result<axum::Json<ApiResponse<Vec<ProjectedTurn>>>, AppError> {
    let hub = require_hub(&state)?;
    let conversation_id = query.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(AppError::BadRequest(
            "conversation_id query parameter is required".into(),
        ));
    }
    state.service.get(&user.id, conversation_id).await?;
    let turns = hub
        .list_session_observations(conversation_id, clamp_list_limit(query.limit))
        .await
        .map_err(|error| error.into_app_error())?;
    Ok(axum::Json(ApiResponse::ok(turns)))
}

async fn get_session_observation_turn(
    State(state): State<ConversationRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(root_turn_id): Path<String>,
    Query(query): Query<SessionObservationQuery>,
) -> Result<axum::Json<ApiResponse<ProjectedTurn>>, AppError> {
    let hub = require_hub(&state)?;
    let conversation_id = query.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(AppError::BadRequest(
            "conversation_id query parameter is required".into(),
        ));
    }
    state.service.get(&user.id, conversation_id).await?;
    let turn = hub
        .get_session_observation_turn(conversation_id, &root_turn_id)
        .await
        .map_err(|error| error.into_app_error())?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "session observation turn '{root_turn_id}' not found"
            ))
        })?;
    Ok(axum::Json(ApiResponse::ok(turn)))
}
