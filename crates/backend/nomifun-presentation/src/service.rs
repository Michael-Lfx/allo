//! Install lifecycle of the presentation capability.
//!
//! Invariants (see docs/architecture/presentation-ppt-master.zh.md §2.3):
//! - one install at a time: an in-process task slot plus a cross-process
//!   directory lease;
//! - every install builds a fresh `snapshots/<id>/` in place (a venv cannot be
//!   moved), and `current.json` is switched atomically only after the
//!   self-test passed, so a failed install never touches the working snapshot;
//! - the previous snapshot (and the Python runtime its venv points at) is
//!   kept for rollback, older ones are collected;
//! - every realtime event is published while holding the `active` lock, so
//!   clients receive states in the order they happened.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use nomifun_api_types::{
    PRESENTATION_CAPABILITY_STATUS_EVENT, PresentationCancelResponse, PresentationCapabilityStatus,
    PresentationCapabilityView, PresentationInstallFailure, PresentationInstallResponse,
    PresentationInstallStage, WebSocketMessage,
};
use nomi_process_runtime::{ProcessSupervisor, SupervisorConfig};
use nomifun_realtime::EventBroadcaster;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::manifest;
use crate::progress::{CANCELLED, InstallProgress, is_cancelled};
use crate::python_dist;
use crate::runtime::{
    ManagedPythonRuntime, PrepareContext, ProcessExit, ProcessSpec, RuntimeLayout, RuntimePreparer, run_process,
    venv_python,
};
use crate::scripts;
use crate::source::{self, SourceSpec};

const CURRENT_FILE: &str = "current.json";
const STATE_FILE: &str = "install-state.json";
const LOCK_FILE: &str = ".install.lock";
const SNAPSHOTS_DIR: &str = "snapshots";
const PYTHONS_DIR: &str = "python";
const READY_MARKER: &str = "runtime-ready";
const FETCH_WORK_DIR: &str = ".fetch";
const TRASH_PREFIX: &str = ".trash-";
/// Progress events while installing; identical views are not re-sent.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);

/// `current.json`: the active snapshot. Paths are derived, never stored, so a
/// moved data dir keeps working.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledRecord {
    capability_id: String,
    skill_version: String,
    commit: String,
    tree: String,
    source_url: String,
    snapshot_id: String,
    /// Interpreter home under `python/`; absent for snapshots whose venv was
    /// built from a system Python.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    python_id: Option<String>,
    installed_at_ms: i64,
}

/// `install-state.json`: outcome of the most recent install attempt. A
/// cancelled attempt is recorded without a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallState {
    finished_at_ms: i64,
    commit: String,
    failure: Option<PresentationInstallFailure>,
    #[serde(default)]
    cancelled: bool,
}

/// Shared between the install task and status readers.
struct InstallShared {
    stage: Mutex<PresentationInstallStage>,
    progress: InstallProgress,
    /// Stops every step. Also set by the snapshot guard when the task dies.
    cancel: AtomicBool,
    /// Set only by [`PresentationCapabilityService::cancel`]: the outcome is
    /// "cancelled", not "failed".
    user_cancelled: AtomicBool,
}

impl InstallShared {
    fn new() -> Self {
        Self {
            stage: Mutex::new(PresentationInstallStage::FetchingSkill),
            progress: InstallProgress::default(),
            cancel: AtomicBool::new(false),
            user_cancelled: AtomicBool::new(false),
        }
    }

    /// Counters describe the current stage only.
    fn set_stage(&self, stage: PresentationInstallStage) {
        let mut current = lock(&self.stage);
        if *current != stage {
            *current = stage;
            self.progress.reset();
        }
    }

    fn stage(&self) -> PresentationInstallStage {
        *lock(&self.stage)
    }
}

struct ActiveInstall {
    handle: JoinHandle<()>,
    shared: Arc<InstallShared>,
}

impl ActiveInstall {
    fn running(&self) -> bool {
        !self.handle.is_finished()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallError {
    /// An install is running here or in another Flowy process.
    Busy,
    Io(String),
}

impl std::fmt::Display for UninstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("the presentation capability is being installed"),
            Self::Io(message) => f.write_str(message),
        }
    }
}

