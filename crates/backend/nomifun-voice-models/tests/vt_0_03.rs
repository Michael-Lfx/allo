//! Vertical Slice Test VT-0-03:
//! 空模型目录 → `GET /api/voice/models/status` 返回 `NotInstalled`；SSE 通道可订阅；与 golden JSON 匹配。
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4, §5.1 (TC-AM-014)
//! - `execution/wave-0-skeleton.zh.md` §4 (VT-0-03)

use std::sync::Arc;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteResponse, VoiceModelStatusResponse,
};
use nomifun_voice_models::{ModelManager, voice_model_routes};
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn test_vt_0_03_empty_dir_status_is_not_installed() {
    let temp_dir = tempdir().expect("create temp dir");
    let manager = Arc::new(ModelManager::new(temp_dir.path()));
    let app = voice_model_routes(manager.clone());

    // 1. Direct ModelManager API check
    let direct_status = manager.get_status().await;
    assert_eq!(direct_status.state, ModelInstallState::NotInstalled);
    assert_eq!(direct_status.total_size_bytes, 0);

    // 2. HTTP GET /api/voice/models/status
    let req = Request::builder()
        .method("GET")
        .uri("/api/voice/models/status")
        .body(Body::empty())
        .expect("build request");

    let resp = app.clone().oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.expect("read body").to_bytes();
    let status_resp: VoiceModelStatusResponse =
        serde_json::from_slice(&body).expect("deserialize status response");

    assert_eq!(status_resp.state, ModelInstallState::NotInstalled);
    assert_eq!(status_resp.total_size_bytes, 0);

    // Verify wire JSON format contains "status": "NotInstalled"
    let json_val: Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json_val["state"]["status"], "NotInstalled");
}

#[tokio::test]
async fn test_vt_0_03_sse_progress_channel_subscribable() {
    let temp_dir = tempdir().expect("create temp dir");
    let manager = Arc::new(ModelManager::new(temp_dir.path()));
    let app = voice_model_routes(manager.clone());

    let req = Request::builder()
        .method("GET")
        .uri("/api/voice/models/progress")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected text/event-stream, got: {content_type}"
    );
}

#[tokio::test]
async fn test_vt_0_03_install_starts_pipeline() {
    let temp_dir = tempdir().expect("create temp dir");
    let manager = Arc::new(ModelManager::new(temp_dir.path()));
    let app = voice_model_routes(manager.clone());

    let req = Request::builder()
        .method("POST")
        .uri("/api/voice/models/install")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("execute request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "install must accept and return 200 OK"
    );
}

#[tokio::test]
async fn test_vt_0_03_delete_resets_to_not_installed() {
    let temp_dir = tempdir().expect("create temp dir");
    let manager = Arc::new(ModelManager::new(temp_dir.path()));
    let app = voice_model_routes(manager.clone());

    let req = Request::builder()
        .method("DELETE")
        .uri("/api/voice/models")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.expect("read body").to_bytes();
    let delete_resp: VoiceModelDeleteResponse =
        serde_json::from_slice(&body).expect("deserialize delete response");

    assert!(delete_resp.success);
    assert_eq!(delete_resp.state, ModelInstallState::NotInstalled);
}
