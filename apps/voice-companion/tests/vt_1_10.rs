//! Vertical Slice Test VT-1-10:
//! 空模型目录 + `auto_download=true` 开机自动进 `ProbingSource` 并进入下载安装流程，全流程无人工干预。
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4, §5.1 (TC-AM-015)
//! - `model-management.zh.md` §4 (Scenario B: Headless Auto-Provisioning)
//! - `execution/wave-1-acoustic.zh.md` §3 (VT-1-10), §2 (T1-08)

use std::sync::Arc;
use tempfile::tempdir;

use voice_companion::config::Config;
use voice_companion::provision::check_and_auto_download;
use nomifun_api_types::voice::ModelInstallState;
use nomifun_voice_models::ModelManager;

#[tokio::test]
async fn test_vt_1_10_empty_dir_auto_download_true_triggers_probing_source() {
    let temp_dir = tempdir().expect("create temp dir");
    let mut config = Config::default();
    config.models.models_dir = temp_dir.path().to_path_buf();
    config.models.auto_download = true;

    let manager = Arc::new(ModelManager::new(&config.models.models_dir));

    // 1. Initial boot check: empty directory must report NotInstalled
    let initial_status = manager.get_status().await;
    assert_eq!(
        initial_status.state,
        ModelInstallState::NotInstalled,
        "Empty models directory must initially report NotInstalled"
    );

    // 2. Subscribe to progress events before boot self-healing triggers
    let mut progress_rx = manager.subscribe_progress();

    // 3. Companion host boot self-check triggers auto-download without any human interaction
    let triggered = check_and_auto_download(&config, &manager).await;
    assert!(
        triggered,
        "auto_download=true must trigger auto self-healing provision"
    );

    // 4. Assert direct status transitioned to ProbingSource
    let current_status = manager.get_status().await;
    assert_eq!(
        current_status.state,
        ModelInstallState::ProbingSource,
        "Status must transition to ProbingSource automatically on boot"
    );

    // 5. Assert progress SSE channel received the ProbingSource event
    let event = progress_rx
        .recv()
        .await
        .expect("progress channel must receive auto-download event");
    assert_eq!(
        event.data,
        ModelInstallState::ProbingSource,
        "SSE stream must project ProbingSource to pairing page listeners"
    );
}

#[tokio::test]
async fn test_vt_1_10_auto_download_false_does_not_trigger() {
    let temp_dir = tempdir().expect("create temp dir");
    let mut config = Config::default();
    config.models.models_dir = temp_dir.path().to_path_buf();
    config.models.auto_download = false;

    let manager = Arc::new(ModelManager::new(&config.models.models_dir));
    let initial_status = manager.get_status().await;
    assert_eq!(initial_status.state, ModelInstallState::NotInstalled);

    let triggered = check_and_auto_download(&config, &manager).await;
    assert!(
        !triggered,
        "auto_download=false must NOT trigger auto-download"
    );

    let status_after = manager.get_status().await;
    assert_eq!(
        status_after.state,
        ModelInstallState::NotInstalled,
        "Status must remain NotInstalled when auto_download=false"
    );
}
