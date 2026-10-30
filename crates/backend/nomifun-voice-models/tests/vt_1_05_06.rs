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
use nomifun_voice_models::downloader::{DownloaderError, MirrorConfig, ModelDownloader};
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
                        let stream = futures_util::stream::once(async move {
                            Ok::<_, std::io::Error>(axum::body::Bytes::from(slice))
                        });
                        resp.body(Body::from_stream(stream)).unwrap()
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

    // 1. Mirror 1: ModelScope has highest configured priority
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

    // Assert: probing keeps the configured priority order
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
            is_modelscope: false,
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

#[tokio::test]
async fn test_modelscope_transient_503_retries_and_succeeds() {
    let dummy_payload = vec![88u8; 2048]; // 2KB payload
    let payload = Arc::new(dummy_payload.clone());
    let request_count = Arc::new(AtomicUsize::new(0));
    let req_count = request_count.clone();

    let app = Router::new()
        .route("/probe", head(|| async { StatusCode::OK }))
        .route(
            "/test_model.bin",
            get(move || {
                let req_count = req_count.clone();
                let payload = payload.clone();
                async move {
                    let count = req_count.fetch_add(1, Ordering::SeqCst);
                    if count == 0 {
                        // First request returns transient 503
                        (StatusCode::SERVICE_UNAVAILABLE, Body::empty()).into_response()
                    } else {
                        // Second request succeeds
                        (StatusCode::OK, Body::from(payload.as_ref().clone())).into_response()
                    }
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{addr}");
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let mirrors = vec![MirrorConfig {
        id: "modelscope".to_string(),
        base_url: server_url.clone(),
        probe_url: Some(format!("{server_url}/probe")),
        is_modelscope: true,
        is_fallback_cdn: false,
    }];

    let downloader = ModelDownloader::new(mirrors);
    let temp_dir = tempdir().expect("create temp dir");
    let target_file = temp_dir.path().join("test_model.bin");

    downloader
        .download_with_failover("test_model.bin", &target_file, dummy_payload.len() as u64)
        .await
        .expect("retry after transient 503 should succeed");

    assert!(target_file.exists());
    let downloaded_data = std::fs::read(&target_file).expect("read file");
    assert_eq!(downloaded_data, dummy_payload);

    // Exact contract assertion:
    // ModelScope transient error must retry exactly once, making total requests = 2
    assert_eq!(request_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_modelscope_deterministic_404_does_not_retry() {
    let request_count = Arc::new(AtomicUsize::new(0));
    let req_count = request_count.clone();

    let app = Router::new()
        .route("/probe", head(|| async { StatusCode::OK }))
        .route(
            "/test_model.bin",
            get(move || {
                let req_count = req_count.clone();
                async move {
                    req_count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NOT_FOUND
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{addr}");
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let mirrors = vec![MirrorConfig {
        id: "modelscope".to_string(),
        base_url: server_url.clone(),
        probe_url: Some(format!("{server_url}/probe")),
        is_modelscope: true,
        is_fallback_cdn: false,
    }];

    let downloader = ModelDownloader::new(mirrors);
    let temp_dir = tempdir().expect("create temp dir");
    let target_file = temp_dir.path().join("test_model.bin");

    let err = downloader
        .download_with_failover("test_model.bin", &target_file, 1024)
        .await
        .expect_err("404 must fail");

    assert!(matches!(err, DownloaderError::UnexpectedStatus(StatusCode::NOT_FOUND)));
    // Assert: Deterministic 404 must NOT retry on ModelScope; exactly 1 GET request made!
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_default_mirrors_order_matches_environment() {
    // 1. Explicit global preference (ModelScope is primary authorized source, with official HF as first fallback)
    let global_mirrors = MirrorConfig::mirrors_for_environment(false);
    assert_eq!(global_mirrors[0].id, "modelscope");
    assert_eq!(global_mirrors[1].id, "hf_official");
    assert_eq!(global_mirrors[2].id, "hf_mirror");

    // 2. Explicit China preference
    let china_mirrors = MirrorConfig::mirrors_for_environment(true);
    assert_eq!(china_mirrors[0].id, "modelscope");
    assert_eq!(china_mirrors[1].id, "hf_mirror");
    assert_eq!(china_mirrors[2].id, "hf_official");

    assert!(!global_mirrors[0].is_fallback_cdn);
    assert!(!china_mirrors[0].is_fallback_cdn);
    assert!(china_mirrors[2].is_fallback_cdn);
    assert!(global_mirrors[0].supports("kws/encoder.int8.onnx"));
    assert!(!global_mirrors[1].supports("kws/encoder.int8.onnx"));
}

#[test]
fn test_default_mirror_urls_match_verified_assets() {
    let global_mirrors = MirrorConfig::mirrors_for_environment(false);
    let hf_official = global_mirrors
        .iter()
        .find(|mirror| mirror.id == "hf_official")
        .expect("hf official mirror");
    assert!(hf_official
        .probe_url
        .as_deref()
        .is_some_and(|url| url.contains("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt")));
    assert!(hf_official
        .resolve_url("asr/model.int8.onnx")
        .contains("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx"));
    assert!(hf_official
        .resolve_url("vad/silero_vad.onnx")
        .contains("csukuangfj/vad/resolve/main/silero_vad.onnx"));
    assert!(!hf_official.supports("kws/encoder.int8.onnx"));

    let hf_mirror = global_mirrors
        .iter()
        .find(|mirror| mirror.id == "hf_mirror")
        .expect("hf mirror");
    assert!(hf_mirror
        .resolve_url("asr/tokens.txt")
        .contains("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt"));
    assert!(!hf_mirror.supports("kws/encoder.int8.onnx"));

    let modelscope = global_mirrors
        .iter()
        .find(|mirror| mirror.id == "modelscope")
        .expect("modelscope mirror");
    assert!(modelscope.supports("kws/encoder.int8.onnx"));
    assert!(modelscope
        .resolve_url("kws/encoder.int8.onnx")
        .contains("pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01"));
}

#[tokio::test]
async fn test_probe_respects_priority_order_not_rtt() {
    let payload = vec![7u8; 1024];
    let (first_url, _h1) = run_mock_mirror_server(
        payload.clone(),
        Duration::from_millis(250),
        None,
    )
    .await;
    let (second_url, _h2) = run_mock_mirror_server(payload, Duration::ZERO, None).await;
    let mirrors = vec![
        MirrorConfig {
            id: "first_priority".to_string(),
            base_url: first_url.clone(),
            probe_url: Some(format!("{first_url}/probe")),
            is_modelscope: false,
            is_fallback_cdn: false,
        },
        MirrorConfig {
            id: "faster_second".to_string(),
            base_url: second_url.clone(),
            probe_url: Some(format!("{second_url}/probe")),
            is_modelscope: false,
            is_fallback_cdn: false,
        },
    ];

    let selected = ModelDownloader::new(mirrors)
        .probe_best_source()
        .await
        .expect("probe mirror");
    assert_eq!(selected.id, "first_priority");
}

#[tokio::test]
async fn test_speed_gate_triggers_failover_and_resumes_cleanly() {
    let dummy_payload = vec![123u8; 4096]; // 4KB payload

    // 1. Primary mirror: very slow stream (40ms sleep per 64-byte chunk)
    let payload_primary = Arc::new(dummy_payload.clone());
    let app_primary = Router::new()
        .route("/probe", head(|| async { StatusCode::OK }))
        .route(
            "/test_model.bin",
            get({
                let payload = payload_primary.clone();
                move || {
                    let payload = payload.clone();
                    async move {
                        let total_len = payload.len();
                        let stream = futures_util::stream::unfold(
                            (payload, 0usize),
                            |(bytes, offset)| async move {
                                if offset >= bytes.len() {
                                    None
                                } else {
                                    tokio::time::sleep(Duration::from_millis(40)).await;
                                    let end = (offset + 64).min(bytes.len());
                                    let chunk = axum::body::Bytes::copy_from_slice(&bytes[offset..end]);
                                    Some((Ok::<_, std::io::Error>(chunk), (bytes, end)))
                                }
                            },
                        );
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_LENGTH, total_len.to_string())
                            .body(Body::from_stream(stream))
                            .unwrap()
                    }
                }
            }),
        );

    let listener_primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr_primary = listener_primary.local_addr().unwrap();
    let primary_url = format!("http://{addr_primary}");
    let _h1 = tokio::spawn(async move {
        let _ = axum::serve(listener_primary, app_primary).await;
    });

    // 2. Fast backup mirror: responds at full speed and handles Range resumption
    let (fast_backup_url, _h2) = run_mock_mirror_server(dummy_payload.clone(), Duration::ZERO, None).await;

    let mirrors = vec![
        MirrorConfig {
            id: "slow_primary".to_string(),
            base_url: primary_url.clone(),
            probe_url: Some(format!("{primary_url}/probe")),
            is_modelscope: true,
            is_fallback_cdn: false,
        },
        MirrorConfig {
            id: "fast_backup".to_string(),
            base_url: fast_backup_url.clone(),
            probe_url: Some(format!("{fast_backup_url}/probe")),
            is_modelscope: false,
            is_fallback_cdn: false,
        },
    ];

    // Configure speed gate: trigger if < 100 KB/s during [50ms, 5s] initial observation window
    let downloader = ModelDownloader::new(mirrors.clone())
        .with_speed_gate(100 * 1024, Duration::from_millis(50), Duration::from_secs(5));
    let temp_dir = tempdir().expect("create temp dir");
    let target_file = temp_dir.path().join("test_model.bin");

    let slow_error = downloader
        .download_file_with_resume_internal(&mirrors[0], "test_model.bin", &target_file, false)
        .await
        .expect_err("slow primary must trip the speed gate");
    assert!(matches!(slow_error, DownloaderError::SpeedTooLow { .. }));

    // Execution: Primary should trigger SpeedTooLow, and Fast Backup should resume and finish
    downloader
        .download_with_failover("test_model.bin", &target_file, dummy_payload.len() as u64)
        .await
        .expect("download with speed gate failover should succeed");

    assert!(target_file.exists());
    let downloaded_data = std::fs::read(&target_file).expect("read file");
    assert_eq!(downloaded_data.len(), dummy_payload.len());
    assert_eq!(downloaded_data, dummy_payload);
}

#[test]
fn test_downloader_error_classification() {
    // 1. Transient errors allowed to retry on the same mirror:
    assert!(DownloaderError::Interrupted("stream stalled".into()).is_transient_retryable());
    assert!(DownloaderError::UnexpectedStatus(StatusCode::SERVICE_UNAVAILABLE).is_transient_retryable());
    assert!(DownloaderError::UnexpectedStatus(StatusCode::GATEWAY_TIMEOUT).is_transient_retryable());
    assert!(DownloaderError::UnexpectedStatus(StatusCode::BAD_GATEWAY).is_transient_retryable());
    assert!(DownloaderError::UnexpectedStatus(StatusCode::TOO_MANY_REQUESTS).is_transient_retryable());
    assert!(DownloaderError::UnexpectedStatus(StatusCode::REQUEST_TIMEOUT).is_transient_retryable());

    // 2. Deterministic errors that must NEVER be blindly retried:
    assert!(!DownloaderError::ProtocolViolation("invalid range".into()).is_transient_retryable());
    assert!(!DownloaderError::UnexpectedStatus(StatusCode::NOT_FOUND).is_transient_retryable());
    assert!(!DownloaderError::UnexpectedStatus(StatusCode::FORBIDDEN).is_transient_retryable());
    assert!(!DownloaderError::UnexpectedStatus(StatusCode::UNAUTHORIZED).is_transient_retryable());
    assert!(!DownloaderError::UnexpectedStatus(StatusCode::RANGE_NOT_SATISFIABLE).is_transient_retryable());
    assert!(!DownloaderError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "io error")).is_transient_retryable());
    assert!(!DownloaderError::SpeedTooLow {
        actual_bps: 100,
        threshold_bps: 200,
        mirror_id: "test".into()
    }.is_transient_retryable());
    assert!(!DownloaderError::AllMirrorsFailed.is_transient_retryable());
}
