//! Local Presentation Capability lifecycle for the fixed `ppt-master` bundle.
//!
//! This is intentionally a small host-owned control plane. It installs one
//! pinned upstream archive into a versioned snapshot, prepares an isolated
//! Python runtime, and exposes readiness to the UI. Agent execution is gated
//! on the same snapshot and never accepts a model-provided script path.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use nomifun_api_types::{ApiResponse, ErrorResponse};
use nomi_process_runtime::{
    CapabilityPolicy, CommandSpec, NormalizedProcessRequest, ProcessError, ProcessOutcome,
    ProcessOwner, ProcessPolicy, ProcessRequest, ProcessSupervisor, PollResult,
    SupervisorConfig, Transport, normalize_request,
};
use reqwest::header::CONTENT_LENGTH;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub const PRESENTATION_CAPABILITY_ID: &str = "presentation.ppt-master";
pub const PRESENTATION_VERSION: &str = "6.8.0";
pub const PRESENTATION_SOURCE_COMMIT: &str = "4a68cddaf40dc87891e6b64c6a69ef46adc1110c";
pub const PRESENTATION_RUNTIME_PROFILE: &str = "pptx-core";
pub const PRESENTATION_LICENSE: &str = "MIT";

const DEFAULT_ARCHIVE_URL: &str = "https://codeload.github.com/hugohe3/ppt-master/zip/4a68cddaf40dc87891e6b64c6a69ef46adc1110c";
const ARCHIVE_SHA256: &str = "c4843a7c5bfda734788f25a85a22909790a818ecd869ce6e1e0a798b117be0a4";
const ARCHIVE_SIZE_BYTES: u64 = 57_684_002;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RUNTIME_OUTPUT_BYTES: usize = 64 * 1024;
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresentationCapabilityStatus {
    NotInstalled,
    Installing,
    Installed,
    RuntimeMissing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationCapabilityView {
    pub capability_id: String,
    pub version: String,
    pub source_commit: String,
    pub license: String,
    pub runtime_profile: String,
    pub archive_url: String,
    pub archive_sha256: String,
    pub archive_size_bytes: u64,
    pub status: PresentationCapabilityStatus,
    pub snapshot_dir: Option<String>,
    pub skill_dir: Option<String>,
    pub runtime_dir: Option<String>,
    pub python: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PresentationInstallRequest {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledRecord {
    capability_id: String,
    version: String,
    source_commit: String,
    package_sha256: String,
    package_size_bytes: u64,
    snapshot_dir: String,
    skill_dir: String,
    runtime_dir: String,
    runtime_digest: String,
    installed_at_ms: i64,
}

#[derive(Clone)]
pub struct PresentationCapabilityService {
    data_dir: PathBuf,
    supervisor: Arc<ProcessSupervisor>,
}

impl PresentationCapabilityService {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            supervisor: ProcessSupervisor::new(SupervisorConfig::default()),
        }
    }

    fn root(&self) -> PathBuf {
        self.data_dir.join("capabilities/presentation/ppt-master")
    }

    fn current_path(&self) -> PathBuf {
        self.root().join("current.json")
    }

    fn state_path(&self) -> PathBuf {
        self.root().join("install-state.json")
    }

    fn archive_url(&self) -> String {
        std::env::var("FLOWY_PPT_MASTER_ARCHIVE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ARCHIVE_URL.to_owned())
    }

    pub fn status(&self) -> PresentationCapabilityView {
        let archive_url = self.archive_url();
        let record = fs::read_to_string(self.current_path())
            .ok()
            .and_then(|text| serde_json::from_str::<InstalledRecord>(&text).ok());
        let Some(record) = record else {
            return self.view(PresentationCapabilityStatus::NotInstalled, None, None);
        };
        let snapshot = PathBuf::from(&record.snapshot_dir);
        let skill = PathBuf::from(&record.skill_dir);
        let runtime = PathBuf::from(&record.runtime_dir);
        if !skill.join("SKILL.md").is_file() {
            return self.view_with_record(PresentationCapabilityStatus::Failed, &record, Some("installed snapshot is missing SKILL.md".into()));
        }
        let python = venv_python(&runtime);
        if !python.is_file() {
            return self.view_with_record(PresentationCapabilityStatus::RuntimeMissing, &record, None);
        }
        if runtime_ready(&runtime) {
            return self.view_with_record(PresentationCapabilityStatus::Ready, &record, None);
        }
        self.view_with_record(PresentationCapabilityStatus::Installed, &record, None)
            .with_archive_url(archive_url)
            .with_snapshot(snapshot)
            .with_python(python)
    }

    pub async fn install(&self, force: bool) -> Result<PresentationCapabilityView, String> {
        let current = self.status();
        if !force && matches!(current.status, PresentationCapabilityStatus::Ready) {
            return Ok(current);
        }
        let root = self.root();
        fs::create_dir_all(&root).map_err(|error| format!("create capability root: {error}"))?;
        let staging = root.join(format!(".staging-{}", Uuid::now_v7()));
        fs::create_dir_all(&staging).map_err(|error| format!("create staging: {error}"))?;
        let result = self.install_into(&staging).await;
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    async fn install_into(&self, staging: &Path) -> Result<PresentationCapabilityView, String> {
        let archive = staging.join("ppt-master.zip");
        let url = self.archive_url();
        let mut response = reqwest::Client::builder()
            .build()
            .map_err(|error| format!("build archive client: {error}"))?
            .get(&url)
            .header("User-Agent", "Flowy-Presentation-Capability")
            .send()
            .await
            .map_err(|error| format!("download ppt-master: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("download ppt-master returned {}", response.status()));
        }
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|size| size > MAX_ARCHIVE_BYTES)
        {
            return Err("ppt-master archive exceeds the size limit".into());
        }
        let mut file = tokio::fs::File::create(&archive)
            .await
            .map_err(|error| format!("create archive staging file: {error}"))?;
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("read ppt-master archive: {error}"))?
        {
            size = size.saturating_add(chunk.len() as u64);
            if size > MAX_ARCHIVE_BYTES {
                return Err("ppt-master archive exceeds the size limit".into());
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("write archive staging file: {error}"))?;
        }
        file.flush().await.map_err(|error| format!("flush archive: {error}"))?;
        let digest = format!("{:x}", hasher.finalize());
        if digest != ARCHIVE_SHA256 {
            return Err(format!("ppt-master archive sha256 mismatch: {digest}"));
        }
        if size != ARCHIVE_SIZE_BYTES {
            return Err(format!("ppt-master archive size mismatch: {size}"));
        }

        let extracted = staging.join("extracted");
        let extracted_count = crate::market_source::extract_zip_market(&archive, &extracted)
            .map_err(|error| format!("extract ppt-master archive: {error}"))?;
        if extracted_count == 0 {
            return Err("ppt-master archive is empty".into());
        }
        let skill_source = find_skill_dir(&extracted)
            .ok_or_else(|| "ppt-master archive does not contain skills/ppt-master/SKILL.md".to_owned())?;
        let skill = staging.join("skill");
        copy_tree(&skill_source, &skill).map_err(|error| format!("materialize ppt-master skill: {error}"))?;
        validate_skill_tree(&skill)?;

        let snapshot_id = format!("{}-{}", PRESENTATION_VERSION, &PRESENTATION_SOURCE_COMMIT[..12]);
        let live_snapshot = self.root().join("snapshots").join(&snapshot_id);
        if live_snapshot.exists() {
            fs::remove_dir_all(&live_snapshot).map_err(|error| format!("replace existing snapshot: {error}"))?;
        }
        fs::create_dir_all(live_snapshot.parent().expect("snapshot parent"))
            .map_err(|error| format!("create snapshot parent: {error}"))?;
        fs::create_dir_all(&live_snapshot).map_err(|error| format!("create snapshot: {error}"))?;
        let live_skill = live_snapshot.join("skill");
        copy_tree(&skill, &live_skill).map_err(|error| format!("commit skill snapshot: {error}"))?;

        let runtime_dir = self.root().join("runtimes").join(&snapshot_id);
        fs::create_dir_all(&runtime_dir).map_err(|error| format!("create runtime root: {error}"))?;
        let python = resolve_python().ok_or_else(|| "python is not installed or not discoverable".to_owned())?;
        let venv = runtime_dir.join("venv");
        if !venv_python(&venv).is_file() {
            run_process(
                Arc::clone(&self.supervisor),
                &python,
                &["-m", "venv", venv.to_string_lossy().as_ref()],
                &self.data_dir,
                Duration::from_secs(5 * 60),
            )
            .await
            .map_err(|error| format!("create Python venv: {error}"))?;
        }
        let venv_python_path = venv_python(&venv);
        if !venv_python_path.is_file() {
            return Err("Python venv did not produce an interpreter".into());
        }
        let requirements = runtime_dir.join("requirements.txt");
        fs::write(&requirements, PPTX_CORE_REQUIREMENTS)
            .map_err(|error| format!("write runtime requirements: {error}"))?;
        run_process(
            Arc::clone(&self.supervisor),
            &venv_python_path,
            &["-m", "pip", "install", "-r", requirements.to_string_lossy().as_ref()],
            &self.data_dir,
            RUNTIME_TIMEOUT,
        )
        .await
        .map_err(|error| format!("install pptx-core dependencies: {error}"))?;
        run_process(
            Arc::clone(&self.supervisor),
            &venv_python_path,
            &["-c", "import pptx, yaml, PIL, numpy, skia_pathops, uharfbuzz"],
            &self.data_dir,
            Duration::from_secs(60),
        )
        .await
        .map_err(|error| format!("verify pptx-core dependencies: {error}"))?;
        run_process(
            Arc::clone(&self.supervisor),
            &venv_python_path,
            &["-c", "from pptx import Presentation; Presentation()"],
            &self.data_dir,
            Duration::from_secs(60),
        )
        .await
        .map_err(|error| format!("run PPTX smoke test: {error}"))?;
        let runtime_digest = runtime_digest(&requirements, &venv_python_path)?;
        fs::write(runtime_dir.join("runtime-ready"), b"ready")
            .map_err(|error| format!("mark runtime ready: {error}"))?;
        let now = chrono_like_now_ms();
        let record = InstalledRecord {
            capability_id: PRESENTATION_CAPABILITY_ID.into(),
            version: PRESENTATION_VERSION.into(),
            source_commit: PRESENTATION_SOURCE_COMMIT.into(),
            package_sha256: digest,
            package_size_bytes: size,
            snapshot_dir: live_snapshot.to_string_lossy().into_owned(),
            skill_dir: live_skill.to_string_lossy().into_owned(),
            runtime_dir: runtime_dir.to_string_lossy().into_owned(),
            runtime_digest,
            installed_at_ms: now,
        };
        atomic_json_write(&self.current_path(), &record)?;
        atomic_json_write(&self.state_path(), &record)?;
        let _ = fs::remove_dir_all(staging);
        Ok(self.status())
    }

    fn view(&self, status: PresentationCapabilityStatus, failure: Option<String>, record: Option<&InstalledRecord>) -> PresentationCapabilityView {
        let mut view = PresentationCapabilityView {
            capability_id: PRESENTATION_CAPABILITY_ID.into(),
            version: PRESENTATION_VERSION.into(),
            source_commit: PRESENTATION_SOURCE_COMMIT.into(),
            license: PRESENTATION_LICENSE.into(),
            runtime_profile: PRESENTATION_RUNTIME_PROFILE.into(),
            archive_url: self.archive_url(),
            archive_sha256: ARCHIVE_SHA256.into(),
            archive_size_bytes: ARCHIVE_SIZE_BYTES,
            status,
            snapshot_dir: None,
            skill_dir: None,
            runtime_dir: None,
            python: None,
            failure,
        };
        if let Some(record) = record {
            view.snapshot_dir = Some(record.snapshot_dir.clone());
            view.skill_dir = Some(record.skill_dir.clone());
            view.runtime_dir = Some(record.runtime_dir.clone());
            view.python = Some(venv_python(Path::new(&record.runtime_dir)).to_string_lossy().into_owned());
        }
        view
    }

    fn view_with_record(&self, status: PresentationCapabilityStatus, record: &InstalledRecord, failure: Option<String>) -> PresentationCapabilityView {
        self.view(status, failure, Some(record))
    }
}

trait ViewExt {
    fn with_archive_url(self, _url: String) -> Self;
    fn with_snapshot(self, _snapshot: PathBuf) -> Self;
    fn with_python(self, python: PathBuf) -> Self;
}
impl ViewExt for PresentationCapabilityView {
    fn with_archive_url(mut self, url: String) -> Self { self.archive_url = url; self }
    fn with_snapshot(mut self, snapshot: PathBuf) -> Self { self.snapshot_dir = Some(snapshot.to_string_lossy().into_owned()); self }
    fn with_python(mut self, python: PathBuf) -> Self { self.python = Some(python.to_string_lossy().into_owned()); self }
}

#[derive(Clone)]
pub struct PresentationRouterState {
    pub service: Arc<PresentationCapabilityService>,
}

pub fn presentation_routes(state: PresentationRouterState) -> Router {
    Router::new()
        .route("/api/presentation/capability/status", get(status))
        .route("/api/presentation/capability/install", post(install))
        .with_state(state)
}

async fn status(State(state): State<PresentationRouterState>) -> Json<ApiResponse<PresentationCapabilityView>> {
    Json(ApiResponse::ok(state.service.status()))
}

async fn install(
    State(state): State<PresentationRouterState>,
    body: Result<Json<PresentationInstallRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ApiResponse<PresentationCapabilityView>>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let request = body.map(|Json(value)| value).unwrap_or_default();
    state.service.install(request.force).await
        .map(|view| Json(ApiResponse::ok(view)))
        .map_err(|error| (axum::http::StatusCode::BAD_GATEWAY, Json(ErrorResponse::new(error, "PRESENTATION_CAPABILITY_INSTALL_FAILED"))))
}

fn find_skill_dir(root: &Path) -> Option<PathBuf> {
    let direct = root.join("skills/ppt-master");
    if direct.join("SKILL.md").is_file() { return Some(direct); }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_skill_dir(&path) { return Some(found); }
        }
    }
    None
}

