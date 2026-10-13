//! Voice Companion Child-Process Supervisor (`ai-mate`)
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md
//!
//! Supervises the independent `ai-mate` daemon process alongside the Flowy
//! desktop application:
//! - Hidden console window, completely headless with zero extraneous tray icons;
//! - Strict whole-tree lifecycle authority via `nomi_process_runtime::ChildProcessBuilder`;
//! - Windows Job Object / Unix process-group anchoring so children die immediately
//!   when Flowy desktop shuts down or terminates unexpectedly;
//! - Auto-start on application boot when models are ready and user has enabled the companion.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use nomi_process_runtime::{ChildProcessBuilder, ManagedChildProcess};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub models_ready: bool,
    pub autostart: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleResponse {
    pub success: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VoiceCompanionPersistedConfig {
    pub autostart: bool,
}

pub struct VoiceCompanionProcessManager {
    child: Arc<Mutex<Option<ManagedChildProcess>>>,
    models_manager: Arc<nomifun_voice_models::ModelManager>,
    config_path: PathBuf,
}

impl VoiceCompanionProcessManager {
    pub fn new(models_manager: Arc<nomifun_voice_models::ModelManager>, data_dir: &Path) -> Self {
        let config_path = data_dir.join("config").join("voice_companion.json");
        Self {
            child: Arc::new(Mutex::new(None)),
            models_manager,
            config_path,
        }
    }

    /// Resolves the compiled `ai-mate` executable path on disk.
    pub fn resolve_ai_mate_binary() -> Option<PathBuf> {
        let exe_name = if cfg!(windows) { "ai-mate.exe" } else { "ai-mate" };

        // 1. Explicit environment variable override
        if let Some(env_path) = std::env::var_os("AI_MATE_BIN").map(PathBuf::from) {
            if env_path.is_file() {
                return Some(env_path);
            }
        }

        // 2. Colocated in same directory as current running executable (production bundle)
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                let candidate = parent.join(exe_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        // 3. Known development build target directories
        let candidates = [
            Path::new("target/debug").join(exe_name),
            Path::new("target/release").join(exe_name),
            Path::new("build.noindex/debug").join(exe_name),
            Path::new("build.noindex/release").join(exe_name),
        ];

        for c in &candidates {
            if c.is_file() {
                return Some(c.clone());
            }
        }

        // 4. Resolve relative to workspace root if cwd is a sub-crate
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let root = Path::new(&manifest_dir).ancestors().nth(2);
            if let Some(r) = root {
                for dir in ["target/debug", "target/release", "build.noindex/debug", "build.noindex/release"] {
                    let c = r.join(dir).join(exe_name);
                    if c.is_file() {
                        return Some(c);
                    }
                }
            }
        }

        None
    }

    fn read_autostart_config(&self) -> bool {
        if self.config_path.is_file() {
            if let Ok(raw) = std::fs::read_to_string(&self.config_path) {
                if let Ok(cfg) = serde_json::from_str::<VoiceCompanionPersistedConfig>(&raw) {
                    return cfg.autostart;
                }
            }
        }
        false
    }

    fn write_autostart_config(&self, autostart: bool) {
        if let Some(parent) = self.config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cfg = VoiceCompanionPersistedConfig { autostart };
        if let Ok(serialized) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&self.config_path, serialized);
        }
    }

    /// Queries the real running status of the background companion daemon.
    pub async fn status(&self) -> VoiceCompanionStatus {
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        let autostart = self.read_autostart_config();

        let mut lock = self.child.lock().await;
        let mut running = false;
        let mut pid = None;

        if let Some(ref mut child) = *lock {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    info!(?exit_status, "Voice companion child process has terminated");
                    *lock = None;
                }
                Ok(None) => {
                    running = true;
                    pid = child.id();
                }
                Err(err) => {
                    warn!(error = %err, "Failed to check status of voice companion process");
                    *lock = None;
                }
            }
        }

        VoiceCompanionStatus {
            running,
            pid,
            models_ready,
            autostart,
            error: None,
        }
    }

    /// Launches the `ai-mate` companion daemon under whole-tree supervision.
    pub async fn start(&self) -> Result<(), String> {
        let mut lock = self.child.lock().await;

        // Check if already active
        if let Some(ref mut child) = *lock {
            if let Ok(None) = child.try_wait() {
                info!(pid = child.id(), "Voice companion process is already running");
                return Ok(());
            }
        }

        // Verify models are Ready before attempting launch
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        if !models_ready {
            return Err("Voice acoustic models are not installed yet".to_string());
        }

        let bin_path = Self::resolve_ai_mate_binary().ok_or_else(|| {
            "ai-mate binary not found. Please compile ai-mate with `cargo build -p ai-mate` first.".to_string()
        })?;

        info!(binary = %bin_path.display(), "Spawning AI Mate voice companion daemon...");

        let mut builder = ChildProcessBuilder::new(&bin_path);
        builder.arg("--bypass-pairing");
        builder.arg("--host").arg("127.0.0.1:18080");
        builder.stdout(std::process::Stdio::null());
        builder.stderr(std::process::Stdio::inherit());

        let managed = builder
            .spawn_managed()
            .map_err(|e| format!("Failed to spawn ai-mate process: {e}"))?;

        let pid = managed.id();
        info!(?pid, "AI Mate voice companion daemon successfully started");

        *lock = Some(managed);
        drop(lock);

        self.write_autostart_config(true);
        Ok(())
    }

    /// Gracefully shuts down and reaps the companion child process tree.
    pub async fn stop(&self) -> Result<(), String> {
        self.write_autostart_config(false);

        let mut lock = self.child.lock().await;
        if let Some(mut child) = lock.take() {
            info!(pid = child.id(), "Stopping AI Mate voice companion daemon...");
            if let Err(e) = child.shutdown().await {
                warn!(error = %e, "Error shutting down ai-mate process tree");
            } else {
                info!("AI Mate voice companion daemon stopped cleanly");
            }
        }
        Ok(())
    }

    /// Toggles the companion daemon on or off.
    pub async fn toggle(&self, enabled: bool) -> Result<bool, String> {
        if enabled {
            self.start().await?;
            Ok(true)
        } else {
            self.stop().await?;
            Ok(false)
        }
    }

    /// Automatically starts the companion daemon on Flowy startup if enabled and models are Ready.
    pub async fn auto_start_if_enabled(&self) {
        if self.read_autostart_config() {
            let models_ready = self.models_manager.get_status().await.state.is_ready();
            if models_ready {
                info!("Auto-starting AI Mate voice companion on application boot...");
                if let Err(err) = self.start().await {
                    warn!(error = %err, "Failed to auto-start AI Mate companion on boot");
                }
            } else {
                info!("Voice companion autostart is enabled, but acoustic models are not yet ready; standing by");
            }
        }
    }
}

