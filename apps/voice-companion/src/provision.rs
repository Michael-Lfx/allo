//! Automatic provisioning and model self-healing on companion host boot.
//!
//! Authoritative references:
//! - `docs/architecture/voice-agent/interaction.zh.md` §3.4.4, §5.1 (TC-AM-015)
//! - `docs/architecture/voice-agent/model-management.zh.md` §4 (Scenario B: Headless Auto-Provisioning)
//! - `docs/architecture/voice-agent/execution/wave-1-acoustic.zh.md` §2 (T1-08), §3 (VT-1-10)

use std::sync::Arc;
use nomifun_api_types::voice::ModelInstallState;
use nomifun_voice_models::ModelManager;
use tracing::{info, warn};

use crate::config::Config;

/// Inspects local model assets on boot. If models are missing and `auto_download = true`,
/// automatically triggers background self-healing download and transitions state to `ProbingSource`.
///
/// Returns `true` if auto-download was initiated, `false` otherwise.
pub async fn check_and_auto_download(config: &Config, manager: &Arc<ModelManager>) -> bool {
    let status = manager.get_status().await;

    // Check if models need to be installed
    let needs_download = matches!(status.state, ModelInstallState::NotInstalled);
    if !needs_download {
        info!(
            state = ?status.state,
            "Local voice models already present or active; skipping auto-download."
        );
        return false;
    }

    if !config.models.auto_download {
        info!("Voice models not installed and auto_download is disabled; awaiting manual installation.");
        return false;
    }

    info!(
        models_dir = %config.models.models_dir.display(),
        prefer_mirror = %config.models.prefer_mirror,
        "Auto-download enabled and models missing. Transitioning to ProbingSource and launching installer."
    );

    // Launch model installation pipeline.
    // manager.install transitions state to ProbingSource, broadcasts the SSE event,
    // and asynchronously runs run_install_pipeline in a background task.
    match manager.install(None).await {
        Ok(resp) => {
            info!(
                accepted = resp.accepted,
                state = ?resp.state,
                "Auto-download installation task initiated."
            );
        }
        Err(err) => {
            warn!(
                error = %err,
                "ModelManager::install returned error during auto-provisioning."
            );
        }
    }

    true
}

/// Helper to coordinate boot self-check and auto-provisioning.
pub async fn bootstrap_models(config: &Config, manager: Arc<ModelManager>) -> bool {
    check_and_auto_download(config, &manager).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_auto_download_when_already_ready() {
        let temp_dir = tempdir().unwrap();
        let mut config = Config::default();
        config.models.models_dir = temp_dir.path().to_path_buf();
        config.models.auto_download = true;

        // Match the fixture's real hashes; layout alone cannot grant Ready.
        let mut manifest = std::collections::HashMap::new();
        for &rel_path in nomifun_voice_models::EXPECTED_FILES {
            let p = temp_dir.path().join(rel_path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, b"mock model binary content").unwrap();
            if nomifun_voice_models::verifier::DOWNLOAD_FILES.contains(&rel_path) {
                manifest.insert(rel_path.to_owned(), nomifun_voice_models::verifier::compute_sha256(b"mock model binary content"));
            }
        }

        let manager = Arc::new(ModelManager::with_config(temp_dir.path(), vec![], manifest));
        let status = manager.get_status().await;
        assert!(
            matches!(status.state, ModelInstallState::Ready { .. }),
            "Expected Ready state when all files are populated, got: {:?}",
            status.state
        );

        let triggered = check_and_auto_download(&config, &manager).await;
        assert!(!triggered, "Should not trigger auto-download when Ready");
    }
}
