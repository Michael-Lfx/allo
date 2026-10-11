//! Local voice model lifecycle manager.
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4
//! - `model-management.zh.md` §4

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteResponse, VoiceModelInstallRequest,
    VoiceModelInstallResponse, VoiceModelProgressEvent, VoiceModelStatusResponse,
};

use crate::layout::{check_model_layout, inspect_model_files};

/// Errors encountered during model lifecycle operations.
#[derive(Debug, thiserror::Error)]
pub enum ModelManagerError {
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Operation conflict: {0}")]
    Conflict(String),
}

/// Core manager coordinating local voice model assets, state, and progress events.
#[derive(Debug, Clone)]
pub struct ModelManager {
    models_dir: PathBuf,
    state: Arc<RwLock<ModelInstallState>>,
    progress_tx: broadcast::Sender<VoiceModelProgressEvent>,
}

impl ModelManager {
    /// Initializes a new `ModelManager` targeting the specified directory.
    pub fn new(models_dir: impl AsRef<Path>) -> Self {
        let models_dir = models_dir.as_ref().to_path_buf();
        let initial_state = check_model_layout(&models_dir);
        let (progress_tx, _) = broadcast::channel(64);

        Self {
            models_dir,
            state: Arc::new(RwLock::new(initial_state)),
            progress_tx,
        }
    }

    /// Returns the models directory path.
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Returns the current status response of local voice models.
    pub async fn get_status(&self) -> VoiceModelStatusResponse {
        let (disk_state, files, total_size_bytes) = inspect_model_files(&self.models_dir);
        let mem_state = self.state.read().await.clone();

        // Active transition states in memory take precedence over static disk inspection
        let state = match &mem_state {
            ModelInstallState::ProbingSource
            | ModelInstallState::Downloading { .. }
            | ModelInstallState::VerifyingSha256 { .. }
            | ModelInstallState::Removing => mem_state,
            _ => disk_state,
        };

        VoiceModelStatusResponse {
            state,
            models_dir: self.models_dir.clone(),
            total_size_bytes,
            files,
        }
    }

    /// Triggers model installation.
    ///
    /// In Wave 0 skeleton, this returns `ModelManagerError::NotImplemented`.
    pub async fn install(
        &self,
        _req: Option<VoiceModelInstallRequest>,
    ) -> Result<VoiceModelInstallResponse, ModelManagerError> {
        Err(ModelManagerError::NotImplemented(
            "Model download and installation is not implemented in Wave 0 skeleton".into(),
        ))
    }

    /// Deletes installed model files and resets the state to `NotInstalled`.
    pub async fn remove(&self) -> Result<VoiceModelDeleteResponse, ModelManagerError> {
        let (_, _, freed_bytes) = inspect_model_files(&self.models_dir);

        if self.models_dir.exists() {
            // Delete expected files and clean up directories
            for &rel in crate::layout::EXPECTED_FILES {
                let p = self.models_dir.join(rel);
                if p.is_file() {
                    let _ = tokio::fs::remove_file(&p).await;
                }
            }
            // Remove empty subdirectories if any
            let _ = tokio::fs::remove_dir(self.models_dir.join("kws")).await;
            let _ = tokio::fs::remove_dir(self.models_dir.join("vad")).await;
            let _ = tokio::fs::remove_dir(self.models_dir.join("asr")).await;
        }

        let mut state_guard = self.state.write().await;
        *state_guard = ModelInstallState::NotInstalled;
        let _ = self.progress_tx.send(VoiceModelProgressEvent::new(
            ModelInstallState::NotInstalled,
        ));

        Ok(VoiceModelDeleteResponse {
            success: true,
            freed_bytes,
            state: ModelInstallState::NotInstalled,
            message: Some("Models deleted successfully".into()),
        })
    }

    /// Subscribes to SSE progress events.
    pub fn subscribe_progress(&self) -> broadcast::Receiver<VoiceModelProgressEvent> {
        self.progress_tx.subscribe()
    }

    /// Accesses the broadcast sender for publishing progress events.
    pub fn progress_sender(&self) -> &broadcast::Sender<VoiceModelProgressEvent> {
        &self.progress_tx
    }

    /// Updates internal state and broadcasts a progress event.
    pub async fn set_state(&self, new_state: ModelInstallState) {
        let mut guard = self.state.write().await;
        *guard = new_state.clone();
        let _ = self.progress_tx.send(VoiceModelProgressEvent::new(new_state));
    }
}
