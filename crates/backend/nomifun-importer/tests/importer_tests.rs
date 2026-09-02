//! Importer acceptance tests mapped to `docs/agent-store/agent-store-v1-test-cases.md`
//! TC-IMP-001..009. Static fixtures live in `tests/fixtures/`; dynamic
//! malicious trees (symlink escape) are built at runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomifun_db::{IPluginSnapshotRepository, SqlitePluginSnapshotRepository};
use nomifun_importer::{ImporterService, ImportRequest, SourceKind};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

async fn service(repo: Arc<dyn IPluginSnapshotRepository>) -> (ImporterService, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let service = ImporterService::new(temp.path().join("agent-store-imports"), repo);
    (service, temp)
}

async fn setup() -> (ImporterService, tempfile::TempDir, Arc<SqlitePluginSnapshotRepository>) {
    let db = nomifun_db::init_database_memory().await.unwrap();
    let repo = Arc::new(SqlitePluginSnapshotRepository::new(db.pool().clone()));
    let (service, temp) = service(repo.clone()).await;
    (service, temp, repo)
}

fn count_kind(components: &[nomifun_db::models::PluginSnapshotComponentRow], kind: &str) -> usize {
    components.iter().filter(|component| component.kind == kind).count()
}

fn find_kind<'a>(
    components: &'a [nomifun_db::models::PluginSnapshotComponentRow],
    kind: &str,
) -> Vec<&'a nomifun_db::models::PluginSnapshotComponentRow> {
    components.iter().filter(|component| component.kind == kind).collect()
}

// ---------------------------------------------------------------------------
// TC-IMP-001 + TC-IMP-002: software-company → 5 agents + 1 team
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_001_002_software_company_imports_five_agents_and_one_team() {
    let (service, temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("software-company"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();

    assert_eq!(result.status, "completed", "errors: {:?}", result.errors);
    assert_eq!(result.name, "software-company");
    assert_eq!(result.version, "1.2.0");
    assert!(!result.content_digest.is_empty());
    assert!(result.component_count >= 11, "unexpected count {}", result.component_count);
    assert!(!result.reused);
    // materialized immutable cache exists
    assert!(temp
        .path()
        .join("agent-store-imports")
        .join(&result.snapshot_id)
        .join("agents/software-team-lead.md")
        .exists());

    // persisted rows
    let row = repo.get_by_snapshot_id(&result.snapshot_id).await.unwrap().unwrap();
    assert_eq!(row.status, "completed");
    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "agent"), 5, "5 AgentDefinitions");
    assert_eq!(count_kind(&components, "team"), 1, "1 AgentTeamDefinition");
    assert!(count_kind(&components, "skill") >= 2);
    assert!(count_kind(&components, "command") >= 1);
    assert!(count_kind(&components, "connector") >= 1);
    assert!(count_kind(&components, "hook") >= 1);
    assert!(count_kind(&components, "lsp") >= 1);
    assert!(count_kind(&components, "credential") >= 1);
    assert!(count_kind(&components, "dependency") >= 1);
    assert!(count_kind(&components, "script") >= 2, "bin/ + scripts/");

    // team linkage (02 §6): lead + members point at imported agent ids
    let team = find_kind(&components, "team").pop().unwrap();
    let payload: serde_json::Value = serde_json::from_str(&team.payload_json).unwrap();
    assert_eq!(
        payload["lead_agent_id"],
        "wb-software-company-software-team-lead"
    );
    let members = payload["member_agent_ids"].as_array().unwrap();
    assert_eq!(members.len(), 4);
    for member in members {
        assert!(
            components.iter().any(|component| component.component_id == member.as_str().unwrap()),
            "member {member} must resolve to an imported agent"
        );
    }
    assert_eq!(payload["planner_policy"], "planned");
    assert!(payload["team_runtime_capabilities"].as_array().unwrap().len() >= 8);

    // agent detail fields preserved (02 §5.1)
    let engineer = components
        .iter()
        .find(|component| component.component_id == "wb-software-company-software-engineer")
        .unwrap();
    let agent_payload: serde_json::Value = serde_json::from_str(&engineer.payload_json).unwrap();
    assert_eq!(agent_payload["model"], "gpt-5");
    assert_eq!(agent_payload["max_turns"], 50);
    assert_eq!(agent_payload["permission_mode_ignored"], true);
    assert_eq!(agent_payload["tools"].as_array().unwrap().len(), 3);
    assert_eq!(agent_payload["relative_path"], "agents/software-engineer.md");

    // credential schema: sensitive-only, no values anywhere (02 §10)
    let credential = find_kind(&components, "credential").pop().unwrap();
    let cred_json = serde_json::to_string(&credential.payload_json).unwrap();
    assert!(!cred_json.contains("sk-SECRET-VALUE"), "values must never be imported");
    let schema: serde_json::Value = serde_json::from_str(&credential.payload_json).unwrap();
    let fields = schema["fields"].as_array().unwrap();
    let api_key = fields.iter().find(|field| field["key"] == "apiKey").unwrap();
    assert_eq!(api_key["sensitive"], true);
    assert_eq!(api_key["type"], "apikey");
}

