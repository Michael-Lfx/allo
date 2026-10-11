//! The local model runtime service: install, start, stop, uninstall, status.
//!
//! State lives in memory; only the small install marker, the per-model provider
//! id/api key and the last successful launch options are persisted to
//! `{data_dir}/local-models/state.json`. Processes are supervised through
//! `nomi-process-runtime` (the same boundary the terminal uses) and die with
//! the host — a restart comes back as "installed, not running".

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nomifun_api_types::{
    LocalModelDownloadError, LocalModelLaunchOptions, LocalModelRunStatus, LocalModelSource,
    LocalModelStatus, LocalModelText, LocalModelsResponse,
};
use nomi_process_runtime::{
    CapabilityPolicy, CommandSpec, OutputCursor, PollResult, ProcessOwner, ProcessPolicy,
    ProcessRequest, ProcessSupervisor, SupervisorConfig, Transport, normalize_request,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

use crate::download::{
    FileDownload, download_client, download_file, extract_tar_gz, extract_zip, file_matches,
    file_size_matches, sha256_file, sha256_file_cancellable,
};
use crate::error::{LocalModelError, LocalModelResult};
use crate::houmo::{self, HoumoCache, HoumoModel};
use crate::launch::{build_args, build_houmo_args, default_launch_options, normalize_options};
use crate::manifest::{
    HOUMO_RUNTIME_ARCHIVE, HOUMO_RUNTIME_DIR, HOUMO_RUNTIME_SHA256, HOUMO_RUNTIME_URL,
    HOUMO_RUNTIME_VERSION, ModelManifest, RUNTIME_SHA256, RUNTIME_URL, RUNTIME_VERSION, RuntimeKind,
    find_model, runtime_kind,
};
use crate::progress::DownloadProgress;
use crate::publisher::{LocalModelProviderPublisher, LocalModelProviderSpec};

const REL_DIR: &str = "local-models";
const RUNTIME_DIR: &str = "runtimes/llama.cpp/standard";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(600);
const HEALTH_INTERVAL: Duration = Duration::from_millis(500);
/// Global ceiling on simultaneous download connections across all models.
const MAX_DOWNLOAD_CONNECTIONS: usize = 8;

#[derive(Clone)]
struct ProcessRef {
    owner: ProcessOwner,
    session_id: nomi_process_runtime::SessionId,
}

struct ModelRuntime {
    status: LocalModelRunStatus,
    progress: Arc<DownloadProgress>,
    port: Option<u16>,
    cancel: Option<Arc<AtomicBool>>,
    process: Option<ProcessRef>,
    download_error: Option<LocalModelDownloadError>,
}

impl Default for ModelRuntime {
    fn default() -> Self {
        Self {
            status: LocalModelRunStatus::Uninstalled,
            progress: Arc::new(DownloadProgress::new(0)),
            port: None,
            cancel: None,
            process: None,
            download_error: None,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PersistedState {
    #[serde(default)]
    models: HashMap<String, PersistedModel>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct PersistedModel {
    #[serde(default)]
    installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    launch_options: Option<LocalModelLaunchOptions>,
}

/// The process-wide local model runtime.
pub struct LocalModelsService {
    root: PathBuf,
    publisher: Option<Arc<dyn LocalModelProviderPublisher>>,
    client: reqwest::Client,
    download_semaphore: Arc<Semaphore>,
    supervisor: Arc<ProcessSupervisor>,
    runtimes: Mutex<HashMap<String, ModelRuntime>>,
    persisted: Mutex<PersistedState>,
    runtime_lock: Mutex<()>,
    houmo: Mutex<HoumoCache>,
    houmo_refresh: Mutex<()>,
}

impl LocalModelsService {
    /// Build the service rooted at `{data_dir}/local-models`.
    pub fn new(data_dir: &Path, publisher: Option<Arc<dyn LocalModelProviderPublisher>>) -> Arc<Self> {
        let root = data_dir.join(REL_DIR);
        let persisted = load_persisted(&root);
        let mut runtimes: HashMap<String, ModelRuntime> = HashMap::new();
        for model in crate::manifest::MODELS {
            let record = persisted.models.get(model.id);
            let installed = record.map(|r| r.installed).unwrap_or(false);
            let mut runtime = ModelRuntime::default();
            if installed && model_files_present(model, &root) {
                runtime.status = LocalModelRunStatus::Installed;
                runtime.progress = Arc::new(DownloadProgress::completed(model_total(model)));
            }
            runtimes.insert(model.id.to_owned(), runtime);
        }
        Arc::new(Self {
            root,
            publisher,
            client: download_client(),
            download_semaphore: Arc::new(Semaphore::new(MAX_DOWNLOAD_CONNECTIONS)),
            supervisor: ProcessSupervisor::new(SupervisorConfig {
                max_sessions: 8,
                ..Default::default()
            }),
            runtimes: Mutex::new(runtimes),
            persisted: Mutex::new(persisted),
            runtime_lock: Mutex::new(()),
            houmo: Mutex::new(HoumoCache::default()),
            houmo_refresh: Mutex::new(()),
        })
    }

    fn model_dir(root: &Path, id: &str) -> PathBuf {
        root.join("models").join(id)
    }

    fn runtime_dir(&self) -> PathBuf {
        self.root.join(RUNTIME_DIR)
    }

    fn houmo_runtime_dir(&self) -> PathBuf {
        self.root.join(HOUMO_RUNTIME_DIR)
    }

    fn llama_server(&self) -> PathBuf {
        self.runtime_dir().join("llama-server.exe")
    }

    /// Snapshot every catalog entry with its live state.
    ///
    /// Builtin models are always included. The Houmo partition is the remote
    /// catalog `model-cli query` reads, cached so status polling does not
    /// refetch it.
    pub async fn list(&self) -> LocalModelsResponse {
        let (houmo_models, houmo_error) = self.houmo_snapshot().await;
        let runtimes = self.runtimes.lock().await;
        let persisted = self.persisted.lock().await;
        let mut models: Vec<LocalModelStatus> = crate::manifest::MODELS
            .iter()
            .map(|model| status_for(model, &runtimes, &persisted, &self.root))
            .collect();
        for model in &houmo_models {
            models.push(houmo_status(model, &runtimes, &persisted, &self.root));
        }
        LocalModelsResponse {
            models,
            houmo_error,
        }
    }

    /// Fetch a single entry's status.
    pub async fn status(&self, id: &str) -> LocalModelResult<LocalModelStatus> {
        if let Some(model) = find_model(id) {
            let runtimes = self.runtimes.lock().await;
            let persisted = self.persisted.lock().await;
            return Ok(status_for(model, &runtimes, &persisted, &self.root));
        }
        if houmo::is_houmo_id(id) {
            let (models, _) = self.houmo_snapshot().await;
            if let Some(model) = models.iter().find(|model| model.id == id) {
                let runtimes = self.runtimes.lock().await;
                let persisted = self.persisted.lock().await;
                return Ok(houmo_status(model, &runtimes, &persisted, &self.root));
            }
        }
        Err(LocalModelError::UnknownModel(id.to_owned()))
    }

    /// Kick off (or resume) the model file download.
    pub async fn install(self: &Arc<Self>, id: &str) -> LocalModelResult<LocalModelStatus> {
        if matches!(runtime_kind(id), RuntimeKind::Houmo) {
            return self.install_houmo(id).await;
        }
        let model = find_model(id).ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id) {
                if runtime.status == LocalModelRunStatus::Downloading {
                    return Err(LocalModelError::Busy(format!("{id} is already downloading")));
                }
                if runtime.status == LocalModelRunStatus::Starting
                    || runtime.status == LocalModelRunStatus::Running
                {
                    return Err(LocalModelError::Busy(format!("{id} is running")));
                }
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(DownloadProgress::new(model_total(model)));
        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Downloading;
            runtime.progress = Arc::clone(&progress);
            runtime.cancel = Some(cancel.clone());
            runtime.download_error = None;
        }
        let service = self.clone();
        let id_owned = id.to_owned();
        tokio::spawn(async move {
            service
                .install_worker(model, &id_owned, cancel, progress)
                .await;
        });
        self.status(id).await
    }

    async fn install_worker(
        self: Arc<Self>,
        model: &'static ModelManifest,
        id: &str,
        cancel: Arc<AtomicBool>,
        progress: Arc<DownloadProgress>,
    ) {
        let dir = Self::model_dir(&self.root, id);
        let files = planned_from_manifest(model);
        let result = self.download_files(&files, &dir, &cancel, &progress).await;

        let mut runtimes = self.runtimes.lock().await;
        let runtime = runtimes.entry(id.to_owned()).or_default();
        runtime.cancel = None;
        match result {
            Ok(()) => {
                runtime.status = LocalModelRunStatus::Installed;
                runtime.progress = Arc::new(DownloadProgress::completed(model_total(model)));
                runtime.download_error = None;
                drop(runtimes);
                let mut persisted = self.persisted.lock().await;
                persisted.models.entry(id.to_owned()).or_default().installed = true;
                persist(&self.root, &persisted);
            }
            Err(LocalModelError::Cancelled) => {
                runtime.status = LocalModelRunStatus::Uninstalled;
                runtime.progress = Arc::new(DownloadProgress::new(0));
                runtime.download_error = None;
            }
            Err(error) => {
                tracing::warn!(model = id, %error, "local model install failed");
                runtime.status = LocalModelRunStatus::Uninstalled;
                runtime.progress = Arc::new(DownloadProgress::new(0));
                runtime.download_error = error.download_footprint();
            }
        }
    }

    /// Download every file, hashing each one in the background as soon as its
    /// bytes land so the next file starts without waiting.
    ///
    /// A finished file's SHA-256 runs in a spawned task; the loop moves straight
    /// on to the next file, so the card keeps showing progress and speed while a
    /// large file is verified. When any file fails its checksum the flag is
    /// raised, remaining downloads stop, the bad file is deleted, and the
    /// existing checksum error is returned. Files that already match skip both
    /// the download and a second verification.
    async fn download_files(
        &self,
        files: &[PlannedFile],
        dir: &Path,
        cancel: &Arc<AtomicBool>,
        progress: &Arc<DownloadProgress>,
    ) -> LocalModelResult<()> {
        tokio::fs::create_dir_all(dir).await?;
        let mut completed: u64 = 0;
        let mut verifying: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let verify_failure: Arc<Mutex<Option<LocalModelError>>> = Arc::new(Mutex::new(None));
        let mut download_failure: Option<LocalModelError> = None;

        for file in files {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let dest = dir.join(&file.file_name);
            if file_size_matches(&dest, file.file_size).await {
                // The file on disk is already the right size, so it is about to
                // be re-hashed to confirm it. Surface that as verifying instead
                // of a stalled connection, then decide skip vs. re-download.
                let hashing = file.sha256.is_some();
                if hashing {
                    progress.begin_verify();
                }
                let matches = file_matches(&dest, file.file_size, file.sha256.as_deref()).await;
                if hashing {
                    progress.end_verify();
                }
                if matches {
                    completed += accounted_size(file.file_size, &dest).await;
                    progress.reset(completed, Instant::now());
                    continue;
                }
            }

            let urls: Vec<&str> = file.urls.iter().map(String::as_str).collect();
            progress.begin_download();
            let download_result = download_file(FileDownload {
                client: &self.client,
                urls: &urls,
                dest: &dest,
                file_size: file.file_size,
                cancel,
                progress,
                completed_before: completed,
                semaphore: Arc::clone(&self.download_semaphore),
            })
            .await;
            progress.end_download();
            if let Err(error) = download_result {
                download_failure = Some(error);
                break;
            }

            completed += accounted_size(file.file_size, &dest).await;
            if progress.total() < completed {
                progress.set_total(completed);
            }
            progress.reset(completed, Instant::now());

            if let Some(expected) = file.sha256.clone() {
                let file_name = file.file_name.clone();
                let dest = dest.clone();
                let cancel = Arc::clone(cancel);
                let progress = Arc::clone(progress);
                let failure = Arc::clone(&verify_failure);
                progress.begin_verify();
                verifying.push(tokio::spawn(async move {
                    match sha256_file_cancellable(&dest, &cancel).await {
                        Ok(actual) if actual.eq_ignore_ascii_case(&expected) => {}
                        Ok(actual) => {
                            let _ = tokio::fs::remove_file(&dest).await;
                            let mut slot = failure.lock().await;
                            if slot.is_none() {
                                *slot = Some(LocalModelError::Checksum {
                                    file: file_name,
                                    expected,
                                    actual,
                                });
                            }
                            cancel.store(true, Ordering::Relaxed);
                        }
                        // A cancelled read is not a checksum failure.
                        Err(LocalModelError::Cancelled) => {}
                        // A read failure is a real verification failure.
                        Err(error) => {
                            let mut slot = failure.lock().await;
                            if slot.is_none() {
                                *slot = Some(error);
                            }
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    progress.end_verify();
                }));
            }
        }

        // Collect every in-flight verification before judging the install.
        for handle in verifying {
            let _ = handle.await;
        }

        // A checksum failure wins over the cancellation it triggered.
        if let Some(failure) = verify_failure.lock().await.take() {
            cancel.store(true, Ordering::Relaxed);
            return Err(failure);
        }
        if let Some(error) = download_failure {
            return Err(error);
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(LocalModelError::Cancelled);
        }
        Ok(())
    }

    /// Ensure `llama-server.exe` is present, downloading and extracting the
    /// runtime zip on first use.
    async fn ensure_runtime(&self) -> LocalModelResult<()> {
        let _guard = self.runtime_lock.lock().await;
        if self.llama_server().is_file() {
            return Ok(());
        }
        let dir = self.runtime_dir();
        tokio::fs::create_dir_all(&dir).await?;
        tracing::info!(runtime = RUNTIME_VERSION, "downloading llama.cpp runtime");
        let zip_path = dir.join("llama-runtime.zip");
        let cancel = AtomicBool::new(false);
        let progress = DownloadProgress::new(0);
        let urls = [RUNTIME_URL];
        download_file(FileDownload {
            client: &self.client,
            urls: &urls,
            dest: &zip_path,
            file_size: 0,
            cancel: &cancel,
            progress: &progress,
            completed_before: 0,
            semaphore: Arc::clone(&self.download_semaphore),
        })
        .await?;
        let digest = sha256_file(&zip_path).await?;
        if !digest.eq_ignore_ascii_case(RUNTIME_SHA256) {
            let _ = tokio::fs::remove_file(&zip_path).await;
            return Err(LocalModelError::Checksum {
                file: "llama-runtime.zip".to_owned(),
                expected: RUNTIME_SHA256.to_owned(),
                actual: digest,
            });
        }
        let zip_for_extract = zip_path.clone();
        let dir_for_extract = dir.clone();
        tokio::task::spawn_blocking(move || extract_zip(&zip_for_extract, &dir_for_extract))
            .await
            .map_err(|error| LocalModelError::Internal(error.to_string()))??;
        let _ = tokio::fs::remove_file(&zip_path).await;
        if !self.llama_server().is_file() {
            return Err(LocalModelError::Launch(
                "runtime archive did not contain llama-server.exe".to_owned(),
            ));
        }
        Ok(())
    }

    /// Ensure the linux aarch64 Houmo `HLIELLama` xh2 runtime is extracted:
    /// download the tar.gz, verify its SHA-256, extract it, and confirm
    /// `houmo-HLIELLama-xh2/bin/llama-server` exists.
    #[cfg_attr(
        not(all(target_os = "linux", target_arch = "aarch64")),
        allow(dead_code)
    )]
    async fn ensure_houmo_runtime(&self) -> LocalModelResult<()> {
        let _guard = self.runtime_lock.lock().await;
        let dir = self.houmo_runtime_dir();
        let server = houmo::package_root(&dir).join("bin").join("llama-server");
        if server.is_file() {
            return Ok(());
        }
        tokio::fs::create_dir_all(&dir).await?;
        tracing::info!(
            runtime = HOUMO_RUNTIME_VERSION,
            "downloading Houmo HLIELLama runtime"
        );
        let archive = dir.join(HOUMO_RUNTIME_ARCHIVE);
        let cancel = AtomicBool::new(false);
        let progress = DownloadProgress::new(0);
        let urls = [HOUMO_RUNTIME_URL];
        download_file(FileDownload {
            client: &self.client,
            urls: &urls,
            dest: &archive,
            file_size: 0,
            cancel: &cancel,
            progress: &progress,
            completed_before: 0,
            semaphore: Arc::clone(&self.download_semaphore),
        })
        .await?;
        let digest = sha256_file(&archive).await?;
        if !digest.eq_ignore_ascii_case(HOUMO_RUNTIME_SHA256) {
            let _ = tokio::fs::remove_file(&archive).await;
            return Err(LocalModelError::Checksum {
                file: HOUMO_RUNTIME_ARCHIVE.to_owned(),
                expected: HOUMO_RUNTIME_SHA256.to_owned(),
                actual: digest,
            });
        }
        let archive_for_extract = archive.clone();
        let dir_for_extract = dir.clone();
        tokio::task::spawn_blocking(move || extract_tar_gz(&archive_for_extract, &dir_for_extract))
            .await
            .map_err(|error| LocalModelError::Internal(error.to_string()))??;
        let _ = tokio::fs::remove_file(&archive).await;
        if !server.is_file() {
            return Err(LocalModelError::Launch(
                "Houmo runtime archive did not contain bin/llama-server".to_owned(),
            ));
        }
        Ok(())
    }

    /// Start a model, wait for health, and publish it as a chat provider.
    pub async fn start(
        self: &Arc<Self>,
        id: &str,
        requested: LocalModelLaunchOptions,
    ) -> LocalModelResult<LocalModelStatus> {
        if matches!(runtime_kind(id), RuntimeKind::Houmo) {
            return self.start_houmo(id, requested).await;
        }
        let model = find_model(id).ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id) {
                match runtime.status {
                    LocalModelRunStatus::Running => {
                        let persisted = self.persisted.lock().await;
                        return Ok(status_for(model, &runtimes, &persisted, &self.root));
                    }
                    LocalModelRunStatus::Downloading | LocalModelRunStatus::Starting => {
                        return Err(LocalModelError::Busy(format!("{id} is busy")));
                    }
                    _ => {}
                }
            }
        }
        self.ensure_installed(model).await?;
        self.ensure_runtime().await?;

        let options = normalize_options(model, requested);
        let port = pick_free_port()?;
        let api_key = format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple());
        let provider_id = {
            let mut persisted = self.persisted.lock().await;
            let record = persisted.models.entry(id.to_owned()).or_default();
            if record.provider_id.is_none() {
                record.provider_id = Some(Uuid::now_v7().to_string());
            }
            record.api_key = Some(api_key.clone());
            record.provider_id.clone().unwrap()
        };

        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Starting;
            runtime.port = Some(port);
        }

        let process = match self.spawn_server(model, id, &options, port, &api_key).await {
            Ok(process) => process,
            Err(error) => {
                self.set_not_running(id).await;
                return Err(error);
            }
        };
        {
            let mut runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get_mut(id) {
                runtime.process = Some(process.clone());
            }
        }

        if let Err(error) = self
            .wait_healthy(&process, port, &self.runtime_dir())
            .await
        {
            self.kill_process(&process).await;
            self.set_not_running(id).await;
            return Err(error);
        }

        if let Some(publisher) = &self.publisher {
            let spec = LocalModelProviderSpec {
                provider_id: provider_id.clone(),
                name: model.id.to_owned(),
                base_url: format!("http://127.0.0.1:{port}/v1"),
                api_key: api_key.clone(),
                model: model.id.to_owned(),
                context_size: options.context_size.unwrap_or(model.context_size),
            };
            if let Err(error) = publisher.publish(&spec).await {
                self.kill_process(&process).await;
                self.set_not_running(id).await;
                return Err(LocalModelError::Internal(format!(
                    "failed to publish provider: {error}"
                )));
            }
        }

        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Running;
            runtime.port = Some(port);
            runtime.progress = Arc::new(DownloadProgress::completed(model_total(model)));
        }
        {
            let mut persisted = self.persisted.lock().await;
            if let Some(record) = persisted.models.get_mut(id) {
                record.launch_options = Some(options.clone());
            }
            persist(&self.root, &persisted);
        }
        self.status(id).await
    }

    async fn ensure_installed(&self, model: &ModelManifest) -> LocalModelResult<()> {
        let record = {
            let persisted = self.persisted.lock().await;
            persisted.models.get(model.id).cloned()
        };
        if record.map(|r| r.installed).unwrap_or(false)
            && model_files_present(model, &self.root)
        {
            return Ok(());
        }
        Err(LocalModelError::NotInstalled(model.id.to_owned()))
    }

    async fn spawn_server(
        &self,
        model: &ModelManifest,
        id: &str,
        options: &LocalModelLaunchOptions,
        port: u16,
        api_key: &str,
    ) -> LocalModelResult<ProcessRef> {
        let model_dir = Self::model_dir(&self.root, id);
        let runtime_dir = self.runtime_dir();
        let args = build_args(model, options, &model_dir, port, api_key);
        let env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
            std::collections::BTreeMap::new();
        let request = ProcessRequest {
            owner: ProcessOwner::new(Uuid::now_v7(), Uuid::now_v7()),
            command: CommandSpec::Program {
                program: self.llama_server().into_os_string(),
                args: args.into_iter().map(Into::into).collect(),
            },
            cwd: runtime_dir.clone(),
            env,
            transport: Transport::Pipe,
            policy: ProcessPolicy {
                output_limit_bytes: 4 * 1024 * 1024,
                lease: Duration::from_secs(24 * 3600),
                expire_on_idle: false,
                deadline: None,
                ..Default::default()
            },
            capability: CapabilityPolicy {
                cwd_roots: vec![self.root.clone(), model_dir.clone(), runtime_dir],
                sandbox: nomi_process_runtime::SandboxPolicy::UnrestrictedLocalOwner,
            },
        };
        let normalized = normalize_request(request, &model_dir)
            .map_err(|error| LocalModelError::Launch(error.to_string()))?;
        let handle = self
            .supervisor
            .start(normalized)
            .await
            .map_err(|error| LocalModelError::Launch(error.to_string()))?;
        Ok(ProcessRef {
            owner: handle.owner,
            session_id: handle.session_id,
        })
    }

    async fn wait_healthy(
        &self,
        process: &ProcessRef,
        port: u16,
        _runtime_dir: &Path,
    ) -> LocalModelResult<()> {
        let url = format!("http://127.0.0.1:{port}/health");
        let started = Instant::now();
        loop {
            if started.elapsed() > HEALTH_TIMEOUT {
                return Err(LocalModelError::Launch(
                    "timed out waiting for llama-server health".to_owned(),
                ));
            }
            match self
                .supervisor
                .poll(
                    &process.owner,
                    &process.session_id,
                    OutputCursor::START,
                    Instant::now(),
                )
                .await
            {
                Ok(PollResult::Finished(_)) => {
                    return Err(LocalModelError::Launch(
                        "llama-server exited before becoming healthy".to_owned(),
                    ));
                }
                Ok(PollResult::Running { .. }) => {}
                Err(error) => {
                    return Err(LocalModelError::Launch(error.to_string()));
                }
            }
            match self.client.get(&url).timeout(Duration::from_secs(2)).send().await {
                Ok(response) if response.status().is_success() => return Ok(()),
                _ => tokio::time::sleep(HEALTH_INTERVAL).await,
            }
        }
    }

    async fn kill_process(&self, process: &ProcessRef) {
        if let Err(error) = self
            .supervisor
            .force_kill(&process.owner, &process.session_id)
            .await
        {
            tracing::warn!(%error, "failed to kill local model process");
        }
    }

    async fn set_not_running(&self, id: &str) {
        let mut runtimes = self.runtimes.lock().await;
        if let Some(runtime) = runtimes.get_mut(id) {
            runtime.status = LocalModelRunStatus::Installed;
            runtime.process = None;
            runtime.port = None;
        }
    }

    /// Stop a running model and disable its chat provider.
    pub async fn stop(&self, id: &str) -> LocalModelResult<LocalModelStatus> {
        if houmo::is_houmo_id(id) {
            if !self.houmo_known(id).await {
                return Err(LocalModelError::UnknownModel(id.to_owned()));
            }
            self.stop_process(id).await;
            return self.status(id).await;
        }
        let model = find_model(id).ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        self.stop_process(id).await;
        self.status(id).await.map_err(|_| LocalModelError::UnknownModel(model.id.to_owned()))
    }

    async fn stop_process(&self, id: &str) {
        let process = {
            let mut runtimes = self.runtimes.lock().await;
            runtimes.get_mut(id).and_then(|runtime| runtime.process.take())
        };
        if let Some(process) = process {
            self.kill_process(&process).await;
        }
        let provider_id = {
            let persisted = self.persisted.lock().await;
            persisted.models.get(id).and_then(|r| r.provider_id.clone())
        };
        if let (Some(publisher), Some(provider_id)) = (&self.publisher, provider_id)
            && let Err(error) = publisher.disable(&provider_id).await
        {
            tracing::warn!(%error, "failed to disable local model provider");
        }
        {
            let mut runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get_mut(id) {
                runtime.status = LocalModelRunStatus::Installed;
                runtime.process = None;
                runtime.port = None;
            }
        }
    }

    /// Stop (if needed), delete model files, then remove the chat provider.
    pub async fn uninstall(&self, id: &str) -> LocalModelResult<LocalModelStatus> {
        if houmo::is_houmo_id(id) {
            return self.uninstall_houmo(id).await;
        }
        let model = find_model(id).ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        let _ = self.stop(id).await;

        let dir = Self::model_dir(&self.root, id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }

        let provider_id = {
            let mut persisted = self.persisted.lock().await;
            let record = persisted.models.entry(id.to_owned()).or_default();
            let provider_id = record.provider_id.clone();
            record.installed = false;
            record.launch_options = None;
            provider_id
        };
        if let (Some(publisher), Some(provider_id)) = (&self.publisher, provider_id)
            && let Ok(outcome) = publisher.remove(&provider_id).await
        {
            tracing::debug!(?outcome, "local model provider removed");
        }
        {
            let mut persisted = self.persisted.lock().await;
            if let Some(record) = persisted.models.get_mut(id) {
                record.provider_id = None;
                record.api_key = None;
            }
            persist(&self.root, &persisted);
        }
        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            *runtime = ModelRuntime::default();
        }
        self.status(id).await.map_err(|_| LocalModelError::UnknownModel(model.id.to_owned()))
    }

    /// Cancel an in-flight download.
    pub async fn cancel(&self, id: &str) -> LocalModelResult<LocalModelStatus> {
        if find_model(id).is_none() && !self.houmo_known(id).await {
            return Err(LocalModelError::UnknownModel(id.to_owned()));
        }
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id)
                && let Some(cancel) = &runtime.cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.status(id).await
    }

    async fn start_houmo(
        self: &Arc<Self>,
        id: &str,
        requested: LocalModelLaunchOptions,
    ) -> LocalModelResult<LocalModelStatus> {
        let (models, _) = self.houmo_snapshot().await;
        let model = models
            .into_iter()
            .find(|model| model.id == id)
            .ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id) {
                match runtime.status {
                    LocalModelRunStatus::Running => {
                        let persisted = self.persisted.lock().await;
                        return Ok(houmo_status(&model, &runtimes, &persisted, &self.root));
                    }
                    LocalModelRunStatus::Downloading | LocalModelRunStatus::Starting => {
                        return Err(LocalModelError::Busy(format!("{id} is busy")));
                    }
                    _ => {}
                }
            }
        }
        if !houmo_files_present(&model, &self.root) {
            return Err(LocalModelError::NotInstalled(id.to_owned()));
        }
        let runtime_dir = self.houmo_runtime_dir();
        if houmo::override_path().is_none() {
            #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
            self.ensure_houmo_runtime().await?;
        }
        let program = houmo::server_program(&runtime_dir);
        if program.is_absolute() && !program.is_file() {
            return Err(LocalModelError::Launch(format!(
                "Houmo llama-server was not found at {}",
                program.display()
            )));
        }
        let context = houmo::server_context(&runtime_dir, &program);

        let context_size = requested.context_size.unwrap_or(model.context_size).max(1);
        let port = pick_free_port()?;
        let api_key = format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple());
        let provider_id = {
            let mut persisted = self.persisted.lock().await;
            let record = persisted.models.entry(id.to_owned()).or_default();
            if record.provider_id.is_none() {
                record.provider_id = Some(Uuid::now_v7().to_string());
            }
            record.api_key = Some(api_key.clone());
            record.provider_id.clone().unwrap()
        };
        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Starting;
            runtime.port = Some(port);
        }

        let process = match self
            .spawn_houmo_server(
                &model,
                &program,
                &context.cwd,
                context.env,
                port,
                &api_key,
                context_size,
            )
            .await
        {
            Ok(process) => process,
            Err(error) => {
                self.set_not_running(id).await;
                return Err(error);
            }
        };
        {
            let mut runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get_mut(id) {
                runtime.process = Some(process.clone());
            }
        }
        if let Err(error) = self.wait_healthy(&process, port, &context.cwd).await {
            self.kill_process(&process).await;
            self.set_not_running(id).await;
            return Err(error);
        }
        if let Some(publisher) = &self.publisher {
            let spec = LocalModelProviderSpec {
                provider_id: provider_id.clone(),
                name: model.display_name.clone(),
                base_url: format!("http://127.0.0.1:{port}/v1"),
                api_key: api_key.clone(),
                model: model.id.clone(),
                context_size,
            };
            if let Err(error) = publisher.publish(&spec).await {
                self.kill_process(&process).await;
                self.set_not_running(id).await;
                return Err(LocalModelError::Internal(format!(
                    "failed to publish provider: {error}"
                )));
            }
        }
        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Running;
            runtime.port = Some(port);
        }
        {
            let mut persisted = self.persisted.lock().await;
            let record = persisted.models.entry(id.to_owned()).or_default();
            record.installed = true;
            record.launch_options = Some(LocalModelLaunchOptions {
                context_size: Some(context_size),
                ..requested
            });
            persist(&self.root, &persisted);
        }
        self.status(id).await
    }

    async fn spawn_houmo_server(
        &self,
        model: &HoumoModel,
        program: &Path,
        cwd: &Path,
        env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
        port: u16,
        api_key: &str,
        context_size: u64,
    ) -> LocalModelResult<ProcessRef> {
        let model_dir = Self::model_dir(&self.root, &model.id);
        let args = build_houmo_args(
            &model.id,
            &model.main_gguf,
            model.mmproj.as_deref(),
            context_size,
            &model_dir,
            port,
            api_key,
        );
        let request = ProcessRequest {
            owner: ProcessOwner::new(Uuid::now_v7(), Uuid::now_v7()),
            command: CommandSpec::Program {
                program: program.as_os_str().to_os_string(),
                args: args.into_iter().map(Into::into).collect(),
            },
            cwd: cwd.to_path_buf(),
            env,
            transport: Transport::Pipe,
            policy: ProcessPolicy {
                output_limit_bytes: 4 * 1024 * 1024,
                lease: Duration::from_secs(24 * 3600),
                expire_on_idle: false,
                deadline: None,
                ..Default::default()
            },
            capability: CapabilityPolicy {
                cwd_roots: vec![self.root.clone(), model_dir, cwd.to_path_buf()],
                sandbox: nomi_process_runtime::SandboxPolicy::UnrestrictedLocalOwner,
            },
        };
        let normalized = normalize_request(request, cwd)
            .map_err(|error| LocalModelError::Launch(error.to_string()))?;
        let handle = self
            .supervisor
            .start(normalized)
            .await
            .map_err(|error| LocalModelError::Launch(error.to_string()))?;
        Ok(ProcessRef {
            owner: handle.owner,
            session_id: handle.session_id,
        })
    }

    async fn install_houmo(self: &Arc<Self>, id: &str) -> LocalModelResult<LocalModelStatus> {
        let (models, _) = self.houmo_snapshot().await;
        let model = models
            .into_iter()
            .find(|model| model.id == id)
            .ok_or_else(|| LocalModelError::UnknownModel(id.to_owned()))?;
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id) {
                if runtime.status == LocalModelRunStatus::Downloading {
                    return Err(LocalModelError::Busy(format!("{id} is already downloading")));
                }
                if runtime.status == LocalModelRunStatus::Starting
                    || runtime.status == LocalModelRunStatus::Running
                {
                    return Err(LocalModelError::Busy(format!("{id} is running")));
                }
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(DownloadProgress::new(0));
        {
            let mut runtimes = self.runtimes.lock().await;
            let runtime = runtimes.entry(id.to_owned()).or_default();
            runtime.status = LocalModelRunStatus::Downloading;
            runtime.progress = Arc::clone(&progress);
            runtime.cancel = Some(cancel.clone());
            runtime.download_error = None;
        }
        let service = self.clone();
        tokio::spawn(async move {
            service.install_houmo_worker(model, cancel, progress).await;
        });
        self.status(id).await
    }

    async fn install_houmo_worker(
        self: Arc<Self>,
        model: HoumoModel,
        cancel: Arc<AtomicBool>,
        progress: Arc<DownloadProgress>,
    ) {
        let id = model.id.clone();
        let mut files = Vec::with_capacity(model.files.len());
        for file in &model.files {
            let file_size = houmo::probe_file_size(&self.client, &file.urls)
                .await
                .unwrap_or(0);
            files.push(PlannedFile {
                file_name: file.file_name.clone(),
                file_size,
                sha256: None,
                urls: file.urls.clone(),
            });
        }
        let total: u64 = files.iter().map(|file| file.file_size).sum();
        if total > 0 {
            progress.set_total(total);
        }
        let dir = Self::model_dir(&self.root, &id);
        let result = self.download_files(&files, &dir, &cancel, &progress).await;

        let mut runtimes = self.runtimes.lock().await;
        let runtime = runtimes.entry(id.clone()).or_default();
        runtime.cancel = None;
        match result {
            Ok(()) => {
                runtime.status = LocalModelRunStatus::Installed;
                runtime.progress = Arc::new(DownloadProgress::completed(total));
                runtime.download_error = None;
                drop(runtimes);
                let mut persisted = self.persisted.lock().await;
                persisted.models.entry(id).or_default().installed = true;
                persist(&self.root, &persisted);
            }
            Err(LocalModelError::Cancelled) => {
                runtime.status = LocalModelRunStatus::Uninstalled;
                runtime.progress = Arc::new(DownloadProgress::new(0));
                runtime.download_error = None;
            }
            Err(error) => {
                tracing::warn!(model = %id, %error, "houmo model install failed");
                runtime.status = LocalModelRunStatus::Uninstalled;
                runtime.progress = Arc::new(DownloadProgress::new(0));
                runtime.download_error = error.download_footprint();
            }
        }
    }

    async fn uninstall_houmo(&self, id: &str) -> LocalModelResult<LocalModelStatus> {
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(id)
                && let Some(cancel) = &runtime.cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.stop_process(id).await;
        let dir = Self::model_dir(&self.root, id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        let provider_id = {
            let mut persisted = self.persisted.lock().await;
            let provider_id = persisted
                .models
                .get_mut(id)
                .map(|record| {
                    record.installed = false;
                    record.launch_options = None;
                    record.api_key = None;
                    record.provider_id.take()
                })
                .flatten();
            persist(&self.root, &persisted);
            provider_id
        };
        if let (Some(publisher), Some(provider_id)) = (&self.publisher, provider_id)
            && let Ok(outcome) = publisher.remove(&provider_id).await
        {
            tracing::debug!(?outcome, "houmo model provider removed");
        }
        {
            let mut runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get_mut(id) {
                *runtime = ModelRuntime::default();
            }
        }
        self.status(id).await
    }

    /// Cached Houmo catalog. A cold or stale cache refreshes once; overlapping
    /// list polls share that refresh.
    async fn houmo_snapshot(&self) -> (Vec<HoumoModel>, Option<String>) {
        {
            let cache = self.houmo.lock().await;
            if cache.is_fresh() {
                return (cache.models.clone(), cache.error.clone());
            }
        }
        let _refresh = self.houmo_refresh.lock().await;
        {
            let cache = self.houmo.lock().await;
            if cache.is_fresh() {
                return (cache.models.clone(), cache.error.clone());
            }
        }
        match houmo::fetch_catalog(&self.client).await {
            Ok(models) => {
                tracing::info!(count = models.len(), "loaded Houmo model catalog");
                let mut cache = self.houmo.lock().await;
                cache.models = models.clone();
                cache.fetched_at = Some(Instant::now());
                cache.error = None;
                (models, None)
            }
            Err(error) => {
                tracing::warn!(%error, "Houmo catalog refresh failed");
                let mut cache = self.houmo.lock().await;
                let report = if cache.models.is_empty() {
                    Some("unavailable".to_owned())
                } else {
                    None
                };
                cache.error = report.clone();
                cache.fetched_at = Some(Instant::now());
                (cache.models.clone(), report)
            }
        }
    }

    async fn houmo_known(&self, id: &str) -> bool {
        if !houmo::is_houmo_id(id) {
            return false;
        }
        let (models, _) = self.houmo_snapshot().await;
        models.iter().any(|model| model.id == id)
    }
}