fn validate_skill_tree(skill: &Path) -> Result<(), String> {
    let manifest = skill.join("SKILL.md");
    if !manifest.is_file() { return Err("ppt-master SKILL.md is missing".into()); }
    let text = fs::read_to_string(&manifest).map_err(|error| format!("read SKILL.md: {error}"))?;
    if !text.contains("name: ppt-master") || !text.contains("version: \"6.8.0\"") {
        return Err("ppt-master SKILL.md does not match the pinned version".into());
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() { return Err(std::io::Error::other("symlinks are not allowed")); }
        if metadata.is_dir() { copy_tree(&source_path, &target)?; }
        else if metadata.is_file() { fs::copy(&source_path, &target)?; }
    }
    Ok(())
}

fn resolve_python() -> Option<PathBuf> {
    nomi_process_runtime::resolve_command_path("python")
        .or_else(|| nomi_process_runtime::resolve_command_path("python3"))
}

fn venv_python(root: &Path) -> PathBuf {
    if cfg!(windows) { root.join("Scripts/python.exe") } else { root.join("bin/python") }
}

fn runtime_ready(runtime_dir: &Path) -> bool {
    runtime_dir.join("runtime-ready").is_file()
}

fn runtime_digest(requirements: &Path, python: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(requirements).map_err(|error| format!("read runtime requirements: {error}"))?);
    hasher.update(python.to_string_lossy().as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn atomic_json_write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| format!("serialize state: {error}"))?;
    fs::write(&temp, bytes).map_err(|error| format!("write state: {error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("commit state: {error}"))
}

fn chrono_like_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

async fn run_process(
    supervisor: Arc<ProcessSupervisor>,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let owner = ProcessOwner::new(Uuid::now_v7(), Uuid::now_v7());
    let request = ProcessRequest {
        owner,
        command: CommandSpec::Program { program: program.as_os_str().to_owned(), args: args.iter().map(OsString::from).collect() },
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        transport: Transport::Pipe,
        policy: ProcessPolicy { output_limit_bytes: MAX_RUNTIME_OUTPUT_BYTES, deadline: Some(Instant::now() + timeout), ..Default::default() },
        capability: CapabilityPolicy { cwd_roots: vec![cwd.to_path_buf()], sandbox: nomi_process_runtime::SandboxPolicy::UnrestrictedLocalOwner },
    };
    let normalized: NormalizedProcessRequest = normalize_request(request, cwd).map_err(|error| error.to_string())?;
    let handle = supervisor.start(normalized).await.map_err(|error| error.to_string())?;
    loop {
        match supervisor.poll(&handle.owner, &handle.session_id, nomi_process_runtime::OutputCursor::START, Instant::now() + Duration::from_millis(100)).await {
            Ok(PollResult::Finished(outcome)) => {
                return match outcome {
                    ProcessOutcome::Exited { code, output, .. } if code == Some(0) => Ok(output.text()),
                    ProcessOutcome::Exited { code, output, .. } => Err(format!("process exited with {:?}: {}", code, output.text())),
                    ProcessOutcome::TimedOut { output, .. } => Err(format!("process timed out: {}", output.text())),
                    ProcessOutcome::Cancelled { output, .. } => Err(format!("process cancelled: {}", output.text())),
                    ProcessOutcome::SpawnFailed(error) => Err(format!("process spawn failed: {}", error.message)),
                    ProcessOutcome::Lost { output, .. } => Err(format!("process lost: {}", output.text())),
                };
            }
            Ok(PollResult::Running { .. }) => continue,
            Err(ProcessError::SessionNotFound { .. }) => return Err("process session disappeared".into()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

const PPTX_CORE_REQUIREMENTS: &str = "PyYAML>=6.0\npython-pptx>=0.6.21\nXlsxWriter>=3.0.0\nPillow>=9.0.0\nnumpy>=1.20.0\nskia-pathops>=0.9.2\nuharfbuzz>=0.50.0\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_manifest_constants_are_stable() {
        assert_eq!(PRESENTATION_CAPABILITY_ID, "presentation.ppt-master");
        assert_eq!(PRESENTATION_VERSION, "6.8.0");
        assert_eq!(PRESENTATION_SOURCE_COMMIT.len(), 40);
        assert_eq!(ARCHIVE_SHA256.len(), 64);
    }

    #[test]
    fn venv_path_is_platform_specific() {
        let path = venv_python(Path::new("runtime/venv"));
        assert!(path.to_string_lossy().contains(if cfg!(windows) { "Scripts" } else { "bin" }));
    }
}

