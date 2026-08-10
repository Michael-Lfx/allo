//! Authenticated developer-mode agent-trace read APIs.

use axum::Extension;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use serde::Deserialize;

use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use nomifun_ai_agent::{TraceArtifactIndexEntry, TraceIndexEntry, TurnTrace};

use crate::state::ConversationRouterState;

const DEFAULT_TRACE_LIMIT: usize = 50;
const MAX_TRACE_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
struct ListConversationTracesQuery {
    /// Prefer snake_case; accept camelCase for browser clients.
    #[serde(alias = "conversationId")]
    conversation_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ListRecentTracesQuery {
    #[serde(default)]
    limit: Option<usize>,
}

fn clamp_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_TRACE_LIMIT).clamp(1, MAX_TRACE_LIMIT)
}

fn require_hub(
    state: &ConversationRouterState,
) -> Result<&std::sync::Arc<nomifun_ai_agent::AgentTraceHub>, AppError> {
    state.agent_trace_hub.as_ref().ok_or_else(|| {
        AppError::Internal("agent trace hub is not configured on this host".into())
    })
}

/// Debug routes for inspecting persisted agent turn traces.
///
/// All handlers require authentication (applied by the caller) and developer mode
/// (enforced by [`nomifun_ai_agent::AgentTraceHub::require_developer_mode`]).
pub fn conversation_trace_routes() -> Router<ConversationRouterState> {
    Router::new()
        .route("/api/debug/agent-traces", get(list_for_conversation))
        .route("/api/debug/agent-traces/recent", get(list_recent))
        // Static segment must be registered before `{trace_id}`.
        .route("/api/debug/agent-traces/artifacts", get(list_artifacts))
        .route("/api/debug/agent-traces/{trace_id}", get(get_trace))
}

async fn list_for_conversation(
    State(state): State<ConversationRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ListConversationTracesQuery>,
) -> Result<axum::Json<ApiResponse<Vec<TraceIndexEntry>>>, AppError> {
    let hub = require_hub(&state)?;
    let conversation_id = query.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(AppError::BadRequest(
            "conversation_id query parameter is required".into(),
        ));
    }
    let entries = hub
        .list_for_conversation(conversation_id, clamp_limit(query.limit))
        .await
        .map_err(|error| error.into_app_error())?;
    Ok(axum::Json(ApiResponse::ok(entries)))
}

async fn list_recent(
    State(state): State<ConversationRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ListRecentTracesQuery>,
) -> Result<axum::Json<ApiResponse<Vec<TraceIndexEntry>>>, AppError> {
    let hub = require_hub(&state)?;
    let entries = hub
        .list_recent(clamp_limit(query.limit))
        .await
        .map_err(|error| error.into_app_error())?;
    Ok(axum::Json(ApiResponse::ok(entries)))
}

async fn list_artifacts(
    State(state): State<ConversationRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ListConversationTracesQuery>,
) -> Result<axum::Json<ApiResponse<Vec<TraceArtifactIndexEntry>>>, AppError> {
    let hub = require_hub(&state)?;
    let conversation_id = query.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(AppError::BadRequest(
            "conversation_id query parameter is required".into(),
        ));
    }
    let entries = hub
        .list_artifacts_for_conversation(conversation_id, clamp_limit(query.limit))
        .await
        .map_err(|error| error.into_app_error())?;
    Ok(axum::Json(ApiResponse::ok(entries)))
}

async fn get_trace(
    State(state): State<ConversationRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(trace_id): Path<String>,
) -> Result<axum::Json<ApiResponse<TurnTrace>>, AppError> {
    let hub = require_hub(&state)?;
    let trace = hub
        .get_trace(&trace_id)
        .await
        .map_err(|error| error.into_app_error())?
        .ok_or_else(|| AppError::NotFound(format!("agent trace '{trace_id}' not found")))?;
    Ok(axum::Json(ApiResponse::ok(trace)))
}