pub struct PresentationCapabilityService {
    root: PathBuf,
    spec: SourceSpec,
    skill_version: String,
    runtime: Arc<dyn RuntimePreparer>,
    active: Mutex<Option<ActiveInstall>>,
    events: OnceLock<Arc<dyn EventBroadcaster>>,
    /// Supervisor for agent-requested scripts, created on first use.
    scripts: OnceLock<Arc<ProcessSupervisor>>,
}

impl PresentationCapabilityService {
    /// Production service: pinned sources, private portable Python.
    pub fn new(data_dir: impl AsRef<Path>) -> Arc<Self> {
        Self::with_parts(
            data_dir,
            SourceSpec::pinned(),
            manifest::SKILL_VERSION,
            Arc::new(ManagedPythonRuntime::new()),
        )
    }

    /// Injectable constructor for tests (local repositories, fake runtime).
    pub fn with_parts(
        data_dir: impl AsRef<Path>,
        spec: SourceSpec,
        skill_version: impl Into<String>,
        runtime: Arc<dyn RuntimePreparer>,
    ) -> Arc<Self> {
        Arc::new(Self {
            root: data_dir.as_ref().join("capabilities").join("presentation").join("ppt-master"),
            spec,
            skill_version: skill_version.into(),
            runtime,
            active: Mutex::new(None),
            events: OnceLock::new(),
            scripts: OnceLock::new(),
        })
    }

    /// Publish [`PRESENTATION_CAPABILITY_STATUS_EVENT`] on every change.
    /// Instance scoped: the capability lives in the data dir, not a user row.
    /// Only the first call takes effect.
    pub fn set_event_broadcaster(&self, events: Arc<dyn EventBroadcaster>) {
        let _ = self.events.set(events);
    }

    pub fn status(&self) -> PresentationCapabilityView {
        let active = lock(&self.active);
        self.view_locked(active.as_ref())
    }

    /// Starts a background install unless one is running or the capability is
    /// already `Ready` (and `force` is false). Returns immediately.
    pub fn start_install(self: &Arc<Self>, force: bool) -> PresentationInstallResponse {
        let mut active = lock(&self.active);
        if active.as_ref().is_some_and(ActiveInstall::running) {
            return PresentationInstallResponse { accepted: false, capability: self.view_locked(active.as_ref()) };
        }
        let current = self.disk_view();
        if !force && current.status == PresentationCapabilityStatus::Ready {
            return PresentationInstallResponse { accepted: false, capability: current };
        }
        let shared = Arc::new(InstallShared::new());
        let service = Arc::clone(self);
        let task_shared = Arc::clone(&shared);
        // The task's first publish needs this lock, so it cannot overtake ours.
        let handle = tokio::spawn(async move { service.run_install(task_shared).await });
        *active = Some(ActiveInstall { handle, shared });
        let view = self.view_locked(active.as_ref());
        self.publish(&view);
        PresentationInstallResponse { accepted: true, capability: view }
    }

    /// Ask the running install to stop. The task removes its half-built
    /// snapshot and settles on the previous state (`not_installed` or the
    /// still-working `ready`), not `failed`.
    pub fn cancel(&self) -> PresentationCancelResponse {
        let active = lock(&self.active);
        let Some(install) = active.as_ref().filter(|install| install.running()) else {
            return PresentationCancelResponse { cancelled: false, capability: self.view_locked(active.as_ref()) };
        };
        install.shared.user_cancelled.store(true, Ordering::SeqCst);
        install.shared.cancel.store(true, Ordering::SeqCst);
        let view = self.view_locked(active.as_ref());
        PresentationCancelResponse { cancelled: true, capability: view }
    }

