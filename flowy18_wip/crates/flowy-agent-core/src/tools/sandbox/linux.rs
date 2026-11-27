//! Linux Shell execution through Bubblewrap using pi's native process lifecycle.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rpi_tools::env::{AppendFile, ReadFile, ShellOverride};
use rpi_tools::{
    ExecutionEnv, ExecutionError, ExecutionErrorCode, FileContent, FileError,
    FileInfo, FileSystem, OsExecutionEnv, Shell, ShellExecOptions, ShellOutput,
    WriterLease,
};
use tokio_util::sync::CancellationToken;

use super::{
    SandboxAvailability, SandboxBackend, SandboxPolicy, linux_bwrap_args,
};

const BWRAP: &str = "/usr/bin/bwrap";
const SHELL_ENV: [&str; 8] = [
    "HOME", "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "LOGNAME", "PATH", "TERM",
];

pub(super) fn availability() -> &'static SandboxAvailability {
    static AVAILABILITY: OnceLock<SandboxAvailability> = OnceLock::new();
    AVAILABILITY.get_or_init(|| {
        let binary_detected = Path::new(BWRAP).is_file();
        if binary_detected && probe() {
            SandboxAvailability::Enforced {
                backend: SandboxBackend::LinuxBubblewrap,
            }
        } else {
            SandboxAvailability::Degraded {
                platform: "linux",
                planned_backend: SandboxBackend::LinuxBubblewrap,
                binary_detected,
                reason: "bwrap unavailable or namespace setup failed; active layers: path_guard + shell_policy + process_group_reaping",
            }
        }
    })
}

