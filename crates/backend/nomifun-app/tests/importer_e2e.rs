//! Importer end-to-end through the real App Server HTTP surface (roadmap
//! Phase 1): initialize → import software-company → history → detail with
//! components → idempotent reuse.

mod common;

use axum::http::StatusCode;
use tower::ServiceExt;

use common::{body_json, build_app, setup_and_login};

const SOFTWARE_COMPANY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../nomifun-importer/tests/fixtures/software-company"
);
const PROTOCOL_VERSION: &str = "2026-08-26";

fn bearer_json(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    token: &str,
    csrf: &str,
    connection_id: Option<&str>,
) -> axum::http::Request<axum::body::Body> {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"));
    if let Some(connection_id) = connection_id {
        builder = builder.header("x-app-server-connection-id", connection_id);
    }
    builder
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn bearer_get(uri: &str, token: &str, csrf: &str, connection_id: &str) -> axum::http::Request<axum::body::Body> {
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

async fn app_server_handshake(
    app: &mut axum::Router,
    token: &str,
    csrf: &str,
) -> String {
    let response = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/initialize",
            serde_json::json!({
                "protocol_version": PROTOCOL_VERSION,
                "client": { "name": "importer-e2e", "version": "1" },
                "capabilities": {},
            }),
            token,
            csrf,
            None,
        ))
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
    assert!(init["capabilities"]["imports"].as_bool().unwrap(), "imports capability advertised");
    assert!(init["capabilities"]["agents"].as_bool().unwrap(), "agents catalog advertised");
    assert!(init["capabilities"]["teams"].as_bool().unwrap(), "teams catalog advertised");

    let ready = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/initialized",
            serde_json::json!({}),
            token,
            csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert!(ready.status().is_success(), "initialized must succeed");
    connection_id
}

#[tokio::test]
async fn importer_end_to_end_imports_software_company() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Run the import through POST /api/app-server/imports.
    let run = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/imports",
            serde_json::json!({
                "source_path": SOFTWARE_COMPANY,
                "source_kind": "codebuddy-plugin",
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(run.status(), StatusCode::OK, "import POST must succeed");
    let result = body_json(run).await;
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["name"], "software-company");
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();
    assert!(!result["content_digest"].as_str().unwrap().is_empty());
    assert!(!result["reused"].as_bool().unwrap());

    // History lists exactly one snapshot.
    let list = app
        .clone()
        .oneshot(bearer_get("/api/app-server/imports", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let history = body_json(list).await;
    assert_eq!(history.as_array().unwrap().len(), 1);

    // Detail exposes the standardized components: 5 agents + 1 team.
    let detail = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/imports/{snapshot_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_json = body_json(detail).await;
    let components = detail_json["components"].as_array().unwrap();
    let agents = components.iter().filter(|component| component["kind"] == "agent").count();
    let teams = components.iter().filter(|component| component["kind"] == "team").count();
    assert_eq!(agents, 5, "software-company must yield 5 AgentDefinitions");
    assert_eq!(teams, 1, "software-company must yield 1 AgentTeamDefinition");
    let team = components.iter().find(|component| component["kind"] == "team").unwrap();
    assert_eq!(team["compatibility"]["semantic_status"], "compatible_with_adapter");
    assert_eq!(
        team["compatibility"]["runtime_status"],
        "not-verified",
        "import never claims runtime readiness (03 §6)"
    );

    // Idempotency: the same digest reuses the same immutable snapshot.
    let again = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/imports",
            serde_json::json!({
                "source_path": SOFTWARE_COMPANY,
                "source_kind": "codebuddy-plugin",
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let again_json = body_json(again).await;
    assert_eq!(again_json["status"], "completed");
    assert!(again_json["reused"].as_bool().unwrap(), "same digest must reuse the snapshot");
    assert_eq!(again_json["snapshot_id"], snapshot_id);

    // Missing source returns a 404-style error without leaking sensitive text.
    let missing = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/imports",
            serde_json::json!({
                "source_path": "/definitely/not/a/real/plugin",
                "source_kind": "codebuddy-plugin",
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_json = body_json(missing).await;
    assert_eq!(missing_json["code"], "not_found");
}