use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{get, post};
use serde::Deserialize;

use nomifun_api_types::{
    ApiResponse, EvalCaseTraceView, EvalRunDiffView, EvalRunListItem, EvalRunView,
    EvalSuiteDescriptor, PullEvalDatasetResponse, SessionObservationListDto, StartEvalRunRequest,
};
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;

use crate::agent_eval::EvalQualityCase;
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

#[derive(Debug, Deserialize)]
struct TraceQuery {
    trial: Option<u32>,
}

pub fn eval_routes(state: AgentRouterState) -> Router {
    Router::new()
        .route("/api/debug/agent-evals/suites", get(list_suites))
        .route(
            "/api/debug/agent-evals/datasets/{suite}/pull",
            post(pull_dataset),
        )
        .route("/api/debug/agent-evals/runs", get(latest_run).post(start_run))
        .route("/api/debug/agent-evals/history", get(history))
        .route(
            "/api/debug/agent-evals/runs/{a}/diff/{b}",
            get(diff_runs),
        )
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
        .route("/api/debug/agent-evals/report-case", post(report_case))
        .route("/api/debug/agent-evals/private/sync", post(sync_private))
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

async fn history(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<EvalRunListItem>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.eval_lab.history().await?)))
}

async fn diff_runs(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path((a, b)): Path<(String, String)>,
) -> Result<Json<ApiResponse<EvalRunDiffView>>, AppError> {
    Ok(Json(ApiResponse::ok(state.eval_lab.diff_runs(&a, &b).await?)))
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
    Query(query): Query<TraceQuery>,
) -> Result<Json<ApiResponse<EvalCaseTraceView>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .eval_lab
            .get_case_trace(&run_id, &case_id, query.trial)
            .await?,
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

async fn report_case(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<EvalQualityCase>, JsonRejection>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Json(case) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    state.eval_lab.report_case(case).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn sync_private(
    State(state): State<AgentRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<usize>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.eval_lab.sync_private_corpus().await?,
    )))
}
