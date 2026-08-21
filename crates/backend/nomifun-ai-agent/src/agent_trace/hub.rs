//! Process-wide session observation hub: recorder + developer-mode read gate.

use std::path::Path;
use std::sync::Arc;

use nomifun_api_types::{
    ObservationSummaryDto, RecorderHealthDto, SessionObservationCallDto,
    SessionObservationGapDto, SessionObservationListDto, SessionObservationRequestSummaryDto,
    SessionObservationResponseSummaryDto, SessionObservationTokenUsageDto,
    SessionObservationToolDto, SessionObservationTurnDto,
};
use nomifun_db::IClientPreferenceRepository;
use nomi_agent_trace::{
    project_call_detail, project_turn_by_id, project_turns, strip_projected_turn_payloads,
    ExecutionStatus, Integrity, ObservationRecorder, ObservationScope, ObservationSummary,
    ProjectedGap, ProjectedModelCall, ProjectedRequestSummary, ProjectedResponseSummary,
    ProjectedTokenUsage, ProjectedToolExecution, ProjectedTurn, RecorderError, RecorderHealth,
    RecorderHealthStatus, ToolExecutionStatus,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use super::prefs::developer_mode_enabled;

pub const DEFAULT_SESSION_OBSERVATION_LIST_LIMIT: usize = 50;
pub const MAX_SESSION_OBSERVATION_LIST_LIMIT: usize = 200;
const OBSERVATION_READ_PERMITS: usize = 4;

fn execution_status_string(value: ExecutionStatus) -> String {
    match value {
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::Interrupted => "interrupted",
        ExecutionStatus::Truncated => "truncated",
        ExecutionStatus::Unknown => "unknown",
    }
    .to_owned()
}

fn integrity_string(value: Integrity) -> String {
    match value {
        Integrity::Complete => "complete",
        Integrity::Degraded => "degraded",
    }
    .to_owned()
}

fn observation_scope_string(value: ObservationScope) -> String {
    match value {
        ObservationScope::SessionWorkflow => "session_workflow",
        ObservationScope::SessionAuxiliary => "session_auxiliary",
        ObservationScope::ProcessDiagnostic => "process_diagnostic",
    }
    .to_owned()
}

fn tool_status_string(value: ToolExecutionStatus) -> String {
    match value {
        ToolExecutionStatus::Started => "started",
        ToolExecutionStatus::Completed => "completed",
        ToolExecutionStatus::Failed => "failed",
        ToolExecutionStatus::Cancelled => "cancelled",
    }
    .to_owned()
}

fn recorder_health_status_string(value: RecorderHealthStatus) -> String {
    match value {
        RecorderHealthStatus::Healthy => "healthy",
        RecorderHealthStatus::QueueDropped => "queue_dropped",
        RecorderHealthStatus::StorageError => "storage_error",
        RecorderHealthStatus::WriterDisconnected => "writer_disconnected",
    }
    .to_owned()
}

fn recorder_health_dto(value: RecorderHealth) -> RecorderHealthDto {
    RecorderHealthDto {
        status: recorder_health_status_string(value.status),
        last_error: value.last_error,
    }
}

fn observation_summary_dto(value: ObservationSummary) -> ObservationSummaryDto {
    ObservationSummaryDto {
        turn_count: value.turn_count,
        model_call_count: value.model_call_count,
        tool_count: value.tool_count,
        active_duration_ms: value.active_duration_ms,
        wall_span_ms: value.wall_span_ms,
        integrity: integrity_string(value.integrity),
        coverage: value.coverage,
        max_event_seq: value.max_event_seq,
    }
}

fn token_usage_dto(value: ProjectedTokenUsage) -> SessionObservationTokenUsageDto {
    SessionObservationTokenUsageDto {
        input_tokens: value.input_tokens,
        output_tokens: value.output_tokens,
        cache_read_tokens: value.cache_read_tokens,
        cache_creation_tokens: value.cache_creation_tokens,
    }
}

fn request_summary_dto(value: ProjectedRequestSummary) -> SessionObservationRequestSummaryDto {
    SessionObservationRequestSummaryDto {
        model: value.model,
        has_system: value.has_system,
        message_count: value.message_count,
        tool_definition_count: value.tool_definition_count,
        system_omitted: value.system_omitted,
        messages_omitted: value.messages_omitted,
        tools_omitted: value.tools_omitted,
    }
}

fn response_summary_dto(value: ProjectedResponseSummary) -> SessionObservationResponseSummaryDto {
    SessionObservationResponseSummaryDto {
        has_text: value.has_text,
        has_thinking: value.has_thinking,
        text_omitted: value.text_omitted,
        thinking_omitted: value.thinking_omitted,
        tool_use_count: value.tool_use_count,
        elapsed_ms: value.elapsed_ms,
        ttft_ms: value.ttft_ms,
        stop_reason: value.stop_reason,
        text_preview: value.text_preview,
    }
}

fn tool_dto(value: ProjectedToolExecution) -> SessionObservationToolDto {
    SessionObservationToolDto {
        tool_call_id: value.tool_call_id,
        name: value.name,
        started_at_ms: value.started_at_ms,
        ended_at_ms: value.ended_at_ms,
        status: tool_status_string(value.status),
        argument_preview: value.argument_preview,
        started: value.started,
        completed: value.completed,
        failed: value.failed,
        cancelled: value.cancelled,
    }
}

fn gap_dto(value: ProjectedGap) -> SessionObservationGapDto {
    SessionObservationGapDto {
        event_seq: value.event_seq,
        reason: value.reason,
        from_seq: value.from_seq,
        to_seq: value.to_seq,
    }
}

fn call_dto(value: ProjectedModelCall) -> SessionObservationCallDto {
    SessionObservationCallDto {
        model_call_id: value.model_call_id,
        call_kind: value.call_kind,
        observation_scope: value.observation_scope.map(observation_scope_string),
        status: execution_status_string(value.status),
        integrity: integrity_string(value.integrity),
        interrupted: value.interrupted,
        started_at_ms: value.started_at_ms,
        ended_at_ms: value.ended_at_ms,
        usage: value.usage.map(token_usage_dto),
        request: value.request,
        response: value.response,
        request_summary: value.request_summary.map(request_summary_dto),
        response_summary: value.response_summary.map(response_summary_dto),
        tools: value.tools.into_iter().map(tool_dto).collect(),
    }
}

fn turn_dto(value: ProjectedTurn) -> SessionObservationTurnDto {
    SessionObservationTurnDto {
        root_turn_id: value.root_turn_id,
        conversation_id: value.conversation_id,
        msg_id: value.msg_id,
        session_kind: value.session_kind,
        execution_id: value.execution_id,
        step_id: value.step_id,
        execution_attempt_id: value.execution_attempt_id,
        status: execution_status_string(value.status),
        integrity: integrity_string(value.integrity),
        interrupted: value.interrupted,
        started_at_ms: value.started_at_ms,
        ended_at_ms: value.ended_at_ms,
        elapsed_ms: value.elapsed_ms,
        prompt_preview: value.prompt_preview,
        prompt_preview_context_only: value.prompt_preview_context_only,
        max_event_seq: value.max_event_seq,
        has_turn_start: value.has_turn_start,
        has_turn_end: value.has_turn_end,
        gap_count: value.gap_count,
        model_calls: value.model_calls.into_iter().map(call_dto).collect(),
        gaps: value.gaps.into_iter().map(gap_dto).collect(),
    }
}

/// Convert the agent-layer projection to the stable HTTP list contract.
pub fn session_observation_list_dto(value: SessionObservationList) -> SessionObservationListDto {
    SessionObservationListDto {
        recorder_health: recorder_health_dto(value.recorder_health),
        summary: observation_summary_dto(value.summary),
        turns: value.turns.into_iter().map(turn_dto).collect(),
    }
}

/// Convert the agent-layer turn projection to the stable HTTP contract.
pub fn session_observation_turn_dto(value: ProjectedTurn) -> SessionObservationTurnDto {
    turn_dto(value)
}

/// Convert the agent-layer model-call projection to the stable HTTP contract.
pub fn session_observation_call_dto(value: ProjectedModelCall) -> SessionObservationCallDto {
    call_dto(value)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionObservationList {
    pub recorder_health: RecorderHealth,
    pub summary: ObservationSummary,
    pub turns: Vec<ProjectedTurn>,
}

/// Shared observability service for session observation JSONL.
#[derive(Clone)]
pub struct AgentTraceHub {
    client_prefs: Option<Arc<dyn IClientPreferenceRepository>>,
    recorder: Arc<ObservationRecorder>,
    read_permits: Arc<Semaphore>,
}

impl AgentTraceHub {
    pub fn new(
        data_dir: impl AsRef<Path>,
        client_prefs: Option<Arc<dyn IClientPreferenceRepository>>,
    ) -> Self {
        let data_dir = data_dir.as_ref();
        Self {
            client_prefs,
            recorder: ObservationRecorder::shared(data_dir),
            read_permits: Arc::new(Semaphore::new(OBSERVATION_READ_PERMITS)),
        }
    }

    async fn with_read<T, F>(&self, work: F) -> Result<T, TraceApiError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, RecorderError> + Send + 'static,
    {
        let _permit = self
            .read_permits
            .acquire()
            .await
            .map_err(|_| TraceApiError::ObservationJoin("observation read semaphore closed".into()))?;
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|error| TraceApiError::ObservationJoin(error.to_string()))?
            .map_err(TraceApiError::Observation)
    }

    pub async fn developer_mode_enabled(&self) -> bool {
        developer_mode_enabled(self.client_prefs.as_ref()).await
    }

    pub fn observation_recorder(&self) -> Arc<ObservationRecorder> {
        Arc::clone(&self.recorder)
    }

    /// Best-effort delete of one conversation's observation JSONL.
    ///
    /// Lifecycle cleanup is not gated on developer mode: files may exist from
    /// a previous enabled period. Failures are logged and never fail the
    /// conversation mutation. Waits for the writer ACK so queued events cannot
    /// recreate a deleted directory.
    pub fn drop_conversation_observations(&self, conversation_id: &str) {
        if let Err(error) = self.recorder.remove_conversation(conversation_id) {
            tracing::warn!(
                conversation_id,
                error = %error,
                "Failed to remove session observation files"
            );
        }
    }

    /// Clear observation files for a conversation that still exists (reset /
    /// clear context / clear messages). Bumps generation so the same id can
    /// keep recording after ACK.
    pub fn clear_conversation_observations(&self, conversation_id: &str) {
        if let Err(error) = self.recorder.clear_conversation(conversation_id) {
            tracing::warn!(
                conversation_id,
                error = %error,
                "Failed to clear session observation files"
            );
        }
    }

    /// Wipe the observation root while the writer is still alive (factory reset).
    pub fn reset_all_observations(&self) {
        if let Err(error) = self.recorder.reset_all() {
            tracing::warn!(error = %error, "Failed to reset session observation store");
        }
    }

    /// Require developer mode for HTTP read APIs. Returns `Ok(())` when enabled.
    pub async fn require_developer_mode(&self) -> Result<(), TraceApiError> {
        if self.developer_mode_enabled().await {
            Ok(())
        } else {
            Err(TraceApiError::DeveloperModeRequired)
        }
    }

    pub async fn list_session_observations(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<SessionObservationList, TraceApiError> {
        self.require_developer_mode().await?;
        let limit = limit.clamp(1, MAX_SESSION_OBSERVATION_LIST_LIMIT);
        let conversation_id = conversation_id.to_owned();
        let recorder = Arc::clone(&self.recorder);
        self.with_read(move || {
            let summary = recorder.read_summary(Some(&conversation_id))?;
            let events = recorder.read_events_for_latest_turns(Some(&conversation_id), limit)?;
            let mut all = project_turns(&events);
            let start = all.len().saturating_sub(limit);
            let mut turns = all.split_off(start);
            for turn in &mut turns {
                strip_projected_turn_payloads(turn);
            }
            Ok(SessionObservationList {
                recorder_health: recorder.health(),
                summary,
                turns,
            })
        })
        .await
    }

    pub async fn get_session_observation_turn(
        &self,
        conversation_id: &str,
        root_turn_id: &str,
    ) -> Result<Option<ProjectedTurn>, TraceApiError> {
        self.require_developer_mode().await?;
        let conversation_id = conversation_id.to_owned();
        let root_turn_id = root_turn_id.to_owned();
        let recorder = Arc::clone(&self.recorder);
        self.with_read(move || {
            let events = recorder.read_events_for_turn(Some(&conversation_id), &root_turn_id)?;
            Ok(project_turn_by_id(&events, &root_turn_id).map(|mut turn| {
                strip_projected_turn_payloads(&mut turn);
                turn
            }))
        })
        .await
    }

    pub async fn get_session_observation_call(
        &self,
        conversation_id: &str,
        root_turn_id: &str,
        model_call_id: &str,
    ) -> Result<ProjectedModelCall, TraceApiError> {
        self.require_developer_mode().await?;
        let conversation_id = conversation_id.to_owned();
        let root_turn_id = root_turn_id.to_owned();
        let model_call_id = model_call_id.to_owned();
        let not_found_id = model_call_id.clone();
        let recorder = Arc::clone(&self.recorder);
        let call = self
            .with_read(move || {
                let events = recorder.read_events_for_turn(Some(&conversation_id), &root_turn_id)?;
                let Some(turn) = project_turn_by_id(&events, &root_turn_id) else {
                    return Ok(None);
                };
                Ok(project_call_detail(&events, &root_turn_id, &model_call_id)
                    .map(|detail| (turn.has_turn_end, detail)))
            })
            .await?;
        resolve_session_observation_call(not_found_id, call)
    }
}

fn call_payloads_missing(detail: &ProjectedModelCall) -> bool {
    detail.request.is_none()
        && detail.response.is_none()
        && detail.tools.iter().all(|tool| {
            tool.started.is_none()
                && tool.completed.is_none()
                && tool.failed.is_none()
                && tool.cancelled.is_none()
        })
}

fn resolve_session_observation_call(
    model_call_id: String,
    call: Option<(bool, ProjectedModelCall)>,
) -> Result<ProjectedModelCall, TraceApiError> {
    match call {
        Some((turn_ended, detail)) if call_payloads_missing(&detail) && turn_ended => {
            Err(TraceApiError::ObservationRetention)
        }
        Some((_, detail)) => Ok(detail),
        None => Err(TraceApiError::NotFound(format!(
            "session observation call '{model_call_id}' not found"
        ))),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TraceApiError {
    #[error("developer mode is required to access session observations")]
    DeveloperModeRequired,
    #[error(transparent)]
    Observation(#[from] RecorderError),
    #[error("session observation read failed: {0}")]
    ObservationJoin(String),
    #[error("observation_retention")]
    ObservationRetention,
    #[error("{0}")]
    NotFound(String),
}

impl TraceApiError {
    pub fn into_app_error(self) -> nomifun_common::AppError {
        match self {
            Self::DeveloperModeRequired => nomifun_common::AppError::Forbidden(
                "Enable Developer Mode in Settings → System to inspect session observations".into(),
            ),
            Self::Observation(error) => nomifun_common::AppError::Internal(format!(
                "session observation store error: {error}"
            )),
            Self::ObservationJoin(error) => nomifun_common::AppError::Internal(format!(
                "session observation store error: {error}"
            )),
            // Call GET maps this in `routes_trace.rs` to HTTP 410. Do not send
            // this variant through the generic mapper — Internal would be 500.
            Self::ObservationRetention => nomifun_common::AppError::Internal(
                "observation_retention".into(),
            ),
            Self::NotFound(message) => nomifun_common::AppError::NotFound(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_observation_reads_require_developer_mode() {
        let dir = tempfile::tempdir().unwrap();
        let hub = AgentTraceHub::new(dir.path(), None);
        let error = hub
            .list_session_observations("conv", DEFAULT_SESSION_OBSERVATION_LIST_LIMIT)
            .await
            .expect_err("developer mode must gate observation reads");
        assert!(matches!(error, TraceApiError::DeveloperModeRequired));
    }

    #[test]
    fn drop_conversation_observations_deletes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let hub = AgentTraceHub::new(dir.path(), None);
        let recorder = hub.observation_recorder();
        recorder.set_enabled(true);
        recorder
            .emit(
                nomi_agent_trace::EVENT_LLM_REQUEST,
                &nomi_agent_trace::ObservationIds {
                    conversation_id: Some("conv-drop".into()),
                    root_turn_id: Some("turn-drop".into()),
                    ..nomi_agent_trace::ObservationIds::default()
                },
                serde_json::json!({}),
            )
            .unwrap();
        assert!(!recorder.read_events(Some("conv-drop")).unwrap().is_empty());

        hub.drop_conversation_observations("conv-drop");
        assert!(recorder.read_events(Some("conv-drop")).unwrap().is_empty());
        assert!(!recorder.root().join("conv-drop").exists());
    }

    fn empty_call(model_call_id: &str) -> ProjectedModelCall {
        ProjectedModelCall {
            model_call_id: model_call_id.to_owned(),
            call_kind: None,
            observation_scope: None,
            status: nomi_agent_trace::ExecutionStatus::Running,
            integrity: nomi_agent_trace::Integrity::Complete,
            interrupted: false,
            started_at_ms: None,
            ended_at_ms: None,
            usage: None,
            request: None,
            response: None,
            request_summary: None,
            response_summary: None,
            tools: Vec::new(),
        }
    }

    #[test]
    fn session_observation_dto_mapping_preserves_wire_contract() {
        let dto = session_observation_list_dto(SessionObservationList {
            recorder_health: RecorderHealth {
                status: RecorderHealthStatus::QueueDropped,
                last_error: Some("overflow".into()),
            },
            summary: ObservationSummary {
                turn_count: 1,
                model_call_count: 1,
                tool_count: 0,
                active_duration_ms: 12,
                wall_span_ms: Some(20),
                integrity: Integrity::Degraded,
                coverage: "retained_observation_history".into(),
                max_event_seq: 4,
            },
            turns: vec![ProjectedTurn {
                root_turn_id: "turn-1".into(),
                conversation_id: Some("conv-1".into()),
                msg_id: Some("msg-1".into()),
                session_kind: None,
                execution_id: None,
                step_id: None,
                execution_attempt_id: None,
                status: ExecutionStatus::Interrupted,
                integrity: Integrity::Degraded,
                interrupted: true,
                started_at_ms: Some(1),
                ended_at_ms: Some(13),
                elapsed_ms: Some(12),
                prompt_preview: Some("66".into()),
                prompt_preview_context_only: false,
                max_event_seq: 4,
                has_turn_start: true,
                has_turn_end: true,
                gap_count: 1,
                model_calls: vec![empty_call("call-1")],
                gaps: vec![],
            }],
        });
        let json = serde_json::to_value(dto).expect("DTO should serialize");
        assert_eq!(json["recorder_health"]["status"], "queue_dropped");
        assert_eq!(json["summary"]["integrity"], "degraded");
        assert_eq!(json["turns"][0]["status"], "interrupted");
        assert_eq!(json["turns"][0]["prompt_preview"], "66");
        assert_eq!(json["turns"][0]["prompt_preview_context_only"], false);
        assert_eq!(json["turns"][0]["model_calls"][0]["status"], "running");
    }

    #[test]
    fn empty_payload_is_retention_only_after_turn_end() {
        let detail = empty_call("mc-empty");
        assert!(call_payloads_missing(&detail));
        let in_flight =
            resolve_session_observation_call("mc-empty".into(), Some((false, detail.clone())));
        assert!(in_flight.is_ok(), "running turn must return the empty body");
        let ended = resolve_session_observation_call("mc-empty".into(), Some((true, detail)));
        assert!(matches!(ended, Err(TraceApiError::ObservationRetention)));
    }

    #[test]
    fn missing_call_on_ended_turn_is_not_found() {
        let error = resolve_session_observation_call("mc-missing".into(), None)
            .expect_err("unknown call after turn end is 404");
        assert!(matches!(error, TraceApiError::NotFound(_)));
    }

    #[test]
    fn clear_conversation_observations_allows_same_id_again() {
        let dir = tempfile::tempdir().unwrap();
        let hub = AgentTraceHub::new(dir.path(), None);
        let recorder = hub.observation_recorder();
        recorder.set_enabled(true);
        recorder
            .emit(
                nomi_agent_trace::EVENT_LLM_REQUEST,
                &nomi_agent_trace::ObservationIds {
                    conversation_id: Some("conv-clear".into()),
                    root_turn_id: Some("turn-old".into()),
                    ..nomi_agent_trace::ObservationIds::default()
                },
                serde_json::json!({}),
            )
            .unwrap();
        hub.clear_conversation_observations("conv-clear");
        assert!(recorder.read_events(Some("conv-clear")).unwrap().is_empty());
        recorder
            .emit(
                nomi_agent_trace::EVENT_LLM_REQUEST,
                &nomi_agent_trace::ObservationIds {
                    conversation_id: Some("conv-clear".into()),
                    root_turn_id: Some("turn-new".into()),
                    ..nomi_agent_trace::ObservationIds::default()
                },
                serde_json::json!({}),
            )
            .unwrap();
        let events = recorder.read_events(Some("conv-clear")).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            nomi_agent_trace::ids_from_payload(&events[0].payload)
                .root_turn_id
                .as_deref(),
            Some("turn-new")
        );
    }
}
