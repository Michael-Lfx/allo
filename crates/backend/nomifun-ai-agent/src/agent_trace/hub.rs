//! Process-wide session observation hub: recorder + developer-mode preference gate.

use std::path::Path;
use std::sync::Arc;

use nomifun_db::IClientPreferenceRepository;
use nomi_agent_trace::{
    project_turn_by_id, project_turns, strip_projected_turn_payloads, ObservationRecorder,
    ProjectedTurn, RecorderError,
};

use super::prefs::developer_mode_enabled;

pub const DEFAULT_SESSION_OBSERVATION_LIST_LIMIT: usize = 50;
pub const MAX_SESSION_OBSERVATION_LIST_LIMIT: usize = 200;

/// Shared observability service for session observation JSONL.
#[derive(Clone)]
pub struct AgentTraceHub {
    client_prefs: Option<Arc<dyn IClientPreferenceRepository>>,
    recorder: Arc<ObservationRecorder>,
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
        }
    }

    pub async fn developer_mode_enabled(&self) -> bool {
        developer_mode_enabled(self.client_prefs.as_ref()).await
    }

    pub fn observation_recorder(&self) -> Arc<ObservationRecorder> {
        Arc::clone(&self.recorder)
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
    ) -> Result<Vec<ProjectedTurn>, TraceApiError> {
        self.require_developer_mode().await?;
        let limit = limit.clamp(1, MAX_SESSION_OBSERVATION_LIST_LIMIT);
        let events = self
            .recorder
            .read_events_for_latest_turns(Some(conversation_id), limit)
            .map_err(TraceApiError::Observation)?;
        let mut all = project_turns(&events);
        let start = all.len().saturating_sub(limit);
        let mut turns = all.split_off(start);
        for turn in &mut turns {
            strip_projected_turn_payloads(turn);
        }
        Ok(turns)
    }

    pub async fn get_session_observation_turn(
        &self,
        conversation_id: &str,
        root_turn_id: &str,
    ) -> Result<Option<ProjectedTurn>, TraceApiError> {
        self.require_developer_mode().await?;
        let events = self
            .recorder
            .read_events_for_turn(Some(conversation_id), root_turn_id)
            .map_err(TraceApiError::Observation)?;
        Ok(project_turn_by_id(&events, root_turn_id))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TraceApiError {
    #[error("developer mode is required to access session observations")]
    DeveloperModeRequired,
    #[error(transparent)]
    Observation(#[from] RecorderError),
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
}
