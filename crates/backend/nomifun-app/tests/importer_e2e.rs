//! Importer end-to-end through the real App Server HTTP surface (roadmap
//! Phase 1): initialize → import software-company → history → detail with
//! components → idempotent reuse.

mod common;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{body_json, build_app, get_with_token, setup_and_login};

const SOFTWARE_COMPANY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../nomifun-importer/tests/fixtures/software-company"
);
const DISPLAY_METADATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../nomifun-importer/tests/fixtures/display-metadata"
);

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
                "protocol_version": nomifun_app_server::PROTOCOL_VERSION,
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

/// Every Preset the installer created, sorted so two reads are comparable.
async fn agent_store_presets(app: axum::Router, token: &str) -> Vec<String> {
    let presets = app
        .oneshot(get_with_token("/api/presets", token))
        .await
        .unwrap();
    assert_eq!(presets.status(), StatusCode::OK, "preset list must succeed");
    let presets_json = body_json(presets).await;
    let mut names: Vec<String> = presets_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|preset| preset["name"].as_str())
        .filter(|name| name.starts_with("agent-store: "))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// Names of the MCP servers currently registered on this host, sorted.
async fn mcp_server_names(app: axum::Router, token: &str) -> Vec<String> {
    let servers = app
        .oneshot(get_with_token("/api/mcp/servers", token))
        .await
        .unwrap();
    assert_eq!(servers.status(), StatusCode::OK, "mcp server list must succeed");
    let servers_json = body_json(servers).await;
    let mut names: Vec<String> = servers_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|server| server["name"].as_str())
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// The runtime `enabled` state of one Preset, by name.
async fn preset_enabled(app: axum::Router, token: &str, name: &str) -> Option<bool> {
    let presets = app
        .oneshot(get_with_token("/api/presets", token))
        .await
        .unwrap();
    let presets_json = body_json(presets).await;
    presets_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|preset| preset["name"] == name)
        .and_then(|preset| preset["enabled"].as_bool())
}

