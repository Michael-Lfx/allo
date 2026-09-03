//! App Server Installer adapter for the composition root (`nomifun-app`),
//! mirroring `app_server_importer.rs`.
//!
//! Installation registers imported snapshot components into the runtime:
//! - `skill`      → copied under the managed skills root (`skills/agent-store/`),
//!                  which the system skill scanner already observes;
//! - `agent/team` → a user Preset (PresetService.create) so the preset list and
//!                  agent runs can use it;
//! - `connector`  → an MCP server config (McpRegistrar.upsert) with the
//!                  transport derived from the component payload.
//!
//! The adapter never executes content, never returns absolute source paths,
//! and never carries credential values across the protocol seam.

use std::sync::Arc;

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerInstallComponent, AppServerInstallRequest, AppServerInstallResult,
    AppServerInstallState, AppServerInstallStatus,
};
use nomifun_app_server::InstallProvider;
use nomifun_common::AppError;
use nomifun_db::{ComponentRuntimeRef, IPluginSnapshotRepository};
use nomifun_importer::{InstallerConfig, InstallerService};

/// Internal Nomi runtime agent id (`agent_builtin_nomi`). The App Server
/// compatibility surface only allows Presets resolved to a Nomi Runtime
/// Agent, so agent-store presets pin this agent to be runnable.
const NOMI_RUNTIME_AGENT_ID: &str = "0190f5fe-7c00-7a00-8000-000000000114";

/// MCP config seam (kept narrow so tests can fake it; the production adapter
/// wraps `nomifun_mcp::McpConfigService`).
#[async_trait]
pub trait McpRegistrar: Send + Sync {
    /// Upsert an MCP server config by name; returns the server id.
    async fn upsert(&self, name: &str, transport_json: &str) -> Result<String, AppError>;
}

/// Preset creation seam (the production adapter wraps
/// `nomifun_preset::PresetService`; tests fake it).
#[async_trait]
pub trait PresetRegistrar: Send + Sync {
    async fn create_agent_store_preset(
        &self,
        name: &str,
        description: Option<&str>,
        agent_id: Option<&str>,
        model: Option<nomifun_api_types::ModelPreference>,
    ) -> Result<String, AppError>;
}

/// Production MCP registrar over `nomifun_mcp::McpConfigService`.
pub struct AppServerMcpRegistrar {
    config: nomifun_mcp::McpConfigService,
}

impl AppServerMcpRegistrar {
    pub fn new(config: nomifun_mcp::McpConfigService) -> Self {
        Self { config }
    }
}

#[async_trait]
impl McpRegistrar for AppServerMcpRegistrar {
    async fn upsert(&self, name: &str, transport_json: &str) -> Result<String, AppError> {
        let request: nomifun_api_types::CreateMcpServerRequest =
            serde_json::from_str(&serde_json::json!({
                "name": name,
                "transport": serde_json::from_str::<serde_json::Value>(transport_json)
                    .unwrap_or_else(|_| serde_json::json!({ "type": "http", "url": "" })),
            })
            .to_string())
            .map_err(|error| AppError::Internal(format!("mcp upsert request: {error}")))?;
        let response = self
            .config
            .add_server(request)
            .await
            .map_err(|error| AppError::Internal(format!("mcp upsert: {error}")))?;
        Ok(response.mcp_server_id.to_string())
    }
}

/// Production preset registrar over `nomifun_preset::PresetService`.
pub struct AppServerPresetRegistrar {
    service: Arc<nomifun_preset::PresetService>,
}