impl LocalModelsService {
    /// The provider id recorded for a model, if any.
    pub async fn provider_id(&self, id: &str) -> Option<String> {
        let persisted = self.persisted.lock().await;
        persisted.models.get(id).and_then(|r| r.provider_id.clone())
    }

    /// The context size chosen at the last successful launch.
    pub async fn launch_context_size(&self, id: &str) -> Option<u64> {
        let persisted = self.persisted.lock().await;
        persisted
            .models
            .get(id)
            .and_then(|r| r.launch_options.as_ref())
            .and_then(|options| options.context_size)
    }
}

/// One file the install pipeline fetches, with mirrors in try order.
struct PlannedFile {
    file_name: String,
    file_size: u64,
    sha256: Option<String>,
    urls: Vec<String>,
}

/// Bytes to add to the install counter. An undeclared size uses the file now
/// on disk, so a Houmo download does not reset the bar to zero between files.
async fn accounted_size(declared: u64, dest: &Path) -> u64 {
    if declared > 0 {
        return declared;
    }
    tokio::fs::metadata(dest)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0)
}

fn planned_from_manifest(model: &ModelManifest) -> Vec<PlannedFile> {
    model
        .files
        .iter()
        .map(|file| PlannedFile {
            file_name: file.file_name.to_owned(),
            file_size: file.file_size,
            sha256: file.sha256.map(str::to_owned),
            urls: vec![file.modelscope.to_owned(), file.huggingface.to_owned()],
        })
        .collect()
}

