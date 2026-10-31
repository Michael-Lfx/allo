//! Local voice model lifecycle manager.
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4
//! - `model-management.zh.md` §4

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use tokio::sync::{Mutex, RwLock, broadcast};
use tracing::{error, info, warn};

use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteResponse, VoiceModelInstallRequest,
    VoiceModelInstallResponse, VoiceModelProgressEvent, VoiceModelStatusResponse,
};

use crate::downloader::{MirrorConfig, ModelDownloader};
use crate::layout::{EXPECTED_FILES, check_model_layout, inspect_model_files};
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

/// Cross-manager/cross-process write lease for one canonical models directory.
/// The OS lock is held by the open file handle, so a hard process exit releases
/// it automatically. The lock file may remain on disk; its existence is not
/// evidence that a writer is active.
#[derive(Debug)]
struct DirectoryWriteLease {
    _file: std::fs::File,
}

impl DirectoryWriteLease {
    fn acquire(models_dir: &Path) -> Result<Self, ModelManagerError> {
        // Create then canonicalize so aliases, trailing separators and `..`
        // all resolve to the exact same lock inode.
        std::fs::create_dir_all(models_dir)?;
        let canonical_dir = std::fs::canonicalize(models_dir)?;
        let path = canonical_dir.join(".voice-suite-write.lock");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => {
                use std::io::{Seek, SeekFrom, Write};
                let _ = file.set_len(0);
                let _ = file.seek(SeekFrom::Start(0));
                let _ = writeln!(
                    file,
                    "pid={} at_ms={}",
                    std::process::id(),
                    nomifun_common::now_ms()
                );
                Ok(Self { _file: file })
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                Err(ModelManagerError::Conflict(format!(
                    "models_directory_busy: another voice model writer owns {}",
                    canonical_dir.display()
                )))
            }
            Err(std::fs::TryLockError::Error(error)) => Err(ModelManagerError::Io(error)),
        }
    }
}

#[derive(Clone)]
struct InUseGuard(Option<Arc<dyn Fn() -> bool + Send + Sync>>);

impl Default for InUseGuard {
    fn default() -> Self {
        Self(None)
    }
}

