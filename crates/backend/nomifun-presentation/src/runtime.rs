//! Python runtime preparation for one snapshot: private interpreter, venv,
//! locked dependencies, self-test.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use nomi_process_runtime::{
    CapabilityPolicy, CommandSpec, OutputCursor, PollResult, ProcessError, ProcessOutcome,
    ProcessOwner, ProcessPolicy, ProcessRequest, ProcessSupervisor, SandboxPolicy,
    SupervisorConfig, Transport, normalize_request,
};
use nomifun_api_types::PresentationInstallStage;
use uuid::Uuid;

use crate::manifest;
use crate::progress::{CANCELLED, InstallProgress, is_cancelled};
use crate::python_dist::{self, PythonSource};

const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const ERROR_TAIL_CHARS: usize = 2_000;
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const VENV_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PIP_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const SELF_TEST_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const POLL_SLICE: Duration = Duration::from_millis(200);

/// Paths of one snapshot being prepared.
#[derive(Debug, Clone)]
pub struct RuntimeLayout {
    pub snapshot_dir: PathBuf,
    pub skill_dir: PathBuf,
    pub venv_dir: PathBuf,
    /// Shared interpreter homes (`<capability>/python/`).
    pub pythons_dir: PathBuf,
}

impl RuntimeLayout {
    pub fn python(&self) -> PathBuf {
        venv_python(&self.venv_dir)
    }
}

/// Everything a preparer may touch while it runs.
pub struct PrepareContext<'a> {
    pub layout: &'a RuntimeLayout,
    pub progress: &'a InstallProgress,
    pub cancel: &'a AtomicBool,
    /// Moves the install to a new stage (and resets the progress counters).
    pub report: &'a (dyn Fn(PresentationInstallStage) + Send + Sync),
}

#[derive(Debug, Clone, Default)]
pub struct PreparedRuntime {
    /// Interpreter home under `pythons_dir` the venv was built from; kept
    /// alive by garbage collection while the snapshot is.
    pub python_id: Option<String>,
}

