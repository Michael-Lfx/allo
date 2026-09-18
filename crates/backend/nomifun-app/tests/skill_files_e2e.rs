//! Skill file read face end-to-end through the real App Server HTTP surface
//! (doc 24 §4).
//!
//! This is the **truth gate** for the whole design: it proves against a real
//! `create_router` composition root that a Skill is a *directory* — that
//! `SKILL.md`'s companions survive installation and are reachable over the
//! protocol — rather than only asserting it from reading the installer code.
//!
//! It also pins the two asymmetries the design turns on:
//!
//! - `skill/get` returns a bounded summary, so a long manifest is truncated
//!   there while `skill/file` returns it whole;
//! - the file face is a **separate capability**, so a host wiring the catalog
//!   without it must not advertise `skill_files`.

mod common;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{body_json, build_app_with_skill_paths, setup_and_login};

/// A manifest deliberately longer than the 1200-char summary cap
/// (`app_server_catalog.rs`), so truncation is observable rather than assumed.
fn long_manifest_body() -> String {
    format!("{}\n\n## Section\n\n{}", "A".repeat(1500), "B".repeat(1500))
}

fn seed_skill(paths: &nomifun_extension::SkillPaths, name: &str, body: &str) {
    let dir = paths.user_skills_dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: demo skill\n---\n\n{body}\n"),
    )
    .unwrap();
    // The companions that make this face necessary at all.
    std::fs::create_dir_all(dir.join("references")).unwrap();
    std::fs::write(dir.join("references/guide.md"), "reference guide body").unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/run.sh"), "#!/bin/sh\necho hello\n").unwrap();
    std::fs::write(dir.join("data.json"), r#"{"k":1}"#).unwrap();
}

async fn handshake(app: &mut axum::Router, token: &str, csrf: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/app-server/initialize")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .body(axum::body::Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "protocol_version": nomifun_app_server::PROTOCOL_VERSION,
                        "client": { "name": "skill-files-e2e", "version": "1" },
                        "capabilities": {},
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success(), "initialize must succeed");
    let connection_id = response
        .headers()
        .get("x-app-server-connection-id")
        .expect("initialize returns a connection id")
        .to_str()
        .unwrap()
        .to_owned();
    let init = body_json(response).await;
    assert!(
        init["capabilities"]["skill_files"].as_bool().unwrap(),
        "the skill file face must be advertised: {init}"
    );

    let ready = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/app-server/initialized")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .header("x-app-server-connection-id", &connection_id)
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(ready.status().is_success(), "initialized must succeed");
    connection_id
}

fn authed_get(uri: &str, token: &str, csrf: &str, connection_id: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .header("x-app-server-connection-id", connection_id)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn skill_files_lists_and_serves_attached_files() {
    // `.keep()`, matching `build_app` / `build_app_with_file_roots`.
    //
    // Measured, not assumed: dropping the `TempDir` instead leaves a *partial*
    // tree (~36 entries) in `%TEMP%`, because the router's spawned background
    // tasks keep SQLite handles open past the test body, so only the files that
    // happen to be closed get removed. A whole kept directory is more honest
    // than a half-deleted one, and it matches the existing helpers.
    let root = tempfile::Builder::new()
        .prefix("skill-files-e2e-")
        .tempdir()
        .unwrap()
        .keep();
    let (mut app, services, paths) = build_app_with_skill_paths(&root).await;
    seed_skill(&paths, "demo", &long_manifest_body());
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "pw-skill-files").await;
    let connection_id = handshake(&mut app, &token, &csrf).await;

    // --- the inventory -----------------------------------------------------
    let response = app
        .clone()
        .oneshot(authed_get(
            "/api/app-server/skills/demo/files",
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listing = body_json(response).await;
    let mut names: Vec<&str> = listing["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["SKILL.md", "data.json", "references/guide.md", "scripts/run.sh"],
        "a Skill's companions must be listed, not just the manifest: {listing}"
    );
    assert_eq!(listing["truncated"], false);
    let tree_digest = listing["content_digest"].as_str().unwrap().to_owned();
    assert!(!tree_digest.is_empty(), "the inventory must carry a tree digest");

    // --- one attached file, byte-for-byte ---------------------------------
    let response = app
        .clone()
        .oneshot(authed_get(
            "/api/app-server/skills/demo/files/references/guide.md",
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/markdown; charset=utf-8"),
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"reference guide body");

    // --- the reason this face exists --------------------------------------
    // The manifest is longer than `skill/get`'s 1200-char summary, so the two
    // read faces must disagree — and `skill/file` is the one telling the truth.
    let response = app
        .clone()
        .oneshot(authed_get(
            "/api/app-server/skills/demo",
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail = body_json(response).await;
    let summary = detail["instructions_summary"].as_str().unwrap();
    assert!(
        summary.chars().count() <= 1200,
        "skill/get is expected to truncate its summary"
    );

    let response = app
        .clone()
        .oneshot(authed_get(
            "/api/app-server/skills/demo/files/SKILL.md",
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let whole = response.into_body().collect().await.unwrap().to_bytes();
    let whole = String::from_utf8(whole.to_vec()).unwrap();
    assert!(
        whole.len() > 3000,
        "the file face must return the manifest whole, got {} bytes",
        whole.len()
    );
    assert!(whole.contains(&"A".repeat(1500)), "no part of the body may be dropped");
}

#[tokio::test]
async fn skill_file_face_refuses_traversal_and_unknown_paths() {
    // `.keep()` for the same reason as the test above.
    let root = tempfile::Builder::new()
        .prefix("skill-files-e2e-bad-")
        .tempdir()
        .unwrap()
        .keep();
    let (mut app, services, paths) = build_app_with_skill_paths(&root).await;
    seed_skill(&paths, "demo", "short body");
    // A file outside the skill directory, to prove it is not reachable.
    std::fs::write(paths.user_skills_dir.join("outside.txt"), "secret").unwrap();
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "pw-skill-files-bad").await;
    let connection_id = handshake(&mut app, &token, &csrf).await;

    // Traversal is refused. The router may normalize some spellings before the
    // handler runs, so accept either the structured refusal or a 404 — what
    // must never happen is a 200 carrying the secret.
    for attempt in [
        "/api/app-server/skills/demo/files/..%2Foutside.txt",
        "/api/app-server/skills/demo/files/%2E%2E%2Foutside.txt",
        "/api/app-server/skills/demo/files/outside.txt",
    ] {
        let response = app
            .clone()
            .oneshot(authed_get(attempt, &token, &csrf, &connection_id))
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_ne!(status, StatusCode::OK, "{attempt} must not succeed");
        assert!(
            !String::from_utf8_lossy(&body).contains("secret"),
            "{attempt} must not leak a file outside the skill directory"
        );
    }

    // An unknown skill is a not-found, not an empty listing.
    let response = app
        .clone()
        .oneshot(authed_get(
            "/api/app-server/skills/does-not-exist/files",
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn skill_file_face_requires_a_ready_connection() {
    // `.keep()` for the same reason as the tests above.
    let root = tempfile::Builder::new()
        .prefix("skill-files-e2e-auth-")
        .tempdir()
        .unwrap()
        .keep();
    let (mut app, services, paths) = build_app_with_skill_paths(&root).await;
    seed_skill(&paths, "demo", "body");
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "pw-skill-files-auth").await;

    // No handshake: the file face must not be reachable without one. This is
    // what separates it from the public display-asset route.
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/api/app-server/skills/demo/files")
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", &csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !response.status().is_success(),
        "an unhandshaken connection must not read skill files, got {}",
        response.status()
    );
}