fn houmo_status(
    model: &HoumoModel,
    runtimes: &HashMap<String, ModelRuntime>,
    persisted: &PersistedState,
    root: &Path,
) -> LocalModelStatus {
    let runtime = runtimes.get(&model.id);
    let record = persisted.models.get(&model.id);
    let mut status = runtime
        .map(|entry| entry.status)
        .unwrap_or(LocalModelRunStatus::Uninstalled);
    if status == LocalModelRunStatus::Uninstalled
        && record.map(|entry| entry.installed).unwrap_or(false)
        && houmo_files_present(model, root)
    {
        status = LocalModelRunStatus::Installed;
    }
    let running = status == LocalModelRunStatus::Running || status == LocalModelRunStatus::Starting;
    let port = if running {
        runtime.and_then(|entry| entry.port)
    } else {
        None
    };
    let install_path = LocalModelsService::model_dir(root, &model.id);
    LocalModelStatus {
        id: model.id.clone(),
        source: LocalModelSource::Houmo,
        display_name: LocalModelText {
            zh: model.display_name.clone(),
            en: model.display_name.clone(),
        },
        description: LocalModelText {
            zh: model.description_zh.clone(),
            en: model.description_en.clone(),
        },
        model_type: model.model_type.clone(),
        capabilities: Vec::new(),
        tags: model.tags.clone(),
        scenarios: Vec::new(),
        parameter_count: model.parameter_count,
        size_bytes: 0,
        engines: model.engines.clone(),
        npu: true,
        realtime: false,
        author: "Houmo".to_owned(),
        license: String::new(),
        icon: model.icon.clone(),
        status,
        download_progress: runtime.map(|entry| entry.progress.percent()).unwrap_or(0.0),
        download_speed: runtime
            .map(|entry| entry.progress.speed_bps(Instant::now()))
            .unwrap_or(0),
        download_phase: match status {
            LocalModelRunStatus::Downloading => runtime.map(|entry| entry.progress.phase()),
            _ => None,
        },
        download_error: runtime.and_then(|entry| entry.download_error.clone()),
        port,
        base_url: port.map(|port| format!("http://127.0.0.1:{port}/v1")),
        install_path: Some(install_path.to_string_lossy().into_owned()),
        main_gguf: model.main_gguf.clone(),
        mmproj: model.mmproj.clone(),
        dflash: None,
        supports_mtp: model.supports_mtp,
        supports_dflash: false,
        context_size: model.context_size,
        max_tokens: model.context_size,
        last_launch_options: record.and_then(|entry| entry.launch_options.clone()),
    }
}