/// Builds the venv for a snapshot. A trait so service tests can run the whole
/// install state machine without Python or the network.
#[async_trait]
pub trait RuntimePreparer: Send + Sync {
    async fn prepare(&self, context: &PrepareContext<'_>) -> Result<PreparedRuntime, String>;
}

/// Production preparer: pinned python-build-standalone + hashed lock file.
pub struct ManagedPythonRuntime {
    supervisor: Arc<ProcessSupervisor>,
}

impl ManagedPythonRuntime {
    pub fn new() -> Self {
        Self { supervisor: ProcessSupervisor::new(SupervisorConfig::default()) }
    }
}

impl Default for ManagedPythonRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RuntimePreparer for ManagedPythonRuntime {
    async fn prepare(&self, context: &PrepareContext<'_>) -> Result<PreparedRuntime, String> {
        let layout = context.layout;
        let cwd = &layout.snapshot_dir;
        let exec = |program: PathBuf, args: Vec<String>, timeout: Duration, extra: Vec<(&'static str, String)>| {
            let supervisor = Arc::clone(&self.supervisor);
            async move { run(&supervisor, &program, &args, cwd, timeout, &extra, context.cancel).await }
        };

        (context.report)(PresentationInstallStage::PreparingPython);
        let source = PythonSource::pinned()?;
        let mut home = python_dist::ensure(&layout.pythons_dir, &source, context.progress, context.cancel).await?;
        let probe = vec!["-c".to_owned(), manifest::PYTHON_PROBE.to_owned()];
        if let Err(error) = exec(python_dist::interpreter(&home), probe.clone(), PROBE_TIMEOUT, vec![]).await {
            if error == CANCELLED {
                return Err(error);
            }
            // A damaged cached runtime: rebuild it once.
            tracing::warn!(error = %error, "cached Python runtime failed its probe; reinstalling");
            std::fs::remove_dir_all(&home).map_err(|error| format!("remove broken Python: {error}"))?;
            home = python_dist::ensure(&layout.pythons_dir, &source, context.progress, context.cancel).await?;
            exec(python_dist::interpreter(&home), probe, PROBE_TIMEOUT, vec![])
                .await
                .map_err(|error| format!("Python probe: {error}"))?;
        }
        let venv = layout.venv_dir.to_string_lossy().into_owned();
        exec(python_dist::interpreter(&home), args(&["-m", "venv", &venv]), VENV_TIMEOUT, vec![])
            .await
            .map_err(|error| format!("create venv: {error}"))?;
        let python = layout.python();
        if !python.is_file() {
            return Err(format!("venv did not produce {}", python.display()));
        }

        (context.report)(PresentationInstallStage::InstallingDependencies);
        let lock = layout.snapshot_dir.join("requirements-core.lock.txt");
        std::fs::write(&lock, manifest::CORE_LOCK).map_err(|error| format!("write lock file: {error}"))?;
        let lock = lock.to_string_lossy().into_owned();
        let pip = args(&[
            "-m",
            "pip",
            "install",
            "--require-hashes",
            "--only-binary=:all:",
            "--no-deps",
            "--disable-pip-version-check",
            "-r",
            &lock,
        ]);
        // First the user's own pip setup (PIP_INDEX_URL, pip.conf), then a
        // mirror reachable from mainland China. Hashes pin the bytes either way.
        if let Err(first) = exec(python.clone(), pip.clone(), PIP_TIMEOUT, vec![]).await {
            if first == CANCELLED {
                return Err(first);
            }
            tracing::warn!(error = %first, "pip install failed; retrying with the fallback index");
            let fallback = vec![("PIP_INDEX_URL", manifest::PIP_FALLBACK_INDEX.to_owned())];
            exec(python.clone(), pip, PIP_TIMEOUT, fallback)
                .await
                .map_err(|second| format!("install dependencies: {second} (default index: {first})"))?;
        }

        (context.report)(PresentationInstallStage::SelfTest);
        let scripts = layout.skill_dir.join("scripts");
        let self_tests: [(&str, Vec<String>); 4] = [
            ("import check", args(&["-c", manifest::CORE_IMPORT_CHECK])),
            ("attribution guard", vec![scripts.join("attribution_guard.py").to_string_lossy().into_owned()]),
            ("pptx smoke test", args(&["-c", "from pptx import Presentation; Presentation()"])),
            // Imports the whole converter package graph (drawingml, pathops, harfbuzz).
            ("svg_to_pptx", vec![scripts.join("svg_to_pptx.py").to_string_lossy().into_owned(), "--help".to_owned()]),
        ];
        for (name, test_args) in self_tests {
            exec(python.clone(), test_args, SELF_TEST_TIMEOUT, vec![])
                .await
                .map_err(|error| format!("{name}: {error}"))?;
        }
        Ok(PreparedRuntime { python_id: Some(source.id) })
    }
}

pub fn venv_python(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn child_env(extra: &[(&'static str, String)]) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> = [
        ("PYTHONUTF8", "1"),
        ("PYTHONIOENCODING", "utf-8"),
        // Keep the pristine Skill snapshot free of __pycache__.
        ("PYTHONDONTWRITEBYTECODE", "1"),
        // The private runtime must not see the user's site-packages.
        ("PYTHONNOUSERSITE", "1"),
        ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
        ("PIP_NO_INPUT", "1"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect();
    for (key, value) in nomifun_net::proxy::child_proxy_env(std::iter::empty()) {
        env.insert(key.into(), value.into());
    }
    for (key, value) in extra {
        env.insert(OsString::from(*key), OsString::from(value));
    }
    env
}

/// Run one install helper to completion. Returns stdout+stderr on exit code 0;
/// on cancel, stops the process tree and returns exactly [`CANCELLED`].
async fn run(
    supervisor: &Arc<ProcessSupervisor>,
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    extra_env: &[(&'static str, String)],
    cancel: &AtomicBool,
) -> Result<String, String> {
    let spec = ProcessSpec { program, args, cwd, cwd_roots: &[cwd.to_path_buf()], timeout, extra_env, output_limit: MAX_OUTPUT_BYTES };
    let finished = run_process(supervisor, &spec, cancel).await?;
    match finished.exit {
        ProcessExit::Code(0) => Ok(finished.output),
        ProcessExit::Code(code) => Err(format!("exited with {code}: {}", tail(&finished.output))),
        ProcessExit::Killed => Err(format!("terminated by a signal: {}", tail(&finished.output))),
        ProcessExit::TimedOut => Err(format!("timed out: {}", tail(&finished.output))),
    }
}

/// One supervised child process.
pub(crate) struct ProcessSpec<'a> {
    pub program: &'a Path,
    pub args: &'a [String],
    pub cwd: &'a Path,
    /// Directories the child may use as its cwd.
    pub cwd_roots: &'a [PathBuf],
    pub timeout: Duration,
    pub extra_env: &'a [(&'static str, String)],
    pub output_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessExit {
    Code(i32),
    /// Exited without a code (killed by a signal).
    Killed,
    TimedOut,
}

pub(crate) struct FinishedProcess {
    pub exit: ProcessExit,
    /// Combined stdout+stderr; the newest `output_limit` bytes.
    pub output: String,
    /// Older output dropped to stay within `output_limit`.
    pub dropped_bytes: u64,
}

/// Run `spec` to completion under the supervisor. `Err` means the process
/// could not be started, was lost, or was cancelled ([`CANCELLED`]).
pub(crate) async fn run_process(
    supervisor: &Arc<ProcessSupervisor>,
    spec: &ProcessSpec<'_>,
    cancel: &AtomicBool,
) -> Result<FinishedProcess, String> {
    if is_cancelled(cancel) {
        return Err(CANCELLED.into());
    }
    let request = ProcessRequest {
        owner: ProcessOwner::new(Uuid::now_v7(), Uuid::now_v7()),
        command: CommandSpec::Program {
            program: spec.program.as_os_str().to_owned(),
            args: spec.args.iter().map(OsString::from).collect(),
        },
        cwd: spec.cwd.to_path_buf(),
        env: child_env(spec.extra_env),
        transport: Transport::Pipe,
        policy: ProcessPolicy {
            output_limit_bytes: spec.output_limit,
            deadline: Some(Instant::now() + spec.timeout),
            ..Default::default()
        },
        capability: CapabilityPolicy {
            cwd_roots: spec.cwd_roots.to_vec(),
            sandbox: SandboxPolicy::UnrestrictedLocalOwner,
        },
    };
    let normalized = normalize_request(request, spec.cwd).map_err(|error| error.to_string())?;
    let handle = supervisor.start(normalized).await.map_err(|error| error.to_string())?;
    loop {
        if is_cancelled(cancel) {
            let _ = supervisor.cancel(&handle.owner, &handle.session_id).await;
            return Err(CANCELLED.into());
        }
        let poll = supervisor
            .poll(&handle.owner, &handle.session_id, OutputCursor::START, Instant::now() + POLL_SLICE)
            .await;
        let finished = |exit, output: nomi_process_runtime::OutputSnapshot| FinishedProcess {
            exit,
            output: output.text(),
            dropped_bytes: output.dropped_bytes,
        };
        match poll {
            Ok(PollResult::Finished(outcome)) => {
                return match outcome {
                    ProcessOutcome::Exited { code: Some(code), output, .. } => Ok(finished(ProcessExit::Code(code), output)),
                    ProcessOutcome::Exited { code: None, output, .. } => Ok(finished(ProcessExit::Killed, output)),
                    ProcessOutcome::TimedOut { output, .. } => Ok(finished(ProcessExit::TimedOut, output)),
                    ProcessOutcome::Cancelled { output, .. } => Err(format!("cancelled: {}", tail(&output.text()))),
                    ProcessOutcome::SpawnFailed(error) => Err(format!("spawn failed: {}", error.message)),
                    ProcessOutcome::Lost { output, .. } => Err(format!("lost: {}", tail(&output.text()))),
                };
            }
            Ok(PollResult::Running { .. }) => continue,
            Err(ProcessError::SessionNotFound { .. }) => return Err("process session disappeared".into()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// pip errors end with the useful line; keep the end, not the start.
fn tail(text: &str) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= ERROR_TAIL_CHARS {
        return text.to_owned();
    }
    let skip = count - ERROR_TAIL_CHARS;
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_python_is_platform_specific() {
        let python = venv_python(Path::new("snap/venv"));
        let text = python.to_string_lossy();
        if cfg!(windows) {
            assert!(text.ends_with("python.exe") && text.contains("Scripts"));
        } else {
            assert!(text.ends_with("python") && text.contains("bin"));
        }
    }

    #[test]
    fn child_env_isolates_python_and_applies_overrides() {
        let env = child_env(&[("PIP_INDEX_URL", "https://mirror.example/simple/".to_owned())]);
        assert_eq!(env.get(&OsString::from("PYTHONNOUSERSITE")), Some(&OsString::from("1")));
        assert_eq!(
            env.get(&OsString::from("PIP_INDEX_URL")),
            Some(&OsString::from("https://mirror.example/simple/"))
        );
    }

    #[test]
    fn error_tail_keeps_the_end() {
        let long = format!("{}END", "x".repeat(ERROR_TAIL_CHARS * 2));
        let kept = tail(&long);
        assert!(kept.ends_with("END"));
        assert!(kept.chars().count() <= ERROR_TAIL_CHARS + 1);
    }
}
