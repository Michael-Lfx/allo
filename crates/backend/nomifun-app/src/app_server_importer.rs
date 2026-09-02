//! App Server Importer / Agent / Team catalog adapters for the composition
//! root (`nomifun-app`), mirroring `app_server_catalog.rs`.
//!
//! These adapters are deliberately thin: the real pipeline lives in
//! `nomifun-importer`, storage in `nomifun-db`. No absolute source paths,
//! credentials or internal execution ids cross into the protocol layer.

use std::sync::Arc;

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerAgentDetail, AppServerAgentSummary, AppServerCompatibilityStatus,
    AppServerCompatibilityTriple, AppServerImportComponent, AppServerImportDetail,
    AppServerImportRequest, AppServerImportResult, AppServerImportSummary, AppServerTeamDetail,
    AppServerTeamSummary,
};
use nomifun_app_server::{AgentCatalogProvider, ImportProvider, TeamCatalogProvider};
use nomifun_common::AppError;
use nomifun_db::{
    IPluginSnapshotRepository, PluginSnapshotComponentRow,
};
use nomifun_importer::ImporterService;

// ---------------------------------------------------------------------------
// Import pipeline
// ---------------------------------------------------------------------------

/// Runs the importer on the trusted host and reads the persisted snapshot
/// history. `ImportError::SourceNotFound` maps to a `404`-style AppError so
/// the protocol can return `import_source_not_found`.
#[derive(Clone)]
pub struct AppServerImportProvider {
    importer: ImporterService,
    repo: Arc<dyn IPluginSnapshotRepository>,
}

impl AppServerImportProvider {
    pub fn new(importer: ImporterService, repo: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { importer, repo }
    }
}

fn map_import_error(error: nomifun_importer::ImportError) -> AppError {
    match error {
        nomifun_importer::ImportError::SourceNotFound => {
            AppError::NotFound("import source does not exist or is not a directory".into())
        }
        nomifun_importer::ImportError::Internal(message) => AppError::Internal(message),
    }
}

#[async_trait]
impl ImportProvider for AppServerImportProvider {
    async fn run(&self, request: AppServerImportRequest) -> Result<AppServerImportResult, AppError> {
        let source_kind = match request.source_kind {
            nomifun_api_types::AppServerImportSourceKind::CodeBuddyPlugin => {
                nomifun_importer::SourceKind::CodeBuddyPlugin
            }
            nomifun_api_types::AppServerImportSourceKind::WorkBuddySkillMarket => {
                nomifun_importer::SourceKind::WorkBuddySkillMarket
            }
            nomifun_api_types::AppServerImportSourceKind::WorkBuddyConnectorMarket => {
                nomifun_importer::SourceKind::WorkBuddyConnectorMarket
            }
        };
        let import_request = nomifun_importer::ImportRequest {
            source_path: request.source_path.into(),
            source_kind,
        };
        self.importer
            .run_import(&import_request)
            .await
            .map_err(map_import_error)
    }

    async fn list(&self, limit: u32) -> Result<Vec<AppServerImportSummary>, AppError> {
        let rows = self.repo.list_snapshots(limit).await.map_err(AppError::from)?;
        Ok(rows
            .into_iter()
            .map(|row| AppServerImportSummary {
                snapshot_id: row.snapshot_id,
                name: row.name,
                version: row.version,
                source_kind: row.source_kind,
                status: row.status,
                component_count: 0,
                imported_at: row.imported_at,
            })
            .collect())
    }

    async fn get(&self, snapshot_id: &str) -> Result<AppServerImportDetail, AppError> {
        let row = self
            .repo
            .get_by_snapshot_id(snapshot_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("snapshot {snapshot_id} not found")))?;
        let components = self.repo.get_components(snapshot_id).await.map_err(AppError::from)?;
        let component_count = components.len();
        Ok(AppServerImportDetail {
            summary: AppServerImportSummary {
                snapshot_id: row.snapshot_id,
                name: row.name,
                version: row.version,
                source_kind: row.source_kind,
                status: row.status,
                component_count,
                imported_at: row.imported_at,
            },
            content_digest: row.content_digest,
            component_status: components
                .first()
                .map(|component| decode_triple(&component.compatibility_json))
                .unwrap_or_else(|| AppServerCompatibilityTriple {
                    semantic_status: "compatible".into(),
                    runtime_status: "not-verified".into(),
                    distribution_status: "local-only".into(),
                    reasons: vec![],
                }),
            warnings: vec![],
            errors: vec![],
            components: components.iter().map(component_view).collect(),
        })
    }
}

fn decode_triple(json: &str) -> AppServerCompatibilityTriple {
    serde_json::from_str(json).unwrap_or_else(|_| AppServerCompatibilityTriple {
        semantic_status: "unsupported".into(),
        runtime_status: "not-verified".into(),
        distribution_status: "local-only".into(),
        reasons: vec!["compatibility record could not be decoded".into()],
    })
}

fn component_view(row: &PluginSnapshotComponentRow) -> AppServerImportComponent {
    AppServerImportComponent {
        id: row.component_id.clone(),
        kind: row.kind.clone(),
        name: row.name.clone(),
        compatibility: decode_triple(&row.compatibility_json),
        warnings: vec![],
    }
}

// ---------------------------------------------------------------------------
// Agent / Team catalog over imported definitions
// ---------------------------------------------------------------------------

