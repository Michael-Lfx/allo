//! Connector tool read face end-to-end through the real App Server HTTP surface
//! (doc `26` §5).
//!
//! This is the **truth gate** for that design, and the reason it is a separate
//! file: the projection has unit tests and the SDK has a mock-host smoke, but
//! neither proves the value a caller reads is the *server's own* `inputSchema`.
//! This one runs the real composition root (`create_router`), registers a real
//! MCP server (the cross-platform stdio fixture, spawned by the host exactly as
//! production would), and compares what comes back over
//! `/api/app-server/connectors/{id}/test` with what that server declared.
//!
//! It also pins the two asymmetries the read face is built on:
//!
//! - a tool whose server declared no schema keeps **none** (absent ≠ empty), and
//! - nothing is flagged as omitted while everything fits, so `tools_truncated`
//!   means what it says.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use common::{body_json, build_app, setup_and_login};

/// The stdio MCP fixture owned by the `nomifun-mcp` crate. It answers
/// `initialize` → `notifications/initialized` → `tools/list` → `tools/call`.
const FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../nomifun-mcp/tests/fixtures/fake_stdio_mcp.mjs");

/// The schema the fixture publishes for `echo`, written out here as the expected
/// value on purpose: the assertion is equality with a *fixture-owned* fact, so a
/// projection that reformats, reorders or drops a key fails rather than passes.
fn expected_echo_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "message": { "type": "string", "description": "text to echo" } },
        "required": ["message"],
    })
}

/// `POST` with both the cookie auth and the App Server connection header.
fn app_server_post(uri: &str, token: &str, csrf: &str, connection_id: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-csrf-token", csrf)
        .header("cookie", format!("nomifun-csrf-token={csrf}"))
        .header("x-app-server-connection-id", connection_id)
        .body(Body::from("{}"))
        .unwrap()
}

/// `initialize` + `initialized`, asserting both halves of the connector face are
/// advertised — the read face must not depend on the call proxy being usable.
async fn app_server_handshake(app: &mut axum::Router, token: &str, csrf: &str) -> String {
    let initialize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/app-server/initialize")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "protocol_version": nomifun_app_server::PROTOCOL_VERSION,
                        "client": { "name": "connector-tools-e2e", "version": "1" },
                        "capabilities": {},
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(initialize.status().is_success(), "initialize must succeed");
    let connection_id = initialize
        .headers()
        .get("x-app-server-connection-id")
        .expect("initialize returns a connection id")
        .to_str()
        .unwrap()
        .to_owned();

    let handshake = body_json(initialize).await;
    assert!(
        handshake["capabilities"]["connectors"].as_bool().unwrap(),
        "the connector catalog must be advertised: {handshake}"
    );
    assert!(
        handshake["capabilities"]["connector_calls"].as_bool().unwrap(),
        "the call proxy is wired in this composition root; the read face below \
         must not be gated on it: {handshake}"
    );

    let ready = app
        .clone()
        .oneshot(app_server_post("/api/app-server/initialized", token, csrf, &connection_id))
        .await
        .unwrap();
    assert!(ready.status().is_success(), "initialized must succeed");
    connection_id
}

async fn register_fixture_connector(
    app: &mut axum::Router,
    token: &str,
    csrf: &str,
) -> String {
    let created = app
        .clone()
        .oneshot(common::json_with_token(
            "POST",
            "/api/mcp/servers",
            json!({
                "name": "connector-tools-fixture",
                "transport": { "type": "stdio", "command": "bun", "args": [FIXTURE] },
            }),
            token,
            csrf,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await;
    created["data"]["mcp_server_id"]
        .as_str()
        .expect("the created server carries its id")
        .to_owned()
}

#[tokio::test]
async fn connector_test_carries_the_servers_own_input_schema() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;

    // Registered but *not* enabled on purpose: the read face is a catalog read,
    // not a call, so it must work for a connector the proxy could never invoke.
    let connector_id = register_fixture_connector(&mut app, &token, &csrf).await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    let probed = app
        .clone()
        .oneshot(app_server_post(
            &format!("/api/app-server/connectors/{connector_id}/test"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(probed.status(), StatusCode::OK);
    let probed = body_json(probed).await;
    assert_eq!(probed["success"], json!(true), "the probe must reach the fixture: {probed}");
    assert_eq!(
        probed["tools_truncated"],
        json!(false),
        "nothing was omitted, so nothing may be flagged: {probed}"
    );

    let tools = probed["tools"].as_array().expect("tools is an array");
    let echo = tools
        .iter()
        .find(|tool| tool["name"] == json!("echo"))
        .unwrap_or_else(|| panic!("the fixture's `echo` tool must be listed: {probed}"));
    assert_eq!(
        echo["input_schema"],
        expected_echo_schema(),
        "the schema must be the server's own, verbatim"
    );

    let bare = tools
        .iter()
        .find(|tool| tool["name"] == json!("bare"))
        .unwrap_or_else(|| panic!("the fixture's `bare` tool must be listed: {probed}"));
    assert!(
        bare.get("input_schema").is_none(),
        "a tool whose server declared no schema must not gain one: {bare}"
    );
}
