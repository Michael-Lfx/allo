//! Process-wide session observation hub: recorder + developer-mode preference gate.

use std::path::Path;
use std::sync::Arc;

use nomifun_db::IClientPreferenceRepository;
use nomi_agent_trace::{
    project_call_detail, project_turn_by_id, project_turns, strip_projected_turn_payloads,
    ObservationRecorder, ObservationSummary, ProjectedModelCall, ProjectedTurn, RecorderError,
    RecorderHealth,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use super::prefs::developer_mode_enabled;

pub const DEFAULT_SESSION_OBSERVATION_LIST_LIMIT: usize = 50;
pub const MAX_SESSION_OBSERVATION_LIST_LIMIT: usize = 200;
const OBSERVATION_READ_PERMITS: usize = 4;

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

    pub async fn refresh_recording_enabled(&self) {
        self.recorder
            .set_enabled(self.developer_mode_enabled().await);
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
            tools: Vec::new(),
        }
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
