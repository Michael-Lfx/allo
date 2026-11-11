//! Formal regressions migrated from the rev2 startup/TTS diagnosis:
//! wrong Content-Range promotion and false-success deletion.

use std::collections::HashMap;
use std::path::PathBuf;

use nomifun_voice_models::{EXPECTED_FILES, ModelManager};
use nomifun_voice_models::downloader::{MirrorConfig, ModelDownloader};
use nomifun_voice_models::manager::ModelManagerError;
use nomifun_voice_models::verifier::{DOWNLOAD_FILES, compute_sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn populate_small_suite(models_dir: &std::path::Path) -> HashMap<String, String> {
    let mut manifest = HashMap::new();
    for &relative in EXPECTED_FILES {
        let path = models_dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = if DOWNLOAD_FILES.contains(&relative) {
            format!("fixed-suite-content-for-{relative}").into_bytes()
        } else {
            b"fixed-suite-metadata".to_vec()
        };
        std::fs::write(&path, &content).unwrap();
        if DOWNLOAD_FILES.contains(&relative) {
            manifest.insert(relative.to_owned(), compute_sha256(&content));
        }
    }
    manifest
}

#[test]
fn leaf_verification_survives_a_fixed_one_megabyte_stack() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = populate_small_suite(dir.path());
    let models_dir = dir.path().to_path_buf();
    let worker = std::thread::Builder::new()
        .name("provision-1mb".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let manager =
                    ModelManager::with_config(&models_dir, Vec::new(), manifest);
                let status = manager.get_status().await;
                assert!(
                    status.state.is_ready(),
                    "the real verification chain must finish on a 1MB stack, got {:?}",
                    status.state
                );
            });
        })
        .unwrap();
    worker.join().expect("1MB provision verification thread must not overflow or panic");
}

async fn one_shot_server(
    response: &'static [u8],
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        socket.write_all(response).await.unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (url, server)
}

fn mirror_for(url: String) -> MirrorConfig {
    MirrorConfig {
        id: "owned-loopback".into(),
        base_url: url,
        probe_url: None,
        is_modelscope: false,
        is_fallback_cdn: false,
    }
}

#[tokio::test]
async fn wrong_206_content_range_is_rejected_and_never_promoted() {
    let request = one_shot_server(
        b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/6\r\nConnection: close\r\n\r\nABC",
    );
    let (url, server) = request.await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("asset.onnx");
    let part = dir.path().join("asset.onnx.part");
    std::fs::write(&part, b"ABC").unwrap();

    let mirror = mirror_for(url);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        ModelDownloader::new(vec![mirror.clone()])
            .download_file_with_resume(&mirror, "asset.onnx", &target),
    )
    .await
    .expect("loopback download must finish");

    let request_text = server.await.unwrap().to_ascii_lowercase();
    assert!(
        request_text.contains("range: bytes=3-"),
        "the downloader must request the remaining range, got: {request_text}"
    );
    assert!(
        result.is_err(),
        "a 206 whose Content-Range start does not match the requested offset must never be promoted"
    );
    assert!(
        !target.exists(),
        "a rejected range response must not become the final model file"
    );
    assert!(
        !part.exists(),
        "a rejected range response must discard the unusable partial file"
    );
}

#[tokio::test]
async fn unknown_total_206_body_must_match_its_content_range() {
    let cases = [
        (
            "oversized",
            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-2/*\r\nConnection: close\r\n\r\nABCDEF"
                .as_slice(),
        ),
        (
            "truncated",
            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-8/*\r\nConnection: close\r\n\r\nABC"
                .as_slice(),
        ),
    ];
    for (name, response) in cases {
        let (url, server) = one_shot_server(response).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(format!("{name}.onnx"));
        std::fs::write(dir.path().join(format!("{name}.onnx.part")), b"").unwrap();
        let mirror = mirror_for(url);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            ModelDownloader::new(vec![mirror.clone()])
                .download_file_with_resume(&mirror, "asset.onnx", &target),
        )
        .await
        .expect("loopback download must finish");
        let _ = server.await.unwrap();
        assert!(
            result.is_err(),
            "{name}: a 206 body that does not match Content-Range must fail even when total is *"
        );
        assert!(
            !target.exists(),
            "{name}: an invalid range body must never be promoted to the model path"
        );
    }
}

#[tokio::test]
async fn unsatisfiable_range_rechecks_local_state_instead_of_trusting_min_size() {
    let (url, server) = one_shot_server(
        b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */6\r\nConnection: close\r\n\r\n",
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("asset.onnx");
    let part = dir.path().join("asset.onnx.part");
    std::fs::write(&target, b"stale-and-wrong").unwrap();
    std::fs::write(&part, b"unrelated-tail").unwrap();

    let mirror = mirror_for(url);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        ModelDownloader::new(vec![mirror.clone()])
            .download_file_with_resume(&mirror, "asset.onnx", &target),
    )
    .await
    .expect("loopback download must finish");
    let _ = server.await.unwrap();

    assert!(
        result.is_err(),
        "416 must not report success from a local minimum-size guess"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"stale-and-wrong",
        "416 must not overwrite an existing target with a partial file"
    );
    assert!(
        !part.exists(),
        "416 must discard the unusable partial file"
    );
}

#[tokio::test]
async fn delete_reports_failure_when_an_expected_asset_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let models_dir: PathBuf = dir.path().to_path_buf();
    // A directory in place of an expected file makes removal fail on every OS
    // without relying on Windows share modes.
    let blocker = models_dir.join("asr/model.int8.onnx");
    std::fs::create_dir_all(&blocker).unwrap();
    std::fs::write(blocker.join("held.txt"), b"held").unwrap();
    std::fs::write(models_dir.join("asr/tokens.txt"), b"removable").unwrap();

    let response = ModelManager::new(&models_dir).remove().await.unwrap();

    assert!(
        !response.success,
        "deletion must not report success while an expected asset remains"
    );
    assert_eq!(
        response.freed_bytes, 9,
        "freed_bytes must only count assets actually removed"
    );
    assert!(
        blocker.exists(),
        "the undeletable asset must still be present after the failed removal"
    );
}

