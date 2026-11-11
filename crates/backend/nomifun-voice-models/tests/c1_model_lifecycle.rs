use std::{collections::HashMap, sync::Arc, time::Duration};

use nomifun_voice_models::{
    EXPECTED_FILES, MirrorConfig, ModelInstallState, ModelManager,
    verifier::{DOWNLOAD_FILES, compute_sha256},
};

fn assets(root: &std::path::Path) -> HashMap<String, String> {
    let mut manifest = HashMap::new();
    for rel in EXPECTED_FILES {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = format!("real fixture bytes for {rel}");
        std::fs::write(path, &content).unwrap();
        if DOWNLOAD_FILES.contains(rel) {
            manifest.insert(rel.to_string(), compute_sha256(content.as_bytes()));
        }
    }
    manifest
}

#[tokio::test]
async fn c1_corrupt_complete_layout_never_becomes_ready_including_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = assets(dir.path());
    std::fs::write(dir.path().join("kws/encoder.int8.onnx"), "corrupt model").unwrap();
    for _ in 0..2 {
        let manager = ModelManager::with_config(dir.path(), vec![], manifest.clone());
        for _ in 0..2 {
            assert!(
                matches!(
                    manager.get_status().await.state,
                    ModelInstallState::Failed { .. }
                ),
                "complete layout is not a SHA verification proof"
            );
        }
    }
}

#[tokio::test]
async fn c1_integrity_cache_invalidates_on_change_and_recovers_after_offline_repair() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = assets(dir.path());
    let manager = ModelManager::with_config(dir.path(), vec![], manifest);
    let mut progress = manager.subscribe_progress();
    assert!(manager.get_status().await.state.is_ready());
    assert!(progress.try_recv().unwrap().data.is_ready());
    assert!(manager.get_status().await.state.is_ready());
    assert!(
        matches!(
            progress.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "unchanged status polling must reuse the integrity result"
    );
    let file = dir.path().join("asr/model.int8.onnx");
    let original = std::fs::read(&file).unwrap();
    std::fs::write(&file, "changed bytes").unwrap();
    assert!(manager.get_status().await.state.is_failed());
    assert!(manager.get_status().await.state.is_failed());
    std::fs::write(&file, original).unwrap();
    assert!(manager.get_status().await.state.is_ready());
    manager.remove().await.unwrap();
    assert_eq!(
        manager.get_status().await.state,
        ModelInstallState::NotInstalled
    );
}

#[tokio::test]
async fn c1_same_size_replacement_is_reverified() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = assets(dir.path());
    let manager = ModelManager::with_config(dir.path(), vec![], manifest);
    assert!(manager.get_status().await.state.is_ready());
    let file = dir.path().join("asr/model.int8.onnx");
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let mut changed = std::fs::read(&file).unwrap();
    changed[0] ^= 1;
    std::fs::write(&file, changed).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(modified + Duration::from_secs(1))
        .unwrap();
    assert!(manager.get_status().await.state.is_failed());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn c1_concurrent_install_has_one_owner_and_remove_reaps_it() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let manager = Arc::new(ModelManager::with_config(
        dir.path(),
        vec![MirrorConfig {
            id: "owned-stalled-mirror".into(),
            base_url: format!("http://{}", listener.local_addr().unwrap()),
            probe_url: None,
            is_modelscope: false,
            is_fallback_cdn: false,
        }],
        Default::default(),
    ));
    let barrier = Arc::new(tokio::sync::Barrier::new(32));
    let mut tasks = Vec::new();
    for _ in 0..32 {
        let (manager, barrier) = (manager.clone(), barrier.clone());
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            manager.install(None).await.unwrap().accepted
        }));
    }
    let mut accepted = 0;
    for task in tasks {
        accepted += usize::from(task.await.unwrap());
    }
    tokio::time::timeout(Duration::from_secs(3), manager.remove())
        .await
        .unwrap()
        .unwrap();
    drop(listener);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        manager.get_status().await.state,
        ModelInstallState::NotInstalled
    );
    let remaining: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() != ".voice-suite-write.lock")
        .collect();
    assert!(
        remaining.is_empty(),
        "only the OS lock file may remain after remove: {remaining:?}"
    );
    assert_eq!(
        accepted, 1,
        "simultaneous requests must not own the same .part files"
    );
}
