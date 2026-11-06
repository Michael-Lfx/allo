//! HTTP contract coverage for the user-facing Skill catalog.
//!
//! The catalog intentionally differs from the legacy `/api/skills` list:
//! source-qualified IDs preserve same-name skills, while system-owned
//! auto-injected skills are not user-selectable catalog entries.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nomifun_api_types::SkillCatalogSource;
use nomifun_extension::external_paths::ExternalPathsManager;
use nomifun_extension::skill_routes::{SkillRouterState, skill_routes};
use nomifun_extension::skill_service::{
    CatalogSkillRoots, SkillPaths,
};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    router: axum::Router,
    paths: SkillPaths,
    _temp: TempDir,
}

fn write_skill(root: &std::path::Path, directory_name: &str, name: &str, description: &str) {
    std::fs::create_dir_all(root.join(directory_name)).unwrap();
    std::fs::write(
        root.join(directory_name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nBody."),
    )
    .unwrap();
}

async fn fixture() -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let builtin_skills_dir = root.join("builtin-skills");
    let user_skills_dir = root.join("skills");

    write_skill(&builtin_skills_dir, "core-pdf", "pdf", "Built-in PDF workflow");
    std::fs::create_dir_all(builtin_skills_dir.join("core-pdf").join("assets")).unwrap();
    std::fs::write(
        builtin_skills_dir.join("core-pdf").join("assets").join("icon.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .unwrap();
    write_skill(
        &builtin_skills_dir.join("auto-inject"),
        "cron",
        "cron",
        "System scheduler",
    );
    write_skill(&user_skills_dir, "local-pdf", "pdf", "User PDF workflow");
    write_skill(&user_skills_dir, "team-pdf", "pdf", "Team PDF workflow");

    let paths = SkillPaths {
        data_dir: root.to_path_buf(),
        user_skills_dir,
        cron_skills_dir: root.join("cron").join("skills"),
        builtin_skills_dir,
        builtin_rules_dir: root.join("builtin-rules"),
        preset_rules_dir: root.join("preset-rules"),
        preset_skills_dir: root.join("preset-skills"),
        catalog_roots: Default::default(),
    };
    let db = nomifun_db::init_database_memory().await.unwrap();
    let state = SkillRouterState {
        skill_paths: paths.clone(),
        external_paths_manager: Arc::new(ExternalPathsManager::with_file(root.join("paths.json")).await),
        preset_dispatcher: None,
        market_package_preset_installer: None,
        skill_tag_repo: Arc::new(nomifun_db::SqliteSkillTagRepository::new(db.pool().clone())),
        builtin_skill_tags: Arc::new(std::collections::HashMap::new()),
        skill_discovery: None,
    };

    Fixture {
        router: skill_routes(state),
        paths,
        _temp: temp,
    }
}

async fn body_json(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn catalog_keeps_same_named_skills_from_distinct_sources_and_hides_system_skills() {
    let fixture = fixture().await;
    let response = fixture
        .router
        .oneshot(
            Request::builder()
                .uri("/api/skills/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["success"], true);

    let skills = body["data"]["skills"].as_array().expect("catalog skills array");
    assert!(skills.iter().any(|skill| {
        skill["skill_id"] == "builtin:core-pdf"
            && skill["name"] == "pdf"
            && skill["description"] == "Built-in PDF workflow"
            && skill["source"] == "builtin"
            && skill["avatar"] == "/api/skills/pdf/icon"
    }));
    assert!(skills.iter().any(|skill| {
        skill["skill_id"] == "user:local-pdf"
            && skill["name"] == "pdf"
            && skill["description"] == "User PDF workflow"
            && skill["source"] == "user"
    }));
    assert!(skills.iter().any(|skill| {
        skill["skill_id"] == "user:team-pdf"
            && skill["name"] == "pdf"
            && skill["description"] == "Team PDF workflow"
            && skill["source"] == "user"
    }));
    assert!(skills.iter().all(|skill| skill["name"] != "cron"));
}

#[tokio::test]
async fn catalog_loader_returns_the_selected_source_qualified_markdown_snapshot() {
    let fixture = fixture().await;
    let catalog = nomifun_extension::skill_service::list_catalog_skills(&fixture.paths)
        .await
        .expect("catalog lists skills");
    assert!(catalog.iter().any(|skill| {
        skill.source == SkillCatalogSource::User && skill.local_key == "local-pdf"
    }));

    let loaded = nomifun_extension::skill_service::load_catalog_skills(
        &fixture.paths,
        &["user:local-pdf".to_owned()],
    )
    .await
    .expect("user catalog skill loads");

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].skill_id, "user:local-pdf");
    assert!(loaded[0].content.contains("User PDF workflow"));
    assert_eq!(loaded[0].source, "user");
}

#[tokio::test]
async fn catalog_lists_and_loads_global_agent_and_project_skills() {
    let fixture = fixture().await;
    let global_agent_skills_dir = fixture._temp.path().join("global").join(".agents").join("skills");
    let project_agent_skills_dir = fixture._temp.path().join("project").join(".agents").join("skills");
    write_skill(
        &global_agent_skills_dir,
        "global-review",
        "review",
        "Global agent review workflow",
    );
    write_skill(
        &project_agent_skills_dir,
        "project-review",
        "review",
        "Project review workflow",
    );
    let roots = CatalogSkillRoots {
        global_agent_skills_dir: Some(global_agent_skills_dir),
        project_agent_skills_dir: Some(project_agent_skills_dir),
    };
    let mut paths = fixture.paths.clone();
    paths.catalog_roots = roots;

    let catalog = nomifun_extension::skill_service::list_catalog_skills(&paths)
        .await
        .expect("catalog lists global and project Skills");
    assert!(catalog.iter().any(|skill| {
        skill.name == "review"
            && skill.source == SkillCatalogSource::User
            && skill.source_key.as_deref() == Some("agents")
            && skill.local_key == "global-review"
    }));
    assert!(catalog.iter().any(|skill| {
        skill.name == "review"
            && skill.source == SkillCatalogSource::Project
            && skill.source_key.as_deref() == Some("workspace")
            && skill.local_key == "project-review"
    }));

    let loaded = nomifun_extension::skill_service::load_catalog_skills(
        &paths,
        &[
            "user:agents:global-review".to_owned(),
            "project:workspace:project-review".to_owned(),
        ],
    )
    .await
    .expect("selected global and project Skills load");

    assert_eq!(loaded.len(), 2);
    assert!(loaded[0].content.contains("Global agent review workflow"));
    assert!(loaded[1].content.contains("Project review workflow"));
}

/// Lay out a `Ready` managed ppt-master snapshot the way the presentation
/// installer leaves it (current.json + snapshot with SKILL.md, venv python,
/// self-test marker).
fn install_managed_ppt_master(data_dir: &std::path::Path) -> std::path::PathBuf {
    // Joined per component, like the installer: the host note embeds this path.
    let root = data_dir.join("capabilities").join("presentation").join("ppt-master");
    let snapshot = "6.8.0-4a68cddaf40d-abc123";
    let snapshot_dir = root.join("snapshots").join(snapshot);
    let skill_dir = snapshot_dir.join("skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: ppt-master\ndescription: Managed deck workflow\nmetadata:\n  version: \"6.8.0\"\n---\n\n# PPT Master Skill\nRun `python3 \"${SKILL_DIR}/scripts/attribution_guard.py\"`.\n",
    )
    .unwrap();
    let python = if cfg!(windows) {
        snapshot_dir.join("venv").join("Scripts").join("python.exe")
    } else {
        snapshot_dir.join("venv").join("bin").join("python")
    };
    std::fs::create_dir_all(python.parent().unwrap()).unwrap();
    std::fs::write(&python, b"").unwrap();
    std::fs::write(snapshot_dir.join("runtime-ready"), b"ready").unwrap();
    std::fs::write(
        root.join("current.json"),
        serde_json::json!({
            "capability_id": "presentation.ppt-master",
            "skill_version": "6.8.0",
            "commit": nomifun_presentation::manifest::SOURCE_COMMIT,
            "tree": "t",
            "source_url": "https://atomgit.com/hugohe3/ppt-master.git",
            "snapshot_id": snapshot,
            "installed_at_ms": 1
        })
        .to_string(),
    )
    .unwrap();
    skill_dir
}

#[tokio::test]
async fn managed_ppt_master_is_listed_only_when_ready_and_shadows_same_named_copies() {
    let fixture = fixture().await;
    let user_skills_dir = fixture.paths.user_skills_dir.clone();
    write_skill(&user_skills_dir, "ppt-master", "ppt-master", "Hand-installed copy");

    let before = nomifun_extension::skill_service::list_catalog_skills(&fixture.paths).await.unwrap();
    assert!(before.iter().all(|skill| skill.source != SkillCatalogSource::Capability), "not installed yet");
    assert!(before.iter().any(|skill| skill.name == "ppt-master" && skill.source == SkillCatalogSource::User));

    let skill_dir = install_managed_ppt_master(&fixture.paths.data_dir);
    let after = nomifun_extension::skill_service::list_catalog_skills(&fixture.paths).await.unwrap();
    let managed: Vec<_> = after.iter().filter(|skill| skill.name == "ppt-master").collect();
    assert_eq!(managed.len(), 1, "only the managed copy is discoverable: {managed:?}");
    assert_eq!(managed[0].source, SkillCatalogSource::Capability);
    assert_eq!(managed[0].source_key.as_deref(), Some("presentation"));
    assert_eq!(managed[0].description, "Managed deck workflow");
    assert!(after.iter().any(|skill| skill.name == "pdf"), "unrelated skills are untouched");

    let loaded = nomifun_extension::skill_service::load_catalog_skills(
        &fixture.paths,
        &["capability:presentation:ppt-master".to_owned(), "user:ppt-master".to_owned()],
    )
    .await
    .expect("managed and previously bound user copies both load");
    let content = &loaded[0].content;
    assert!(content.starts_with("---\nname: ppt-master\n"), "frontmatter stays first: {content}");
    let note = content.find("Flowy host note").expect("host note injected");
    assert!(note < content.find("# PPT Master Skill").unwrap());
    assert!(content.contains("nomi_presentation_run_script"));
    assert!(content.contains(&skill_dir.to_string_lossy().into_owned()));
    assert_eq!(loaded[0].source, "capability");
    assert!(loaded[1].content.contains("Hand-installed copy"));

    let response = fixture
        .router
        .oneshot(Request::builder().uri("/api/skills/catalog").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_json(response).await;
    let skills = body["data"]["skills"].as_array().unwrap();
    assert!(skills.iter().any(|skill| {
        skill["skill_id"] == "capability:presentation:ppt-master" && skill["source"] == "capability"
    }));
    assert!(skills.iter().all(|skill| skill["skill_id"] != "user:ppt-master"));

    // A broken snapshot drops the managed entry and the user copy reappears.
    std::fs::remove_file(skill_dir.parent().unwrap().join("runtime-ready")).unwrap();
    let broken = nomifun_extension::skill_service::list_catalog_skills(&fixture.paths).await.unwrap();
    assert!(broken.iter().all(|skill| skill.source != SkillCatalogSource::Capability));
    assert!(broken.iter().any(|skill| skill.name == "ppt-master" && skill.source == SkillCatalogSource::User));
}
