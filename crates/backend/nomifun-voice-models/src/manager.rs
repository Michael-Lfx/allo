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
use crate::layout::{
    EXPECTED_FILES, EXPECTED_TTS_FILES, check_model_layout, inspect_model_files,
    inspect_tts_files,
};
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
        let mut manifest = crate::verifier::default_canonical_manifest();
        manifest.extend(crate::verifier::default_tts_manifest());
        Self::with_config(
            models_dir,
            MirrorConfig::default_mirrors(),
            manifest,
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
        for relative in self.tracked_relative_paths() {
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
        let (tts_layout_ready, tts_files, tts_size) = inspect_tts_files(&self.models_dir);
        files.extend(tts_files);
        total_size_bytes = total_size_bytes.saturating_add(tts_size);
        // Only enforce TTS completeness when the configured manifest actually
        // pins TTS assets (the production manifest does; reduced/custom suites
        // may not). ASR/KWS/VAD files alone cannot run a pinned voice suite, so
        // a partial TTS install (for example an older download without the
        // espeak voice definitions) must surface as repairable instead of
        // pretending the suite is usable.
        let tts_suite_is_pinned = self.tts_suite_is_pinned();
        let current = self.state.read().await.clone();
        let read_lease = if current.is_busy() { None } else { DirectoryWriteLease::acquire(&self.models_dir).ok() };
        let state = if !current.is_busy() && read_lease.is_none() {
            *self.checked_files.write().await = None;
            ModelInstallState::Failed { reason: "models_directory_busy: external model writer is active; retry status after it completes".into(), recoverable: true }
        } else if current.is_busy() {
            // Never turn a running download into NotInstalled just because the
            // TTS subtree is not complete yet.
            current
        } else if disk_state.is_ready() && tts_suite_is_pinned && !tts_layout_ready {
            // A Ready cache may predate a removed TTS file; force the next
            // refresh to re-verify even if restored files preserve metadata.
            *self.checked_files.write().await = None;
            self.set_state(ModelInstallState::NotInstalled).await;
            ModelInstallState::NotInstalled
        } else {
            self.refresh_integrity(disk_state).await
        };

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

    /// Waits for an in-flight install/remove to reach a terminal state and
    /// returns that state. Hosts use this before restarting the daemon that
    /// owns the model directory; polling only the atomic state avoids hashing
    /// the 200MB TTS subtree on every poll.
    pub async fn wait_until_settled(
        &self,
        timeout: std::time::Duration,
    ) -> ModelInstallState {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.state.read().await.clone();
            if !state.is_busy() || Instant::now() >= deadline {
                return state;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for the actual writer task to terminate, including lease destruction.
    /// Does not hold admission while waiting, so remove can abort and join it.
    pub async fn wait_for_completion(&self) -> ModelInstallState {
        loop {
            let task = self.install_task.lock().await;
            let finished = task.as_ref().is_none_or(|task| task.is_finished());
            if finished {
                let _lease = match DirectoryWriteLease::acquire(&self.models_dir) {
                    Ok(lease) => lease,
                    Err(_) => return ModelInstallState::Failed { reason: "models_directory_busy: external writer active; daemon resume deferred".into(), recoverable: true },
                };
                let state = self.state.read().await.clone();
                if state.is_busy() {
                    let failed = ModelInstallState::Failed { reason: "model_writer_cancelled: installation task ended before completion".into(), recoverable: true };
                    self.set_state(failed.clone()).await;
                    return failed;
                }
                return state;
            }
            drop(task);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Whether this manager's manifest pins the offline TTS subtree. Reduced
    /// suites that intentionally omit TTS must not be forced to download it.
    fn tts_suite_is_pinned(&self) -> bool {
        self.verifier
            .baseline_manifest()
            .keys()
            .any(|path| path.starts_with("tts/"))
    }

    /// All locally tracked assets for ready/revision cache invalidation.
    fn tracked_relative_paths(&self) -> Vec<&str> {
        let mut relative_paths: Vec<&str> = EXPECTED_FILES.to_vec();
        if self.tts_suite_is_pinned() {
            relative_paths.extend(EXPECTED_TTS_FILES.iter().copied());
        }
        relative_paths
    }

    /// Metadata stamps cover every pinned file, including TTS, so a TTS
    /// checksum failure is not hidden by an unchanged ASR cache entry.
    fn file_stamps(&self) -> std::io::Result<Vec<(u64, SystemTime)>> {
        self.tracked_relative_paths()
            .into_iter()
            .map(|rel| {
                let metadata = std::fs::metadata(self.models_dir.join(rel))?;
                Ok((metadata.len(), metadata.modified()?))
            })
            .collect()
    }

    /// Direct TTS layout + SHA check used by install admission. Unlike
    /// `refresh_integrity`, this deliberately ignores the metadata cache.
    async fn tts_integrity_ready(&self) -> bool {
        !self.tts_suite_is_pinned()
            || (inspect_tts_files(&self.models_dir).0
                && self.verifier.verify_tts(&self.models_dir).await)
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
            let cache_matches = self.checked_files.read().await.as_ref() == Some(&stamps);
            if cache_matches && (current.is_ready() || current.is_failed()) {
                return current;
            }
        }
        self.verify_files().await
    }

    async fn verify_files(&self) -> ModelInstallState {
        let before = self.file_stamps().ok();
        let mut verified = self.verifier.verify(&self.models_dir).await;
        if verified.is_ready() && self.tts_suite_is_pinned() {
            let (tts_layout_ready, _, _) = inspect_tts_files(&self.models_dir);
            if !tts_layout_ready {
                verified = ModelInstallState::NotInstalled;
            } else if !self.verifier.verify_tts(&self.models_dir).await {
                verified = ModelInstallState::Failed {
                    reason: "Offline TTS checksum verification failed; reinstall or repair the voice suite"
                        .into(),
                    recoverable: true,
                };
            }
        }
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
        let rebuild_default_kws = req.as_ref().and_then(|r| r.rebuild_default_kws).unwrap_or(false);

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

        // Admission and verification share the directory lease with every writer.
        let lease = DirectoryWriteLease::acquire(&self.models_dir)?;
        // A persisted confirmation is followed by ordinary retries; an explicit
        // request publishes its target before the first model write.
        let persisted_rebuild = crate::intent::confirmed_default_rebuild(&self.models_dir)
            .map_err(ModelManagerError::Conflict)?;
        if rebuild_default_kws {
            crate::intent::persist_default_rebuild(&self.models_dir)
                .map_err(ModelManagerError::Conflict)?;
        }
        let confirmed_rebuild = rebuild_default_kws || persisted_rebuild;
        if !force
            && !confirmed_rebuild
            && self.verify_files().await.is_ready()
            && self.tts_integrity_ready().await
        {
            return Ok(VoiceModelInstallResponse {
                accepted: false,
                state: self.state.read().await.clone(),
                message: Some("Models already installed and ready".into()),
            });
        }

        // 4. Mark state as ProbingSource and broadcast event
        *self.checked_files.write().await = None;
        self.set_state(ModelInstallState::ProbingSource).await;

        // 5. Spawn background install task; it owns the lease until completion.
        let this = self.clone();
        let handle = tokio::spawn(async move {
            let _lease = lease;
            this.run_install_pipeline(force, confirmed_rebuild).await;
        });

        *task_guard = Some(handle);

        Ok(VoiceModelInstallResponse {
            accepted: true,
            state: ModelInstallState::ProbingSource,
            message: Some("Installation started".into()),
        })
    }

    /// Internal asynchronous install pipeline execution.
    async fn run_install_pipeline(&self, force: bool, rebuild_default_kws: bool) {
        info!("Starting voice model installation pipeline (force={force})");

        // Recover interrupted target->backup moves before KWS group inference.
        for file in &crate::verifier::embedded_manifest().files {
            let target = self.models_dir.join(&file.path);
            let [_, _, backup] = crate::downloader::transaction_paths(&target, &file.sha256, file.size);
            if !target.exists() && backup.exists() {
                if let Err(error) = tokio::fs::rename(&backup, &target).await {
                    self.set_state(ModelInstallState::Failed { reason: format!("recovery: {}: {error}", file.path), recoverable: true }).await;
                    return;
                }
            }
        }
        for group in &crate::verifier::embedded_manifest().kws_groups {
            for (relative, hash) in &group.files {
                let target = self.models_dir.join(relative);
                let size = crate::verifier::embedded_manifest().files.iter().find(|f| f.path == *relative && f.sha256 == *hash).map(|f| f.size).unwrap_or(0);
                let [_, _, backup] = crate::downloader::transaction_paths(&target, hash, size);
                if !target.exists() && backup.exists() {
                    // Legacy artifacts are recovered only when their full hash proves identity.
                    if crate::verifier::compute_file_sha256(&backup).await.ok().as_ref() != Some(hash) {
                        self.set_state(ModelInstallState::Failed { reason: format!("recovery: {relative}: legacy backup identity mismatch"), recoverable: true }).await;
                        return;
                    }
                    if let Err(error) = tokio::fs::rename(&backup, &target).await {
                        self.set_state(ModelInstallState::Failed { reason: format!("recovery: {relative}: {error}"), recoverable: true }).await;
                        return;
                    }
                }
            }
        }
        // Existing complete supported KWS groups are retained. A partial group is
        // repaired only when the present files identify exactly one target; a
        // persisted confirmation overrides ambiguity but is still validated
        // against the embedded default identity.
        let mut kws_actual = HashMap::new();
        for path in DOWNLOAD_FILES.iter().filter(|p| p.starts_with("kws/")) {
            if let Ok(hash) = crate::verifier::compute_file_sha256(&self.models_dir.join(path)).await {
                kws_actual.insert((*path).to_string(), hash);
            }
        }
        let baseline = self.verifier.baseline_manifest();
        let marker = tokio::fs::read_to_string(self.models_dir.join("kws/.voice-suite-group")).await.unwrap_or_default();
        let target_group = if rebuild_default_kws {
            crate::verifier::default_kws_group()
        } else {
            match crate::verifier::infer_kws_group(&kws_actual, baseline, &marker) {
                Ok(group) => group,
                Err(reason) => {
                    self.set_state(ModelInstallState::Failed { reason, recoverable: true }).await;
                    return;
                }
            }
        };
        // A complete legacy group may be kept, but a damaged member without a
        // pinned repair source must block before any other resource changes.
        for (relative, expected) in &target_group.files {
            let pinned = crate::verifier::embedded_manifest()
                .files
                .iter()
                .any(|file| file.path == *relative && file.sha256 == *expected);
            if pinned {
                continue;
            }
            let target = self.models_dir.join(relative);
            let valid = crate::verifier::compute_file_sha256(&target)
                .await
                .ok()
                .is_some_and(|hash| hash.eq_ignore_ascii_case(expected));
            if !valid {
                self.set_state(ModelInstallState::Failed {
                    reason: format!("configuration: {relative}: selected KWS group has no verified repair source"),
                    recoverable: true,
                })
                .await;
                return;
            }
        }
        let kws_identities: HashMap<String, (String, Option<u64>)> = target_group
            .files
            .iter()
            .map(|(relative, expected)| {
                let pinned = crate::verifier::embedded_manifest()
                    .files
                    .iter()
                    .find(|file| file.path == *relative && file.sha256 == *expected);
                let identity = match pinned {
                    Some(file) => {
                        // The manager baseline is authoritative for a pinned path;
                        // the intent document never supplies verification hashes.
                        let hash = baseline
                            .get(relative)
                            .cloned()
                            .unwrap_or_else(|| expected.clone());
                        let size = if hash.eq_ignore_ascii_case(&file.sha256) {
                            Some(file.size)
                        } else {
                            None
                        };
                        (hash, size)
                    }
                    None => (expected.clone(), None),
                };
                (relative.clone(), identity)
            })
            .collect();
        // Step 2: Download files with progress tracking
        let all_files: Vec<&str> = if self.tts_suite_is_pinned() {
            DOWNLOAD_FILES
                .iter()
                .chain(crate::verifier::DOWNLOAD_TTS_FILES.iter())
                .copied()
                .collect()
        } else {
            DOWNLOAD_FILES.to_vec()
        };
        let total_files = all_files.len();
        let mut overall_downloaded: u64 = 0;
        let mut total_estimated_bytes: u64 = 0;

        // Estimate size if possible or use cumulative progress
        for (idx, &rel_path) in all_files.iter().enumerate() {
            let file_index = idx + 1;
            let target_path = self.models_dir.join(rel_path);

            if rel_path.starts_with("kws/") {
                let Some((hash, size)) = kws_identities.get(rel_path) else {
                    self.set_state(ModelInstallState::Failed {
                        reason: format!("manifest: missing identity for {rel_path}"),
                        recoverable: false,
                    })
                    .await;
                    return;
                };
                if let Err(error) = self
                    .downloader
                    .download_verified(rel_path, &target_path, hash, *size)
                    .await
                {
                    self.set_state(ModelInstallState::Failed { reason: format!("recovery: {rel_path}: {error}"), recoverable: true }).await;
                    return;
                }
                continue;
            }
            let Some(expected_hash) = baseline.get(rel_path) else {
                self.set_state(ModelInstallState::Failed { reason: format!("manifest: missing identity for {rel_path}"), recoverable: false }).await;
                return;
            };
            // download_verified re-hashes even force requests and recovers owned
            // transaction artifacts before deciding whether any GET is needed.
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
                .download_verified(rel_path, &target_path, expected_hash, if baseline.get(rel_path) == crate::verifier::embedded_manifest().files.iter().find(|f| f.path == rel_path).map(|f| &f.sha256) { crate::verifier::pinned_size(rel_path) } else { None })
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

        let checksums_path = self.models_dir.join("checksums.sha256");
        if !checksums_path.exists() {
            let content = baseline.iter().map(|(p,h)| format!("{h}  {p}\n")).collect::<String>();
            if let Err(error) = tokio::fs::write(&checksums_path, content).await {
                self.set_state(ModelInstallState::Failed { reason: format!("metadata: checksums.sha256: {error}"), recoverable: true }).await;
                return;
            }
        }
        let keywords_path = self.models_dir.join("kws/keywords.txt");
        if !keywords_path.exists() {
            const DEFAULT: &str = "▁HE Y ▁F L OW Y :1.5 #0.15 @1\n";
            if let Err(error) = tokio::fs::write(&keywords_path, DEFAULT).await {
                self.set_state(ModelInstallState::Failed { reason: format!("configuration: kws/keywords.txt: {error}"), recoverable: true }).await;
                return;
            }
        }

        // Step 3: SHA256 Verification
        for (idx, &rel_path) in all_files.iter().enumerate() {
            self.set_state(ModelInstallState::VerifyingSha256 {
                current_file: rel_path.to_string(),
                verified_files: idx,
                total_files,
            })
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        let verified = self.verify_files().await;
        if verified.is_ready() {
            // Clearing the confirmation is allowed only after the complete target
            // group re-hashes successfully; otherwise a retry must keep following it.
            for (relative, (hash, size)) in &kws_identities {
                let target = self.models_dir.join(relative);
                let size_matches = match size {
                    Some(size) => tokio::fs::metadata(&target).await.map(|meta| meta.len()).ok() == Some(*size),
                    None => true,
                };
                let hash_matches = crate::verifier::compute_file_sha256(&target)
                    .await
                    .ok()
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(hash));
                if !size_matches || !hash_matches {
                    self.set_state(ModelInstallState::Failed {
                        reason: format!("verify: {relative}: KWS group is incomplete; rebuild confirmation retained"),
                        recoverable: true,
                    })
                    .await;
                    return;
                }
            }
            let group = target_group.group_id.as_str();
            let marker_path = self.models_dir.join("kws/.voice-suite-group");
            if marker.trim() != group {
                if let Err(error) = tokio::fs::write(&marker_path, format!("{group}\n")).await {
                    self.set_state(ModelInstallState::Failed { reason: format!("configuration: KWS group marker: {error}"), recoverable: true }).await;
                    return;
                }
            }
            if let Err(error) = crate::intent::clear(&self.models_dir) {
                self.set_state(ModelInstallState::Failed { reason: error, recoverable: true }).await;
            }
        }
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
        for (rel, hash) in self.verifier.baseline_manifest() {
            let target = self.models_dir.join(rel);
            let size = crate::verifier::embedded_manifest().files.iter()
                .find(|f| f.path == *rel && f.sha256 == *hash).map(|f| f.size).unwrap_or(0);
            targets.extend(crate::downloader::transaction_paths(&target, hash, size));
        }
        for group in &crate::verifier::embedded_manifest().kws_groups {
            for (relative, hash) in &group.files {
                let size = crate::verifier::embedded_manifest().files.iter().find(|f| f.path == *relative && f.sha256 == *hash).map(|f| f.size).unwrap_or(0);
                targets.extend(crate::downloader::transaction_paths(&self.models_dir.join(relative), hash, size));
            }
        }
        targets.push(self.models_dir.join("checksums.sha256"));
        targets.push(self.models_dir.join("kws/.voice-suite-group"));
        targets.push(self.models_dir.join(crate::intent::REBUILD_INTENT_REL));
        targets.push(self.models_dir.join(crate::intent::REBUILD_INTENT_TMP_REL));

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

    #[tokio::test]
    async fn status_fails_closed_while_external_writer_holds_directory() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(dir.path());
        manager.set_state(ModelInstallState::Ready { models_dir: dir.path().to_path_buf(), total_size_bytes: 0 }).await;
        let lease = DirectoryWriteLease::acquire(dir.path()).unwrap();
        assert!(manager.get_status().await.state.is_failed());
        assert!(manager.wait_for_completion().await.is_failed());
        drop(lease);
        assert_eq!(manager.get_status().await.state, ModelInstallState::NotInstalled);
    }

    #[tokio::test]
    async fn completion_waits_past_ready_until_lease_release() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(dir.path());
        let lease = DirectoryWriteLease::acquire(dir.path()).unwrap();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let writer = manager.clone();
        *manager.install_task.lock().await = Some(tokio::spawn(async move {
            let _lease = lease;
            writer.set_state(ModelInstallState::Ready { models_dir: writer.models_dir.clone(), total_size_bytes: 0 }).await;
            let _ = release_rx.await;
        }));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(80), manager.wait_for_completion()).await.is_err());
        release_tx.send(()).unwrap();
        assert!(manager.wait_for_completion().await.is_ready());
        assert!(DirectoryWriteLease::acquire(dir.path()).is_ok());
    }

    #[tokio::test]
    async fn panic_and_remove_cancellation_do_not_leave_busy_completion() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(dir.path());
        manager.set_state(ModelInstallState::ProbingSource).await;
        *manager.install_task.lock().await = Some(tokio::spawn(async { panic!("injected writer panic") }));
        assert!(manager.wait_for_completion().await.is_failed());
        assert!(!manager.get_status().await.state.is_busy());
        manager.set_state(ModelInstallState::ProbingSource).await;
        *manager.install_task.lock().await = Some(tokio::spawn(std::future::pending()));
        let waiter = manager.clone();
        let waiting = tokio::spawn(async move { waiter.wait_for_completion().await });
        manager.remove().await.unwrap();
        assert_eq!(waiting.await.unwrap(), ModelInstallState::NotInstalled);
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

#[cfg(test)]
mod tts_completeness_tests {
    use super::*;
    use crate::layout::{EXPECTED_FILES, EXPECTED_TTS_FILES};

    fn write_stub(root: &Path, rel: &str) {
        let full = root.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, vec![0u8; 16]).unwrap();
    }

    fn write_suite_fixture(
        root: &Path,
        manifest: &mut std::collections::HashMap<String, String>,
    ) {
        use crate::verifier::{DOWNLOAD_FILES, DOWNLOAD_TTS_FILES, compute_sha256};
        for &relative in EXPECTED_FILES {
            let bytes = format!("asr-fixture:{relative}").into_bytes();
            let full = root.join(relative);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, &bytes).unwrap();
            if DOWNLOAD_FILES.contains(&relative) {
                manifest.insert(relative.to_owned(), compute_sha256(&bytes));
            }
        }
        for &relative in EXPECTED_TTS_FILES {
            let bytes = format!("tts-fixture:{relative}").into_bytes();
            let full = root.join(relative);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, &bytes).unwrap();
            if DOWNLOAD_TTS_FILES.contains(&relative) {
                manifest.insert(relative.to_owned(), compute_sha256(&bytes));
            }
        }
    }

    async fn wait_until_not_busy(manager: &ModelManager) {
        for _ in 0..200 {
            if !manager.get_status().await.state.is_busy() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("install pipeline did not settle");
    }

    #[tokio::test]
    async fn status_keeps_the_busy_install_state_when_tts_is_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = std::collections::HashMap::new();
        write_suite_fixture(dir.path(), &mut manifest);
        for &relative in EXPECTED_TTS_FILES {
            std::fs::remove_file(dir.path().join(relative)).unwrap();
        }
        let manager = ModelManager::with_config(dir.path(), Vec::new(), manifest);
        manager.set_state(ModelInstallState::ProbingSource).await;
        assert_eq!(
            manager.get_status().await.state,
            ModelInstallState::ProbingSource,
            "a running install must not be rewritten to NotInstalled while TTS is incomplete"
        );
    }

    #[tokio::test]
    async fn wait_until_settled_returns_the_terminal_install_state() {
        let dir = tempfile::tempdir().unwrap();
        let manager = std::sync::Arc::new(ModelManager::new(dir.path()));
        manager.set_state(ModelInstallState::ProbingSource).await;
        let path = dir.path().to_path_buf();
        let finish = manager.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            finish
                .set_state(ModelInstallState::Ready {
                    models_dir: path,
                    total_size_bytes: 0,
                })
                .await;
        });
        let state = manager
            .wait_until_settled(std::time::Duration::from_secs(1))
            .await;
        assert!(state.is_ready(), "expected terminal Ready, got {state:?}");
    }

    #[tokio::test]
    async fn missing_tts_clears_the_ready_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = std::collections::HashMap::new();
        write_suite_fixture(dir.path(), &mut manifest);
        let manager = ModelManager::with_config(dir.path(), Vec::new(), manifest);
        assert!(manager.get_status().await.state.is_ready());
        assert!(manager.checked_files.read().await.is_some());
        std::fs::remove_file(
            dir.path()
                .join("tts/matcha-icefall-en_US-ljspeech/tokens.txt"),
        )
        .unwrap();
        assert_eq!(
            manager.get_status().await.state,
            ModelInstallState::NotInstalled
        );
        assert!(
            manager.checked_files.read().await.is_none(),
            "a missing TTS file must invalidate the previous Ready metadata cache"
        );
    }

    #[tokio::test]
    async fn install_rehashes_tts_even_when_the_ready_cache_is_warm() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = std::collections::HashMap::new();
        write_suite_fixture(dir.path(), &mut manifest);
        let manager = ModelManager::with_config(dir.path(), Vec::new(), manifest);
        assert!(manager.get_status().await.state.is_ready());

        let path = dir
            .path()
            .join("tts/matcha-icefall-en_US-ljspeech/tokens.txt");
        let original = std::fs::metadata(&path).unwrap();
        let modified = original.modified().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] ^= 0xff;
        assert_eq!(bytes.len() as u64, original.len());
        std::fs::write(&path, &bytes).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(
            manager.get_status().await.state.is_ready(),
            "the test must reach install through the cached Ready path"
        );

        let response = manager.install(None).await.unwrap();
        assert!(
            response.accepted,
            "install must directly re-hash TTS instead of trusting the metadata cache"
        );
        wait_until_not_busy(&manager).await;
    }

    #[tokio::test]
    async fn a_partial_tts_suite_is_actually_repaired_by_install() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = std::collections::HashMap::new();
        write_suite_fixture(dir.path(), &mut manifest);
        for &relative in EXPECTED_TTS_FILES {
            std::fs::remove_file(dir.path().join(relative)).unwrap();
        }
        let manager = ModelManager::with_config(dir.path(), Vec::new(), manifest);
        assert_eq!(
            manager.get_status().await.state,
            ModelInstallState::NotInstalled,
            "missing TTS files must surface as installable, not ready"
        );
        let response = manager.install(None).await.unwrap();
        assert!(
            response.accepted,
            "install must not short-circuit a partial TTS suite: {response:?}"
        );
        wait_until_not_busy(&manager).await;
    }

    #[tokio::test]
    async fn corrupt_tts_is_not_reported_ready_and_can_be_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = std::collections::HashMap::new();
        write_suite_fixture(dir.path(), &mut manifest);
        let manager = ModelManager::with_config(dir.path(), Vec::new(), manifest);
        assert!(manager.get_status().await.state.is_ready());
        assert!(!manager.install(None).await.unwrap().accepted);

        std::fs::write(
            dir.path()
                .join("tts/matcha-icefall-en_US-ljspeech/tokens.txt"),
            b"corrupted",
        )
        .unwrap();
        let status = manager.get_status().await;
        assert!(
            matches!(
                status.state,
                ModelInstallState::Failed {
                    recoverable: true,
                    ..
                }
            ),
            "a corrupt TTS hash must fail readiness, got {:?}",
            status.state
        );
        assert!(!manager.is_tts_ready().await);
        let response = manager.install(None).await.unwrap();
        assert!(
            response.accepted,
            "a failed TTS integrity check must be repairable by install"
        );
        wait_until_not_busy(&manager).await;
    }

    #[tokio::test]
    async fn base_models_without_the_tts_suite_report_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        for rel in EXPECTED_FILES {
            write_stub(dir.path(), rel);
        }
        for rel in EXPECTED_TTS_FILES {
            write_stub(dir.path(), rel);
        }
        // Remove only the espeak voice definitions that the stale downloader
        // used to omit; the suite must no longer look installable.
        for rel in [
            "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en",
            "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en-US",
        ] {
            std::fs::remove_file(dir.path().join(rel)).unwrap();
        }

        let manager = ModelManager::new(dir.path());
        let status = manager.get_status().await;
        assert_eq!(
            status.state,
            ModelInstallState::NotInstalled,
            "a partial TTS suite must be reported as not installed, got {:?}",
            status.state
        );
        let missing = status
            .files
            .iter()
            .find(|file| file.path.ends_with("espeak-ng-data/lang/gmw/en"))
            .expect("voice definition detail present");
        assert!(!missing.exists);
    }
}