fn houmo_files_present(model: &HoumoModel, root: &Path) -> bool {
    let dir = LocalModelsService::model_dir(root, &model.id);
    !model.files.is_empty()
        && model.files.iter().all(|file| {
            std::fs::metadata(dir.join(&file.file_name))
                .map(|meta| meta.len() > 0)
                .unwrap_or(false)
        })
}

fn status_for(
    model: &ModelManifest,
    runtimes: &HashMap<String, ModelRuntime>,
    persisted: &PersistedState,
    root: &Path,
) -> LocalModelStatus {
    let runtime = runtimes.get(model.id);
    let record = persisted.models.get(model.id);
    let status = runtime
        .map(|r| r.status)
        .unwrap_or(LocalModelRunStatus::Uninstalled);
    let running = status == LocalModelRunStatus::Running || status == LocalModelRunStatus::Starting;
    let port = if running { runtime.and_then(|r| r.port) } else { None };
    let install_path = LocalModelsService::model_dir(root, model.id);
    LocalModelStatus {
        id: model.id.to_owned(),
        source: LocalModelSource::Builtin,
        display_name: LocalModelText {
            zh: model.display_name.0.to_owned(),
            en: model.display_name.1.to_owned(),
        },
        description: LocalModelText {
            zh: model.description.0.to_owned(),
            en: model.description.1.to_owned(),
        },
        model_type: model.model_type.to_owned(),
        capabilities: model.capabilities.iter().map(|v| (*v).to_owned()).collect(),
        tags: model.tags.iter().map(|v| (*v).to_owned()).collect(),
        scenarios: model.scenarios.iter().map(|v| (*v).to_owned()).collect(),
        parameter_count: model.parameter_count,
        size_bytes: model.size_bytes,
        engines: vec!["llama.cpp".to_owned()],
        npu: false,
        realtime: false,
        author: model.author.to_owned(),
        license: model.license.to_owned(),
        icon: Some(model.icon.to_owned()),
        status,
        download_progress: runtime.map(|r| r.progress.percent()).unwrap_or(0.0),
        download_speed: runtime
            .map(|r| r.progress.speed_bps(Instant::now()))
            .unwrap_or(0),
        download_phase: match status {
            LocalModelRunStatus::Downloading => runtime.map(|r| r.progress.phase()),
            _ => None,
        },
        download_error: runtime.and_then(|r| r.download_error.clone()),
        port,
        base_url: port.map(|port| format!("http://127.0.0.1:{port}/v1")),
        install_path: Some(install_path.to_string_lossy().into_owned()),
        main_gguf: model.main_gguf.to_owned(),
        mmproj: model.mmproj.map(|v| v.to_owned()),
        dflash: model.dflash.map(|v| v.to_owned()),
        supports_mtp: model.supports_mtp,
        supports_dflash: model.supports_dflash,
        context_size: model.context_size,
        max_tokens: model.max_tokens,
        last_launch_options: record
            .and_then(|r| r.launch_options.clone())
            .or_else(|| Some(default_launch_options(model))),
    }
}

fn model_files_present(model: &ModelManifest, root: &Path) -> bool {
    let dir = LocalModelsService::model_dir(root, model.id);
    model.files.iter().all(|file| {
        let path = dir.join(file.file_name);
        match std::fs::metadata(&path) {
            Ok(meta) => file.file_size == 0 || meta.len() == file.file_size,
            Err(_) => false,
        }
    })
}

/// Sum of every file's declared size for a model.
fn model_total(model: &ModelManifest) -> u64 {
    model.files.iter().map(|file| file.file_size).sum()
}

fn pick_free_port() -> LocalModelResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn load_persisted(root: &Path) -> PersistedState {
    let path = root.join("state.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => PersistedState::default(),
    }
}

fn persist(root: &Path, state: &PersistedState) {
    let _ = std::fs::create_dir_all(root);
    let path = root.join("state.json");
    let temp = root.join("state.json.tmp");
    if let Ok(text) = serde_json::to_string_pretty(state)
        && std::fs::write(&temp, text).is_ok()
    {
        let _ = std::fs::rename(&temp, &path);
    }
}