    /// Remove the installed Skill, its venvs and the private Python. Refused
    /// while an install runs. Files Windows still holds open (a script that is
    /// running right now) are left for the next install to collect.
    pub fn uninstall(&self) -> Result<PresentationCapabilityView, UninstallError> {
        let active = lock(&self.active);
        if active.as_ref().is_some_and(ActiveInstall::running) {
            return Err(UninstallError::Busy);
        }
        if !self.root.exists() {
            return Ok(self.view_locked(active.as_ref()));
        }
        let lease = match DirectoryLease::acquire(&self.root) {
            Ok(lease) => lease,
            Err(LeaseError::Busy) => return Err(UninstallError::Busy),
            Err(LeaseError::Io(message)) => return Err(UninstallError::Io(message)),
        };
        for file in [CURRENT_FILE, STATE_FILE] {
            match fs::remove_file(self.root.join(file)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(UninstallError::Io(format!("remove {file}: {error}"))),
            }
        }
        // Rename now (cheap, atomic), delete later (13k+ files).
        let mut trash = Vec::new();
        for dir in [SNAPSHOTS_DIR, PYTHONS_DIR] {
            let path = self.root.join(dir);
            if !path.exists() {
                continue;
            }
            let target = self.root.join(format!("{TRASH_PREFIX}{dir}-{}", short_nonce()));
            match fs::rename(&path, &target) {
                Ok(()) => trash.push(target),
                Err(error) => tracing::warn!(dir, error = %error, "uninstall could not move directory; collected later"),
            }
        }
        drop(lease);
        let view = self.view_locked(active.as_ref());
        self.publish(&view);
        drop(active);
        if !trash.is_empty() {
            std::thread::spawn(move || {
                for dir in trash {
                    let _ = fs::remove_dir_all(dir);
                }
            });
        }
        Ok(view)
    }

    fn view_locked(&self, active: Option<&ActiveInstall>) -> PresentationCapabilityView {
        let mut view = self.disk_view();
        if let Some(install) = active.filter(|install| install.running()) {
            view.status = PresentationCapabilityStatus::Installing;
            view.stage = Some(install.shared.stage());
            view.progress = Some(install.shared.progress.snapshot());
            view.failure = None;
        }
        view
    }

    fn publish(&self, view: &PresentationCapabilityView) {
        let Some(events) = self.events.get() else { return };
        match serde_json::to_value(view) {
            Ok(value) => events.broadcast(WebSocketMessage::new(PRESENTATION_CAPABILITY_STATUS_EVENT, value)),
            Err(error) => tracing::warn!(error = %error, "serialize presentation capability event"),
        }
    }

    /// Run one ppt-master script with the active snapshot's interpreter.
    /// Allowed while a re-install runs: the active snapshot is only replaced
    /// atomically and the previous one is kept, so the paths stay valid.
    pub async fn run_script(
        &self,
        script: &str,
        args: &[String],
        workspace: &Path,
        cwd: Option<&str>,
    ) -> Result<scripts::ScriptRun, scripts::ScriptError> {
        use scripts::ScriptError;
        let view = self.disk_view();
        if view.status != PresentationCapabilityStatus::Ready {
            return Err(ScriptError::NotReady(format!(
                "the presentation capability is {} — call nomi_presentation_install first",
                serde_json::to_value(view.status).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
            )));
        }
        let record = read_json::<InstalledRecord>(&self.root.join(CURRENT_FILE))
            .ok_or_else(|| ScriptError::NotReady("installed record disappeared".into()))?;
        let layout = self.check_snapshot(&record).map_err(ScriptError::NotReady)?;

        let relative = scripts::normalize_script(script)?;
        let program = scripts::resolve_script(&layout.skill_dir.join("scripts"), &relative)?;
        let cwd = scripts::resolve_cwd(workspace, cwd)?;
        scripts::check_args(args)?;

        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(program.to_string_lossy().into_owned());
        argv.extend(args.iter().cloned());
        let python = layout.python();
        let roots = [cwd.clone()];
        let spec = ProcessSpec {
            program: &python,
            args: &argv,
            cwd: &cwd,
            cwd_roots: &roots,
            timeout: scripts::SCRIPT_TIMEOUT,
            extra_env: &[],
            output_limit: scripts::MAX_SCRIPT_OUTPUT_BYTES,
        };
        let supervisor = self.scripts.get_or_init(|| ProcessSupervisor::new(SupervisorConfig::default()));
        let finished = run_process(supervisor, &spec, &AtomicBool::new(false))
            .await
            .map_err(ScriptError::Failed)?;
        Ok(scripts::ScriptRun {
            script: relative.to_string_lossy().replace('\\', "/"),
            cwd: cwd.to_string_lossy().into_owned(),
            exit_code: match finished.exit {
                ProcessExit::Code(code) => Some(code),
                ProcessExit::Killed | ProcessExit::TimedOut => None,
            },
            timed_out: finished.exit == ProcessExit::TimedOut,
            output: finished.output,
            truncated_bytes: finished.dropped_bytes,
        })
    }