impl std::fmt::Debug for InUseGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InUseGuard")
    }
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
    checked_files: Arc<RwLock<Option<Vec<(u64, SystemTime)>>>>,
    in_use_guard: Arc<std::sync::RwLock<InUseGuard>>,
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
            checked_files: Arc::new(RwLock::new(None)),
            in_use_guard: Arc::new(std::sync::RwLock::new(InUseGuard::default())),
        }
    }

    /// Installs a host-owned predicate that reports active calls, tasks or
    /// approvals. Model writes are refused while the suite is in use.
    pub fn set_in_use_guard(&self, guard: Arc<dyn Fn() -> bool + Send + Sync>) {
        *self
            .in_use_guard
            .write()
            .unwrap_or_else(|error| error.into_inner()) = InUseGuard(Some(guard));
    }

    fn suite_in_use(&self) -> bool {
        self.in_use_guard
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .0
            .as_ref()
            .is_some_and(|guard| guard())
    }

    /// Returns the models directory path.
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Stable, secret-free identity for the installed files used by readiness
    /// evidence. It is not a substitute for SHA verification; it detects
    /// replacement/provision changes between a check and later operations.
    pub fn identity_revision(&self) -> String {
        let stable_dir = match std::fs::canonicalize(&self.models_dir) {
            Ok(canonical) => canonical.to_string_lossy().to_string(),
            Err(_) => format!("unresolved:{}", self.models_dir.display()),
        };
        let mut parts = Vec::new();
        for &relative in EXPECTED_FILES {
            let path = self.models_dir.join(relative);
            match std::fs::metadata(&path) {
                Ok(metadata) => {
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default();
                    parts.push(format!("{relative}:{}:{modified}", metadata.len()));
                }
                Err(_) => parts.push(format!("{relative}:missing")),
            }
        }
        format!("{}|{}", stable_dir, parts.join(","))
    }
    /// Returns the current status response of local voice models.
    pub async fn get_status(&self) -> VoiceModelStatusResponse {
        let _operation = self.install_task.lock().await;
        let (disk_state, mut files, mut total_size_bytes) = inspect_model_files(&self.models_dir);
        let (_, tts_files, tts_size) = crate::layout::inspect_tts_files(&self.models_dir);
        files.extend(tts_files);
        total_size_bytes = total_size_bytes.saturating_add(tts_size);
        let state = self.refresh_integrity(disk_state).await;

        VoiceModelStatusResponse {
            state,
            models_dir: self.models_dir.clone(),
            total_size_bytes,
            files,
        }
    }

    /// Returns whether the offline TTS models are present and pass SHA256 integrity verification.
    pub async fn is_tts_ready(&self) -> bool {
        self.verifier.verify_tts(&self.models_dir).await
    }

    fn file_stamps(&self) -> std::io::Result<Vec<(u64, SystemTime)>> {
        EXPECTED_FILES
            .iter()
            .map(|rel| {
                let metadata = std::fs::metadata(self.models_dir.join(rel))?;
                Ok((metadata.len(), metadata.modified()?))
            })
            .collect()
    }

    // Called under the lifecycle lock. Cache both success and failure only for
    // unchanged files; an offline repair or replacement must be checked again.
    async fn refresh_integrity(&self, disk_state: ModelInstallState) -> ModelInstallState {
        let current = self.state.read().await.clone();
        if current.is_busy() {
            return current;
        }
        if !disk_state.is_ready() {
            *self.checked_files.write().await = None;
            if current.is_failed() {
                return current;
            }
            if current != disk_state {
                self.set_state(disk_state.clone()).await;
            }
            return disk_state;
        }
        if let Ok(stamps) = self.file_stamps() {
            if self.checked_files.read().await.as_ref() == Some(&stamps) {
                return current;
            }
        }
        self.verify_files().await
    }

    async fn verify_files(&self) -> ModelInstallState {
        let before = self.file_stamps().ok();
        let verified = self.verifier.verify(&self.models_dir).await;
        let after = self.file_stamps().ok();
        let stable = before.is_some() && before == after;
        let state = if stable || !verified.is_ready() {
            verified
        } else {
            ModelInstallState::Failed {
                reason: "Model files changed during integrity verification; retry after installation completes".into(),
                recoverable: true,
            }
        };
        self.set_state(state.clone()).await;
        *self.checked_files.write().await = if stable { after } else { None };
        state
    }

    /// Triggers model installation asynchronously with idempotency protection.
    pub async fn install(
        &self,
        req: Option<VoiceModelInstallRequest>,
    ) -> Result<VoiceModelInstallResponse, ModelManagerError> {
        // The same lock owns admission, handle registration and removal.
        let mut task_guard = self.install_task.lock().await;
        let current_state = self.state.read().await.clone();
        let force = req.as_ref().and_then(|r| r.force).unwrap_or(false);

        // 1. Idempotency check: if already busy (in progress), reject duplicate request
        if current_state.is_busy() || task_guard.as_ref().is_some_and(|task| !task.is_finished()) {
            return Ok(VoiceModelInstallResponse {
                accepted: false,
                state: current_state,
                message: Some("Installation already in progress".into()),
            });
        }

        if self.suite_in_use() {
            return Err(ModelManagerError::Conflict(
                "voice_suite_in_use: finish the active voice call, task or approval before installing models"
                    .into(),
            ));
        }

        // 2. Check actual disk layout state in addition to in-memory state
        let (disk_state, _, _) = inspect_model_files(&self.models_dir);
        let current_state = self.refresh_integrity(disk_state.clone()).await;

        // If not force requested, and both memory state and physical on-disk files are Ready:
        if !force && current_state.is_ready() && disk_state.is_ready() {
            return Ok(VoiceModelInstallResponse {
                accepted: false,
                state: current_state,
                message: Some("Models already installed and ready".into()),
            });
        }

        // 3. Take the cross-process directory lease before changing any file.
        let lease = DirectoryWriteLease::acquire(&self.models_dir)?;

        // 4. Mark state as ProbingSource and broadcast event
        *self.checked_files.write().await = None;
        self.set_state(ModelInstallState::ProbingSource).await;

        // 5. Spawn background install task; it owns the lease until completion.
        let this = self.clone();
        let handle = tokio::spawn(async move {
            let _lease = lease;
            this.run_install_pipeline(force).await;
        });

        *task_guard = Some(handle);

        Ok(VoiceModelInstallResponse {
            accepted: true,
            state: ModelInstallState::ProbingSource,
            message: Some("Installation started".into()),
        })
    }

    /// Internal asynchronous install pipeline execution.
    async fn run_install_pipeline(&self, force: bool) {
        info!("Starting voice model installation pipeline (force={force})");

        if force {
            info!(
                "Force re-install: purging existing model files and .part artifacts in {:?}",
                self.models_dir
            );
            for &rel in EXPECTED_FILES {
                let p = self.models_dir.join(rel);
                if p.exists() {
                    if let Err(e) = tokio::fs::remove_file(&p).await {
                        // On Windows, if a file handle is briefly held, rename to a disposable tombstone
                        let tombstone = p.with_extension(format!(
                            "old.{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                        ));
                        if let Err(rename_err) = tokio::fs::rename(&p, &tombstone).await {
                            warn!(
                                "Failed to remove or tombstone file {}: remove_err={}, rename_err={}",
                                p.display(),
                                e,
                                rename_err
                            );
                        } else {
                            let _ = tokio::fs::remove_file(&tombstone).await;
                        }
                    }
                }
                let part = PathBuf::from(format!("{}.part", p.display()));
                if part.exists() {
                    let _ = tokio::fs::remove_file(&part).await;
                }
            }
        }

        // Step 1: Probe mirror sources
        let mirror = match self.downloader.probe_best_source().await {
            Ok(m) => {
                info!("Selected mirror: {} ({})", m.id, m.base_url);
                Some(m)
            }
            Err(e) => {
                warn!(
                    "All candidate mirror probes failed ({e}). Proceeding with offline local fallback if remote download fails."
                );
                None
            }
        };

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

            let success = match download_result {
                Ok(_) => true,
                Err(e) => {
                    let reason = match &e {
                        crate::downloader::DownloaderError::AllMirrorsFailed => {
                            "无法连接到模型镜像源 (ModelScope)。请检查网络连接后重试，或运行 `python scripts/prepare-voice-models.py` 离线部署".to_string()
                        }
                        other => format!("下载模型文件 {rel_path} 失败: {other}。请检查网络后点击重试，或运行 `python scripts/prepare-voice-models.py` 离线部署"),
                    };
                    error!("{reason}");
                    self.set_state(ModelInstallState::Failed {
                        reason,
                        recoverable: true,
                    })
                    .await;
                    return;
                }
            };

            if success {
                if let Ok(meta) = tokio::fs::metadata(&target_path).await {
                    file_downloaded_bytes = meta.len();
                }
                overall_downloaded = overall_downloaded.saturating_add(file_downloaded_bytes);
                total_estimated_bytes = total_estimated_bytes.max(overall_downloaded);

                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.05 {
                    (file_downloaded_bytes as f64 / elapsed) as u64
                } else {
                    0
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
        }

        // Also fetch checksums.sha256 from mirror if available, or write authoritative checksums
        let checksums_path = self.models_dir.join("checksums.sha256");
        let mut fetched_checksums = false;
        if let Some(ref m) = mirror {
            if self
                .downloader
                .download_file_with_resume(m, "checksums.sha256", &checksums_path)
                .await
                .is_ok()
            {
                fetched_checksums = true;
            }
        }
        if !fetched_checksums && !checksums_path.exists() {
            let mut content = String::new();
            for (path, hash) in self.verifier.baseline_manifest() {
                content.push_str(&format!("{hash}  {path}\n"));
            }
            let _ = tokio::fs::write(&checksums_path, content).await;
        }

        // Ensure keywords.txt is present and properly tokenized for Zipformer KWS
        const DEFAULT_KWS_KEYWORDS: &str = " ▁HE Y  ▁F L OW Y :1.5 #0.15 @1\n ▁HE Y  ▁F LO W Y :1.5 #0.15 @1\n ▁HE Y  ▁F L OW :1.5 #0.15 @1\n ▁HI  ▁F L OW Y :1.5 #0.15 @1\n ▁HI  ▁F L OW :1.5 #0.15 @1\n ▁HE LL O  ▁F L OW Y :1.5 #0.15 @1\n";
        let keywords_path = self.models_dir.join("kws/keywords.txt");
        if !keywords_path.exists() {
            let mut fetched_keywords = false;
            if let Some(ref m) = mirror {
                if self
                    .downloader
                    .download_file_with_resume(m, "kws/keywords.txt", &keywords_path)
                    .await
                    .is_ok()
                {
                    fetched_keywords = true;
                }
            }
            if !fetched_keywords {
                if let Some(parent) = keywords_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&keywords_path, DEFAULT_KWS_KEYWORDS).await;
            }
        } else if let Ok(content) = tokio::fs::read_to_string(&keywords_path).await {
            if !content.contains('\u{2581}') {
                let _ = tokio::fs::write(&keywords_path, DEFAULT_KWS_KEYWORDS).await;
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
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        self.verify_files().await;
    }

    /// Deletes installed model files and reports what was actually removed.
    ///
    /// A locked or otherwise undeletable asset must never be reported as a
    /// successful removal, and `freed_bytes` only counts files that disappeared.
    pub async fn remove(&self) -> Result<VoiceModelDeleteResponse, ModelManagerError> {
        if self.suite_in_use() {
            return Err(ModelManagerError::Conflict(
                "voice_suite_in_use: finish the active voice call, task or approval before removing models"
                    .into(),
            ));
        }

        // Abort running install task if active so its lease is dropped first.
        let mut task_guard = self.install_task.lock().await;
        if let Some(handle) = task_guard.take() {
            handle.abort();
            let _ = handle.await;
        }

        // The directory lease is held for the whole removal, not just admission.
        let _lease = DirectoryWriteLease::acquire(&self.models_dir)?;

        *self.checked_files.write().await = None;
        self.set_state(ModelInstallState::Removing).await;

        let mut freed_bytes: u64 = 0;
        let mut failures: Vec<String> = Vec::new();

        let mut targets: Vec<PathBuf> = Vec::new();
        for &rel in EXPECTED_FILES.iter().chain(crate::layout::EXPECTED_TTS_FILES.iter()) {
            let path = self.models_dir.join(rel);
            targets.push(path.clone());
            targets.push(PathBuf::from(format!("{}.part", path.display())));
        }
        targets.push(self.models_dir.join("checksums.sha256"));

        for path in targets {
            if !path.exists() {
                continue;
            }
            let metadata = match tokio::fs::metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) => {
                    failures.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if !metadata.is_file() {
                failures.push(format!("{}: not a regular file", path.display()));
                continue;
            }
            match tokio::fs::remove_file(&path).await {
                Ok(()) => freed_bytes = freed_bytes.saturating_add(metadata.len()),
                Err(error) => failures.push(format!("{}: {error}", path.display())),
            }
        }

        for directory in ["kws", "vad", "asr", "tts"] {
            let _ = tokio::fs::remove_dir(self.models_dir.join(directory)).await;
        }

        let remaining: Vec<String> = EXPECTED_FILES
            .iter()
            .map(|rel| self.models_dir.join(rel))
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect();
        let (disk_state, _, _) = inspect_model_files(&self.models_dir);
        let success = failures.is_empty() && remaining.is_empty();
        // When removal failed, keep the state that the disk actually shows.
        let state = if success {
            ModelInstallState::NotInstalled
        } else {
            disk_state
        };
        self.set_state(state.clone()).await;

        let message = if success {
            "Models deleted successfully".to_string()
        } else {
            format!(
                "Removal incomplete: {} expected asset(s) still present; {} removal error(s)",
                remaining.len(),
                failures.len()
            )
        };
        if !failures.is_empty() {
            warn!(failures = ?failures, "Voice model removal reported incomplete");
        }

        Ok(VoiceModelDeleteResponse {
            success,
            freed_bytes,
            state,
            message: Some(message),
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
        let _ = self
            .progress_tx
            .send(VoiceModelProgressEvent::new(new_state));
    }
}

#[cfg(test)]
mod write_lease_tests {
    use super::*;

    #[test]
    fn one_canonical_directory_has_exactly_one_writer_across_managers() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        // Two independent managers (and any other process) share the directory
        // lease; a second writer must fail closed before touching files.
        let _first_manager = ModelManager::new(&models);
        let _second_manager = ModelManager::new(&models);

        let lease = DirectoryWriteLease::acquire(&models).unwrap();
        let error = DirectoryWriteLease::acquire(&models).unwrap_err();
        assert!(
            matches!(&error, ModelManagerError::Conflict(message) if message.contains("models_directory_busy")),
            "second writer must report a canonical-directory conflict, got {error:?}"
        );
        // Different aliases/trailing separators must target the same OS lock.
        let alias = models.join("..").join("models");
        assert!(
            matches!(
                DirectoryWriteLease::acquire(&alias),
                Err(ModelManagerError::Conflict(_))
            ),
            "an equivalent path alias must not acquire a second writer lock"
        );        drop(lease);
        assert!(
            DirectoryWriteLease::acquire(&models).is_ok(),
            "the lease must be reusable after the first writer finishes"
        );
    }

    #[test]
    fn identity_revision_is_consistent_across_path_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        let slash_str = models.to_string_lossy().to_string();
        let fwd_path = std::path::PathBuf::from(slash_str.replace('\\', "/"));
        let dot_path = models.join(".").join("..").join("models");

        let m1 = ModelManager::new(&fwd_path);
        let m3 = ModelManager::new(&dot_path);

        let r1 = m1.identity_revision();
        let r3 = m3.identity_revision();
        assert_eq!(r1, r3, "relative dot-dot aliases must produce identical identity_revision");

        #[cfg(windows)]
        {
            let back_path = std::path::PathBuf::from(slash_str.replace('/', "\\"));
            let m2 = ModelManager::new(&back_path);
            let r2 = m2.identity_revision();
            assert_eq!(
                r1, r2,
                "forward and backward slash aliases must produce identical identity_revision on Windows"
            );
        }

        // Non-existent distinct directories must not share identity
        let non_existent1 = dir.path().join("non_existent_1");
        let non_existent2 = dir.path().join("non_existent_2");
        let m_ne1 = ModelManager::new(&non_existent1);
        let m_ne2 = ModelManager::new(&non_existent2);
        assert_ne!(m_ne1.identity_revision(), m_ne2.identity_revision());
    }
}