fn semantic_status(triple: &AppServerCompatibilityTriple) -> AppServerCompatibilityStatus {
    match triple.semantic_status.as_str() {
        "compatible" => AppServerCompatibilityStatus::Compatible,
        "compatible_with_adapter" => AppServerCompatibilityStatus::CompatibleWithAdapter,
        "manual_review" => AppServerCompatibilityStatus::ManualReview,
        "unsupported" => AppServerCompatibilityStatus::Unsupported,
        "pending_legal_review" => AppServerCompatibilityStatus::PendingLegalReview,
        _ => AppServerCompatibilityStatus::Unsupported,
    }
}

fn payload(row: &PluginSnapshotComponentRow) -> serde_json::Value {
    serde_json::from_str(&row.payload_json).unwrap_or_else(|_| serde_json::json!({}))
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| value.as_str()).map(str::to_owned)
}

fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// AgentDefinition catalog over snapshot components (docs 05 §4.1).
#[derive(Clone)]
pub struct AppServerAgentCatalog {
    repo: Arc<dyn IPluginSnapshotRepository>,
}

impl AppServerAgentCatalog {
    pub fn new(repo: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { repo }
    }

    async fn rows(&self) -> Result<Vec<PluginSnapshotComponentRow>, AppError> {
        self.repo
            .list_components_by_kind("agent")
            .await
            .map_err(AppError::from)
    }
}

#[async_trait]
impl AgentCatalogProvider for AppServerAgentCatalog {
    async fn list(&self) -> Result<Vec<AppServerAgentSummary>, AppError> {
        Ok(self.rows().await?.iter().map(agent_summary).collect())
    }

    async fn get(&self, id: &str) -> Result<AppServerAgentDetail, AppError> {
        let row = self
            .rows()
            .await?
            .into_iter()
            .find(|row| row.component_id == id)
            .ok_or_else(|| AppError::NotFound(format!("agent {id} not found")))?;
        Ok(agent_detail(&row))
    }
}

fn agent_summary(row: &PluginSnapshotComponentRow) -> AppServerAgentSummary {
    let value = payload(row);
    let triple = decode_triple(&row.compatibility_json);
    let model_summary = string_field(&value, "model");
    let tools = string_array(&value, "tools");
    let disallowed = string_array(&value, "disallowed_tools");
    let tool_policy_summary = if tools.is_empty() && disallowed.is_empty() {
        None
    } else {
        let mut summary: Vec<String> = tools;
        if !disallowed.is_empty() {
            summary.push(format!("禁用: {}", disallowed.join(", ")));
        }
        Some(summary.join(", "))
    };
    AppServerAgentSummary {
        id: row.component_id.clone(),
        version: string_field(&value, "version").unwrap_or_default(),
        name: row.name.clone(),
        description: string_field(&value, "description"),
        skills: string_array(&value, "skills"),
        connectors: vec![],
        model_summary,
        tool_policy_summary,
        source: string_field(&value, "source").unwrap_or_else(|| "imported".into()),
        compatibility_status: semantic_status(&triple),
    }
}

fn agent_detail(row: &PluginSnapshotComponentRow) -> AppServerAgentDetail {
    let value = payload(row);
    AppServerAgentDetail {
        summary: agent_summary(row),
        effort: string_field(&value, "effort"),
        max_turns: value.get("max_turns").and_then(|value| value.as_u64()).map(|n| n as u32),
        disallowed_tools: string_array(&value, "disallowed_tools"),
        memory: string_field(&value, "memory"),
        background: string_field(&value, "background"),
        isolation: string_field(&value, "isolation"),
        permission_mode_ignored: value
            .get("permission_mode_ignored")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    }
}

/// Team catalog over snapshot components (docs 05 §4.2).
#[derive(Clone)]
pub struct AppServerTeamCatalog {
    repo: Arc<dyn IPluginSnapshotRepository>,
}

impl AppServerTeamCatalog {
    pub fn new(repo: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { repo }
    }

    async fn rows(&self) -> Result<Vec<PluginSnapshotComponentRow>, AppError> {
        self.repo.list_components_by_kind("team").await.map_err(AppError::from)
    }
}

#[async_trait]
impl TeamCatalogProvider for AppServerTeamCatalog {
    async fn list(&self) -> Result<Vec<AppServerTeamSummary>, AppError> {
        Ok(self.rows().await?.iter().map(team_summary).collect())
    }

    async fn get(&self, id: &str) -> Result<AppServerTeamDetail, AppError> {
        let row = self
            .rows()
            .await?
            .into_iter()
            .find(|row| row.component_id == id)
            .ok_or_else(|| AppError::NotFound(format!("team {id} not found")))?;
        Ok(team_detail(&row))
    }
}

fn team_summary(row: &PluginSnapshotComponentRow) -> AppServerTeamSummary {
    let value = payload(row);
    let triple = decode_triple(&row.compatibility_json);
    AppServerTeamSummary {
        id: row.component_id.clone(),
        version: string_field(&value, "version").unwrap_or_default(),
        name: row.name.clone(),
        description: string_field(&value, "description"),
        lead_agent_id: string_field(&value, "lead_agent_id").unwrap_or_default(),
        member_agent_ids: string_array(&value, "member_agent_ids"),
        source: string_field(&value, "source").unwrap_or_else(|| "imported".into()),
        compatibility_status: semantic_status(&triple),
    }
}

fn team_detail(row: &PluginSnapshotComponentRow) -> AppServerTeamDetail {
    let value = payload(row);
    AppServerTeamDetail {
        summary: team_summary(row),
        planner_policy: string_field(&value, "planner_policy").unwrap_or_else(|| "planned".into()),
        routing_constraints: string_array(&value, "routing_constraints"),
        workflow_limits: value
            .get("workflow_limits")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        team_runtime_capabilities: string_array(&value, "team_runtime_capabilities"),
    }
}