    /// Status from disk alone: `current.json`, the snapshot, the last attempt.
    fn disk_view(&self) -> PresentationCapabilityView {
        let mut view = self.base_view();
        let last_failure = read_json::<InstallState>(&self.root.join(STATE_FILE)).and_then(|state| state.failure);
        let record = read_json::<InstalledRecord>(&self.root.join(CURRENT_FILE))
            .filter(|record| record.commit == self.spec.commit);
        match record {
            Some(record) => match self.check_snapshot(&record) {
                Ok(layout) => {
                    view.status = PresentationCapabilityStatus::Ready;
                    view.skill_dir = Some(layout.skill_dir.to_string_lossy().into_owned());
                    view.python = Some(layout.python().to_string_lossy().into_owned());
                    view.installed_from = Some(record.source_url);
                    view.installed_at_ms = Some(record.installed_at_ms);
                    // A failed re-install leaves the old snapshot Ready; still say why.
                    view.failure = last_failure;
                }
                Err(message) => {
                    view.status = PresentationCapabilityStatus::Failed;
                    view.failure = Some(PresentationInstallFailure { stage: None, message, at_ms: now_ms() });
                }
            },
            None => {
                // No usable record (never installed, or the pin moved on).
                if let Some(failure) = last_failure {
                    view.status = PresentationCapabilityStatus::Failed;
                    view.failure = Some(failure);
                }
            }
        }
        view
    }

    fn check_snapshot(&self, record: &InstalledRecord) -> Result<RuntimeLayout, String> {
        let layout = self.layout(&record.snapshot_id)?;
        if !layout.skill_dir.join("SKILL.md").is_file() {
            return Err("installed snapshot is missing SKILL.md".into());
        }
        if !layout.python().is_file() {
            return Err("installed snapshot is missing its Python interpreter".into());
        }
        // The venv launcher survives a deleted base interpreter but cannot run.
        if let Some(python_id) = &record.python_id {
            let home = source::child_dir(&layout.pythons_dir, python_id)?;
            if !python_dist::is_ready(&home) {
                return Err("installed snapshot is missing its Python runtime".into());
            }
        }
        if !layout.snapshot_dir.join(READY_MARKER).is_file() {
            return Err("installed snapshot never passed its self-test".into());
        }
        Ok(layout)
    }

    fn layout(&self, snapshot_id: &str) -> Result<RuntimeLayout, String> {
        let snapshot_dir = source::child_dir(&self.root.join(SNAPSHOTS_DIR), snapshot_id)?;
        Ok(RuntimeLayout {
            skill_dir: snapshot_dir.join("skill"),
            venv_dir: snapshot_dir.join("venv"),
            pythons_dir: self.root.join(PYTHONS_DIR),
            snapshot_dir,
        })
    }

    async fn run_install(self: Arc<Self>, shared: Arc<InstallShared>) {
        let install = self.install_once(&shared);
        tokio::pin!(install);
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + PUBLISH_INTERVAL, PUBLISH_INTERVAL);
        let mut last_sent = None;
        let outcome = loop {
            tokio::select! {
                outcome = &mut install => break outcome,
                _ = ticker.tick() => self.publish_progress(&mut last_sent),
            }
        };

