use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{get, post};
use serde::Deserialize;

use nomifun_api_types::{
    ApiResponse, EvalCaseTraceView, EvalRunView, EvalSuiteDescriptor, PullEvalDatasetResponse,
    SessionObservationListDto, StartEvalRunRequest,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::session_observation_list_dto;
use crate::routes::state::AgentRouterState;

#[derive(Debug, Deserialize)]
struct PullEvalQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ObservationQuery {
    limit: Option<usize>,
}

pub fn eval_routes(state: AgentRouterState) -> Router {
    Router::new()
        .route("/api/debug/agent-evals/suites", get(list_suites))
        .route(
            "/api/debug/agent-evals/datasets/{suite}/pull",
            post(pull_dataset),
        )
        .route("/api/debug/agent-evals/runs", get(latest_run).post(start_run))
        .route("/api/debug/agent-evals/runs/{run_id}", get(get_run))
        .route(
            "/api/debug/agent-evals/runs/{run_id}/cancel",
            post(cancel_run),
        )
        .route(
            "/api/debug/agent-evals/runs/{run_id}/cases/{case_id}/trace",
            get(get_case_trace),
        )
        .route(
            "/api/debug/agent-evals/runs/{run_id}/cases/{case_id}/observation",
            get(get_case_observation),
        )
        .with_state(state)
}

async fn list_suites(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<EvalSuiteDescriptor>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.eval_lab.list_suites().await?)))
}

async fn pull_dataset(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(suite): Path<String>,
    Query(query): Query<PullEvalQuery>,
) -> Result<Json<ApiResponse<PullEvalDatasetResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.eval_lab.pull_dataset(&suite, query.limit).await?,
    )))
}

async fn start_run(
    State(state): State<AgentRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<StartEvalRunRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<EvalRunView>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(ApiResponse::ok(
        state
            .eval_lab
            .start_run(req, Some(user.id.to_string()))
            .await?,
    )))
}

async fn latest_run(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Option<EvalRunView>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.eval_lab.latest().await?)))
}

async fn get_run(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(run_id): Path<String>,
) -> Result<Json<ApiResponse<EvalRunView>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.eval_lab.current_or_get(&run_id).await?,
    )))
}

async fn cancel_run(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(run_id): Path<String>,
) -> Result<Json<ApiResponse<EvalRunView>>, AppError> {
    Ok(Json(ApiResponse::ok(state.eval_lab.cancel(&run_id).await?)))
}

async fn get_case_trace(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((run_id, case_id)): Path<(String, String)>,
) -> Result<Json<ApiResponse<EvalCaseTraceView>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.eval_lab.get_case_trace(&run_id, &case_id).await?,
    )))
}

async fn get_case_observation(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((run_id, case_id)): Path<(String, String)>,
    Query(query): Query<ObservationQuery>,
) -> Result<Json<ApiResponse<SessionObservationListDto>>, AppError> {
    let observation = state
        .eval_lab
        .get_case_observation(&run_id, &case_id, query.limit)
        .await?;
    let observation = session_observation_list_dto(observation);
    Ok(Json(ApiResponse::ok(observation)))
}
