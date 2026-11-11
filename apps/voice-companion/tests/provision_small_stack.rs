//! Real upper-level provisioning entry on a fixed 1MB stack.
//!
//! `check_and_auto_download` is the same function the production daemon calls
//! before binding its listener. This test must not be replaced by a leaf
//! `get_status` check: the historical overflow only appeared through this entry.

use std::collections::HashMap;
use std::sync::Arc;

use nomifun_voice_models::verifier::{DOWNLOAD_FILES, compute_sha256};
use nomifun_voice_models::{EXPECTED_FILES, ModelManager};
use voice_companion::config::Config;
use voice_companion::provision::check_and_auto_download;

fn populate_suite(models_dir: &std::path::Path) -> HashMap<String, String> {
    let mut manifest = HashMap::new();
    for &relative in EXPECTED_FILES {
        let path = models_dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = if DOWNLOAD_FILES.contains(&relative) {
            format!("fixed-provision-content-for-{relative}").into_bytes()
        } else {
            b"fixed-provision-metadata".to_vec()
        };
        std::fs::write(&path, &content).unwrap();
        if DOWNLOAD_FILES.contains(&relative) {
            manifest.insert(relative.to_owned(), compute_sha256(&content));
        }
    }
    manifest
}

#[test]
fn production_provision_entry_survives_a_fixed_one_megabyte_stack() {
    let temp = tempfile::tempdir().unwrap();
    // An installed real suite can be supplied explicitly; the synthetic suite
    // exercises the same async call chain and manifest path.
    let models_dir = std::env::var_os("FLOWY_VOICE_PROVISION_MODEL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().to_path_buf());
    let manager = if models_dir == temp.path() {
        Arc::new(ModelManager::with_config(
            &models_dir,
            Vec::new(),
            populate_suite(&models_dir),
        ))
    } else {
        // An installed suite must still pass the authoritative production
        // manifest, so keep the default canonical verifier.
        Arc::new(ModelManager::new(&models_dir))
    };
    let mut config = Config::default();
    config.models.auto_download = false;
    config.models.models_dir = models_dir;

    let worker = std::thread::Builder::new()
        .name("production-provision-1mb".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let downloaded = check_and_auto_download(&config, &manager).await;
                assert!(
                    !downloaded,
                    "auto_download=false must never start a network download"
                );
                assert!(
                    manager.get_status().await.state.is_ready(),
                    "the production provisioning entry must reach the verified suite"
                );
            });
        })
        .unwrap();
    worker
        .join()
        .expect("production provision entry must not overflow a 1MB stack");
}
