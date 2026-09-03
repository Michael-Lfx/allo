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
    AppServerImportRequest, AppServerImportResult, AppServerImportSummary, AppServerLocalizedText,
    AppServerTeamDetail, AppServerTeamSummary,
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
            nomifun_api_types::AppServerImportSourceKind::WorkBuddyCliConnector => {
                nomifun_importer::SourceKind::WorkBuddyCliConnector
            }
            nomifun_api_types::AppServerImportSourceKind::WorkBuddyMcpConnector => {
                nomifun_importer::SourceKind::WorkBuddyMcpConnector
            }
        };
        let import_request = nomifun_importer::ImportRequest {
            source_path: request.source_path.into(),
            source_kind,
            marketplace_id: None,
            entry_name: None,
            source_revision: None,
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
                snapshot_id: row.snapshot.snapshot_id,
                name: row.snapshot.name,
                version: row.snapshot.version,
                source_kind: row.snapshot.source_kind,
                status: row.snapshot.status,
                component_count: row.component_count.max(0) as usize,
                imported_at: row.snapshot.imported_at,
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

fn localized_field(value: &serde_json::Value, key: &str) -> Option<AppServerLocalizedText> {
    let field = value.get(key)?;
    let mut out = AppServerLocalizedText { en: None, zh: None };
    match field {
        serde_json::Value::String(text) => out.zh = Some(text.clone()),
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if let Some(text) = v.as_str() {
                    match k.as_str() {
                        "en" => out.en = Some(text.to_owned()),
                        "zh" => out.zh = Some(text.to_owned()),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    if out.en.is_none() && out.zh.is_none() { None } else { Some(out) }
}

fn localized_list(value: &serde_json::Value, key: &str) -> Vec<AppServerLocalizedText> {
    value
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    serde_json::Value::String(text) => Some(AppServerLocalizedText { en: None, zh: Some(text.clone()) }),
                    serde_json::Value::Object(map) => {
                        let mut out = AppServerLocalizedText { en: None, zh: None };
                        for (k, v) in map {
                            if let Some(text) = v.as_str() {
                                match k.as_str() {
                                    "en" => out.en = Some(text.to_owned()),
                                    "zh" => out.zh = Some(text.to_owned()),
                                    _ => {}
                                }
                            }
                        }
                        if out.en.is_none() && out.zh.is_none() { None } else { Some(out) }
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
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
        preset_id: row.preset_id.clone(),
        description: string_field(&value, "description"),
        skills: string_array(&value, "skills"),
        connectors: vec![],
        model_summary,
        tool_policy_summary,
        source: string_field(&value, "source").unwrap_or_else(|| "imported".into()),
        compatibility_status: semantic_status(&triple),
        display_name: localized_field(&value, "display_name"),
        profession: localized_field(&value, "profession"),
        avatar_url: asset_url(&value, row),
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
        display_description: localized_field(&value, "display_description"),
        quick_prompts: localized_list(&value, "quick_prompts"),
        tags: localized_list(&value, "tags"),
        default_init_prompt: localized_field(&value, "default_init_prompt"),
        expert_type: string_field(&value, "expert_type"),
        category_id: string_field(&value, "category_id"),
    }
}

/// Build the public asset URL for an avatar declared as a snapshot-relative
/// path (`avatars/expert.png`). The asset endpoint validates the path and
/// serves only whitelisted types; `None` when no avatar was declared.
fn asset_url(value: &serde_json::Value, row: &PluginSnapshotComponentRow) -> Option<String> {
    let avatar = value.get("avatar").and_then(|value| value.as_str())?;
    Some(format!(
        "/api/app-server/imports/{}/assets/{}",
        row.snapshot_id,
        avatar.trim_start_matches('/')
    ))
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