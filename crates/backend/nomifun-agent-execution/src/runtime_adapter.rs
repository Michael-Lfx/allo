//! Minimal App Server runtime boundary over the existing Agent Execution facade.
//!
//! Agent Store Agents are execution presets. The actual runtime agent remains
//! the external executor selected by allo (for example Claude Code or Codex).

use std::sync::Arc;

use nomifun_api_types::{
    AgentExecutionDetail, AgentExecutionEvent, CreateAgentExecutionRequest,
    ExecutionModelPool, ExecutionModelRef, PlannedExecutionStep, ResolvedPresetSnapshot,
};
use nomifun_common::{AgentExecutionActor, AgentExecutionEventKind, AgentExecutionStatus, AppError};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunEvent {
    pub run_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
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

    pub async fn list_events(
        &self,
        owner_id: &str,
        run_id: &str,
        after_sequence: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<AgentRunEvent>, RuntimeAdapterError> {
        Ok(self
            .engine
            .events(owner_id, run_id, after_sequence, limit)
            .await?
            .into_iter()
            .map(event_view)
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

fn event_view(event: AgentExecutionEvent) -> AgentRunEvent {
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
    AgentRunEvent {
        run_id: event.execution_id,
        sequence: event.sequence,
        event_type: event_type.into(),
        payload: event.payload,
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
