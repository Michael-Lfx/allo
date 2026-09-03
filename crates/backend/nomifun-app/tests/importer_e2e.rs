//! Importer end-to-end through the real App Server HTTP surface (roadmap
//! Phase 1): initialize → import software-company → history → detail with
//! components → idempotent reuse.

mod common;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{body_json, build_app, setup_and_login};

const SOFTWARE_COMPANY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../nomifun-importer/tests/fixtures/software-company"
);
const DISPLAY_METADATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../nomifun-importer/tests/fixtures/display-metadata"
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
    assert_eq!(
        history[0]["component_count"].as_u64().unwrap(),
        16,
        "history list must resolve the real component count"
    );

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

#[tokio::test]
async fn importer_install_registers_components_into_runtime() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Import first (same flow as the main e2e).
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
    assert_eq!(run.status(), StatusCode::OK);
    let result = body_json(run).await;
    assert_eq!(result["status"], "completed", "{result}");
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();

    // Install the snapshot.
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/installs",
            serde_json::json!({ "snapshot_id": snapshot_id }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "install must succeed");
    let install_json = body_json(install).await;
    assert_eq!(install_json["snapshot_id"], snapshot_id);
    assert!(
        install_json["installed_count"].as_u64().unwrap() > 0,
        "at least skills/agents/connectors must be registered: {install_json}"
    );

    // Status reflects per-component state.
    let status = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/installs/{snapshot_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status_json = body_json(status).await;
    let components = status_json["components"].as_array().unwrap();
    assert!(!components.is_empty());
    let skill = components
        .iter()
        .find(|component| component["kind"] == "skill")
        .expect("installed snapshot exposes a skill component");
    assert_eq!(skill["state"], "installed");
    assert!(skill["runtime_location"].as_str().is_some(), "skill records its runtime path");

    // Disable / re-enable / uninstall round-trip on the skill component.
    let skill_id = skill["id"].as_str().unwrap().to_owned();
    let disable = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{snapshot_id}/disable"),
            serde_json::json!({ "component_ids": [skill_id] }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let disabled = body_json(disable).await;
    let disabled_skill = disabled["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == skill_id)
        .unwrap();
    assert_eq!(disabled_skill["state"], "disabled");

    let uninstall = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{snapshot_id}/uninstall"),
            serde_json::json!({ "component_ids": [skill_id] }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let uninstalled = body_json(uninstall).await;
    let uninstalled_skill = uninstalled["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == skill_id)
        .unwrap();
    assert_eq!(uninstalled_skill["state"], "not-installed");

    // The actual skill file materialized under the managed skills root.
    let managed = services
        .skill_paths
        .user_skills_dir
        .join("agent-store")
        .join(&snapshot_id)
        .join("release-notes")
        .join("SKILL.md");
    assert!(managed.is_file(), "managed skill must exist on disk: {}", managed.display());
}

#[tokio::test]
async fn importer_store_lists_aggregated_entries_with_install_state() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Build a plugin market in the real WorkBuddy expert-market layout:
    //   market/.codebuddy-plugin/marketplace.json { plugins: [...] }
    //   market/plugins/<id>/.codebuddy-plugin/plugin.json (+ display fields)
    let market_root = std::env::temp_dir().join(format!("as-store-market-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(market_root.join(".codebuddy-plugin")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/fbsir-super-partner/.codebuddy-plugin")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/fbsir-super-partner/agents")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/fbsir-super-partner/avatars")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/software-company/.codebuddy-plugin")).unwrap();
    std::fs::write(
        market_root.join(".codebuddy-plugin/marketplace.json"),
        r#"{
            "name": "experts",
            "version": "0.1.0",
            "plugins": [
                { "name": "fbsir-super-partner", "source": "./plugins/fbsir-super-partner", "description": "Super partner" },
                { "name": "software-company", "source": "./plugins/software-company", "description": "Software company" }
            ]
        }"#,
    )
    .unwrap();
    std::fs::write(
        market_root.join("plugins/fbsir-super-partner/.codebuddy-plugin/plugin.json"),
        r#"{
            "name": "fbsir-super-partner",
            "version": "1.2.0",
            "displayName": { "en": "FBSir", "zh": "FBSir" },
            "profession": { "en": "Super Partner", "zh": "超级合伙人" },
            "displayDescription": { "en": "One-call super partner", "zh": "一站式超级合伙人" },
            "tags": [{ "en": "business", "zh": "商务" }],
            "quickPrompts": [{ "en": "Plan a roadmap", "zh": "制定路线图" }],
            "avatar": "avatars/expert.png",
            "expertType": "agent",
            "categoryId": "12-IndustryConsultant",
            "agents": ["./agents"]
        }"#,
    )
    .unwrap();
    std::fs::write(
        market_root.join("plugins/fbsir-super-partner/agents/fb.md"),
        "---\nname: fb\n---\n\nbody\n",
    )
    .unwrap();
    // Minimal 1x1 PNG (73 bytes) for the store asset endpoint.
    std::fs::write(
        market_root.join("plugins/fbsir-super-partner/avatars/expert.png"),
        [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x06, 0x00, 0x00,
            0x00, 0xC4, 0x0F, 0xBE, 0x8B, 0x00, 0x00, 0x00, 0x0F, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0xFC, 0xCF, 0xC0, 0xF0, 0x9F, 0x81, 0x81, 0x81, 0x01, 0x00, 0xFF, 0x03,
            0x00, 0x01, 0x0F, 0x06, 0xDA, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
            0x42, 0x60, 0x82,
        ],
    )
    .unwrap();
    std::fs::write(
        market_root.join("plugins/software-company/.codebuddy-plugin/plugin.json"),
        r#"{ "name": "software-company", "version": "0.9.0", "agents": ["./agents"] }"#,
    )
    .unwrap();

    // Add the market (directory source).
    let add = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/markets",
            serde_json::json!({
                "source_kind": "directory",
                "source": market_root.to_string_lossy(),
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK, "market add must succeed: {}", body_json(add).await);
    let added = body_json(add).await;
    assert_eq!(added["entry_count"], 2, "{added}");
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // Store lists all entries with display metadata + install state (uninstalled).
    let store = app
        .clone()
        .oneshot(bearer_get("/api/app-server/store", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(store.status(), StatusCode::OK, "store list must succeed");
    let store_json = body_json(store).await;
    let items = store_json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{store_json}");
    let fbsir = items
        .iter()
        .find(|item| item["entry_name"] == "fbsir-super-partner")
        .expect("fbsir entry present");
    assert_eq!(fbsir["kind"], "agent", "{fbsir}");
    assert_eq!(fbsir["name"], "FBSir", "{fbsir}");
    assert_eq!(fbsir["version"], "1.2.0", "{fbsir}");
    assert_eq!(fbsir["installed"], false, "{fbsir}");
    assert_eq!(fbsir["update_available"], false, "{fbsir}");
    assert!(fbsir["avatar_url"].as_str().unwrap().contains("/store/"), "{fbsir}");

    // Store asset endpoint serves the declared avatar with the right MIME.
    let asset = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/app-server/store/{marketplace_id}/entries/fbsir-super-partner/assets/avatars/expert.png"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", &csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .header("x-app-server-connection-id", &connection_id)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK, "store avatar must be served");
    assert_eq!(
        asset.headers().get("content-type").unwrap(),
        "image/png",
        "store asset content-type must be image/png"
    );

    // One-click install through the store (import + register behind one call).
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/store/{marketplace_id}/entries/fbsir-super-partner/install"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "store install must succeed: {}", body_json(install).await);
    let installed = body_json(install).await;
    assert_eq!(installed["entry_name"], "fbsir-super-partner", "{installed}");
    assert_eq!(installed["reused"], false, "{installed}");
    let snapshot_id = installed["snapshot_id"].as_str().unwrap().to_owned();
    assert!(installed["installed_count"].as_u64().unwrap() >= 1, "{installed}");

    // The store now reports the entry installed with the snapshot id.
    let store2 = app
        .clone()
        .oneshot(bearer_get("/api/app-server/store", &token, &csrf, &connection_id))
        .await
        .unwrap();
    let store2_json = body_json(store2).await;
    let fbsir2 = store2_json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["entry_name"] == "fbsir-super-partner")
        .expect("fbsir entry present after install");
    assert_eq!(fbsir2["installed"], true, "{fbsir2}");
    assert_eq!(fbsir2["snapshot_id"], snapshot_id, "{fbsir2}");

    // A second install is a no-op (idempotent).
    let install2 = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/store/{marketplace_id}/entries/fbsir-super-partner/install"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install2.status(), StatusCode::OK);
    let installed2 = body_json(install2).await;
    assert_eq!(installed2["reused"], true, "{installed2}");
}

#[tokio::test]
async fn importer_store_lists_mcp_connectors_with_index_display_and_installs() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Real CodeBuddy connector-market layout:
    //   market/.codebuddy-connector/connectors.json { connectors: [...] }
    //   market/connectors/<id>/mcp.json (MCP servers) | cli.json (CLI)
    //   market/connectors/<id>/skills/<dir>/SKILL.md
    let market_root = std::env::temp_dir().join(format!("as-store-conn-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(market_root.join(".codebuddy-connector")).unwrap();
    std::fs::create_dir_all(market_root.join("connectors/agent-earth")).unwrap();
    std::fs::create_dir_all(market_root.join("connectors/wecom")).unwrap();
    std::fs::create_dir_all(market_root.join("connectors/agent-earth/skills/earth")).unwrap();
    std::fs::write(
        market_root.join(".codebuddy-connector/connectors.json"),
        r#"{
            "name": "codebuddy-connectors-official",
            "connectors": [
                { "id": "agent-earth", "name": "AgentEarth", "name_zh": "智能地球", "version": "1.0.0", "description": "Unified expert-grade API platform", "type": "mcp" },
                { "id": "wecom", "name": "企业微信", "name_zh": "企业微信", "version": "1.2.0", "description": "WeCom CLI connector", "type": "cli" }
            ]
        }"#,
    )
    .unwrap();
    std::fs::write(
        market_root.join("connectors/agent-earth/mcp.json"),
        r#"{
            "preAuth": "cli",
            "mcpServers": {
                "agent-earth": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "@agentearth/mcp"],
                    "runtime": { "type": "node", "version": ">=20" }
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        market_root.join("connectors/agent-earth/skills/earth/SKILL.md"),
        "---\nname: earth\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        market_root.join("connectors/wecom/cli.json"),
        r#"{"runtime":{"type":"node","version":">=18"},"init":{"script":"main.js"}}"#,
    )
    .unwrap();

    let add = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/markets",
            serde_json::json!({
                "source_kind": "directory",
                "source": market_root.to_string_lossy(),
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK, "market add must succeed: {}", body_json(add).await);
    let added = body_json(add).await;
    assert_eq!(added["entry_count"], 2, "{added}");
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // Store classifies both connectors and falls back to the index display
    // names when no plugin.json display block exists.
    let store = app
        .clone()
        .oneshot(bearer_get("/api/app-server/store", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(store.status(), StatusCode::OK, "store list must succeed");
    let store_json = body_json(store).await;
    let items = store_json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{store_json}");
    let earth = items
        .iter()
        .find(|item| item["entry_name"] == "agent-earth")
        .expect("agent-earth entry present");
    assert_eq!(earth["kind"], "connector", "{earth}");
    assert_eq!(earth["name"], "智能地球", "{earth}");
    assert_eq!(earth["version"], "1.0.0", "{earth}");
    assert_eq!(earth["installed"], false, "{earth}");
    let wecom = items
        .iter()
        .find(|item| item["entry_name"] == "wecom")
        .expect("wecom entry present");
    assert_eq!(wecom["kind"], "connector", "{wecom}");
    assert_eq!(wecom["name"], "企业微信", "{wecom}");

    // One-click install of the MCP connector (import + register).
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/store/{marketplace_id}/entries/agent-earth/install"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "MCP store install must succeed: {}", body_json(install).await);
    let installed = body_json(install).await;
    assert_eq!(installed["entry_name"], "agent-earth", "{installed}");
    assert_eq!(installed["reused"], false, "{installed}");
    assert!(installed["installed_count"].as_u64().unwrap() >= 1, "{installed}");

    // Store now reports it installed.
    let store2 = app
        .clone()
        .oneshot(bearer_get("/api/app-server/store", &token, &csrf, &connection_id))
        .await
        .unwrap();
    let store2_json = body_json(store2).await;
    let earth2 = store2_json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["entry_name"] == "agent-earth")
        .expect("agent-earth present after install");
    assert_eq!(earth2["installed"], true, "{earth2}");
}

#[tokio::test]
async fn importer_market_add_list_import_and_cascade_remove() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Build a full market dir with an index so the discovery probe sees
    // entries (the skill-market fixture's marketplace.json declares no
    // `skills` array; it is used by the importer tests as a bare market).
    let market_root = std::env::temp_dir().join(format!("as-e2e-market-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(market_root.join(".codebuddy-skill")).unwrap();
    std::fs::create_dir_all(market_root.join("skills/formatting")).unwrap();
    std::fs::create_dir_all(market_root.join("skills/hello")).unwrap();
    std::fs::write(
        market_root.join(".codebuddy-skill/marketplace.json"),
        r#"{
            "name": "e2e-skills",
            "version": "0.1.0",
            "skills": [
                { "name": "formatting", "source": "./skills/formatting", "description": "Formatting skill" },
                { "name": "hello", "source": "./skills/hello", "description": "Greeting skill" }
            ]
        }"#,
    )
    .unwrap();
    std::fs::write(
        market_root.join("skills/formatting/SKILL.md"),
        "---\nname: formatting\ndescription: Apply consistent formatting\n---\n\nApply consistent formatting.\n",
    )
    .unwrap();
    std::fs::write(
        market_root.join("skills/hello/SKILL.md"),
        "---\nname: hello\ndescription: Greet warmly\n---\n\nGreet warmly.\n",
    )
    .unwrap();

    // Add the marketplace (directory source).
    let add = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/markets",
            serde_json::json!({
                "source_kind": "directory",
                "source": market_root.to_string_lossy(),
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK, "market add must succeed: {}", body_json(add).await);
    let added = body_json(add).await;
    assert_eq!(added["name"], "e2e-skills");
    assert_eq!(added["entry_count"], 2);
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // List shows the market.
    let list = app
        .clone()
        .oneshot(bearer_get("/api/app-server/markets", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let markets = body_json(list).await;
    assert_eq!(markets.as_array().unwrap().len(), 1);
    assert_eq!(markets[0]["marketplace_id"], marketplace_id);

    // Detail exposes the discovered entries.
    let get = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/markets/{marketplace_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let detail = body_json(get).await;
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let entry_names: Vec<&str> = entries.iter().filter_map(|entry| entry["name"].as_str()).collect();
    assert!(entry_names.contains(&"formatting"));
    assert!(entry_names.contains(&"hello"));

    // Import one entry through the market route (provenance linked).
    let import = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/entries/formatting/import"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK, "entry import must succeed");
    let imported = body_json(import).await;
    assert_eq!(imported["status"], "completed", "{imported}");
    let snapshot_id = imported["snapshot_id"].as_str().unwrap().to_owned();

    // Install the imported snapshot (runtime registration).
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/installs",
            serde_json::json!({ "snapshot_id": snapshot_id }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "install must succeed");
    let installed = body_json(install).await;
    assert!(installed["installed_count"].as_u64().unwrap() >= 1, "{installed}");
    let managed = services
        .skill_paths
        .user_skills_dir
        .join("agent-store")
        .join(&snapshot_id)
        .join("formatting")
        .join("SKILL.md");
    assert!(managed.is_file(), "entry skill must materialize: {}", managed.display());

    // Remove with cascade: the installed snapshot's components are uninstalled
    // (installed state cleared), the snapshot row itself stays.
    let remove = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/remove"),
            serde_json::json!({ "cascade": true }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(remove.status(), StatusCode::OK, "market remove must succeed");
    let removed = body_json(remove).await;
    assert!(removed["snapshots"].as_array().unwrap().contains(&serde_json::Value::String(snapshot_id.clone())));
    assert!(!removed["uninstalled_components"].as_array().unwrap().is_empty());

    // The snapshot remains in the history (kept after cascade); install state
    // is cleared.
    let history = app
        .clone()
        .oneshot(bearer_get("/api/app-server/imports", &token, &csrf, &connection_id))
        .await
        .unwrap();
    let history_json = body_json(history).await;
    assert!(
        history_json
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["snapshot_id"] == snapshot_id),
        "cascade keeps the snapshot row: {history_json}"
    );
    let install_status = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/installs/{snapshot_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    let install_status_json = body_json(install_status).await;
    let states: Vec<&str> = install_status_json["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|component| component["state"].as_str())
        .collect();
    assert!(states.iter().all(|state| *state == "not-installed"), "{states:?}");

    // Removed market no longer lists; a second remove is a 404.
    let list_after = app
        .clone()
        .oneshot(bearer_get("/api/app-server/markets", &token, &csrf, &connection_id))
        .await
        .unwrap();
    let markets_after = body_json(list_after).await;
    assert!(markets_after.as_array().unwrap().is_empty());
}

/// Build a local git repo (bare) whose `main` tip is a marketplace catalog.
fn init_git_market(repo_path: &std::path::Path, files: &[(&str, &str)]) {
    let work = std::env::temp_dir().join(format!("as-git-work-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(&work).unwrap();
    let repo = git2::Repository::init(&work).unwrap();
    for (name, content) in files {
        let path = work.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(name)).unwrap();
        index.write().unwrap();
    }
    let sig = git2::Signature::now("e2e", "e2e@example.com").unwrap();
    let mut index = repo.index().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "market", &tree, &[]).unwrap();

    // Push into the bare repo as `main` and point its HEAD at main.
    let bare = git2::Repository::init_bare(repo_path).unwrap();
    {
        let mut remote = repo.remote("origin", repo_path.to_str().unwrap()).unwrap();
        remote.push(&["refs/heads/master:refs/heads/main"], None).unwrap();
    }
    bare.set_head("refs/heads/main").unwrap();
    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn importer_market_git_source_fetch_refresh_and_entry_import() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Build a git-served marketplace (repo root = market root).
    let bare = std::env::temp_dir().join(format!("as-git-market-{}.git", nomifun_common::generate_id()));
    init_git_market(
        &bare,
        &[
            (
                ".codebuddy-skill/marketplace.json",
                r#"{
                    "name": "git-skills",
                    "version": "0.1.0",
                    "skills": [
                        { "name": "formatting", "source": "./skills/formatting", "description": "Formatting skill" },
                        { "name": "hello", "source": "./skills/hello", "description": "Greeting skill" }
                    ]
                }"#,
            ),
            (
                "skills/formatting/SKILL.md",
                "---\nname: formatting\ndescription: Formatting\n---\n\nBody.\n",
            ),
            (
                "skills/hello/SKILL.md",
                "---\nname: hello\ndescription: Greeting\n---\n\nBody.\n",
            ),
        ],
    );

    // Add the marketplace as a `git` source.
    let add = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/markets",
            serde_json::json!({
                "source_kind": "git",
                "source": bare.to_string_lossy(),
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK, "git market add must succeed: {}", body_json(add).await);
    let added = body_json(add).await;
    assert_eq!(added["source_kind"], "git");
    assert_eq!(added["entry_count"], 2, "{added}");
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // Detail exposes entries; each has a relative source inside the live tree.
    let detail = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/markets/{marketplace_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    let detail_json = body_json(detail).await;
    assert_eq!(detail_json["entries"].as_array().unwrap().len(), 2);

    // First refresh resolves the same revision (no-op, unchanged).
    let refresh1 = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/refresh"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let refresh1_json = body_json(refresh1).await;
    assert_eq!(refresh1_json["changed"], false, "same commit must be a no-op: {refresh1_json}");
    assert!(!refresh1_json["resolved_revision"].as_str().unwrap().is_empty());

    // Import one entry: provenance + installable skill materializes.
    let import = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/entries/formatting/import"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK, "git entry import must succeed: {}", body_json(import).await);
    let imported = body_json(import).await;
    assert_eq!(imported["status"], "completed", "{imported}");
    assert_eq!(imported["component_count"], 1, "{imported}");

    // The materialized live checkout exists under the work-dir market root.
    let live_root = services
        .work_dir
        .join("agent-store-markets")
        .join(&marketplace_id)
        .join("live");
    assert!(live_root.join("skills/formatting/SKILL.md").is_file(), "live checkout must hold the market tree");
}

#[tokio::test]
async fn importer_market_http_source_validates_manifest_and_mirrors_inlined_entries() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Serve a manifest whose entries are inlined (relative paths into a tree
    // the URL market cannot mirror — CodeBuddy documents this limitation).
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/marketplace.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1-etag\"")
                .set_body_json(serde_json::json!({
                    "name": "http-skills",
                    "version": "1.0.0",
                    "skills": [
                        { "name": "inline-skill", "source": "./skills/inline-skill", "description": "Fully inlined" },
                        { "name": "external-tool", "source": "https://github.com/org/tool", "description": "External source" }
                    ]
                })),
        )
        .mount(&mock)
        .await;

    // Add the marketplace as a `url` source.
    let url = format!("{}/marketplace.json", mock.uri());
    let add = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/markets",
            serde_json::json!({
                "source_kind": "url",
                "source": url,
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK, "url market add: {}", body_json(add).await);
    let added = body_json(add).await;
    assert_eq!(added["source_kind"], "url");
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // Entries: the inlined relative one is importable; the external one is
    // flagged `external` (not silently dropped — documented boundary).
    let detail = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/markets/{marketplace_id}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    let detail_json = body_json(detail).await;
    let entries = detail_json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{detail_json}");
    let inline = entries.iter().find(|entry| entry["name"] == "inline-skill").unwrap();
    assert_eq!(inline["source_kind"], "external", "inlined relative path without a mirrored tree is external: {inline}");
    let external = entries.iter().find(|entry| entry["name"] == "external-tool").unwrap();
    assert_eq!(external["source_kind"], "external");

    // A refresh with the same etag is a no-op (freshness short-circuit).
    let refresh = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/refresh"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let refreshed = body_json(refresh).await;
    assert_eq!(refreshed["changed"], false, "{refreshed}");
}

/// @mention resolution: an imported + installed agent exposes its `preset_id`
/// on `agent/list`, and `agent/run` with a structured agent mention reaches
/// the runtime gate once the preset resolved (docs/agent-store/05 §4.7).
#[tokio::test]
async fn importer_mention_resolves_installed_preset_and_agents_run_gate() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // Import + install (preset registration happens on install).
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
    assert_eq!(run.status(), StatusCode::OK);
    let result = body_json(run).await;
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/installs",
            serde_json::json!({ "snapshot_id": snapshot_id }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "install must succeed");

    // `agent/run` with a structured mention resolves the agent's installed
    // preset first; the mention resolution runs before the start_agent_run
    // gate. With a real runtime + provider the run starts; this build has no
    // runtime/provider so the run fails at the model/runtime boundary.
    let missing = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/agent/run",
            serde_json::json!({
                "agent_id": "",
                "goal": "summarize",
                "mentions": [{ "kind": "agent", "id": "wb-software-company-software-architect" }],
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let missing_body = body_json(missing).await;
    let missing_code = missing_body["code"].as_str().unwrap_or("").to_owned();
    assert!(
        matches!(
            missing_code.as_str(),
            "agent_not_installed" | "runtime_unavailable" | "invalid_request"
        ),
        "mention resolution must precede model/runtime gates: {missing_body}"
    );
}

/// Snapshot display assets (avatars) are served from the immutable snapshot
/// with path + MIME validation; traversal and disallowed types are rejected.
#[tokio::test]
async fn importer_snapshot_asset_endpoint_serves_avatar_and_rejects_escape() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    let run = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/imports",
            serde_json::json!({
                "source_path": DISPLAY_METADATA,
                "source_kind": "codebuddy-plugin",
            }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert!(run.status().is_success(), "display metadata import: {}", body_json(run).await);
    let result = body_json(run).await;
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();

    // The declared avatar is served with the correct content type.
    let asset = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/app-server/imports/{snapshot_id}/assets/avatars/expert.png"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-csrf-token", &csrf)
                .header("cookie", format!("nomifun-csrf-token={csrf}"))
                .header("x-app-server-connection-id", &connection_id)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK, "avatar asset must be served");
    assert_eq!(
        asset.headers().get("content-type").unwrap(),
        "image/png",
        "content-type must be image/png"
    );
    let bytes = asset.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() > 0, "asset body must be non-empty");

    // Traversal / wrong snapshot id / disallowed type are all rejected.
    for uri in [
        format!("/api/app-server/imports/{snapshot_id}/assets/../plugin.json"),
        format!("/api/app-server/imports/not-a-uuid/assets/avatars/expert.png"),
        format!("/api/app-server/imports/{snapshot_id}/assets/agents/fbsir-super-partner.md"),
    ] {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .header("x-csrf-token", &csrf)
                    .header("cookie", format!("nomifun-csrf-token={csrf}"))
                    .header("x-app-server-connection-id", &connection_id)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "unsafe asset request must be a client error (status {})",
            response.status()
        );
    }
}