pub fn voice_companion_routes(manager: Arc<VoiceCompanionProcessManager>) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/api/voice/companion/status", get(companion_status_handler))
        .route("/api/voice/companion/toggle", post(companion_toggle_handler))
        .route("/api/voice/companion/events", get(companion_events_handler))
        .with_state(manager)
}

async fn companion_status_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceCompanionStatus> {
    axum::Json(manager.status().await)
}

async fn companion_toggle_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<VoiceCompanionToggleRequest>,
) -> axum::Json<VoiceCompanionToggleResponse> {
    match manager.toggle(payload.enabled).await {
        Ok(running) => axum::Json(VoiceCompanionToggleResponse {
            success: true,
            running,
            error: None,
        }),
        Err(err) => axum::Json(VoiceCompanionToggleResponse {
            success: false,
            running: false,
            error: Some(err),
        }),
    }
}

async fn companion_events_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use futures_util::StreamExt;

    let status = manager.status().await;
    if !status.running {
        return (
            StatusCode::OK,
            [
                ("content-type", "text/event-stream"),
                ("cache-control", "no-cache"),
            ],
            "data: {\"type\":\"Idle\"}\n\n",
        )
            .into_response();
    }

    let client = reqwest::Client::new();
    match client.get("http://127.0.0.1:18080/api/voice/events").send().await {
        Ok(resp) => {
            let status = resp.status();
            let mut builder = axum::response::Response::builder().status(status);
            if let Some(ct) = resp.headers().get("content-type") {
                builder = builder.header("content-type", ct);
            } else {
                builder = builder.header("content-type", "text/event-stream");
            }
            builder = builder.header("cache-control", "no-cache");
            builder = builder.header("connection", "keep-alive");

            let body_stream = resp
                .bytes_stream()
                .map(|item| item.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
            builder
                .body(axum::body::Body::from_stream(body_stream))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => (
            StatusCode::OK,
            [
                ("content-type", "text/event-stream"),
                ("cache-control", "no-cache"),
            ],
            "data: {\"type\":\"Idle\"}\n\n",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_companion_status_serialization() {
        let status = VoiceCompanionStatus {
            running: true,
            pid: Some(12345),
            models_ready: true,
            autostart: true,
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"models_ready\":true"));

        let deserialized: VoiceCompanionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.running, true);
        assert_eq!(deserialized.pid, Some(12345));
    }

    #[test]
    fn test_autostart_config_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let models_mgr = Arc::new(nomifun_voice_models::ModelManager::new(temp_dir.path().to_path_buf()));
        let manager = VoiceCompanionProcessManager::new(models_mgr, temp_dir.path());

        assert_eq!(manager.read_autostart_config(), false);
        manager.write_autostart_config(true);
        assert_eq!(manager.read_autostart_config(), true);
        manager.write_autostart_config(false);
        assert_eq!(manager.read_autostart_config(), false);
    }

    #[test]
    fn test_resolve_ai_mate_binary_candidate_search() {
        let resolved = VoiceCompanionProcessManager::resolve_ai_mate_binary();
        if let Some(path) = resolved {
            assert!(path.is_file());
            assert!(path.to_string_lossy().contains("ai-mate"));
        }
    }
}