// ---------------------------------------------------------------------------
// TC-IMP-003: agents/ without teamInfo never becomes a Team
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_003_agents_only_never_creates_a_team() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("agents-only"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "agent"), 1);
    assert_eq!(count_kind(&components, "team"), 0, "agents/ 目录本身 ≠ Team（02 §6）");
}

// ---------------------------------------------------------------------------
// TC-IMP-004: path traversal / absolute paths block the import
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_004_path_traversal_and_absolute_paths_are_blocked() {
    for fixture in ["path-traversal", "absolute-path"] {
        let (service, temp, repo) = setup().await;
        let result = service
            .run_import(&ImportRequest {
                source_path: fixtures().join(fixture),
                source_kind: SourceKind::CodeBuddyPlugin,
            })
            .await
            .unwrap();
        assert_eq!(result.status, "blocked", "{fixture} must block");
        assert!(!result.errors.is_empty(), "{fixture} must report a safe error");
        // nothing persisted, nothing materialized
        assert!(repo.get_by_snapshot_id(&result.snapshot_id).await.unwrap().is_none());
        assert!(!temp
            .path()
            .join("agent-store-imports")
            .join(&result.snapshot_id)
            .exists());
        // safe error strings never leak the absolute source path
        let joined = result.errors.join(" | ");
        let source_abs = fixtures().join(fixture).canonicalize().unwrap();
        assert!(
            !joined.contains(source_abs.to_string_lossy().as_ref()),
            "errors must stay source-path-free: {joined}"
        );
    }
}