impl AppServerPresetRegistrar {
    pub fn new(service: Arc<nomifun_preset::PresetService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl PresetRegistrar for AppServerPresetRegistrar {
    async fn create_agent_store_preset(
        &self,
        name: &str,
        description: Option<&str>,
        agent_id: Option<&str>,
        model: Option<nomifun_api_types::ModelPreference>,
    ) -> Result<String, AppError> {
        let response = self
            .service
            .create(nomifun_api_types::CreatePresetRequest {
                preset_id: None,
                name: name.to_owned(),
                description: description.map(str::to_owned),
                routing_description: None,
                instructions: String::new(),
                avatar: None,
                fallback_allowed: false,
                targets: vec![],
                agent_preferences: agent_id
                    .map(|agent_id| vec![nomifun_api_types::AgentPreference {
                        agent_id: agent_id.to_owned(),
                        required: true,
                    }])
                    .unwrap_or_default(),
                model_preferences: model.into_iter().collect(),
                included_skills: vec![],
                excluded_auto_skills: vec![],
                knowledge_policy: Default::default(),
                knowledge_bases: vec![],
                mcp_server_ids: vec![],
                examples: vec![],
                examples_i18n: Default::default(),
                audience_tag_ids: vec![],
                scenario_tag_ids: vec![],
                name_i18n: Default::default(),
                description_i18n: Default::default(),
                instructions_i18n: Default::default(),
            })
            .await?;
        Ok(response.preset_id.clone())
    }
}

/// Composition-root Installer. Owns the runtime seams; the InstallerService in
/// `nomifun-importer` owns the copy primitive.
#[derive(Clone)]
pub struct AppServerInstallProvider {
    installer: InstallerService,
    repo: Arc<dyn IPluginSnapshotRepository>,
    presets: Arc<dyn PresetRegistrar>,
    mcp: Arc<dyn McpRegistrar>,
}

impl AppServerInstallProvider {
    pub fn new(
        snapshot_root: std::path::PathBuf,
        skills_root: std::path::PathBuf,
        repo: Arc<dyn IPluginSnapshotRepository>,
        presets: Arc<dyn PresetRegistrar>,
        mcp: Arc<dyn McpRegistrar>,
    ) -> Self {
        let installer = InstallerService::new(InstallerConfig { snapshot_root, skills_root });
        Self { installer, repo, presets, mcp }
    }
}

#[async_trait]
impl InstallProvider for AppServerInstallProvider {
    async fn install(&self, request: AppServerInstallRequest) -> Result<AppServerInstallResult, AppError> {
        let snapshot_id = request.snapshot_id;
        let snapshot = self
            .repo
            .get_by_snapshot_id(&snapshot_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("snapshot {snapshot_id} not found")))?;

        let components = self.repo.get_components(&snapshot_id).await.map_err(AppError::from)?;
        let mut skipped = Vec::new();
        let mut warnings = Vec::new();

        // Registrations accumulate as owned strings first; the borrowed
        // ComponentRuntimeRef vec is built last inside one scope so the
        // borrows outlive only the repo write.
        struct Pending {
            component_id: String,
            runtime_type: &'static str,
            location: String,
            mcp_server_id: Option<String>,
        }
        let mut pending: Vec<Pending> = Vec::new();

        // 1. skills → managed skills root
        let materialized = self
            .installer
            .materialize_skills(&snapshot_id)
            .await
            .map_err(|error| AppError::Internal(error.to_string()))?;
        for location in &materialized.skills {
            let suffix = format!("{}/SKILL.md", location.slug);
            let component_id = components
                .iter()
                .find(|component| {
                    if component.kind != "skill" {
                        return false;
                    }
                    // Nested skills: `skills/<slug>/SKILL.md` matches by path.
                    // Single-skill snapshots keep `SKILL.md` at the snapshot
                    // root but record `<slug>/SKILL.md`; match by name as a
                    // fallback so those still register.
                    let by_path = component
                        .relative_path
                        .as_deref()
                        .map(|rel| rel.ends_with(&suffix))
                        .unwrap_or(false);
                    by_path || component.name == location.slug
                })
                .map(|component| component.component_id.clone());
            match component_id {
                Some(component_id) => pending.push(Pending {
                    component_id,
                    runtime_type: "skill",
                    location: location.location.display().to_string(),
                    mcp_server_id: None,
                }),
                None => skipped.push(format!("skill:{}", location.slug)),
            }
        }

        // 2. agent/team → Preset. The agent-store preset binds the internal
        // Nomi runtime agent (`agent_builtin_nomi`, the only runtime type the
        // App Server compatibility surface accepts) so `agent/run` can resolve
        // it to a Nomi Runtime Agent; the model stays unbound and the run
        // layer falls back to the owner's first enabled provider/model.
        for component in &components {
            if component.kind != "agent" && component.kind != "team" {
                continue;
            }
            let payload = decode_payload(&component.payload_json);
            let description = payload.get("description").and_then(|v| v.as_str());
            let preset_name = format!("agent-store: {}", component.name);
            match self
                .presets
                .create_agent_store_preset(&preset_name, description, Some(NOMI_RUNTIME_AGENT_ID), None)
                .await
            {
                Ok(preset_id) => pending.push(Pending {
                    component_id: component.component_id.clone(),
                    runtime_type: "preset",
                    location: preset_id.clone(),
                    mcp_server_id: None,
                }),
                Err(error) => {
                    warnings.push(format!("preset create failed for {}: {error}", component.component_id));
                    skipped.push(component.component_id.clone());
                }
            }
        }

        // 3. connector → MCP server
        for component in &components {
            if component.kind != "connector" {
                continue;
            }
            let payload = decode_payload(&component.payload_json);
            let transport_json = connector_transport(&payload);
            match self.mcp.upsert(&component.name, &transport_json).await {
                Ok(server_id) => pending.push(Pending {
                    component_id: component.component_id.clone(),
                    runtime_type: "connector",
                    location: component.name.clone(),
                    mcp_server_id: Some(server_id.clone()),
                }),
                Err(error) => {
                    warnings.push(format!("connector register failed for {}: {error}", component.component_id));
                    skipped.push(component.component_id.clone());
                }
            }
        }

        // One scope: build borrowed refs from the owned pending list.
        let refs: Vec<ComponentRuntimeRef<'_>> = pending
            .iter()
            .map(|p| ComponentRuntimeRef {
                component_id: &p.component_id,
                runtime_type: p.runtime_type,
                location: &p.location,
                mcp_server_id: p.mcp_server_id.as_deref(),
            })
            .collect();

        let installed_count = refs.len();
        if !refs.is_empty() {
            self.repo
                .mark_components_installed(&refs, nomifun_common::now_ms())
                .await
                .map_err(AppError::from)?;
        }
        warnings.dedup();
        skipped.dedup();
        Ok(AppServerInstallResult {
            snapshot_id,
            name: snapshot.name,
            version: snapshot.version,
            installed_count,
            skipped,
            warnings,
            errors: vec![],
        })
    }

    async fn status(&self, snapshot_id: &str) -> Result<AppServerInstallStatus, AppError> {
        let components = self
            .repo
            .list_installation_state(Some(snapshot_id))
            .await
            .map_err(AppError::from)?;
        Ok(project_status(snapshot_id.to_owned(), &components))
    }

    async fn disable(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError> {
        let ids: Vec<&str> = component_ids.iter().map(String::as_str).collect();
        self.repo.set_components_disabled(&ids, true).await.map_err(AppError::from)?;
        self.status(snapshot_id).await
    }

    async fn enable(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError> {
        let ids: Vec<&str> = component_ids.iter().map(String::as_str).collect();
        self.repo.set_components_disabled(&ids, false).await.map_err(AppError::from)?;
        self.status(snapshot_id).await
    }

    async fn uninstall(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError> {
        let ids: Vec<&str> = component_ids.iter().map(String::as_str).collect();
        self.repo.clear_components_installed(&ids).await.map_err(AppError::from)?;
        self.status(snapshot_id).await
    }
}

// ---------------------------------------------------------------------------
// projections / helpers
// ---------------------------------------------------------------------------

fn project_status(snapshot_id: String, rows: &[nomifun_db::PluginSnapshotComponentRow]) -> AppServerInstallStatus {
    AppServerInstallStatus {
        snapshot_id,
        components: rows
            .iter()
            .map(|row| {
                let state = if row.installed == 0 {
                    AppServerInstallState::NotInstalled
                } else if row.disabled == 1 {
                    AppServerInstallState::Disabled
                } else {
                    AppServerInstallState::Installed
                };
                let runtime_location = row
                    .runtime_ref
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                    .and_then(|value| value.get("location").and_then(|v| v.as_str()).map(str::to_owned));
                AppServerInstallComponent {
                    id: row.component_id.clone(),
                    kind: row.kind.clone(),
                    name: row.name.clone(),
                    state,
                    runtime_location,
                    preset_id: row.preset_id.clone(),
                }
            })
            .collect(),
    }
}

fn decode_payload(json: &str) -> serde_json::Value {
    serde_json::from_str(json).unwrap_or_else(|_| serde_json::json!({}))
}

/// Derive an MCP transport JSON from a connector component payload. The
/// importer stores `kind` (`remote-mcp` / `stdio-mcp` / `cli`) plus
/// `transport_summary` (URL or command). Unknown shapes land in `http` only
/// when a URL is present; otherwise the component stays unregistered.
fn connector_transport(payload: &serde_json::Value) -> String {
    let summary = payload.get("transport_summary").and_then(|v| v.as_str()).unwrap_or("");
    let transport = if summary.starts_with("http") {
        serde_json::json!({ "type": "http", "url": summary })
    } else if !summary.is_empty() {
        let mut parts = summary.split_whitespace();
        let base = parts.next().unwrap_or("").to_owned();
        serde_json::json!({
            "type": "stdio",
            "command": base,
            "args": parts.map(str::to_owned).collect::<Vec<_>>()
        })
    } else {
        serde_json::json!({ "type": "stdio", "command": "", "args": [] })
    };
    transport.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_transport_derives_http_from_url() {
        let payload = serde_json::json!({ "kind": "remote-mcp", "transport_summary": "https://mcp.example.com/x" });
        let json = connector_transport(&payload);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "http");
        assert_eq!(value["url"], "https://mcp.example.com/x");
    }

    #[test]
    fn connector_transport_derives_stdio_from_command() {
        let payload = serde_json::json!({ "kind": "stdio-mcp", "transport_summary": "npx @playwright/mcp --flag" });
        let json = connector_transport(&payload);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "stdio");
        assert_eq!(value["command"], "npx");
        assert_eq!(value["args"], serde_json::json!(["@playwright/mcp", "--flag"]));
    }

    #[test]
    fn project_status_maps_db_flags_to_states() {
        let rows = vec![
            nomifun_db::PluginSnapshotComponentRow {
                id: 1,
                snapshot_id: "snap".into(),
                component_id: "wb-a".into(),
                kind: "skill".into(),
                name: "a".into(),
                relative_path: Some("skills/a/SKILL.md".into()),
                compatibility_json: "{}".into(),
                payload_json: "{}".into(),
                installed: 1,
                disabled: 0,
                installed_at: Some(1),
                preset_id: None,
                runtime_ref: Some(r#"{"type":"skill","location":"/x/SKILL.md"}"#.into()),
            },
            nomifun_db::PluginSnapshotComponentRow {
                id: 2,
                snapshot_id: "snap".into(),
                component_id: "wb-b".into(),
                kind: "agent".into(),
                name: "b".into(),
                relative_path: None,
                compatibility_json: "{}".into(),
                payload_json: "{}".into(),
                installed: 0,
                disabled: 0,
                installed_at: None,
                preset_id: None,
                runtime_ref: None,
            },
        ];
        let status = project_status("snap".into(), &rows);
        assert_eq!(status.components.len(), 2);
        assert_eq!(status.components[0].state, AppServerInstallState::Installed);
        assert_eq!(status.components[0].runtime_location.as_deref(), Some("/x/SKILL.md"));
        assert_eq!(status.components[1].state, AppServerInstallState::NotInstalled);
    }
}
