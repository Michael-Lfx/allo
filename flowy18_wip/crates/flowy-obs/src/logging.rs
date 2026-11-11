//! Process-global tracing + Sentry panic hook.
//!
//! Adapted from allo `tracing_init` / `nomi-config::logging` for Flowy targets.

use crate::boot::{BootNoteLevel, drain_boot_notes};
use std::path::{Path, PathBuf};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const NOISE_SUPPRESSIONS: &[&str] =
    &["hyper_util=warn", "reqwest=warn", "rustls=warn"];

/// Hold for the process lifetime so non-blocking appenders and Sentry flush.
pub struct LogGuards {
    _file: tracing_appender::non_blocking::WorkerGuard,
    _sentry: sentry::ClientInitGuard,
}

impl std::fmt::Debug for LogGuards {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LogGuards")
    }
}

/// Platform log directory (outside the app data root).
#[must_use]
pub fn default_log_dir() -> PathBuf {
    let Some(base) = directories::BaseDirs::new() else {
        return PathBuf::from("Flowy/logs");
    };
    #[cfg(target_os = "macos")]
    {
        base.home_dir().join("Library/Logs/Flowy")
    }
    #[cfg(target_os = "linux")]
    {
        base.state_dir()
            .map(|state| state.join("Flowy").join("logs"))
            .unwrap_or_else(|| base.home_dir().join(".local/state/Flowy/logs"))
    }
    #[cfg(target_os = "windows")]
    {
        base.data_dir().join("Flowy").join("logs")
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        let _ = base;
        PathBuf::from("Flowy/logs")
    }
}

/// Preferred production path: `{data_dir}/logs`.
#[must_use]
pub fn logs_dir_for_data(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

fn build_env_filter(log_level: Option<&str>) -> EnvFilter {
    let user = log_level.unwrap_or("info");
    let suppressions = NOISE_SUPPRESSIONS.join(",");
    EnvFilter::new(format!("{suppressions},{user}"))
}

fn init_sentry() -> sentry::ClientInitGuard {
    let dsn = std::env::var("SENTRY_DSN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok());
    sentry::init(sentry::ClientOptions {
        dsn,
        release: sentry::release_name!(),
        send_default_pii: false,
        traces_sample_rate: 0.0,
        ..Default::default()
    })
}

/// Install console + JSON rolling file + Sentry layers. Idempotent via `try_init`.
pub fn init_tracing(log_dir: &Path, log_level: Option<&str>) -> LogGuards {
    let _ = std::fs::create_dir_all(log_dir);
    let sentry_guard = init_sentry();

    let console_layer = fmt::layer()
        .with_target(true)
        .with_filter(build_env_filter(log_level));

    let file_appender =
        tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_suffix("flowy.log")
            .build(log_dir)
            .expect("failed to create Flowy log appender");
    let (non_blocking, file_guard) =
        tracing_appender::non_blocking(file_appender);
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_filter(build_env_filter(log_level));

    if let Err(error) = tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(sentry_tracing::layer())
        .try_init()
    {
        eprintln!(
            "[flowy-obs] global subscriber already installed, reusing it: {error}"
        );
    }

    for (level, message) in drain_boot_notes() {
        match level {
            BootNoteLevel::Info => {
                tracing::info!(target: "flowy.boot", "{message}");
            }
            BootNoteLevel::Warn => {
                tracing::warn!(target: "flowy.boot", "{message}");
            }
        }
    }

    LogGuards {
        _file: file_guard,
        _sentry: sentry_guard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_log_dir_is_nonempty() {
        assert!(!default_log_dir().as_os_str().is_empty());
    }

    #[test]
    fn data_logs_nest_under_root() {
        assert!(
            logs_dir_for_data(Path::new("/tmp/flowy-data")).ends_with("logs")
        );
    }
}
