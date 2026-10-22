//! Vertical Slice Tests VT-1-05 & VT-1-06:
//! 双源智能探针测速选源、HTTP Range 断点续传与突发中断平滑回退 CDN。
//!
//! Baseline:
//! - `model-management.zh.md` §3
//! - `wave-1-acoustic.zh.md` §3 (VT-1-05, VT-1-06)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, head};
use axum::Router;
use nomifun_voice_models::downloader::{MirrorConfig, ModelDownloader};
use tempfile::tempdir;
use tokio::net::TcpListener;

/// Starts a test HTTP server serving a single synthetic file.
/// If `interrupt_after_bytes` is Some(n), drops/truncates the stream after n bytes on first request.
async fn run_mock_mirror_server(
    file_bytes: Vec<u8>,
    head_delay: Duration,
    interrupt_after: Option<usize>,
) -> (String, tokio::task::JoinHandle<()>) {
    let file_bytes = Arc::new(file_bytes);
    let request_count = Arc::new(AtomicUsize::new(0));

    let app = Router::new()
        .route(
            "/probe",
            head({
                let head_delay = head_delay;
                move || async move {
                    if !head_delay.is_zero() {
                        tokio::time::sleep(head_delay).await;
                    }
                    StatusCode::OK
                }
            }),
        )
        .route(
            "/test_model.bin",
            get({
                let file_bytes = file_bytes.clone();
                let request_count = request_count.clone();
                move |headers: HeaderMap| {
                    let file_bytes = file_bytes.clone();
                    let request_count = request_count.clone();
                    async move {
                        let total_len = file_bytes.len();
                        let count = request_count.fetch_add(1, Ordering::SeqCst);

                        // Check Range header
                        let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
                        let (start, end) = if let Some(range_val) = range_header {
                            // Format: bytes=START-
                            if let Some(stripped) = range_val.strip_prefix("bytes=") {
                                let parts: Vec<&str> = stripped.split('-').collect();
                                let start: usize = parts[0].parse().unwrap_or(0);
                                let end: usize = if parts.len() > 1 && !parts[1].is_empty() {
                                    parts[1].parse().unwrap_or(total_len - 1)
                                } else {
                                    total_len - 1
                                };
                                (start, end)
                            } else {
                                (0, total_len - 1)
                            }
                        } else {
                            (0, total_len - 1)
                        };

                        if start >= total_len {
                            return (StatusCode::RANGE_NOT_SATISFIABLE, Body::empty()).into_response();
                        }

                        let mut slice = file_bytes[start..=end].to_vec();

                        // Simulate interruption on first attempt if configured
                        if count == 0 {
                            if let Some(limit) = interrupt_after {
                                if slice.len() > limit {
                                    slice.truncate(limit);
                                }
                            }
                        }

                        let is_partial = range_header.is_some();
                        let status = if is_partial {
                            StatusCode::PARTIAL_CONTENT
                        } else {
                            StatusCode::OK
                        };

                        let mut resp = Response::builder().status(status);
                        if is_partial {
                            resp = resp.header(
                                CONTENT_RANGE,
                                format!("bytes {start}-{end}/{total_len}"),
                            );
                        }
                        let declared_len = end - start + 1;
                        resp = resp.header(CONTENT_LENGTH, declared_len.to_string());
                        resp.body(Body::from(slice)).unwrap()
                    }
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (server_url, handle)
}

#[tokio::test]
async fn test_vt_1_05_probe_selection_and_range_resume() {
    let dummy_payload = vec![42u8; 1024]; // 1KB test payload

    // 1. Mirror 1: ModelScope fast (<800ms)
    let (modelscope_url, _h1) = run_mock_mirror_server(
        dummy_payload.clone(),
        Duration::from_millis(50),
        None,
    )
    .await;

    // 2. Mirror 2: GitHub slower
    let (github_url, _h2) = run_mock_mirror_server(
        dummy_payload.clone(),
        Duration::from_millis(300),
        None,
    )
    .await;

    let mirrors = vec![
        MirrorConfig {
            id: "modelscope".to_string(),
            base_url: modelscope_url.clone(),
            probe_url: Some(format!("{modelscope_url}/probe")),
            is_modelscope: true,
            is_fallback_cdn: false,
        },
        MirrorConfig {
            id: "github".to_string(),
            base_url: github_url.clone(),
            probe_url: Some(format!("{github_url}/probe")),
            is_modelscope: false,
            is_fallback_cdn: false,
        },
    ];

    let downloader = ModelDownloader::new(mirrors);

    // Assert: Probing selects ModelScope because it connected and responded < 800ms
    let selected = downloader.probe_best_source().await.expect("probe mirror");
    assert_eq!(selected.id, "modelscope");

    // Test HTTP Range Resume:
    // Create partial file with first 256 bytes
    let temp_dir = tempdir().expect("create temp dir");
    let target_file = temp_dir.path().join("test_model.bin");
    let partial_part_file = temp_dir.path().join("test_model.bin.part");
    std::fs::write(&partial_part_file, &dummy_payload[0..256]).expect("write partial");

    // Downloader should resume from 256 bytes using Range
    downloader
        .download_file_with_resume(&selected, "test_model.bin", &target_file)
        .await
        .expect("download with resume");

    // Assert: Target file exists and content is byte-level identical (1024 bytes)
    assert!(target_file.exists());
    let downloaded_data = std::fs::read(&target_file).expect("read downloaded file");
    assert_eq!(downloaded_data, dummy_payload);
}

#[tokio::test]
async fn test_vt_1_06_interrupted_stream_failover_to_cdn() {
    let dummy_payload = vec![99u8; 2048]; // 2KB payload

    // 1. Primary mirror: interrupts download after 500 bytes
    let (primary_url, _h1) = run_mock_mirror_server(
        dummy_payload.clone(),
        Duration::from_millis(20),
        Some(500),
    )
    .await;

    // 2. CDN mirror: healthy fallback
    let (cdn_url, _h2) = run_mock_mirror_server(
        dummy_payload.clone(),
        Duration::from_millis(30),
        None,
    )
    .await;

    let mirrors = vec![
        MirrorConfig {
            id: "primary".to_string(),
            base_url: primary_url.clone(),
            probe_url: Some(format!("{primary_url}/probe")),
            is_modelscope: true,
            is_fallback_cdn: false,
        },
        MirrorConfig {
            id: "official_cdn".to_string(),
            base_url: cdn_url.clone(),
            probe_url: Some(format!("{cdn_url}/probe")),
            is_modelscope: false,
            is_fallback_cdn: true,
        },
    ];

    let downloader = ModelDownloader::new(mirrors);
    let temp_dir = tempdir().expect("create temp dir");
    let target_file = temp_dir.path().join("test_model.bin");

    // Download with automatic failover to CDN upon stream error / unexpected truncation
    downloader
        .download_with_failover("test_model.bin", &target_file, dummy_payload.len() as u64)
        .await
        .expect("failover download should succeed");

    // Assert: File is fully recovered and complete
    assert!(target_file.exists());
    let downloaded_data = std::fs::read(&target_file).expect("read file");
    assert_eq!(downloaded_data.len(), dummy_payload.len());
    assert_eq!(downloaded_data, dummy_payload);
}
