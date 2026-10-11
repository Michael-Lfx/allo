//! Local voice model lifecycle manager.
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4
//! - `model-management.zh.md` §4

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{error, info};

use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteResponse, VoiceModelInstallRequest,
    VoiceModelInstallResponse, VoiceModelProgressEvent, VoiceModelStatusResponse,
};

use crate::downloader::{MirrorConfig, ModelDownloader};
use crate::layout::{check_model_layout, inspect_model_files, EXPECTED_FILES};
use crate::verifier::{DOWNLOAD_FILES, ModelVerifier};

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
    downloader: Arc<ModelDownloader>,
    verifier: Arc<ModelVerifier>,
    install_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ModelManager {
    /// Initializes a new `ModelManager` targeting the specified directory with defaults.
    pub fn new(models_dir: impl AsRef<Path>) -> Self {
        Self::with_config(
            models_dir,
            MirrorConfig::default_mirrors(),
            crate::verifier::default_canonical_manifest(),
        )
    }

    /// Initializes a `ModelManager` with custom mirrors and baseline verification manifest.
    pub fn with_config(
        models_dir: impl AsRef<Path>,
        mirrors: Vec<MirrorConfig>,
        baseline_manifest: HashMap<String, String>,
    ) -> Self {
        let models_dir = models_dir.as_ref().to_path_buf();
        let initial_state = check_model_layout(&models_dir);
        let (progress_tx, _) = broadcast::channel(128);

        Self {
            models_dir,
            state: Arc::new(RwLock::new(initial_state)),
            progress_tx,
            downloader: Arc::new(ModelDownloader::new(mirrors)),
            verifier: Arc::new(ModelVerifier::with_manifest(baseline_manifest)),
            install_task: Arc::new(Mutex::new(None)),
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

    /// Triggers model installation asynchronously with idempotency protection.
    pub async fn install(
        &self,
        _req: Option<VoiceModelInstallRequest>,
    ) -> Result<VoiceModelInstallResponse, ModelManagerError> {
        let current_state = self.state.read().await.clone();

        // 1. Idempotency check: if already ready, reject smoothly
        if current_state.is_ready() {
            return Ok(VoiceModelInstallResponse {
                accepted: false,
                state: current_state,
                message: Some("Models already installed and ready".into()),
            });
        }

        // 2. Idempotency check: if already busy (in progress), reject duplicate request
        if current_state.is_busy() {
            return Ok(VoiceModelInstallResponse {
                accepted: false,
                state: current_state,
                message: Some("Installation already in progress".into()),
            });
        }

        // 3. Mark state as ProbingSource and broadcast event
        self.set_state(ModelInstallState::ProbingSource).await;

        // 4. Spawn background install task
        let this = self.clone();
        let handle = tokio::spawn(async move {
            this.run_install_pipeline().await;
        });

        let mut task_guard = self.install_task.lock().await;
        *task_guard = Some(handle);

        Ok(VoiceModelInstallResponse {
            accepted: true,
            state: ModelInstallState::ProbingSource,
            message: Some("Installation started".into()),
        })
    }

    /// Internal asynchronous install pipeline execution.
    async fn run_install_pipeline(&self) {
        info!("Starting voice model installation pipeline");

        // Step 1: Probe mirror sources
        let mirror = match self.downloader.probe_best_source().await {
            Ok(m) => m,
            Err(e) => {
                let reason = format!("Failed to probe mirror sources: {e}");
                error!("{reason}");
                self.set_state(ModelInstallState::Failed {
                    reason,
                    recoverable: true,
                })
                .await;
                return;
            }
        };

        info!("Selected mirror: {} ({})", mirror.id, mirror.base_url);

        // Step 2: Download files with progress tracking
        let total_files = DOWNLOAD_FILES.len();
        let mut overall_downloaded: u64 = 0;
        let mut total_estimated_bytes: u64 = 0;

        // Estimate size if possible or use cumulative progress
        for (idx, &rel_path) in DOWNLOAD_FILES.iter().enumerate() {
            let file_index = idx + 1;
            let target_path = self.models_dir.join(rel_path);

            let start_time = Instant::now();
            let mut file_downloaded_bytes: u64 = 0;

            // Broadcast initial downloading state for this file
            self.set_state(ModelInstallState::Downloading {
                current_file: rel_path.to_string(),
                file_index,
                total_files,
                downloaded_bytes: overall_downloaded,
                total_bytes: total_estimated_bytes.max(overall_downloaded + 1),
                progress_percent: if total_files > 0 {
                    (idx as f32 / total_files as f32) * 100.0
                } else {
                    0.0
                },
                speed_bytes_per_sec: 0,
            })
            .await;

            let download_result = self
                .downloader
                .download_with_failover(rel_path, &target_path, 0)
                .await;

            match download_result {
                Ok(_) => {
                    if let Ok(meta) = tokio::fs::metadata(&target_path).await {
                        file_downloaded_bytes = meta.len();
                    }
                    overall_downloaded = overall_downloaded.saturating_add(file_downloaded_bytes);
                    total_estimated_bytes = total_estimated_bytes.max(overall_downloaded);

                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.001 {
                        (file_downloaded_bytes as f64 / elapsed) as u64
                    } else {
                        file_downloaded_bytes * 10
                    };

                    self.set_state(ModelInstallState::Downloading {
                        current_file: rel_path.to_string(),
                        file_index,
                        total_files,
                        downloaded_bytes: overall_downloaded,
                        total_bytes: total_estimated_bytes,
                        progress_percent: ((file_index as f32) / (total_files as f32)) * 100.0,
                        speed_bytes_per_sec: speed,
                    })
                    .await;
                }
                Err(e) => {
                    let reason = format!("Failed to download {rel_path}: {e}");
                    error!("{reason}");
                    self.set_state(ModelInstallState::Failed {
                        reason,
                        recoverable: true,
                    })
                    .await;
                    return;
                }
            }
        }

        // Also fetch checksums.sha256 if available
        let checksums_path = self.models_dir.join("checksums.sha256");
        let _ = self
            .downloader
            .download_file_with_resume(&mirror, "checksums.sha256", &checksums_path)
            .await;

        // Ensure keywords.txt is present (compiled locally or default fallback)
        let keywords_path = self.models_dir.join("kws/keywords.txt");
        if !keywords_path.exists() {
            // Try downloading from mirror first
            if self
                .downloader
                .download_file_with_resume(&mirror, "kws/keywords.txt", &keywords_path)
                .await
                .is_err()
            {
                // Create standard default
                if let Some(parent) = keywords_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&keywords_path, b"HEY FLOWY :1.0 #0.25 @1\n").await;
            }
        }

        // Step 3: SHA256 Verification
        for (idx, &rel_path) in DOWNLOAD_FILES.iter().enumerate() {
            self.set_state(ModelInstallState::VerifyingSha256 {
                current_file: rel_path.to_string(),
                verified_files: idx,
                total_files,
            })
            .await;
        }

        let verify_state = self.verifier.verify(&self.models_dir).await;
        self.set_state(verify_state).await;
    }

    /// Deletes installed model files and resets the state to `NotInstalled`.
    pub async fn remove(&self) -> Result<VoiceModelDeleteResponse, ModelManagerError> {
        // Abort running install task if active
        let mut task_guard = self.install_task.lock().await;
        if let Some(handle) = task_guard.take() {
            handle.abort();
        }

        self.set_state(ModelInstallState::Removing).await;

        let (_, _, freed_bytes) = inspect_model_files(&self.models_dir);

        if self.models_dir.exists() {
            // Delete all expected model assets and part files
            for &rel in EXPECTED_FILES {
                let p = self.models_dir.join(rel);
                if p.is_file() {
                    let _ = tokio::fs::remove_file(&p).await;
                }
                let part = PathBuf::from(format!("{}.part", p.display()));
                if part.is_file() {
                    let _ = tokio::fs::remove_file(&part).await;
                }
            }
            let _ = tokio::fs::remove_file(self.models_dir.join("checksums.sha256")).await;
            let _ = tokio::fs::remove_dir(self.models_dir.join("kws")).await;
            let _ = tokio::fs::remove_dir(self.models_dir.join("vad")).await;
            let _ = tokio::fs::remove_dir(self.models_dir.join("asr")).await;
        }

        self.set_state(ModelInstallState::NotInstalled).await;

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
