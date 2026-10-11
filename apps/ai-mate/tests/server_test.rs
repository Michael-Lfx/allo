use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ai_mate::config::Config;
use ai_mate::server::build_router;

#[tokio::test]
async fn test_healthz_endpoint() {
    let config = Config::default();
    let app = build_router(&config);

    let request = Request::builder()
        .uri("/healthz")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.expect("handler must succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header must be present")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("application/json"),
        "expected application/json, got {content_type}"
    );

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).expect("valid JSON response");

    assert_eq!(json["status"], "ok");
    assert_eq!(json["app"], "ai-mate");
}

#[tokio::test]
async fn test_index_oobe_pairing_page_structure() {
    let config = Config::default();
    let app = build_router(&config);

    let request = Request::builder()
        .uri("/")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.expect("handler must succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header must be present")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/html"),
        "expected text/html, got {content_type}"
    );

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body_bytes.to_vec()).expect("HTML must be valid utf8");

    // 1. Viewport meta tag (360px mobile-first)
    assert!(
        html.contains("viewport") && html.contains("width=device-width"),
        "HTML must include mobile-first viewport meta tag"
    );

    // 2. Section 1: Small brand mark
    assert!(
        html.contains("Flowy AI Mate"),
        "HTML must include brand header 'Flowy AI Mate'"
    );

    // 3. Section 2: Device status row
    assert!(
        html.contains("device-status"),
        "HTML must include device status section"
    );

    // 4. Section 3: Pairing OTP code input (3+3 grouping, numeric inputmode)
    assert!(
        html.contains("inputmode=\"numeric\""),
        "HTML must specify inputmode='numeric' on OTP input"
    );
    assert!(
        html.contains("maxlength=\"6\""),
        "HTML must restrict OTP input to 6 characters"
    );
    assert!(
        html.contains("otp-box") || html.contains("otp-digit"),
        "HTML must contain visual OTP box projections"
    );

    // 5. Section 4: Model readiness projection zone with aria-live="polite"
    assert!(
        html.contains("aria-live=\"polite\""),
        "HTML must include aria-live='polite' for accessibility announcements"
    );
    assert!(
        html.contains("hairline-progress") || html.contains("progress-track"),
        "HTML must contain hairline progress track"
    );

    // 6. Section 5: Completion state zone
    assert!(
        html.contains("completion") || html.contains("ready-state"),
        "HTML must contain completion section"
    );

    // 7. Styling: prefers-color-scheme responsive dark/light theme support
    assert!(
        html.contains("prefers-color-scheme"),
        "CSS must handle prefers-color-scheme"
    );

    // 8. Accessibility: touch target sizing (>= 44px)
    assert!(
        html.contains("44px") || html.contains("48px"),
        "CSS must ensure touch targets >= 44px"
    );
}

#[tokio::test]
async fn test_not_found_route() {
    let config = Config::default();
    let app = build_router(&config);

    let request = Request::builder()
        .uri("/nonexistent_endpoint")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
