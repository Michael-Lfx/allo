//! Vertical Slice Test VT-1-09:
//! REST / SSE 全量实现：install 幂等、progress SSE 流式广播与 delete 安全清理。
//!
//! Baseline:
//! - `model-management.zh.md` §4 (场景 A, 场景 B)
//! - `wave-1-acoustic.zh.md` §3 (VT-1-09)
//! - `interaction.zh.md` §5.1 (TC-AM-014)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteResponse, VoiceModelInstallResponse,
};
use nomifun_voice_models::downloader::MirrorConfig;
use nomifun_voice_models::{voice_model_routes, ModelManager};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[tokio::test]
async fn test_vt_1_09_install_sse_progress_and_delete_lifecycle() {
    let temp_dir = tempdir().expect("create temp dir");
    let models_dir = temp_dir.path().to_path_buf();

    // 1. Setup local mock mirror server serving the 7 distribution files + keywords.txt
    let test_files = [
        ("kws/encoder.int8.onnx", b"dummy-encoder-onnx-content-12345".to_vec()),
        ("kws/decoder.int8.onnx", b"dummy-decoder-onnx-content-67890".to_vec()),
        ("kws/joiner.int8.onnx", b"dummy-joiner-onnx-content-abcde".to_vec()),
        ("kws/tokens.txt", b"<blk> 0\nhey 1\nflowy 2\n".to_vec()),
        ("kws/keywords.txt", b"HEY FLOWY :1.0 #0.25 @1\n".to_vec()),
        ("vad/silero_vad.onnx", b"dummy-silero-vad-model-bytes-999".to_vec()),
        ("asr/model.int8.onnx", b"dummy-sensevoice-asr-int8-bytes".to_vec()),
        ("asr/tokens.txt", b"hello 1\nworld 2\n".to_vec()),
    ];

    let mut baseline_manifest = HashMap::new();
    let mut checksum_file_content = String::new();
    let mut file_map: HashMap<String, Vec<u8>> = HashMap::new();

    for (rel_path, content) in &test_files {
        file_map.insert(rel_path.to_string(), content.clone());
        if *rel_path != "kws/keywords.txt" {
            let hash = sha256_hex(content);
            baseline_manifest.insert(rel_path.to_string(), hash.clone());
            checksum_file_content.push_str(&format!("{hash}  {rel_path}\n"));
        }
    }
    file_map.insert(
        "checksums.sha256".to_string(),
        checksum_file_content.into_bytes(),
    );

    let file_map_arc = Arc::new(file_map);

    let mock_app = axum::Router::new()
        .route(
            "/probe",
            axum::routing::head(|| async { StatusCode::OK }),
        )
        .fallback({
            let file_map_arc = file_map_arc.clone();
            move |req: axum::extract::Request| {
                let file_map = file_map_arc.clone();
                async move {
                    let path = req.uri().path().trim_start_matches('/');
                    if let Some(content) = file_map.get(path) {
                        (StatusCode::OK, content.clone()).into_response()
                    } else {
                        (StatusCode::NOT_FOUND, "Not Found").into_response()
                    }
                }
            }
        });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mirror_url = format!("http://{addr}");
    tokio::spawn(async move {
        let _ = axum::serve(listener, mock_app).await;
    });

    // 2. Initialize ModelManager configured with local mirror & baseline manifest
    let mirrors = vec![MirrorConfig {
        id: "local_mock".to_string(),
        base_url: mirror_url.clone(),
        probe_url: Some(format!("{mirror_url}/probe")),
        is_modelscope: true,
        is_fallback_cdn: false,
    }];

    let manager = Arc::new(ModelManager::with_config(
        models_dir.clone(),
        mirrors,
        baseline_manifest,
    ));
    let app = voice_model_routes(manager.clone());

    // Subscribe to SSE progress channel
    let mut progress_rx = manager.subscribe_progress();

    // 3. Trigger POST /api/voice/models/install
    let req = Request::builder()
        .method("POST")
        .uri("/api/voice/models/install")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("build request");

    let resp = app.clone().oneshot(req).await.expect("execute install");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.expect("read body").to_bytes();
    let install_resp: VoiceModelInstallResponse =
        serde_json::from_slice(&body).expect("deserialize install response");
    assert!(install_resp.accepted, "first install call must be accepted");

    // 4. Test idempotency: Calling install immediately while in progress should return accepted: false
    let req_dup = Request::builder()
        .method("POST")
        .uri("/api/voice/models/install")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("build duplicate request");

    let resp_dup = app.clone().oneshot(req_dup).await.expect("execute duplicate install");
    assert_eq!(resp_dup.status(), StatusCode::OK);
    let body_dup = resp_dup.into_body().collect().await.expect("read body").to_bytes();
    let install_resp_dup: VoiceModelInstallResponse =
        serde_json::from_slice(&body_dup).expect("deserialize duplicate response");
    assert!(!install_resp_dup.accepted, "concurrent install must be rejected as duplicate");

    // 5. Collect progress events from SSE stream
    let mut events = Vec::new();
    let timeout = Duration::from_secs(10);
    let start_time = tokio::time::Instant::now();

    while start_time.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(500), progress_rx.recv()).await {
            Ok(Ok(event)) => {
                let is_final = matches!(event.data, ModelInstallState::Ready { .. } | ModelInstallState::Failed { .. });
                events.push(event);
                if is_final {
                    break;
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
            _ => continue,
        }
    }

    assert!(!events.is_empty(), "progress events should be emitted");

    // Verify presence of Downloading events with required fields
    let has_probing = events.iter().any(|e| matches!(e.data, ModelInstallState::ProbingSource));
    assert!(has_probing, "must emit ProbingSource event");

    let downloading_events: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.data {
            ModelInstallState::Downloading {
                current_file,
                file_index,
                total_files,
                downloaded_bytes,
                total_bytes,
                progress_percent,
                speed_bytes_per_sec,
            } => Some((
                current_file,
                *file_index,
                *total_files,
                *downloaded_bytes,
                *total_bytes,
                *progress_percent,
                *speed_bytes_per_sec,
            )),
            _ => None,
        })
        .collect();

    assert!(
        !downloading_events.is_empty(),
        "must emit Downloading progress events"
    );

    // Verify presence of VerifyingSha256 and Ready
    let has_verifying = events.iter().any(|e| matches!(e.data, ModelInstallState::VerifyingSha256 { .. }));
    assert!(has_verifying, "must emit VerifyingSha256 event");

    let final_event = events.last().expect("last event");
    assert!(
        matches!(final_event.data, ModelInstallState::Ready { .. }),
        "final event must be Ready, got: {:?}",
        final_event.data
    );

    // 6. Test DELETE /api/voice/models
    let req_del = Request::builder()
        .method("DELETE")
        .uri("/api/voice/models")
        .body(Body::empty())
        .expect("build delete request");

    let resp_del = app.clone().oneshot(req_del).await.expect("execute delete");
    assert_eq!(resp_del.status(), StatusCode::OK);

    let body_del = resp_del.into_body().collect().await.expect("read body").to_bytes();
    let delete_resp: VoiceModelDeleteResponse =
        serde_json::from_slice(&body_del).expect("deserialize delete response");
    assert!(delete_resp.success);
    assert_eq!(delete_resp.state, ModelInstallState::NotInstalled);

    // Verify models directory files were deleted
    let status_after = manager.get_status().await;
    assert_eq!(status_after.state, ModelInstallState::NotInstalled);
    assert_eq!(status_after.total_size_bytes, 0);
}