        let cancelled = outcome.is_err() && shared.user_cancelled.load(Ordering::SeqCst);
        let failure = match outcome {
            Ok(()) => None,
            Err(_) if cancelled => {
                tracing::info!("presentation capability install cancelled");
                None
            }
            Err(message) => {
                tracing::warn!(error = %message, "presentation capability install failed");
                Some(PresentationInstallFailure { stage: Some(shared.stage()), message, at_ms: now_ms() })
            }
        };
        let state = InstallState { finished_at_ms: now_ms(), commit: self.spec.commit.clone(), failure, cancelled };
        if let Err(error) = write_json_atomic(&self.root.join(STATE_FILE), &state) {
            tracing::warn!(error = %error, "failed to persist presentation install state");
        }

        // Retire the slot before the final event so `status` can never report
        // `installing` after clients were told the install settled.
        let mut active = lock(&self.active);
        if active.as_ref().is_some_and(|install| Arc::ptr_eq(&install.shared, &shared)) {
            *active = None;
        }
        let view = self.view_locked(active.as_ref());
        self.publish(&view);
    }

    fn publish_progress(&self, last_sent: &mut Option<serde_json::Value>) {
        if self.events.get().is_none() {
            return;
        }
        let active = lock(&self.active);
        let view = self.view_locked(active.as_ref());
        let value = serde_json::to_value(&view).ok();
        if value.is_some() && value != *last_sent {
            self.publish(&view);
            *last_sent = value;
        }
    }

    async fn install_once(&self, shared: &Arc<InstallShared>) -> Result<(), String> {
        fs::create_dir_all(self.root.join(SNAPSHOTS_DIR))
            .map_err(|error| format!("create {}: {error}", self.root.display()))?;
        let lease = DirectoryLease::acquire(&self.root).map_err(|error| match error {
            LeaseError::Busy => "another Flowy process is installing the presentation capability".to_owned(),
            LeaseError::Io(message) => message,
        })?;
        remove_trash(&self.root);

        shared.set_stage(PresentationInstallStage::FetchingSkill);
        let snapshot_id = snapshot_id(&self.skill_version, &self.spec.commit);
        let layout = self.layout(&snapshot_id)?;
        fs::create_dir(&layout.snapshot_dir)
            .map_err(|error| format!("create {}: {error}", layout.snapshot_dir.display()))?;
        let mut guard = SnapshotGuard { path: layout.snapshot_dir.clone(), shared: Arc::clone(shared), armed: true };

        let fetched = {
            let spec = self.spec.clone();
            let work = layout.snapshot_dir.join(FETCH_WORK_DIR);
            let dest = layout.skill_dir.clone();
            let shared = Arc::clone(shared);
            tokio::task::spawn_blocking(move || {
                let result = source::fetch_skill(&spec, &work, &dest, &shared.progress, &shared.cancel);
                let _ = fs::remove_dir_all(&work);
                result
            })
            .await
            .map_err(|error| format!("fetch task failed: {error}"))??
        };
        validate_skill_manifest(&layout.skill_dir, &self.skill_version)?;
        check_cancel(shared)?;

        let report = {
            let shared = Arc::clone(shared);
            move |stage| shared.set_stage(stage)
        };
        let context = PrepareContext { layout: &layout, progress: &shared.progress, cancel: &shared.cancel, report: &report };
        let prepared = self.runtime.prepare(&context).await?;
        if !venv_python(&layout.venv_dir).is_file() {
            return Err("runtime preparation did not produce an interpreter".into());
        }
        check_cancel(shared)?;

        shared.set_stage(PresentationInstallStage::Committing);
        fs::write(layout.snapshot_dir.join(READY_MARKER), b"ready")
            .map_err(|error| format!("mark snapshot ready: {error}"))?;
        let previous = read_json::<InstalledRecord>(&self.root.join(CURRENT_FILE));
        let record = InstalledRecord {
            capability_id: manifest::CAPABILITY_ID.to_owned(),
            skill_version: self.skill_version.clone(),
            commit: fetched.commit,
            tree: fetched.tree,
            source_url: fetched.url,
            snapshot_id: snapshot_id.clone(),
            python_id: prepared.python_id.clone(),
            installed_at_ms: now_ms(),
        };
        write_json_atomic(&self.root.join(CURRENT_FILE), &record)?;
        guard.armed = false;

        let mut keep_snapshots = vec![snapshot_id];
        let mut keep_pythons: Vec<String> = prepared.python_id.into_iter().collect();
        if let Some(previous) = previous {
            keep_snapshots.push(previous.snapshot_id);
            keep_pythons.extend(previous.python_id);
        }
        collect_old_snapshots(&self.root.join(SNAPSHOTS_DIR), &keep_snapshots);
        python_dist::collect_unused(&layout.pythons_dir, &keep_pythons);
        drop(lease);
        Ok(())
    }

    fn base_view(&self) -> PresentationCapabilityView {
        PresentationCapabilityView {
            capability_id: manifest::CAPABILITY_ID.to_owned(),
            version: self.skill_version.clone(),
            source_commit: self.spec.commit.clone(),
            license: manifest::LICENSE.to_owned(),
            sources: self.spec.urls.clone(),
            estimated_download_bytes: manifest::ESTIMATED_DOWNLOAD_BYTES,
            status: PresentationCapabilityStatus::NotInstalled,
            stage: None,
            progress: None,
            skill_dir: None,
            python: None,
            installed_from: None,
            installed_at_ms: None,
            failure: None,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn check_cancel(shared: &InstallShared) -> Result<(), String> {
    if is_cancelled(&shared.cancel) { Err(CANCELLED.into()) } else { Ok(()) }
}

/// What the Skill catalog needs about the installed capability. Read from
/// disk only: no install machinery, no async runtime, cheap enough to call on
/// every catalog listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadySkill {
    /// Absolute `SKILL_DIR` of the active snapshot.
    pub skill_dir: PathBuf,
    /// The snapshot's private interpreter.
    pub python: PathBuf,
}

/// The pinned ppt-master snapshot under `data_dir`, when it is `Ready`.
pub fn ready_skill(data_dir: impl AsRef<Path>) -> Option<ReadySkill> {
    struct NoRuntime;
    #[async_trait::async_trait]
    impl RuntimePreparer for NoRuntime {
        async fn prepare(&self, _context: &PrepareContext<'_>) -> Result<crate::runtime::PreparedRuntime, String> {
            Err("read-only view".into())
        }
    }
    let service =
        PresentationCapabilityService::with_parts(data_dir, SourceSpec::pinned(), manifest::SKILL_VERSION, Arc::new(NoRuntime));
    let view = service.disk_view();
    if view.status != PresentationCapabilityStatus::Ready {
        return None;
    }
    Some(ReadySkill { skill_dir: PathBuf::from(view.skill_dir?), python: PathBuf::from(view.python?) })
}

/// Removes a half-built snapshot unless disarmed. Also runs when the install
/// task is aborted mid-await; setting `cancel` stops a fetch still running on
/// its blocking thread.
struct SnapshotGuard {
    path: PathBuf,
    shared: Arc<InstallShared>,
    armed: bool,
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        if self.armed {
            self.shared.cancel.store(true, Ordering::SeqCst);
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

enum LeaseError {
    Busy,
    Io(String),
}

/// Cross-process install lock. The OS releases it when the handle closes,
/// including on a hard exit; the file itself may stay on disk.
struct DirectoryLease {
    _file: fs::File,
}

impl DirectoryLease {
    fn acquire(dir: &Path) -> Result<Self, LeaseError> {
        let path = dir.join(LOCK_FILE);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| LeaseError::Io(format!("open {}: {error}", path.display())))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(LeaseError::Busy),
            Err(fs::TryLockError::Error(error)) => Err(LeaseError::Io(format!("lock {}: {error}", path.display()))),
        }
    }
}

fn short_nonce() -> String {
    let nonce = Uuid::now_v7().simple().to_string();
    nonce[nonce.len() - 6..].to_owned()
}

/// Short on purpose: Windows MAX_PATH applies to venv internals too.
fn snapshot_id(version: &str, commit: &str) -> String {
    let commit = commit.get(..12).unwrap_or(commit);
    format!("{version}-{commit}-{}", short_nonce())
}

/// Leftovers of an uninstall whose background delete did not finish.
fn remove_trash(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(TRASH_PREFIX) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Cheap sanity check on top of the commit pin: right Skill, right version.
fn validate_skill_manifest(skill_dir: &Path, version: &str) -> Result<(), String> {
    let path = skill_dir.join("SKILL.md");
    let text = fs::read_to_string(&path).map_err(|error| format!("read SKILL.md: {error}"))?;
    let has_name = text.lines().any(|line| line.trim() == format!("name: {}", manifest::SKILL_NAME));
    let has_version = text.lines().any(|line| {
        let line = line.trim();
        line == format!("version: \"{version}\"") || line == format!("version: {version}")
    });
    if !has_name || !has_version {
        return Err(format!("SKILL.md is not {} {version}", manifest::SKILL_NAME));
    }
    Ok(())
}

fn collect_old_snapshots(snapshots: &Path, keep: &[String]) {
    let Ok(entries) = fs::read_dir(snapshots) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.iter().any(|kept| kept == &name) || name.starts_with('.') {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(entry.path()) {
            // Windows may hold files of a snapshot still in use; retry next time.
            tracing::debug!(snapshot = %name, error = %error, "could not remove old snapshot");
        }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| format!("serialize {}: {error}", path.display()))?;
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("state");
    let temp = path.with_file_name(format!(".{file_name}.tmp"));
    {
        use std::io::Write;
        let mut file = fs::File::create(&temp).map_err(|error| format!("write {}: {error}", temp.display()))?;
        file.write_all(&bytes).map_err(|error| format!("write {}: {error}", temp.display()))?;
        file.sync_all().map_err(|error| format!("sync {}: {error}", temp.display()))?;
    }
    fs::rename(&temp, path).map_err(|error| format!("commit {}: {error}", path.display()))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ids_are_short_unique_and_single_component() {
        let a = snapshot_id("6.8.0", manifest::SOURCE_COMMIT);
        let b = snapshot_id("6.8.0", manifest::SOURCE_COMMIT);
        assert_ne!(a, b);
        assert!(a.starts_with("6.8.0-4a68cddaf40d-"));
        assert!(a.len() <= 30, "{a}");
        assert!(source::validate_entry_name(&a).is_ok());
    }

    #[test]
    fn skill_manifest_check_matches_name_and_version_lines() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SKILL.md"), "---\nname: ppt-master\nmetadata:\n  version: \"6.8.0\"\n---\n").unwrap();
        assert!(validate_skill_manifest(dir.path(), "6.8.0").is_ok());
        assert!(validate_skill_manifest(dir.path(), "6.9.0").is_err());
        fs::write(dir.path().join("SKILL.md"), "name: other\nversion: \"6.8.0\"\n").unwrap();
        assert!(validate_skill_manifest(dir.path(), "6.8.0").is_err());
    }

    #[test]
    fn atomic_json_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CURRENT_FILE);
        write_json_atomic(&path, &serde_json::json!({"n": 1})).unwrap();
        write_json_atomic(&path, &serde_json::json!({"n": 2})).unwrap();
        let value: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(value["n"], 2);
        assert!(!dir.path().join(".current.json.tmp").exists());
    }

    #[test]
    fn layout_rejects_traversal_in_recorded_snapshot_id() {
        let dir = tempfile::tempdir().unwrap();
        let service = PresentationCapabilityService::new(dir.path());
        assert!(service.layout("../escape").is_err());
        assert!(service.layout("6.8.0-4a68cddaf40d-abc123").is_ok());
    }

    #[test]
    fn p0_records_without_python_id_still_parse() {
        let record: InstalledRecord = serde_json::from_str(
            r#"{"capability_id":"presentation.ppt-master","skill_version":"6.8.0","commit":"c","tree":"t",
                "source_url":"u","snapshot_id":"s","installed_at_ms":1}"#,
        )
        .unwrap();
        assert!(record.python_id.is_none());
    }
}
