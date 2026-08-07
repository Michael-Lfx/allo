//! Process-wide agent trace hub: store + developer-mode preference gate.

use std::path::Path;
use std::sync::Arc;

use nomifun_db::IClientPreferenceRepository;
use nomi_agent_trace::{FileTraceStore, TraceIndexEntry, TraceStoreError, TurnTrace};

use super::prefs::developer_mode_enabled;

/// Shared observability service for agent turn traces.
#[derive(Clone)]
pub struct AgentTraceHub {
    store: Arc<FileTraceStore>,
    client_prefs: Option<Arc<dyn IClientPreferenceRepository>>,
}

impl AgentTraceHub {
    pub fn new(
        data_dir: impl AsRef<Path>,
        client_prefs: Option<Arc<dyn IClientPreferenceRepository>>,
    ) -> Self {
        Self {
            store: Arc::new(FileTraceStore::new(data_dir)),
            client_prefs,
        }
    }

    pub fn store(&self) -> &FileTraceStore {
        &self.store
    }

    pub fn store_arc(&self) -> Arc<FileTraceStore> {
        Arc::clone(&self.store)
    }

    pub async fn developer_mode_enabled(&self) -> bool {
        developer_mode_enabled(self.client_prefs.as_ref()).await
    }

    /// Require developer mode for HTTP read APIs. Returns `Ok(())` when enabled.
    pub async fn require_developer_mode(&self) -> Result<(), TraceApiError> {
        if self.developer_mode_enabled().await {
            Ok(())
        } else {
            Err(TraceApiError::DeveloperModeRequired)
        }
    }

    pub async fn list_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<TraceIndexEntry>, TraceApiError> {
        self.require_developer_mode().await?;
        self.store
            .list_for_conversation(conversation_id, limit)
            .map_err(TraceApiError::Store)
    }

    pub async fn list_recent(&self, limit: usize) -> Result<Vec<TraceIndexEntry>, TraceApiError> {
        self.require_developer_mode().await?;
        self.store.list_recent(limit).map_err(TraceApiError::Store)
    }

    pub async fn get_trace(&self, trace_id: &str) -> Result<Option<TurnTrace>, TraceApiError> {
        self.require_developer_mode().await?;
        self.store.get(trace_id).map_err(TraceApiError::Store)
    }

    pub async fn export_paths_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<std::path::PathBuf>, TraceApiError> {
        // Support pack may attach traces only when developer mode is on.
        if !self.developer_mode_enabled().await {
            return Ok(Vec::new());
        }
        self.store
            .export_paths_for_conversation(conversation_id)
            .map_err(TraceApiError::Store)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TraceApiError {
    #[error("developer mode is required to access agent traces")]
    DeveloperModeRequired,
    #[error(transparent)]
    Store(#[from] TraceStoreError),
}

impl TraceApiError {
    pub fn into_app_error(self) -> nomifun_common::AppError {
        match self {
            Self::DeveloperModeRequired => nomifun_common::AppError::Forbidden(
                "Enable Developer Mode in Settings → System to inspect agent traces".into(),
            ),
            Self::Store(error) => {
                nomifun_common::AppError::Internal(format!("agent trace store error: {error}"))
            }
        }
    }
}