#[tokio::test]
async fn two_managers_on_one_directory_have_a_single_writer() {
    let dir = tempfile::tempdir().unwrap();
    // A bound but non-accepting loopback listener keeps the first install's
    // background task alive so its directory lease is observably held.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mirror = MirrorConfig {
        id: "blocked-loopback".into(),
        base_url: base.clone(),
        probe_url: Some(format!("{base}/probe")),
        is_modelscope: false,
        is_fallback_cdn: false,
    };
    let first = ModelManager::with_config(dir.path(), vec![mirror], HashMap::new());
    let second = ModelManager::with_config(dir.path(), Vec::new(), HashMap::new());

    let accepted = first.install(None).await.unwrap();
    assert!(accepted.accepted, "first manager owns the install");
    let conflict = second.install(None).await.unwrap_err();
    assert!(
        matches!(&conflict, ModelManagerError::Conflict(message) if message.contains("models_directory_busy")),
        "a second manager must not write the same canonical directory, got {conflict:?}"
    );

    first.remove().await.unwrap();
    drop(listener);
}

#[tokio::test]
async fn model_writes_are_refused_while_the_voice_suite_is_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ModelManager::new(dir.path());
    manager.set_in_use_guard(std::sync::Arc::new(|| true));

    assert!(
        matches!(
            manager.remove().await,
            Err(ModelManagerError::Conflict(_))
        ),
        "removal during an active call must return an in-use conflict"
    );
    assert!(
        matches!(
            manager.install(None).await,
            Err(ModelManagerError::Conflict(_))
        ),
        "installation during an active task must return an in-use conflict"
    );
    assert!(
        !manager.get_status().await.state.is_busy(),
        "a refused write must not leave a busy installation state"
    );

    manager.set_in_use_guard(std::sync::Arc::new(|| false));
    let response = manager.remove().await.unwrap();
    assert!(response.success);
}

#[cfg(windows)]
#[tokio::test]
async fn windows_locked_model_file_delete_reports_failure_and_true_bytes() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let models_dir = dir.path().to_path_buf();
    std::fs::create_dir_all(models_dir.join("asr")).unwrap();
    let locked = models_dir.join("asr/model.int8.onnx");
    std::fs::write(&locked, b"owned locked asset").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();

    let response = ModelManager::new(&models_dir).remove().await.unwrap();

    assert!(!response.success, "a locked file must not be reported as deleted");
    assert_eq!(response.freed_bytes, 0, "no bytes were actually freed");
    assert!(locked.exists(), "the locked original must remain");
    drop(held);
}

#[tokio::test]
async fn html_200_downloads_end_in_failed_not_ready() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break; };
            let mut buffer = [0u8; 2048];
            let _ = socket.read(&mut buffer).await;
            let body = b"<html>not a model</html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.write_all(body).await;
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let mirror = MirrorConfig {
        id: "html-loopback".into(),
        base_url: base,
        probe_url: None,
        is_modelscope: false,
        is_fallback_cdn: false,
    };
    let manager = ModelManager::with_config(
        dir.path(),
        vec![mirror],
        nomifun_voice_models::verifier::default_canonical_manifest(),
    );
    assert!(manager.install(None).await.unwrap().accepted);
    let mut state = manager.get_status().await.state;
    for _ in 0..100 {
        if !state.is_busy() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        state = manager.get_status().await.state;
    }
    assert!(
        state.is_failed(),
        "an HTML 200 body must end as Failed after SHA verification, got {state:?}"
    );
    server.abort();
}

#[tokio::test]
async fn unwritable_partial_path_reports_io_failure() {
    let (url, server) = one_shot_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nABC",
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("asset.onnx");
    std::fs::create_dir_all(dir.path().join("asset.onnx.part")).unwrap();
    let mirror = mirror_for(url);
    let result = ModelDownloader::new(vec![mirror.clone()])
        .download_file_with_resume(&mirror, "asset.onnx", &target)
        .await;
    let _ = server.await.unwrap();
    assert!(result.is_err(), "an unwritable partial path must fail closed");
    assert!(!target.exists());
}
/// Child entry for the dead-writer test: acquire the real lease, then exit
/// without running any destructor. The OS must release the file lock.
#[test]
fn dead_writer_child_entry() {
    let Some(dir) = std::env::var_os("VOICE_LEASE_CHILD_DIR") else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let manager = ModelManager::new(dir);
        assert!(manager.install(None).await.unwrap().accepted);
        std::process::exit(0);
    });
}

#[tokio::test]
async fn dead_writer_process_releases_the_os_lock_without_drop() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "dead_writer_child_entry", "--nocapture"])
        .env("VOICE_LEASE_CHILD_DIR", &models)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "owned writer must exit successfully");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("owned writer deadline");
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let manager = ModelManager::new(&models);
    let removed = manager.remove().await;
    assert!(
        removed.is_ok(),
        "a new manager must recover after the owner process exits without Drop: {removed:?}"
    );
}