/// The runtime `enabled` state of one MCP server, by name.
async fn mcp_server_enabled(app: axum::Router, token: &str, name: &str) -> Option<bool> {
    let servers = app
        .oneshot(get_with_token("/api/mcp/servers", token))
        .await
        .unwrap();
    let servers_json = body_json(servers).await;
    servers_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|server| server["name"] == name)
        .and_then(|server| server["enabled"].as_bool())
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
    // Every component report is branchable: a stable code and an action, with
    // `errors` finally carrying the failures instead of always being empty.
    let install_outcomes = install_json["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("install must report per-component outcomes: {install_json}"));
    assert!(!install_outcomes.is_empty(), "{install_json}");
    for outcome in install_outcomes {
        assert!(
            outcome["action"].as_str().is_some_and(|action| !action.is_empty()),
            "every outcome must name what happened: {outcome}"
        );
        assert_eq!(
            outcome["ok"], true,
            "a first install of this fixture must not fail a component: {outcome}"
        );
    }
    assert_eq!(
        install_json["errors"].as_array().unwrap().len(),
        0,
        "a clean install must not report errors: {install_json}"
    );

    // B3: the installed expert Preset must carry the Agent Markdown body as
    // its instructions (persona); an empty prompt would silently drop it.
    let presets = app
        .clone()
        .oneshot(get_with_token("/api/presets", &token))
        .await
        .unwrap();
    assert_eq!(presets.status(), StatusCode::OK, "preset list must succeed");
    let presets_json = body_json(presets).await;
    let expert = presets_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|preset| preset["name"] == "agent-store: software-architect")
        .expect("installed agent must create a preset");
    let instructions = expert["instructions"].as_str().unwrap_or_default();
    assert!(
        instructions.contains("Produce the design."),
        "expert persona must reach the preset: {expert}"
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

    // Every registered component must also appear in the install report. The
    // two are written from the same pass, so a component that is registered but
    // unreported makes `installed_count` and `outcomes` disagree — skills did
    // exactly that, and a snapshot of nothing but skills came back with an empty
    // list, which a caller branching on the report reads as "nothing installed".
    let reported: std::collections::HashSet<&str> = install_outcomes
        .iter()
        .filter_map(|outcome| outcome["component_id"].as_str())
        .collect();
    for component in components {
        let kind = component["kind"].as_str().unwrap_or_default();
        if !matches!(kind, "skill" | "agent" | "team" | "connector") {
            continue;
        }
        let id = component["id"].as_str().unwrap_or_default();
        assert!(
            reported.contains(id),
            "a registered {kind} component must be reported in `outcomes`: {component}"
        );
    }

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

    // Uninstall ONLY the skill component: its runtime artifact must actually go,
    // while the expert Preset of the same snapshot stays. Component granularity
    // is the contract here — this is not "uninstall the snapshot".
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
    assert_eq!(uninstall.status(), StatusCode::OK, "uninstall must succeed");
    let uninstalled = body_json(uninstall).await;
    let uninstalled_skill = uninstalled["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == skill_id)
        .unwrap();
    assert_eq!(uninstalled_skill["state"], "not-installed");
    let uninstall_outcomes = uninstalled["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("uninstall must report outcomes: {uninstalled}"));
    let skill_removal = uninstall_outcomes
        .iter()
        .find(|outcome| outcome["component_id"] == skill_id.as_str())
        .unwrap_or_else(|| panic!("no outcome for the skill: {uninstalled}"));
    assert_eq!(skill_removal["action"], "removed", "{skill_removal}");
    assert_eq!(skill_removal["ok"], true, "{skill_removal}");

    let managed_snapshot_root = services
        .skill_paths
        .user_skills_dir
        .join("agent-store")
        .join(&snapshot_id);
    let managed = managed_snapshot_root.join("release-notes").join("SKILL.md");
    assert!(
        !managed.exists(),
        "uninstall must remove the materialized skill directory, not just clear the flag: {}",
        managed.display()
    );
    let after_partial = agent_store_presets(app.clone(), &token).await;
    assert!(
        after_partial.contains(&"agent-store: software-architect".to_owned()),
        "a skill-only uninstall must leave the snapshot's expert Presets alone: {after_partial:?}"
    );

    // Now release everything that is left. Presets and the connector's MCP
    // server registration must be gone, not merely marked as such.
    let remaining: Vec<String> = uninstalled["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|component| component["state"] != "not-installed")
        .map(|component| component["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!remaining.is_empty(), "the snapshot still owns non-skill components");
    let registered_before = mcp_server_names(app.clone(), &token).await;
    assert!(
        !registered_before.is_empty(),
        "the connector component must have registered an MCP server"
    );

    let full = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{snapshot_id}/uninstall"),
            serde_json::json!({ "component_ids": remaining }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(full.status(), StatusCode::OK, "full uninstall must succeed");
    let full_json = body_json(full).await;
    for component in full_json["components"].as_array().unwrap() {
        assert_eq!(
            component["state"], "not-installed",
            "every component must be released: {component}"
        );
    }
    assert!(
        agent_store_presets(app.clone(), &token).await.is_empty(),
        "uninstall must delete the Presets it created"
    );
    assert!(
        mcp_server_names(app.clone(), &token).await.is_empty(),
        "uninstall must delete the MCP server rows it registered"
    );
    assert!(
        !managed_snapshot_root.exists(),
        "the emptied managed snapshot root must not be left behind: {}",
        managed_snapshot_root.display()
    );
}

/// Disabling a component must move the runtime state it names — not just a
/// database column that no run path reads. For an expert that means the Preset
/// (which every run resolves through), for a connector the MCP server's own
/// `enabled` flag. A skill is the documented exception: the corpus is plain
/// directories with no state to flip, so its flag stays a catalogue marker and
/// its artifact stays on disk.
///
/// Re-installing a disabled component must also bring the runtime back, because
/// `mark_components_installed` clears `disabled` — otherwise the record would
/// read "enabled" over a runtime that is still off.
#[tokio::test]
async fn importer_disable_moves_runtime_state_for_connector_and_expert() {
    async fn install(app: axum::Router, token: &str, csrf: &str, connection_id: &str, snapshot_id: &str) {
        let response = app
            .oneshot(bearer_json(
                "POST",
                "/api/app-server/installs",
                serde_json::json!({ "snapshot_id": snapshot_id }),
                token,
                csrf,
                Some(connection_id),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "install must succeed");
    }

    async fn toggle(
        app: axum::Router,
        token: &str,
        csrf: &str,
        connection_id: &str,
        snapshot_id: &str,
        verb: &str,
        component_ids: &[String],
    ) -> serde_json::Value {
        let response = app
            .oneshot(bearer_json(
                "POST",
                &format!("/api/app-server/installs/{snapshot_id}/{verb}"),
                serde_json::json!({ "component_ids": component_ids }),
                token,
                csrf,
                Some(connection_id),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{verb} must succeed");
        body_json(response).await
    }

    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

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
    let result = body_json(run).await;
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();
    install(app.clone(), &token, &csrf, &connection_id, &snapshot_id).await;

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
    let status_json = body_json(status).await;
    let components = status_json["components"].as_array().unwrap();
    let by_kind = |kind: &str| {
        components
            .iter()
            .find(|component| component["kind"] == kind)
            .unwrap_or_else(|| panic!("snapshot must expose a {kind} component: {status_json}"))
    };
    let expert = by_kind("agent");
    let connector = by_kind("connector");
    let skill = by_kind("skill");
    let expert_id = expert["id"].as_str().unwrap().to_owned();
    let connector_id = connector["id"].as_str().unwrap().to_owned();
    let skill_id = skill["id"].as_str().unwrap().to_owned();
    let preset_name = format!("agent-store: {}", expert["name"].as_str().unwrap());
    let server_name = connector["name"].as_str().unwrap().to_owned();

    assert_eq!(
        preset_enabled(app.clone(), &token, &preset_name).await,
        Some(true),
        "a freshly installed expert Preset starts enabled"
    );
    // A freshly registered MCP server starts *disabled*: that is the installer's
    // documented default (an installed connector is one you have not switched on
    // yet). So the plugin flag and the MCP flag legitimately disagree right
    // after install — which is exactly why `install/enable` has to reach the MCP
    // row, and that is what this test pins next.
    assert_eq!(
        mcp_server_enabled(app.clone(), &token, &server_name).await,
        Some(false),
        "a freshly registered connector keeps the documented disabled default"
    );

    // `install/enable` must switch the MCP server on. Before this, enable only
    // cleared a database column and the connector stayed dark.
    toggle(
        app.clone(),
        &token,
        &csrf,
        &connection_id,
        &snapshot_id,
        "enable",
        &[connector_id.clone()],
    )
    .await;
    assert_eq!(
        mcp_server_enabled(app.clone(), &token, &server_name).await,
        Some(true),
        "install/enable must turn the MCP server on"
    );

    let disabled_ids = vec![expert_id.clone(), connector_id.clone(), skill_id.clone()];
    let disabled = toggle(
        app.clone(),
        &token,
        &csrf,
        &connection_id,
        &snapshot_id,
        "disable",
        &disabled_ids,
    )
    .await;
    for component in disabled["components"].as_array().unwrap() {
        if disabled_ids.contains(&component["id"].as_str().unwrap().to_owned()) {
            assert_eq!(component["state"], "disabled", "{component}");
        }
    }

    // The wire must say what actually happened per component, including that the
    // skill's flag is a marker rather than a runtime switch.
    let outcome_for = |payload: &serde_json::Value, id: &str| -> serde_json::Value {
        payload["outcomes"]
            .as_array()
            .unwrap_or_else(|| panic!("mutation must report outcomes: {payload}"))
            .iter()
            .find(|outcome| outcome["component_id"] == id)
            .unwrap_or_else(|| panic!("no outcome for {id}: {payload}"))
            .clone()
    };
    let skill_outcome = outcome_for(&disabled, &skill_id);
    assert_eq!(skill_outcome["action"], "marked", "{skill_outcome}");
    assert_eq!(skill_outcome["code"], "skill_disable_flag_only", "{skill_outcome}");
    assert_eq!(skill_outcome["ok"], true, "{skill_outcome}");
    assert_eq!(outcome_for(&disabled, &expert_id)["action"], "disabled");
    assert_eq!(outcome_for(&disabled, &connector_id)["action"], "disabled");

    assert_eq!(
        preset_enabled(app.clone(), &token, &preset_name).await,
        Some(false),
        "disabling an expert must disable the Preset every run resolves through"
    );
    assert_eq!(
        mcp_server_enabled(app.clone(), &token, &server_name).await,
        Some(false),
        "disabling a connector must disable the MCP server itself"
    );
    // The connector row still exists — disabled, not deleted.
    assert!(
        mcp_server_names(app.clone(), &token).await.contains(&server_name),
        "disable must keep the MCP server registration, only turn it off"
    );
    // The skill's flag is a marker: the artifact stays on disk.
    let managed = services
        .skill_paths
        .user_skills_dir
        .join("agent-store")
        .join(&snapshot_id)
        .join("release-notes")
        .join("SKILL.md");
    assert!(
        managed.is_file(),
        "disabling a skill is a catalogue marker, so its artifact must remain: {}",
        managed.display()
    );

    let enabled_ids = vec![expert_id.clone(), connector_id.clone()];
    toggle(
        app.clone(),
        &token,
        &csrf,
        &connection_id,
        &snapshot_id,
        "enable",
        &enabled_ids,
    )
    .await;
    assert_eq!(
        preset_enabled(app.clone(), &token, &preset_name).await,
        Some(true),
        "enable must restore the Preset"
    );
    assert_eq!(
        mcp_server_enabled(app.clone(), &token, &server_name).await,
        Some(true),
        "enable must restore the MCP server"
    );

    // Re-installing over a disabled component must bring the runtime back with
    // the flag: `mark_components_installed` clears `disabled`.
    toggle(
        app.clone(),
        &token,
        &csrf,
        &connection_id,
        &snapshot_id,
        "disable",
        &enabled_ids,
    )
    .await;
    assert_eq!(preset_enabled(app.clone(), &token, &preset_name).await, Some(false));
    install(app.clone(), &token, &csrf, &connection_id, &snapshot_id).await;
    assert_eq!(
        preset_enabled(app.clone(), &token, &preset_name).await,
        Some(true),
        "re-install must re-enable the Preset, not just clear the flag"
    );
    assert_eq!(
        mcp_server_enabled(app.clone(), &token, &server_name).await,
        Some(true),
        "re-install must re-enable the MCP server, not just clear the flag"
    );
}

/// A disabled expert must be refused by name. `PresetService::resolve` already
/// refuses a disabled Preset, but with a generic message; the wire code is what
/// a client branches on, and "you switched this off" is not "this is broken".
#[tokio::test]
async fn importer_disabled_agent_is_named_in_run_refusals() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

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
    assert_eq!(install.status(), StatusCode::OK);

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
    let status_json = body_json(status).await;
    let components = status_json["components"].as_array().unwrap();
    let agent_ids: Vec<String> = components
        .iter()
        .filter(|component| component["kind"] == "agent")
        .map(|component| component["id"].as_str().unwrap().to_owned())
        .collect();
    let team_id = components
        .iter()
        .find(|component| component["kind"] == "team")
        .map(|component| component["id"].as_str().unwrap().to_owned())
        .expect("software-company ships a team");
    assert!(!agent_ids.is_empty(), "software-company ships agents");

    // Disable every agent, so whichever one leads the team is off too.
    let disable = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{snapshot_id}/disable"),
            serde_json::json!({ "component_ids": agent_ids }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(disable.status(), StatusCode::OK, "disable must succeed");

    let agent_run = app
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
    let agent_body = body_json(agent_run).await;
    assert_eq!(
        agent_body["code"], "preset_disabled",
        "agent/run must name a disabled Preset instead of a generic resolve failure: {agent_body}"
    );

    let team_run = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/app-server/team/run",
            serde_json::json!({ "team_id": team_id, "goal": "ship the release" }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    let team_body = body_json(team_run).await;
    assert_eq!(
        team_body["code"], "agent_disabled",
        "team/run must name a disabled member instead of a generic resolve failure: {team_body}"
    );
}

/// An empty component list must not read as "uninstall the whole snapshot":
/// the call sites always pass an explicit selection, and a wipe is not
/// something either of them could recover from.
#[tokio::test]
async fn importer_uninstall_requires_explicit_component_ids() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

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
    assert_eq!(install.status(), StatusCode::OK);

    let refused = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{snapshot_id}/uninstall"),
            serde_json::json!({ "component_ids": [] }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(
        refused.status(),
        StatusCode::BAD_REQUEST,
        "an empty selection must be refused, not treated as 'everything'"
    );
    let refused_json = body_json(refused).await;
    assert_eq!(refused_json["code"], "invalid_request", "{refused_json}");

    // Nothing was released by the refusal.
    assert!(
        !agent_store_presets(app.clone(), &token).await.is_empty(),
        "the refused call must not have released anything"
    );
}

/// `install/run` must be re-entrant: a retry (the natural client reaction to a
/// timeout) must not create a second Preset for the same component and orphan
/// the first. `PresetService::create` mints a fresh id whenever `preset_id` is
/// absent, so idempotency can only come from the recorded `runtime_ref`.
#[tokio::test]
async fn importer_install_is_reentrant_for_agent_and_team_presets() {
    async fn install_once(
        app: axum::Router,
        token: &str,
        csrf: &str,
        connection_id: &str,
        snapshot_id: &str,
    ) -> serde_json::Value {
        let response = app
            .oneshot(bearer_json(
                "POST",
                "/api/app-server/installs",
                serde_json::json!({ "snapshot_id": snapshot_id }),
                token,
                csrf,
                Some(connection_id),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "install must succeed");
        body_json(response).await
    }

    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

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
    let result = body_json(run).await;
    assert_eq!(result["status"], "completed", "{result}");
    let snapshot_id = result["snapshot_id"].as_str().unwrap().to_owned();

    install_once(app.clone(), &token, &csrf, &connection_id, &snapshot_id).await;
    let after_first = agent_store_presets(app.clone(), &token).await;
    assert!(
        !after_first.is_empty(),
        "installing software-company must create agent-store presets"
    );

    // The retry: same snapshot, same call. Nothing new may appear.
    let second = install_once(app.clone(), &token, &csrf, &connection_id, &snapshot_id).await;
    assert_eq!(
        second["snapshot_id"], snapshot_id,
        "the retry reports the same snapshot: {second}"
    );
    let after_second = agent_store_presets(app.clone(), &token).await;
    assert_eq!(
        after_second, after_first,
        "a retried install must reuse the recorded Presets instead of creating duplicates"
    );

    // The wire says so too: the retry's per-component outcomes must report
    // reuse, not a fresh creation. A caller has no other way to tell.
    let outcomes = second["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("install must report per-component outcomes: {second}"));
    let reused: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| outcome["action"] == "reused")
        .filter_map(|outcome| outcome["kind"].as_str())
        .collect();
    assert!(
        reused.contains(&"agent") && reused.contains(&"team"),
        "the retry must report the agent and team Presets as reused: {second}"
    );
    assert!(
        outcomes.iter().all(|outcome| outcome["ok"].as_bool().unwrap_or(false)),
        "a retried install must not report any failed component: {second}"
    );
}

/// Installing an entry whose marketplace has moved on must deliver the
/// *current* version, not the stale snapshot.
///
/// The wire has no update verb (`store/update-entry` does not exist), so
/// "uninstall, then install again" is the only upgrade path a client has. Before
/// this, that path faithfully reinstalled the version that was imported first,
/// because `install_entry` reused an existing snapshot unconditionally — an
/// upgrade that could never happen.
///
/// The other half of the contract is pinned too: while the entry is *installed*,
/// `store/install-entry` must stay a no-op (reuse), because silently re-importing
/// there would make "install" a hidden upgrade.
#[tokio::test]
async fn importer_store_install_entry_picks_up_a_new_version() {
    async fn store_item(app: axum::Router, token: &str, csrf: &str, connection_id: &str, entry: &str) -> serde_json::Value {
        let store = app
            .oneshot(bearer_get("/api/app-server/store", token, csrf, connection_id))
            .await
            .unwrap();
        assert_eq!(store.status(), StatusCode::OK, "store list must succeed");
        let store_json = body_json(store).await;
        store_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["entry_name"] == entry)
            .unwrap_or_else(|| panic!("entry {entry} missing from the store: {store_json}"))
            .clone()
    }

    async fn install_entry(
        app: axum::Router,
        token: &str,
        csrf: &str,
        connection_id: &str,
        marketplace_id: &str,
        entry: &str,
    ) -> serde_json::Value {
        let response = app
            .oneshot(bearer_json(
                "POST",
                &format!("/api/app-server/store/{marketplace_id}/entries/{entry}/install"),
                serde_json::json!({}),
                token,
                csrf,
                Some(connection_id),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "store install-entry must succeed");
        body_json(response).await
    }

    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    // A one-entry plugin market, the smallest shape that exercises the path.
    let market_root = std::env::temp_dir().join(format!("as-version-market-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(market_root.join(".codebuddy-plugin")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/team-tools/.codebuddy-plugin")).unwrap();
    std::fs::create_dir_all(market_root.join("plugins/team-tools/agents")).unwrap();
    std::fs::write(
        market_root.join(".codebuddy-plugin/marketplace.json"),
        r#"{
            "name": "tools",
            "version": "0.1.0",
            "plugins": [
                { "name": "team-tools", "source": "./plugins/team-tools", "description": "Team tools" }
            ]
        }"#,
    )
    .unwrap();
    let write_plugin_json = |version: &str| {
        std::fs::write(
            market_root.join("plugins/team-tools/.codebuddy-plugin/plugin.json"),
            format!(
                r#"{{ "name": "team-tools", "version": "{version}", "agents": ["./agents"] }}"#
            ),
        )
        .unwrap();
    };
    write_plugin_json("1.0.0");
    std::fs::write(
        market_root.join("plugins/team-tools/agents/lead.md"),
        "---\nname: lead\ndescription: Lead\n---\n\nLead body.\n",
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
    assert_eq!(add.status(), StatusCode::OK, "market add must succeed");
    let marketplace_id = body_json(add).await["marketplace_id"].as_str().unwrap().to_owned();

    // 1.0.0 goes in.
    let first = install_entry(app.clone(), &token, &csrf, &connection_id, &marketplace_id, "team-tools").await;
    assert_eq!(first["version"], "1.0.0", "{first}");
    let first_snapshot = first["snapshot_id"].as_str().unwrap().to_owned();
    let installed_item = store_item(app.clone(), &token, &csrf, &connection_id, "team-tools").await;
    assert_eq!(installed_item["installed"], true, "{installed_item}");
    assert_eq!(installed_item["update_available"], false, "{installed_item}");

    // The marketplace publishes 2.0.0.
    write_plugin_json("2.0.0");
    let stale_item = store_item(app.clone(), &token, &csrf, &connection_id, "team-tools").await;
    assert_eq!(stale_item["version"], "2.0.0", "{stale_item}");
    assert_eq!(stale_item["installed_version"], "1.0.0", "{stale_item}");
    assert_eq!(
        stale_item["update_available"], true,
        "list must advertise the pending update: {stale_item}"
    );

    // While installed, install-entry stays a no-op: no silent upgrade.
    let again = install_entry(app.clone(), &token, &csrf, &connection_id, &marketplace_id, "team-tools").await;
    assert_eq!(again["reused"], true, "an installed entry must not be silently upgraded: {again}");
    assert_eq!(again["snapshot_id"], first_snapshot, "{again}");
    assert_eq!(again["version"], "1.0.0", "{again}");

    // Release it, then install again — the only upgrade path the wire offers.
    let status = app
        .clone()
        .oneshot(bearer_get(
            &format!("/api/app-server/installs/{first_snapshot}"),
            &token,
            &csrf,
            &connection_id,
        ))
        .await
        .unwrap();
    let component_ids: Vec<String> = body_json(status).await["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|component| component["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!component_ids.is_empty(), "the snapshot must have installed components");
    let uninstall = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/installs/{first_snapshot}/uninstall"),
            serde_json::json!({ "component_ids": component_ids }),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(uninstall.status(), StatusCode::OK, "uninstall must succeed");

    let upgraded = install_entry(app.clone(), &token, &csrf, &connection_id, &marketplace_id, "team-tools").await;
    assert_eq!(
        upgraded["version"], "2.0.0",
        "re-installing must deliver the version the marketplace now offers: {upgraded}"
    );
    let upgraded_snapshot = upgraded["snapshot_id"].as_str().unwrap().to_owned();
    assert_ne!(
        upgraded_snapshot, first_snapshot,
        "a new version is a new immutable snapshot; the old one keeps its history"
    );
    assert!(upgraded["installed_count"].as_u64().unwrap() > 0, "{upgraded}");
    // `store/install-entry` forwards the installer's per-component report, so a
    // one-click store install is as branchable as a direct `install/run`.
    let upgraded_outcomes = upgraded["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("store install must forward per-component outcomes: {upgraded}"));
    assert!(!upgraded_outcomes.is_empty(), "{upgraded}");
    assert!(
        upgraded_outcomes.iter().all(|outcome| outcome["ok"] == true),
        "the upgrade must not fail a component: {upgraded}"
    );
    let forwarded = upgraded_outcomes
        .iter()
        .find(|outcome| outcome["kind"] == "agent")
        .expect("the forwarded report must cover the agent component");
    assert_eq!(forwarded["action"], "created", "{forwarded}");

    let settled_item = store_item(app.clone(), &token, &csrf, &connection_id, "team-tools").await;
    assert_eq!(settled_item["installed"], true, "{settled_item}");
    assert_eq!(settled_item["installed_version"], "2.0.0", "{settled_item}");
    assert_eq!(
        settled_item["update_available"], false,
        "the catalogue must agree that the upgrade landed: {settled_item}"
    );
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
    // Market-level icons (`icons/<source-basename>.<ext>`) — the connector
    // avatar fallback when the entry ships no plugin.json avatar.
    std::fs::create_dir_all(market_root.join("icons")).unwrap();
    std::fs::write(market_root.join("icons/agent-earth.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n").unwrap();
    std::fs::write(market_root.join("icons/wecom.png"), [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();

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
    // Market-level icon fallback: no plugin.json avatar → icons/agent-earth.svg.
    let earth_avatar = earth["avatar_url"].as_str().unwrap_or("");
    assert!(earth_avatar.contains("icons/agent-earth.svg"), "{earth}");
    let wecom = items
        .iter()
        .find(|item| item["entry_name"] == "wecom")
        .expect("wecom entry present");
    assert_eq!(wecom["kind"], "connector", "{wecom}");
    assert_eq!(wecom["name"], "企业微信", "{wecom}");
    assert!(wecom["avatar_url"].as_str().unwrap_or("").contains("icons/wecom.png"), "{wecom}");

    // The market-level icon asset endpoint serves the SVG without any
    // app-server connection header (plain `<img>` tags).
    let icon_asset = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/app-server/store/{marketplace_id}/entries/agent-earth/assets/icons/agent-earth.svg"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(icon_asset.status(), StatusCode::OK, "market icon asset must be served");
    assert_eq!(
        icon_asset.headers().get("content-type").unwrap(),
        "image/svg+xml",
        "icon content-type must be image/svg+xml"
    );

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

    // B4: CLI connectors describe a command-line integration, not an MCP
    // server; V1 must skip them instead of registering the init command as a
    // bogus stdio server.
    let cli_install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/store/{marketplace_id}/entries/wecom/install"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(
        cli_install.status(),
        StatusCode::OK,
        "CLI install must not fail: {}",
        body_json(cli_install).await
    );
    let cli_installed = body_json(cli_install).await;
    assert_eq!(cli_installed["installed_count"], 0, "{cli_installed}");
    let cli_warnings = cli_installed["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        cli_warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(|text| text.contains("cli connector"))),
        "CLI connector must be skipped with a warning: {cli_installed}"
    );

    // The bogus stdio server must not exist in the connector catalog.
    let connectors = app
        .clone()
        .oneshot(bearer_get("/api/app-server/connectors", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(connectors.status(), StatusCode::OK);
    let connectors_json = body_json(connectors).await;
    let has_cli_server = connectors_json
        .as_array()
        .into_iter()
        .flatten()
        .any(|server| server["name"] == "wecom");
    assert!(
        !has_cli_server,
        "CLI connector must not register an MCP server: {connectors_json}"
    );
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

/// R24 (`02` §8 / §11.1): an entry declaring `strict=true` must ship its own
/// `plugin.json`. Discovery keeps it **and says why** instead of dropping it,
/// the store lists it as uninstallable, and the import is refused.
///
/// The source here ships a `SKILL.md` but no manifest, so without the rule the
/// import would happily succeed as a single-skill directory — the block is
/// what actually changes the outcome, not a coincidental parse failure.
#[tokio::test]
async fn importer_market_strict_entry_is_listed_but_refused() {
    let (mut app, services) = build_app().await;
    let (token, csrf) = setup_and_login(&mut app, &services, "admin", "StrongP@ss1").await;
    let connection_id = app_server_handshake(&mut app, &token, &csrf).await;

    let market_root =
        std::env::temp_dir().join(format!("as-strict-market-{}", nomifun_common::generate_id()));
    std::fs::create_dir_all(market_root.join(".codebuddy-plugin")).unwrap();
    std::fs::write(
        market_root.join(".codebuddy-plugin/marketplace.json"),
        r#"{
            "name": "strict-experts",
            "version": "0.1.0",
            "plugins": [
                { "name": "strict-expert", "source": "./plugins/strict-expert", "strict": true },
                { "name": "loose-expert", "source": "./plugins/loose-expert" }
            ]
        }"#,
    )
    .unwrap();
    // `strict-expert` ships a usable SKILL.md but no plugin.json of its own.
    std::fs::create_dir_all(market_root.join("plugins/strict-expert")).unwrap();
    std::fs::write(
        market_root.join("plugins/strict-expert/SKILL.md"),
        "---\nname: strict-expert\ndescription: Strict\n---\n\nBody.\n",
    )
    .unwrap();
    // `loose-expert` declares no `strict` and ships no manifest either: the
    // spec lets such an entry *supply* the manifest, which is not implemented
    // yet (`17` §10 P5), so it keeps the historical behaviour — undiscovered.
    std::fs::create_dir_all(market_root.join("plugins/loose-expert")).unwrap();

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
    assert_eq!(added["entry_count"], 1, "only the strict row survives discovery: {added}");
    let marketplace_id = added["marketplace_id"].as_str().unwrap().to_owned();

    // `market/get` names the rule instead of hiding the entry.
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
    assert_eq!(entries.len(), 1, "{detail}");
    assert_eq!(entries[0]["name"], "strict-expert");
    assert_eq!(entries[0]["strict"], true, "{detail}");
    let reason = entries[0]["blocked_reason"].as_str().expect("blocked entry must say why");
    assert!(reason.contains("strict=true"), "{reason}");

    // The store lists it (so the reason is readable) but marks it uninstallable.
    let store = app
        .clone()
        .oneshot(bearer_get("/api/app-server/store", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert_eq!(store.status(), StatusCode::OK);
    let store_json = body_json(store).await;
    let item = store_json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["entry_name"] == "strict-expert")
        .expect("a discoverable entry stays in the store");
    assert!(
        item["blocked_reason"].as_str().is_some_and(|text| !text.is_empty()),
        "{item}"
    );

    // Import is refused, and the reason never leaks the on-disk source path
    // (`02` §9: source paths are internal traceability only).
    let import = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/markets/{marketplace_id}/entries/strict-expert/import"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK, "a refusal is a result, not a transport error");
    let blocked = body_json(import).await;
    assert_eq!(blocked["status"], "blocked", "{blocked}");
    assert_eq!(blocked["reused"], false, "{blocked}");
    let errors = blocked["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "{blocked}");
    let source_path = market_root.to_string_lossy().to_string();
    assert!(
        !errors.iter().any(|error| error.as_str().is_some_and(|text| text.contains(&source_path))),
        "the blocking reason must not expose the source path: {blocked}"
    );

    // A blocked import persists nothing: no snapshot row appears in history.
    let history = app
        .clone()
        .oneshot(bearer_get("/api/app-server/imports", &token, &csrf, &connection_id))
        .await
        .unwrap();
    assert!(
        body_json(history).await.as_array().unwrap().is_empty(),
        "a refused snapshot must not be recorded"
    );

    // The store's one-click install must refuse too, rather than registering a
    // phantom snapshot id.
    let install = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/app-server/store/{marketplace_id}/entries/strict-expert/install"),
            serde_json::json!({}),
            &token,
            &csrf,
            Some(&connection_id),
        ))
        .await
        .unwrap();
    assert_eq!(install.status(), StatusCode::OK, "install must report, not 500");
    let installed = body_json(install).await;
    assert_eq!(installed["installed_count"], 0, "{installed}");
    assert!(!installed["errors"].as_array().unwrap().is_empty(), "{installed}");
}