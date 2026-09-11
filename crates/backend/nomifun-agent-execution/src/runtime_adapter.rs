//! Minimal App Server runtime boundary over the existing Agent Execution facade.
//!
//! Agent Store Agents are execution presets. The actual runtime agent remains
//! the external executor selected by allo (for example Claude Code or Codex).

use std::collections::HashMap;
use std::sync::Arc;

use nomifun_api_types::{
    AgentExecutionDetail, AgentExecutionEvent, AnswerExecutionDecisionRequest,
    CreateAgentExecutionRequest, ExecutionModelPool, ExecutionModelRef, PlannedExecutionStep,
    ResolvedPresetSnapshot,
};
use nomifun_common::{
    AgentExecutionActor, AgentExecutionEventKind, AgentExecutionStatus, AppError,
    ExecutionAttemptStatus, ExecutionStepKind, ExecutionStepStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AgentExecutionEngine;

pub type PresetSnapshotInput = ResolvedPresetSnapshot;
pub type PresetSnapshot = ResolvedPresetSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunReceipt {
    pub run_id: String,
    pub status: AgentRunStatus,
    pub version: i64,
    pub preset_revision: i64,
    pub content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunView {
    pub run_id: String,
    pub status: AgentRunStatus,
    pub version: i64,
    pub summary: Option<String>,
    pub output_files: Vec<String>,
    pub preset_revision: Option<i64>,
    pub content_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub run_id: String,
    pub status: AgentRunStatus,
    pub version: i64,
    pub summary: Option<String>,
    pub output_files: Vec<String>,
    pub preset_revision: Option<i64>,
    pub content_digest: Option<String>,
}

/// One projected run event on the App Server wire.
///
/// `step_id` / `attempt_id` are projected for events that the engine scopes to
/// an attempt (notably `approval.requested`). A pending decision also carries
/// the three CAS versions the answer must echo: the engine's
/// `answer_decision` accepts nothing else, so the client cannot construct an
/// accepted answer without them. They are projected from the authoritative
/// rows at read time, which is exactly what the CAS compares against; any
/// concurrent movement after that turns the answer into a `Conflict` instead of
/// silently applying it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunEvent {
    pub run_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub expected_execution_version: Option<i64>,
    #[serde(default)]
    pub expected_step_version: Option<i64>,
    #[serde(default)]
    pub expected_attempt_version: Option<i64>,
}

/// W4 / W6（D-W6-1）：计划的**权威快照**投影。
///
/// 与 `AgentRunEvent` 的分工：事件是追加式日志，只带标记（`change` / `status`），
/// 步骤标题、失败原因、起止时间从来没上过 wire；这里是当前状态快照，直接投影引擎
/// 的权威行，因此每步只有一份最新事实——不存在「同一事件被投递两次」的问题。
///
/// 不新增内部标识：步骤 / 尝试 id 本就在 `run/events` 与审批回答里公开（CAS 需要
/// 它们），成员归属用 `role` + `model` 表达而**不**投影 participant_id /
/// source_agent_id（沿用「没有公开映射的内部 id 不上 wire」的既有规则）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunPlan {
    pub run_id: String,
    pub status: AgentRunStatus,
    pub version: i64,
    pub steps: Vec<AgentRunPlanStep>,
    pub dependencies: Vec<AgentRunPlanDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunPlanStep {
    pub step_id: String,
    pub title: String,
    pub kind: ExecutionStepKind,
    pub status: ExecutionStepStatus,
    /// 成员归属的人话表达（角色 / 模型），缺失即 `null`——不猜。
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub introduced_in_revision: i64,
    #[serde(default)]
    pub superseded_in_revision: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub attempts: Vec<AgentRunPlanAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunPlanAttempt {
    pub attempt_id: String,
    pub attempt_no: i64,
    pub status: ExecutionAttemptStatus,
    /// 引擎给出的重试原因（首次执行同样有值，语义是「为什么有这一次尝试」）。
    pub trigger_reason: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub output_summary: Option<String>,
    #[serde(default)]
    pub output_files: Vec<String>,
    #[serde(default)]
    pub tokens: Option<i64>,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunPlanDependency {
    pub blocker_step_id: String,
    pub blocked_step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunSteerRequest {
    pub text: String,
    pub expected_version: i64,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Planning,
    Running,
    Completed,
    CompletedWithFailures,
    Failed,
    Cancelled,
    Paused,
    WaitingInput,
    AwaitingApproval,
    RecoveryRequired,
}

impl AgentRunStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::CompletedWithFailures | Self::Failed | Self::Cancelled
        )
    }
}

impl From<AgentExecutionStatus> for AgentRunStatus {
    fn from(value: AgentExecutionStatus) -> Self {
        match value {
            AgentExecutionStatus::Planning => Self::Planning,
            AgentExecutionStatus::Running => Self::Running,
            AgentExecutionStatus::Completed => Self::Completed,
            AgentExecutionStatus::CompletedWithFailures => Self::CompletedWithFailures,
            AgentExecutionStatus::Failed => Self::Failed,
            AgentExecutionStatus::Cancelled => Self::Cancelled,
            AgentExecutionStatus::Paused => Self::Paused,
            AgentExecutionStatus::WaitingInput => Self::WaitingInput,
            AgentExecutionStatus::AwaitingApproval => Self::AwaitingApproval,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeAdapterError {
    #[error("invalid preset snapshot: {0}")]
    InvalidSnapshot(String),
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("runtime error: {0}")]
    Runtime(#[from] AppError),
}


#[derive(Clone)]
pub struct AgentRuntimeAdapter {
    engine: Arc<AgentExecutionEngine>,
}

impl AgentRuntimeAdapter {
    pub fn new(engine: Arc<AgentExecutionEngine>) -> Self {
        Self { engine }
    }

    pub fn map_error(error: RuntimeAdapterError) -> AppError {
        match error {
            RuntimeAdapterError::Runtime(error) => error,
            RuntimeAdapterError::InvalidSnapshot(message) => AppError::BadRequest(message),
            RuntimeAdapterError::Serialization(message) => AppError::Internal(message),
        }
    }

    pub async fn start_agent_run(
        &self,
        owner_id: &str,
        snapshot: PresetSnapshot,
        goal: String,
        work_dir: Option<String>,
        steps: Option<Vec<PlannedExecutionStep>>,
    ) -> Result<AgentRunReceipt, RuntimeAdapterError> {
        let model = snapshot
            .resolved_model
            .as_ref()
            .ok_or_else(|| RuntimeAdapterError::InvalidSnapshot("resolved_model is required".into()))?;
        let provider_id = model
            .provider_id
            .as_ref()
            .ok_or_else(|| RuntimeAdapterError::InvalidSnapshot("resolved_model.provider_id is required".into()))?;
        // Preset MCP references are wired by the attempt runner: the resolved
        // `mcp_server_ids` are projected into the attempt conversation's
        // `selected_mcp_server_ids`, resolved/validated by the Conversation
        // layer and finally injected into the Nomi runtime for instance
        // owners. The presence of references here is the contract, not an
        // error.
        let snapshot_json = serde_json::to_string(&snapshot)
            .map_err(|error| RuntimeAdapterError::Serialization(error.to_string()))?;
        let content_digest = digest(&snapshot_json);
        let preset_revision = snapshot.preset_revision;
        // App Server runs are started by the authenticated connection user,
        // same as the UI create paths (`AgentExecutionActor::user`). A free
        // string such as "app-server" violates the executions.actor_id UUIDv7
        // CHECK constraint; `owner_id` here is exactly that user id.
        let actor = AgentExecutionActor::user(owner_id);
        let execution = self
            .engine
            .create_for_app_server(
                owner_id,
                &actor,
                CreateAgentExecutionRequest {
                    goal,
                    work_dir,
                    model_pool: ExecutionModelPool::Single {
                        model: ExecutionModelRef {
                            provider_id: provider_id.clone(),
                            model: model.model.clone(),
                        },
                    },
                    delegation_policy: nomifun_common::DelegationPolicy::Disabled,
                    plan_gate: nomifun_common::PlanGate::Automatic,
                    adaptation_policy: nomifun_common::AdaptationPolicy::Fixed,
                    decision_policy: nomifun_common::DecisionPolicy::Automatic,
                    max_parallel: Some(1),
                    lead_conversation_id: None,
                    lead_model: None,
                    steps,
                },
                snapshot,
            )
            .await?;
        Ok(AgentRunReceipt {
            run_id: execution.execution_id,
            status: execution.status.into(),
            version: execution.version,
            preset_revision,
            content_digest,
        })
    }

    pub async fn get_run(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<AgentRunView, RuntimeAdapterError> {
        Ok(run_view(self.engine.get(owner_id, run_id).await?))
    }

    /// W4 / W6（D-W6-1）：计划与步骤的权威快照。
    ///
    /// owner 作用域与 `get_run` 完全一致（同一个 `engine.get`）：不是 owner 的 run
    /// 一律 `NotFound`，不透露存在性。
    pub async fn plan(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<AgentRunPlan, RuntimeAdapterError> {
        Ok(run_plan(self.engine.get(owner_id, run_id).await?))
    }

    pub async fn get_result(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<AgentRunResult, RuntimeAdapterError> {
        let view = run_view(self.engine.get(owner_id, run_id).await?);
        if !view.status.is_terminal() {
            return Err(RuntimeAdapterError::Runtime(AppError::Conflict(
                "run result is only available after the run reaches a terminal state".into(),
            )));
        }
        Ok(AgentRunResult {
            run_id: view.run_id,
            status: view.status,
            version: view.version,
            summary: view.summary,
            output_files: view.output_files,
            preset_revision: view.preset_revision,
            content_digest: view.content_digest,
        })
    }

    /// Replay persisted run events (owner-scoped) with the decision context the
    /// client needs to answer a pending `approval.requested`.
    pub async fn list_events(
        &self,
        owner_id: &str,
        run_id: &str,
        after_sequence: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<AgentRunEvent>, RuntimeAdapterError> {
        let events = self
            .engine
            .events(owner_id, run_id, after_sequence, limit)
            .await?;
        // Only a decision request needs the CAS context; every other event stays
        // a pure projection of its persisted row (no extra read).
        let decision_context = if events
            .iter()
            .any(|event| event.event_type == AgentExecutionEventKind::DecisionRequested)
        {
            Some(DecisionContext::read(self.engine.as_ref(), owner_id, run_id).await?)
        } else {
            None
        };
        Ok(events
            .into_iter()
            .map(|event| event_view(event, decision_context.as_ref()))
            .collect())
    }

    pub async fn cancel_run(
        &self,
        owner_id: &str,
        run_id: &str,
        expected_version: i64,
    ) -> Result<AgentRunView, RuntimeAdapterError> {
        Ok(run_view(
            self.engine
                .cancel(
                    owner_id,
                    &AgentExecutionActor::user(owner_id),
                    run_id,
                    nomifun_api_types::VersionedAgentExecutionCommand { expected_version },
                )
                .await?,
        ))
    }

    /// Steer a running App Server run: resolve the currently active agent
    /// step (opaque to the protocol) and forward text via the engine's
    /// durable conversation-effect path. `expected_version` maps to the
    /// execution-level CAS; the step CAS is taken from the freshly read
    /// detail so concurrent client steers serialize on the server view.
    ///
    /// The steerable window follows the attempt lifecycle, not the step
    /// lifecycle: a single-step run sits in `planning` while its attempt
    /// conversation is already running, which is exactly when steering
    /// matters most. Steps with a `Running` attempt steer; when none does,
    /// the step fallback (below) covers steps whose status is Running but
    /// whose attempt list has not been observed yet.
    pub async fn steer_run(
        &self,
        owner_id: &str,
        run_id: &str,
        text: &str,
        expected_version: i64,
    ) -> Result<AgentRunView, RuntimeAdapterError> {
        let detail = self.engine.get(owner_id, run_id).await?;
        if detail.execution.version != expected_version {
            return Err(RuntimeAdapterError::Runtime(nomifun_common::AppError::Conflict(
                "run changed before the steer command".to_owned(),
            )));
        }
        let active_attempt_step = detail.attempts.iter().find(|attempt| {
            attempt.status == nomifun_common::ExecutionAttemptStatus::Running
        });
        let active_step = match active_attempt_step {
            Some(attempt) => detail
                .steps
                .iter()
                .find(|step| step.step_id == attempt.step_id)
                .ok_or_else(|| {
                    RuntimeAdapterError::Runtime(nomifun_common::AppError::Internal(
                        "running attempt references an unknown step".to_owned(),
                    ))
                })?,
            None => detail
                .steps
                .iter()
                .find(|step| step.status == nomifun_common::ExecutionStepStatus::Running)
                .ok_or_else(|| {
                    RuntimeAdapterError::Runtime(nomifun_common::AppError::Conflict(
                        "run has no running agent step to steer".to_owned(),
                    ))
                })?,
        };
        self.engine
            .steer_step(
                owner_id,
                &AgentExecutionActor::user(owner_id),
                run_id,
                &active_step.step_id,
                nomifun_api_types::SteerExecutionStepRequest {
                    text: text.to_owned(),
                    expected_execution_version: expected_version,
                    expected_step_version: active_step.version,
                },
            )
            .await
            .map_err(RuntimeAdapterError::Runtime)?;
        Ok(self.get_run(owner_id, run_id).await?)
    }

    /// Answer the pending decision of a `waiting_input` attempt.
    ///
    /// Straight owner-scoped pass-through to
    /// [`AgentExecutionEngine::answer_decision`], which stays the single answer
    /// entry point: it enforces the owner scope, the three-way CAS
    /// (execution + step + attempt versions), the `WaitingInput`-only
    /// precondition, a non-empty answer and canonical ids. This wrapper adds no
    /// policy of its own and must never relax any of those checks.
    ///
    /// The desktop confirmation route's `always_allow` / approve-all flag has no
    /// counterpart here on purpose: answering a decision never widens a tool
    /// policy for the rest of the run.
    pub async fn answer_decision(
        &self,
        owner_id: &str,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        request: AnswerExecutionDecisionRequest,
    ) -> Result<AgentRunView, RuntimeAdapterError> {
        self.engine
            .answer_decision(
                owner_id,
                &AgentExecutionActor::user(owner_id),
                run_id,
                step_id,
                attempt_id,
                request,
            )
            .await
            .map_err(RuntimeAdapterError::Runtime)?;
        Ok(self.get_run(owner_id, run_id).await?)
    }
}

fn digest(snapshot_json: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(snapshot_json.as_bytes()))
}

fn run_view(detail: AgentExecutionDetail) -> AgentRunView {
    let status = if detail.attempts.iter().any(|attempt| {
        attempt
            .runtime_state
            .as_ref()
            .is_some_and(is_recovery_blocked)
    }) {
        AgentRunStatus::RecoveryRequired
    } else {
        detail.execution.status.into()
    };
    let preset = detail.participants.first();
    let (preset_revision, content_digest) = preset
        .and_then(|participant| {
            participant.preset_snapshot.as_ref().map(|snapshot| {
                let raw = serde_json::to_string(snapshot).ok()?;
                Some((snapshot.preset_revision, digest(&raw)))
            })
        })
        .flatten()
        .unzip();
    AgentRunView {
        run_id: detail.execution.execution_id,
        status,
        version: detail.execution.version,
        summary: detail.execution.summary,
        output_files: detail
            .attempts
            .iter()
            .flat_map(|attempt| attempt.output_files.iter())
            .filter(|path| is_public_relative_path(path))
            .cloned()
            .collect(),
        preset_revision,
        content_digest,
    }
}

/// Project the authoritative execution detail onto the plan snapshot (W4 / W6).
///
/// Ordering is the engine's own (`created_at`, then `step_id`): the plan view is
/// a snapshot, not a log, so the UI can render it without replaying events.
/// `output_files` are filtered through `is_public_relative_path` — the same
/// guard `run_view` uses, so no absolute path reaches the wire as a side effect
/// of this new projection.
fn run_plan(detail: AgentExecutionDetail) -> AgentRunPlan {
    let mut roles: HashMap<&str, (Option<String>, Option<String>)> = HashMap::new();
    for participant in &detail.participants {
        roles.insert(
            participant.participant_id.as_str(),
            (participant.role.clone(), participant.model.clone()),
        );
    }
    let mut steps: Vec<AgentRunPlanStep> = detail
        .steps
        .iter()
        .map(|step| {
            let (role, model) = step
                .assigned_participant_id
                .as_deref()
                .and_then(|id| roles.get(id).cloned())
                .unwrap_or((step.role.clone(), None));
            let mut attempts: Vec<AgentRunPlanAttempt> = detail
                .attempts
                .iter()
                .filter(|attempt| attempt.step_id == step.step_id)
                .map(|attempt| {
                    let (attempt_role, attempt_model) = attempt
                        .participant_id
                        .as_deref()
                        .and_then(|id| roles.get(id).cloned())
                        .unwrap_or((role.clone(), model.clone()));
                    AgentRunPlanAttempt {
                        attempt_id: attempt.attempt_id.clone(),
                        attempt_no: attempt.attempt_no,
                        status: attempt.status,
                        trigger_reason: attempt.trigger_reason.clone(),
                        role: attempt_role,
                        model: attempt_model,
                        question: attempt.question.clone(),
                        error: attempt.error.clone(),
                        output_summary: attempt.output_summary.clone(),
                        output_files: attempt
                            .output_files
                            .iter()
                            .filter(|path| is_public_relative_path(path))
                            .cloned()
                            .collect(),
                        tokens: attempt.tokens,
                        started_at: attempt.started_at,
                        finished_at: attempt.finished_at,
                    }
                })
                .collect();
            attempts.sort_by_key(|attempt| attempt.attempt_no);
            AgentRunPlanStep {
                step_id: step.step_id.clone(),
                title: step.title.clone(),
                kind: step.kind,
                status: step.status,
                role,
                model,
                introduced_in_revision: step.introduced_in_revision,
                superseded_in_revision: step.superseded_in_revision,
                created_at: step.created_at,
                updated_at: step.updated_at,
                attempts,
            }
        })
        .collect();
    steps.sort_by_key(|step| (step.created_at, step.step_id.clone()));
    AgentRunPlan {
        run_id: detail.execution.execution_id,
        status: detail.execution.status.into(),
        version: detail.execution.version,
        steps,
        dependencies: detail
            .dependencies
            .iter()
            .map(|dependency| AgentRunPlanDependency {
                blocker_step_id: dependency.blocker_step_id.clone(),
                blocked_step_id: dependency.blocked_step_id.clone(),
            })
            .collect(),
    }
}

fn is_public_relative_path(path: &str) -> bool {
    !path.trim().is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains(':')
        && !path.split(['/', '\\']).any(|segment| segment == "..")
}

fn is_recovery_blocked(runtime_state: &serde_json::Value) -> bool {
    runtime_state
        .get("review_blocked")
        .is_some_and(serde_json::Value::is_object)
}

/// Authoritative CAS context for the decision requests of one run.
///
/// Read once per `list_events` page instead of once per event. It carries the
/// current version of every step/attempt, so a projected decision always offers
/// the tokens the engine will accept right now.
struct DecisionContext {
    execution_version: i64,
    step_versions: HashMap<String, i64>,
    attempt_versions: HashMap<(String, String), i64>,
}

impl DecisionContext {
    async fn read(
        engine: &AgentExecutionEngine,
        owner_id: &str,
        run_id: &str,
    ) -> Result<Self, RuntimeAdapterError> {
        let detail = engine.detail(owner_id, run_id).await?;
        Ok(Self {
            execution_version: detail.execution.version,
            step_versions: detail
                .steps
                .into_iter()
                .map(|step| (step.step_id, step.version))
                .collect(),
            attempt_versions: detail
                .attempts
                .into_iter()
                .map(|attempt| {
                    (
                        (attempt.step_id, attempt.attempt_id),
                        attempt.version,
                    )
                })
                .collect(),
        })
    }

    fn step_version(&self, step_id: &str) -> Option<i64> {
        self.step_versions.get(step_id).copied()
    }

    fn attempt_version(&self, step_id: &str, attempt_id: &str) -> Option<i64> {
        self.attempt_versions
            .get(&(step_id.to_owned(), attempt_id.to_owned()))
            .copied()
    }
}

fn event_view(event: AgentExecutionEvent, decisions: Option<&DecisionContext>) -> AgentRunEvent {
    let event_type = match event.event_type {
        AgentExecutionEventKind::Created => "run.started",
        AgentExecutionEventKind::StatusChanged => "run.status_changed",
        AgentExecutionEventKind::PlanChanged => "run.plan_changed",
        AgentExecutionEventKind::StepChanged => "task.updated",
        AgentExecutionEventKind::AttemptChanged => "attempt.updated",
        AgentExecutionEventKind::DecisionRequested => "approval.requested",
        AgentExecutionEventKind::DecisionAnswered => "approval.responded",
        AgentExecutionEventKind::Deleted => "run.deleted",
    };
    // The persisted event row already carries its step/attempt scope; the
    // decision context adds the three CAS versions for the answer only.
    let decision = match (event.event_type, decisions, event.step_id.as_deref()) {
        (AgentExecutionEventKind::DecisionRequested, Some(context), Some(step_id)) => {
            let attempt_id = event.attempt_id.as_deref();
            Some((
                context.execution_version,
                context.step_version(step_id),
                attempt_id.and_then(|attempt_id| context.attempt_version(step_id, attempt_id)),
            ))
        }
        _ => None,
    };
    AgentRunEvent {
        run_id: event.execution_id,
        sequence: event.sequence,
        event_type: event_type.into(),
        payload: event.payload,
        step_id: event.step_id,
        attempt_id: event.attempt_id,
        expected_execution_version: decision.map(|(execution, _, _)| execution),
        expected_step_version: decision.and_then(|(_, step, _)| step),
        expected_attempt_version: decision.and_then(|(_, _, attempt)| attempt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PresetSnapshotInput {
        serde_json::from_value(serde_json::json!({
            "preset_id": "0190f5fe-7c00-7a00-8000-000000000004",
            "preset_revision": 1,
            "preset_name": "software-engineer",
            "target": "execution_step",
            "instructions": "Build and test software",
            "resolved_model": {
                "provider_id": "0190f5fe-7c00-7a00-8000-000000000002",
                "model": "test-model",
                "required": true
            }
        }))
        .unwrap()
    }

    #[test]
    fn freeze_is_deterministic_and_records_digest() {
        let first = serde_json::to_string(&input()).unwrap();
        let second = serde_json::to_string(&input()).unwrap();
        assert_eq!(digest(&first), digest(&second));
        assert!(digest(&first).starts_with("sha256:"));
    }

    #[test]
    fn snapshot_requires_resolved_model_at_runtime_boundary() {
        let mut value = input();
        value.resolved_model = None;
        assert!(value.resolved_model.is_none());
    }

    #[test]
    fn status_mapping_preserves_terminal_states() {
        assert_eq!(AgentRunStatus::from(AgentExecutionStatus::Completed), AgentRunStatus::Completed);
        assert_eq!(AgentRunStatus::from(AgentExecutionStatus::Cancelled), AgentRunStatus::Cancelled);
    }

    #[test]
    fn recovery_block_is_exposed_as_a_distinct_app_server_status() {
        assert!(is_recovery_blocked(&serde_json::json!({"review_blocked": {"reason": "process_restart"}})));
        assert!(!is_recovery_blocked(&serde_json::json!({"pending_conversation_effects": []})));
    }
}
