//! HTTP contract coverage for the user-facing Skill catalog.
//!
//! The catalog intentionally differs from the legacy `/api/skills` list:
//! source-qualified IDs preserve same-name skills, while system-owned
//! auto-injected skills are not user-selectable catalog entries.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nomifun_extension::external_paths::ExternalPathsManager;
use nomifun_extension::skill_routes::{SkillRouterState, skill_routes};
use nomifun_extension::skill_service::SkillPaths;
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
    };
    let db = nomifun_db::init_database_memory().await.unwrap();
    let state = SkillRouterState {
        skill_paths: paths.clone(),
        external_paths_manager: Arc::new(ExternalPathsManager::with_file(root.join("paths.json")).await),
        preset_dispatcher: None,
        skill_tag_repo: Arc::new(nomifun_db::SqliteSkillTagRepository::new(db.pool().clone())),
        builtin_skill_tags: Arc::new(std::collections::HashMap::new()),
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
        skill.source == nomifun_extension::skill_service::SkillSource::Custom
            && skill.local_key == "local-pdf"
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
