use std::{io, sync::Arc, time::Instant};

use async_trait::async_trait;

use crate::{ProcessError, NormalizedProcessRequest, OutputBuffer, Transport};

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(unix)]
mod unix_host_lifetime;
#[cfg(unix)]
mod unix_pty;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(target_os = "linux")]
pub(crate) mod linux_recovery;
#[cfg(target_os = "linux")]
mod linux_watchdog;
#[cfg(target_os = "macos")]
mod macos_watchdog;
#[cfg(unix)]
mod unix_protocol;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExitFact {
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) cleanup_errors: Vec<String>,
}

#[async_trait]
pub(crate) trait PlatformProcess: Send + Sync {
    fn pid(&self) -> u32;
    async fn write(&self, bytes: &[u8]) -> io::Result<()>;
    async fn close_stdin(&self) -> io::Result<()>;
    async fn resize(&self, _cols: u16, _rows: u16) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process transport does not support terminal resize",
        ))
    }
    async fn interrupt(&self) -> io::Result<()>;
    async fn terminate(&self) -> io::Result<()>;
    async fn force_kill(&self) -> io::Result<()>;
    async fn wait_reaped(&self, deadline: Instant) -> io::Result<ExitFact>;
}

pub(crate) struct SpawnedPlatformProcess {
    pub(crate) owner: Arc<dyn PlatformProcess>,
}

pub(crate) async fn spawn(
    request: NormalizedProcessRequest,
    output: Arc<OutputBuffer>,
) -> Result<SpawnedPlatformProcess, ProcessError> {
    match request.transport {
        Transport::Pipe => spawn_pipe(request, output).await,
        Transport::Pty { cols, rows } => spawn_pty(request, output, cols, rows).await,
    }
}

pub(crate) async fn spawn_pipe(
    request: NormalizedProcessRequest,
    output: Arc<OutputBuffer>,
) -> Result<SpawnedPlatformProcess, ProcessError> {
    #[cfg(unix)]
    {
        unix::spawn_pipe(request, output).await
    }

    #[cfg(windows)]
    {
        windows::spawn_pipe(request, output).await
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (request, output);
        Err(ProcessError::Transport {
            reason: "platform pipe adapter is pending".to_owned(),
        })
    }
}

pub(crate) async fn spawn_pty(
    request: NormalizedProcessRequest,
    output: Arc<OutputBuffer>,
    cols: u16,
    rows: u16,
) -> Result<SpawnedPlatformProcess, ProcessError> {
    #[cfg(unix)]
    {
        unix::spawn_pty(request, output, cols, rows).await
    }

    #[cfg(windows)]
    {
        windows::spawn_pty(request, output, cols, rows).await
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (request, output, cols, rows);
        Err(ProcessError::Transport {
            reason: "platform PTY adapter is unavailable".to_owned(),
        })
    }
}

/// Create the host-lifetime process authority used by unmanaged helpers.
pub(crate) fn install_host_process_lifetime() {
    #[cfg(windows)]
    windows::install_host_process_lifetime();
    #[cfg(unix)]
    unix_host_lifetime::install_host_process_lifetime();
}

/// Kill unmanaged helper trees that must not outlive the client.
pub(crate) fn terminate_host_lifetime_descendants() {
    #[cfg(windows)]
    windows::terminate_host_lifetime_descendants();
    #[cfg(unix)]
    unix_host_lifetime::terminate_host_lifetime_descendants();
}

pub(crate) fn spawn_host_lifetime_std(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        return windows::spawn_host_lifetime_std(command);
    }
    #[cfg(unix)]
    {
        return unix_host_lifetime::spawn_host_lifetime_std(command);
    }
    #[cfg(not(any(unix, windows)))]
    {
        command.spawn()
    }
}

pub(crate) fn spawn_host_lifetime_tokio(
    command: &mut tokio::process::Command,
) -> io::Result<tokio::process::Child> {
    #[cfg(windows)]
    {
        return windows::spawn_host_lifetime_tokio(command);
    }
    #[cfg(unix)]
    {
        return unix_host_lifetime::spawn_host_lifetime_tokio(command);
    }
    #[cfg(not(any(unix, windows)))]
    {
        command.spawn()
    }
}