// ---------------------------------------------------------------------------
// TC-IMP-005: symlink escape refuses the whole import
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn tc_imp_005_symlink_escape_is_rejected() {
    let (service, temp, repo) = setup().await;
    let source = temp.path().join("symlink-plugin");
    std::fs::create_dir_all(source.join("agents")).unwrap();
    std::fs::write(source.join("agents/real.md"), "---\nname: real\n---\n").unwrap();
    std::fs::write(source.join("outside-secret.txt"), "must not be imported").unwrap();
    std::os::unix::fs::symlink(
        source.join("outside-secret.txt"),
        source.join("agents/linked.md"),
    )
    .unwrap();

    let result = service
        .run_import(&ImportRequest {
            source_path: source,
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "blocked");
    assert!(repo.get_by_snapshot_id(&result.snapshot_id).await.unwrap().is_none());

    let materialized = temp.path().join("agent-store-imports");
    if materialized.exists() {
        // No file from outside the source may exist inside any snapshot dir.
        for entry in walkdir::WalkDir::new(&materialized) {
            let entry = entry.unwrap();
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();
                assert!(
                    name != "outside-secret.txt" && name != "linked.md",
                    "external content must never enter the snapshot"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TC-IMP-006: idempotent reuse + digest conflict blocking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_006_same_digest_reuses_and_conflicting_digest_blocks() {
    let (service, _temp, repo) = setup().await;

    // first import (digest-a content)
    let first = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("digest-a"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(first.status, "completed");
    let rows_after_first = repo.list_snapshots(100).await.unwrap();
    assert_eq!(rows_after_first.len(), 1);

    // exact same content → idempotent reuse, no duplicate row
    let again = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("digest-a"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert!(again.reused, "same digest must reuse the immutable snapshot");
    assert_eq!(again.snapshot_id, first.snapshot_id);
    let rows_after_reuse = repo.list_snapshots(100).await.unwrap();
    assert_eq!(rows_after_reuse.len(), 1, "reuse must not insert a row");

    // same identity+version, different content → blocked, original intact
    let conflict = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("digest-b"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(conflict.status, "blocked", "digest conflict must block");
    assert!(!conflict.errors.is_empty());
    let rows_after_conflict = repo.list_snapshots(100).await.unwrap();
    assert_eq!(rows_after_conflict.len(), 1, "conflict must never overwrite");
    let saved = repo.get_by_snapshot_id(&first.snapshot_id).await.unwrap().unwrap();
    assert_eq!(saved.content_digest, first.content_digest);
}

// ---------------------------------------------------------------------------
// TC-IMP-007: partial component failure → completed-with-warnings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_007_partial_component_failure_keeps_good_components() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("partial-failure"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed-with-warnings");
    assert!(result.errors.iter().any(|error| error.contains("broken.md")), "{:?}", result.errors);
    assert!(result.warnings.iter().any(|warning| warning.contains("missing-commands")));

    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "agent"), 1, "valid agent survives");
    assert_eq!(components[0].name, "good-agent");
}

// ---------------------------------------------------------------------------
// TC-IMP-008: hooks/bin/scripts/lsp import statically, never execute
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_008_high_risk_components_are_static_imports_only() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("hooks-scripts"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    for row in find_kind(&components, "hook") {
        let triple: nomifun_api_types::AppServerCompatibilityTriple =
            serde_json::from_str(&row.compatibility_json).unwrap();
        assert_eq!(triple.semantic_status, "manual_review");
        let payload: serde_json::Value = serde_json::from_str(&row.payload_json).unwrap();
        assert_eq!(payload["static_only"], true);
    }
    for row in find_kind(&components, "script") {
        let triple: nomifun_api_types::AppServerCompatibilityTriple =
            serde_json::from_str(&row.compatibility_json).unwrap();
        assert_eq!(triple.semantic_status, "manual_review");
        let payload: serde_json::Value = serde_json::from_str(&row.payload_json).unwrap();
        assert_eq!(payload["executable"], false);
    }
    assert_eq!(count_kind(&components, "lsp"), 1);
}

// ---------------------------------------------------------------------------
// TC-IMP-009: credentials only produce a schema; values stay out
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tc_imp_009_credential_values_never_enter_snapshot_or_repository() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("userconfig"),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed-with-warnings", "{:?}", result.errors);
    assert!(result.warnings.iter().any(|warning| warning.contains("[REDACTED]")));

    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "credential"), 1);
    let raw = serde_json::to_string(&components[0].payload_json).unwrap();
    assert!(!raw.contains("SECRET"), "raw value must not be stored: {raw}");
    assert!(!components[0].compatibility_json.contains("SECRET"), "compatibility must stay value-free");
    let schema: serde_json::Value = serde_json::from_str(&components[0].payload_json).unwrap();
    assert_eq!(schema["fields"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Market sources: skill market + connector market
// ---------------------------------------------------------------------------

#[tokio::test]
async fn skill_market_imports_skills_with_market_identity() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("skill-market"),
            source_kind: SourceKind::WorkBuddySkillMarket,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.source_kind, "workbuddy-skill-market");
    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "skill"), 2);
    let hello = components.iter().find(|component| component.name == "hello").unwrap();
    let payload: serde_json::Value = serde_json::from_str(&hello.payload_json).unwrap();
    assert!(payload["has_arguments_note"].as_bool().unwrap());
    assert_eq!(payload["instructions_ref"], "skills/hello/SKILL.md");
}

#[tokio::test]
async fn connector_market_imports_connector_entries() {
    let (service, _temp, repo) = setup().await;
    let result = service
        .run_import(&ImportRequest {
            source_path: fixtures().join("connector-market"),
            source_kind: SourceKind::WorkBuddyConnectorMarket,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    let components = repo.get_components(&result.snapshot_id).await.unwrap();
    assert_eq!(count_kind(&components, "connector"), 2);
    assert!(components
        .iter()
        .any(|component| component.name == "jira" && component.payload_json.contains("oauth")));
}

// ---------------------------------------------------------------------------
// Missing manifest → blocked (02 §11.1); missing source → typed error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn missing_manifest_blocks_and_missing_source_is_a_typed_error() {
    let (service, _temp, _repo) = setup().await;
    let empty = tempfile::tempdir().unwrap();
    let result = service
        .run_import(&ImportRequest {
            source_path: empty.path().to_path_buf(),
            source_kind: SourceKind::CodeBuddyPlugin,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "blocked");
    assert!(matches!(
        service
            .run_import(&ImportRequest {
                source_path: fixtures().join("does-not-exist"),
                source_kind: SourceKind::CodeBuddyPlugin,
            })
            .await,
        Err(nomifun_importer::ImportError::SourceNotFound)
    ));
}