fn probe() -> bool {
    let policy = SandboxPolicy {
        writable_roots: Vec::new(),
        protected_paths: Vec::new(),
        allow_network: false,
    };
    let Ok(mut child) = Command::new(BWRAP)
        .args(linux_bwrap_args(&policy, Path::new("/")))
        .args(["/bin/bash", "-c", "exit 0"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

pub(in crate::tools) fn execution_env(
    cwd: &Path,
    policy: SandboxPolicy,
) -> Arc<dyn ExecutionEnv> {
    let inner = OsExecutionEnv::with_cwd(cwd.to_path_buf());
    if availability().is_enforced() {
        Arc::new(SandboxedExecutionEnv { inner, policy })
    } else {
        Arc::new(inner)
    }
}

struct SandboxedExecutionEnv {
    inner: OsExecutionEnv,
    policy: SandboxPolicy,
}

fn setup_error(message: impl Into<String>) -> ExecutionError {
    ExecutionError::new(ExecutionErrorCode::SpawnError, message)
}

fn checked_path(path: PathBuf) -> Result<PathBuf, ExecutionError> {
    if path.to_str().is_none() {
        return Err(setup_error("sandbox paths must be valid UTF-8"));
    }
    Ok(path)
}

#[async_trait]
impl Shell for SandboxedExecutionEnv {
    async fn exec<'a>(
        &'a self,
        command: &str,
        mut options: ShellExecOptions<'a>,
    ) -> Result<ShellOutput, ExecutionError> {
        rpi_tools::env::check_cancel_exec(options.cancel)?;
        let cwd = options.cwd.as_deref().unwrap_or_else(|| self.inner.cwd());
        let cwd = checked_path(cwd.canonicalize().map_err(|error| {
            setup_error(format!("resolve sandbox working directory: {error}"))
        })?)?;

        let mut policy = self.policy.clone();
        for root in &mut policy.writable_roots {
            *root = checked_path(root.canonicalize().map_err(|error| {
                setup_error(format!("resolve sandbox workspace: {error}"))
            })?)?;
            if *root == Path::new("/") {
                return Err(setup_error(
                    "sandbox cannot bind the host root as writable",
                ));
            }
        }
        if !policy
            .writable_roots
            .iter()
            .any(|root| cwd.starts_with(root))
        {
            return Err(setup_error(
                "sandbox working directory is outside the workspace",
            ));
        }
        policy.protected_paths.clear();
        for path in &self.policy.protected_paths {
            match path.canonicalize() {
                Ok(path) => policy.protected_paths.push(checked_path(path)?),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(setup_error(format!(
                        "resolve sandbox protected path: {error}"
                    )));
                }
            }
        }

        let mut args = linux_bwrap_args(&policy, &cwd);
        args.extend(["/bin/bash".to_owned(), "-c".to_owned()]);
        options.shell = Some(ShellOverride {
            program: PathBuf::from(BWRAP),
            args,
        });
        options.cwd = Some(cwd);

        let mut env = HashMap::new();
        if options.inherit_env {
            for name in SHELL_ENV {
                if let Ok(value) = std::env::var(name) {
                    env.insert(name.to_owned(), value);
                }
            }
        }
        if let Some(overrides) = options.env.take() {
            env.extend(
                overrides
                    .into_iter()
                    .filter(|(name, _)| SHELL_ENV.contains(&name.as_str())),
            );
        }
        options.inherit_env = false;
        options.env = Some(env);
        self.inner.exec(command, options).await
    }

    async fn cleanup(&self) {
        Shell::cleanup(&self.inner).await;
    }
}

// Pi still owns the host filesystem side of the Shell tool, including output
// spill files. The override applies when spawning the shell subprocess.
#[async_trait]
impl FileSystem for SandboxedExecutionEnv {
    fn cwd(&self) -> &Path {
        self.inner.cwd()
    }

    async fn absolute_path(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.absolute_path(path, cancel).await
    }

    async fn join_path(
        &self,
        parts: &[&str],
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.join_path(parts, cancel).await
    }

    async fn read_text_file(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, FileError> {
        self.inner.read_text_file(path, cancel).await
    }

    async fn read_text_lines(
        &self,
        path: &str,
        max_lines: Option<usize>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<String>, FileError> {
        self.inner.read_text_lines(path, max_lines, cancel).await
    }

    async fn read_binary_file(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<u8>, FileError> {
        self.inner.read_binary_file(path, cancel).await
    }

    async fn write_file(
        &self,
        path: &str,
        content: FileContent,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.write_file(path, content, cancel).await
    }

    async fn append_file(
        &self,
        path: &str,
        content: FileContent,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.append_file(path, content, cancel).await
    }

    async fn open_append_file(
        &self,
        path: &str,
    ) -> Result<Option<Box<dyn AppendFile>>, FileError> {
        self.inner.open_append_file(path).await
    }

    async fn try_acquire_writer_lease(
        &self,
        path: &str,
    ) -> Result<Option<Box<dyn WriterLease>>, FileError> {
        self.inner.try_acquire_writer_lease(path).await
    }

    async fn sync_file(
        &self,
        path: &str,
        data_only: bool,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.sync_file(path, data_only, cancel).await
    }

    async fn sync_directory(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.sync_directory(path, cancel).await
    }

    async fn open_read_file(
        &self,
        path: &str,
    ) -> Result<Option<Arc<dyn ReadFile>>, FileError> {
        self.inner.open_read_file(path).await
    }

    async fn truncate_file(
        &self,
        path: &str,
        len: u64,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.truncate_file(path, len, cancel).await
    }

    async fn rename_file(
        &self,
        source: &str,
        dest: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.rename_file(source, dest, cancel).await
    }

    async fn file_info(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<FileInfo, FileError> {
        self.inner.file_info(path, cancel).await
    }

    async fn list_dir(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<FileInfo>, FileError> {
        self.inner.list_dir(path, cancel).await
    }

    async fn canonical_path(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.canonical_path(path, cancel).await
    }

    async fn exists(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<bool, FileError> {
        self.inner.exists(path, cancel).await
    }

    async fn create_dir(
        &self,
        path: &str,
        recursive: bool,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.create_dir(path, recursive, cancel).await
    }

    async fn remove(
        &self,
        path: &str,
        recursive: bool,
        force: bool,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        self.inner.remove(path, recursive, force, cancel).await
    }

    async fn create_temp_dir(
        &self,
        prefix: Option<&str>,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.create_temp_dir(prefix, cancel).await
    }

    async fn create_temp_file(
        &self,
        prefix: &str,
        suffix: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.create_temp_file(prefix, suffix, cancel).await
    }

    async fn cleanup(&self) {
        FileSystem::cleanup(&self.inner).await;
    }
}

impl ExecutionEnv for SandboxedExecutionEnv {}

#[cfg(test)]
mod tests;
