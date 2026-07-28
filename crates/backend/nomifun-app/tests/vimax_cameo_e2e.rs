//! Cameo upload / list / delete E2E for `/api/vimax/sessions/{id}/cameos`.

mod common;

use axum::body::Body;
use axum::http::StatusCode;
use common::{body_json, build_app, json_with_token, setup_and_login};
use http_body_util::BodyExt;
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::json;
use tower::ServiceExt;

fn tiny_png_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

struct CameoMultipart {
    boundary: String,
    parts: Vec<u8>,
}

impl CameoMultipart {
    fn new() -> Self {
        Self {
            boundary: "----CameoTestBoundary7QwE".to_owned(),
            parts: Vec::new(),
        }
    }

    fn add_text(mut self, name: &str, value: &str) -> Self {
        self.parts
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.parts.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        self.parts.extend_from_slice(value.as_bytes());
        self.parts.extend_from_slice(b"\r\n");
        self
    }

    fn add_file(mut self, name: &str, filename: &str, mime: &str, data: &[u8]) -> Self {
        self.parts
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.parts.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
            )
            .as_bytes(),
        );
        self.parts
            .extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
        self.parts.extend_from_slice(data);
        self.parts.extend_from_slice(b"\r\n");
        self
    }

    fn build(mut self) -> (String, Vec<u8>) {
        self.parts
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (
            format!("multipart/form-data; boundary={}", self.boundary),
            self.parts,
        )
    }
}

fn cameo_upload_request(
    session_id: &str,
    content_type: &str,
    body: Vec<u8>,
    token: &str,
    csrf: &str,
) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/vimax/sessions/{session_id}/cameos"))
        .header("content-type", content_type)
        .header("content-length", body.len())
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn cameo_upload_list_get_delete_roundtrip() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;

    // Missing file field → 400
    let (ct, body) = CameoMultipart::new()
        .add_text("character_name", "Bob")
        .build();
    let upload = cameo_upload_request("not-a-real-session", &ct, body, &token, &csrf);
    let resp = app.clone().oneshot(upload).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let create = json_with_token(
        "POST",
        "/api/vimax/sessions",
        json!({ "workflow": "idea2video", "title": "Cameo E2E" }),
        &token,
        &csrf,
    );
    let resp = app.clone().oneshot(create).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let created = body_json(resp).await;
    let session_id = created["data"]["id"].as_str().expect("session id").to_string();

    // Missing file on a real session → 400
    let (ct, body) = CameoMultipart::new()
        .add_text("character_name", "Bob")
        .build();
    let upload = cameo_upload_request(&session_id, &ct, body, &token, &csrf);
    let resp = app.clone().oneshot(upload).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let png = tiny_png_bytes();
    let (ct, body) = CameoMultipart::new()
        .add_text("character_name", "Alice")
        .add_text("description", "hero")
        .add_file("file", "alice.png", "image/png", &png)
        .build();
    let upload = cameo_upload_request(&session_id, &ct, body, &token, &csrf);
    let resp = app.clone().oneshot(upload).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let uploaded = body_json(resp).await;
    let cameo_id = uploaded["data"]["id"].as_str().expect("cameo id").to_string();
    assert_eq!(uploaded["data"]["character_name"], "Alice");

    let list = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/vimax/sessions/{session_id}/cameos"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(list).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let listed = body_json(resp).await;
    assert_eq!(listed["data"]["photos"].as_array().unwrap().len(), 1);

    let file = axum::http::Request::builder()
        .method("GET")
        .uri(format!(
            "/api/vimax/sessions/{session_id}/cameos/{cameo_id}/file"
        ))
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(file).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() > 24);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    let patch = json_with_token(
        "PATCH",
        &format!("/api/vimax/sessions/{session_id}/cameos/{cameo_id}"),
        json!({ "character_name": "Alice Prime", "description": "updated" }),
        &token,
        &csrf,
    );
    let resp = app.clone().oneshot(patch).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = body_json(resp).await;
    assert_eq!(patched["data"]["character_name"], "Alice Prime");

    let del = axum::http::Request::builder()
        .method("DELETE")
        .uri(format!("/api/vimax/sessions/{session_id}/cameos/{cameo_id}"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(del).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let list2 = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/vimax/sessions/{session_id}/cameos"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(list2).await.unwrap();
    let listed = body_json(resp).await;
    assert!(listed["data"]["photos"]
        .as_array()
        .unwrap()
        .is_empty());
}
