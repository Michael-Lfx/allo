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

    /// Remove an MCP server this installer registered.
    ///
    /// Only ever called with the id recorded in the component's `runtime_ref`,
    /// never by name: a name-keyed delete could remove a server the user added
    /// themselves that happens to collide.
    async fn remove(&self, server_id: &str) -> Result<(), AppError>;

    /// Enable or disable a registered MCP server. Idempotent: unlike a toggle,
    /// repeating the call leaves the requested state in place.
    async fn set_enabled(&self, server_id: &str, enabled: bool) -> Result<(), AppError>;
}

/// Preset creation seam (the production adapter wraps
/// `nomifun_preset::PresetService`; tests fake it).
#[async_trait]
pub trait PresetRegistrar: Send + Sync {
    /// `instructions` carries the Agent Markdown body (the persona); store
    /// installs must not drop it or the expert runs with an empty prompt.
    async fn create_agent_store_preset(
        &self,
        name: &str,
        description: Option<&str>,
        instructions: Option<&str>,
        agent_id: Option<&str>,
        model: Option<nomifun_api_types::ModelPreference>,
    ) -> Result<String, AppError>;

    /// Whether a Preset the installer recorded for a component still resolves.
    ///
    /// Install must be re-entrant: `PresetService::create` mints a fresh id
    /// whenever `preset_id` is absent, so a client retry (the natural reaction
    /// to a timeout) would otherwise mint a second Preset and orphan the first,
    /// whose id was overwritten in the record. The recorded id is the only
    /// admissible evidence of reuse — see the reuse branch in `install`.
    ///
    /// `NotFound` answers `false` (the user deleted it by hand; recreate), any
    /// other failure is reported so the caller can surface it.
    async fn preset_exists(&self, preset_id: &str) -> Result<bool, AppError>;

    /// Delete a Preset this installer created (uninstall).
    ///
    /// Only ever called with the id recorded for the component. A Preset that
    /// is already gone is not an error — uninstall is re-entrant.
    async fn delete_preset(&self, preset_id: &str) -> Result<(), AppError>;

    /// Enable or disable a Preset this installer created. Idempotent.
    ///
    /// Disabling here is what makes `install/disable` real for an expert: the
    /// run paths resolve through `PresetService`, which refuses a disabled
    /// Preset, so no run-path change is needed for the flag to bite.
    async fn set_preset_enabled(&self, preset_id: &str, enabled: bool) -> Result<(), AppError>;
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

    async fn remove(&self, server_id: &str) -> Result<(), AppError> {
        let parsed = nomifun_api_types::McpServerId::parse(server_id)
            .map_err(|error| AppError::BadRequest(format!("invalid connector id: {error}")))?;
        // `delete_server` reports a missing row as `McpError::NotFound`, which
        // `From<McpError> for AppError` preserves — the uninstall path reads
        // that as "already released".
        self.config.delete_server(&parsed).await.map_err(AppError::from)?;
        Ok(())
    }

    async fn set_enabled(&self, server_id: &str, enabled: bool) -> Result<(), AppError> {
        let parsed = nomifun_api_types::McpServerId::parse(server_id)
            .map_err(|error| AppError::BadRequest(format!("invalid connector id: {error}")))?;
        self.config
            .set_server_enabled(&parsed, enabled)
            .await
            .map_err(AppError::from)?;
        Ok(())
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
        instructions: Option<&str>,
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
                instructions: instructions.unwrap_or_default().to_owned(),
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

    async fn preset_exists(&self, preset_id: &str) -> Result<bool, AppError> {
        match self.service.get(preset_id).await {
            Ok(_) => Ok(true),
            Err(AppError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn delete_preset(&self, preset_id: &str) -> Result<(), AppError> {
        match self.service.delete(preset_id).await {
            // Already gone: the state the caller asked for.
            Err(AppError::NotFound(_)) => Ok(()),
            other => other,
        }
    }

    async fn set_preset_enabled(&self, preset_id: &str, enabled: bool) -> Result<(), AppError> {
        self.service
            .set_state(
                preset_id,
                nomifun_api_types::SetPresetStateRequest {
                    enabled: Some(enabled),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
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

    /// Flip the recorded flag *and* the runtime state it is supposed to mean.
    ///
    /// Before this, a disable only wrote `plugin_snapshot_components.disabled`,
    /// which nothing on the runtime side reads: a "disabled" connector stayed
    /// enabled in `mcp_servers`, and a "disabled" expert still ran. The flag and
    /// the runtime now move together.
    ///
    /// The flag only flips for components whose runtime state actually moved.
    /// Flipping it regardless would put the catalogue back in the business of
    /// claiming a state the runtime never entered — the same defect being fixed
    /// here, one layer up.
    ///
    /// `skill` is the documented exception: the skill corpus is plain
    /// directories with no state to flip, so for a skill the flag stays a
    /// catalogue marker (see `05` §4.5), and it is reported as such on the wire.
    async fn set_components_runtime_enabled(
        &self,
        snapshot_id: &str,
        component_ids: &[String],
        enabled: bool,
    ) -> Result<AppServerInstallStatus, AppError> {
        if component_ids.is_empty() {
            return Err(AppError::BadRequest(
                "install/disable and install/enable require explicit component ids".into(),
            ));
        }
        let rows = self
            .repo
            .list_installation_state(Some(snapshot_id))
            .await
            .map_err(AppError::from)?;
        let mut moved: Vec<String> = Vec::new();
        for row in rows
            .iter()
            .filter(|row| component_ids.iter().any(|id| id == &row.component_id))
        {
            if row.installed != 1 {
                // Nothing is registered, so there is no runtime state to move and
                // the flag would describe nothing.
                tracing::warn!(
                    snapshot_id,
                    component_id = %row.component_id,
                    "ignoring enable/disable for a component that is not installed"
                );
                continue;
            }
            match set_component_runtime_enabled(&*self.presets, &*self.mcp, row, enabled).await {
                Ok(()) => moved.push(row.component_id.clone()),
                Err(error) => tracing::warn!(
                    snapshot_id,
                    component_id = %row.component_id,
                    %error,
                    "could not move the runtime state; leaving the recorded flag alone"
                ),
            }
        }
        if !moved.is_empty() {
            let ids: Vec<&str> = moved.iter().map(String::as_str).collect();
            self.repo
                .set_components_disabled(&ids, !enabled)
                .await
                .map_err(AppError::from)?;
        }
        self.status(snapshot_id).await
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

        // What this snapshot already registered on this host. Install is
        // re-entrant, so every kind consults this before creating anything:
        // `PresetService::create` mints a new id when none is given, and the
        // MCP registrar upserts by name. Both are only idempotent if the
        // *recorded* reference is honoured (see the agent/team branch).
        let installed_state: std::collections::HashMap<String, nomifun_db::PluginSnapshotComponentRow> =
            self.repo
                .list_installation_state(Some(&snapshot_id))
                .await
                .map_err(AppError::from)?
                .into_iter()
                .map(|row| (row.component_id.clone(), row))
                .collect();

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
        //
        // Re-entrancy: a component that already owns a Preset keeps it. The
        // recorded id is the only admissible evidence — adopting a Preset by
        // *name* is unsafe, because two snapshots may legitimately declare the
        // same display name and the second install would silently rebind this
        // component to the other snapshot's Preset.
        for component in &components {
            if component.kind != "agent" && component.kind != "team" {
                continue;
            }
            if let Some(preset_id) = recorded_preset_id(installed_state.get(&component.component_id)) {
                match self.presets.preset_exists(preset_id).await {
                    Ok(true) => {
                        pending.push(Pending {
                            component_id: component.component_id.clone(),
                            runtime_type: "preset",
                            location: preset_id.to_owned(),
                            mcp_server_id: None,
                        });
                        continue;
                    }
                    // The user deleted the Preset by hand: recreate it below
                    // rather than leave the component pointing at nothing.
                    Ok(false) => {}
                    Err(error) => {
                        warnings.push(format!(
                            "preset lookup failed for {}: {error}",
                            component.component_id
                        ));
                        skipped.push(component.component_id.clone());
                        continue;
                    }
                }
            }
            let payload = decode_payload(&component.payload_json);
            let description = payload.get("description").and_then(|v| v.as_str());
            let instructions = payload.get("instructions").and_then(|v| v.as_str());
            let preset_name = format!("agent-store: {}", component.name);
            match self
                .presets
                .create_agent_store_preset(
                    &preset_name,
                    description,
                    instructions,
                    Some(NOMI_RUNTIME_AGENT_ID),
                    None,
                )
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
            // V1 registers MCP connectors only. `cli` connectors describe a
            // command-line integration (`cli.json` init commands), not an MCP
            // server; projecting their init command into a stdio transport
            // would execute the installer command as a server. Controlled CLI
            // wrapping is Phase 2, so skip with a warning instead of
            // registering a bogus server.
            let connector_kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or_default();
            if connector_kind == "cli" {
                warnings.push(format!(
                    "cli connector {} is not registered as an MCP server in V1",
                    component.component_id
                ));
                skipped.push(component.component_id.clone());
                continue;
            }
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

        // Re-installing a component the user had disabled must bring the runtime
        // back with it. `mark_components_installed` clears `disabled`, so
        // without this a row would read "enabled" while the Preset / MCP server
        // it refers to is still off — the same catalogue-vs-runtime split the
        // enable/disable path exists to prevent.
        //
        // A component that cannot be re-enabled is dropped from `pending`, so
        // its record keeps saying `disabled` and stays consistent with the
        // runtime instead of the two drifting apart.
        let mut kept: Vec<Pending> = Vec::with_capacity(pending.len());
        for entry in pending {
            let was_disabled = installed_state
                .get(&entry.component_id)
                .is_some_and(|row| row.installed == 1 && row.disabled == 1);
            if !was_disabled {
                kept.push(entry);
                continue;
            }
            let Some(row) = installed_state.get(&entry.component_id) else {
                kept.push(entry);
                continue;
            };
            match set_component_runtime_enabled(&*self.presets, &*self.mcp, row, true).await {
                Ok(()) => kept.push(entry),
                Err(error) => {
                    warnings.push(format!(
                        "component {} stays disabled: it could not be re-enabled at runtime: {error}",
                        entry.component_id
                    ));
                    skipped.push(entry.component_id.clone());
                }
            }
        }
        let pending = kept;

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
        self.set_components_runtime_enabled(snapshot_id, component_ids, false).await
    }

    async fn enable(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError> {
        self.set_components_runtime_enabled(snapshot_id, component_ids, true).await
    }

    async fn uninstall(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError> {
        // An empty list would read as "uninstall the whole snapshot" while the
        // call sites pass an explicit selection; refusing is the only reading
        // that cannot surprise either of them.
        if component_ids.is_empty() {
            return Err(AppError::BadRequest(
                "uninstall requires explicit component ids".into(),
            ));
        }
        let rows = self
            .repo
            .list_installation_state(Some(snapshot_id))
            .await
            .map_err(AppError::from)?;

        // Release the runtime artifacts first, then clear the record — and only
        // for the components that actually came out. Clearing a row whose
        // release failed would erase the only pointer to an artifact still on
        // disk, turning a recoverable failure into a permanent orphan.
        let mut released: Vec<String> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        for row in rows
            .iter()
            .filter(|row| component_ids.iter().any(|id| id == &row.component_id))
        {
            match release_component(&self.installer, &*self.presets, &*self.mcp, row).await {
                Ok(()) => released.push(row.component_id.clone()),
                Err(error) => {
                    // Reported to the operator now; the structured per-component
                    // outcome on the wire is the follow-up.
                    tracing::warn!(
                        snapshot_id,
                        component_id = %row.component_id,
                        %error,
                        "uninstall could not release a component; keeping its install record"
                    );
                    failures.push(format!("{}: {error}", row.component_id));
                }
            }
        }
        if !released.is_empty() {
            let ids: Vec<&str> = released.iter().map(String::as_str).collect();
            self.repo.clear_components_installed(&ids).await.map_err(AppError::from)?;
        }
        if !failures.is_empty() {
            tracing::warn!(snapshot_id, ?failures, "uninstall finished with components left installed");
        }
        self.status(snapshot_id).await
    }
}

// ---------------------------------------------------------------------------
// projections / helpers
// ---------------------------------------------------------------------------

/// The parsed `runtime_ref` of a component, when it is readable.
fn runtime_ref(row: &nomifun_db::PluginSnapshotComponentRow) -> Option<serde_json::Value> {
    row.runtime_ref
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
}

/// One non-empty string field of a parsed `runtime_ref`.
fn runtime_ref_field<'a>(runtime: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    runtime
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
}

/// Release everything one installed component owns at runtime.
///
/// The recorded `runtime_ref` is the authority for *what* exists. A row that is
/// `installed == 1` but carries no readable ref is a defect, not a no-op: it is
/// refused rather than cleared, because clearing it would drop the only pointer
/// to an artifact that is still on disk.
///
/// Deleting by recorded id (never by name) is what keeps uninstall from taking
/// out content the user added themselves under a colliding name.
async fn release_component(
    installer: &InstallerService,
    presets: &dyn PresetRegistrar,
    mcp: &dyn McpRegistrar,
    row: &nomifun_db::PluginSnapshotComponentRow,
) -> Result<(), AppError> {
    if row.installed != 1 {
        // Never registered, or already released: nothing to take out.
        return Ok(());
    }
    let Some(runtime) = runtime_ref(row) else {
        return Err(AppError::Internal(format!(
            "component {} is recorded as installed without a readable runtime_ref",
            row.component_id
        )));
    };
    match runtime.get("type").and_then(|value| value.as_str()).unwrap_or_default() {
        "skill" => {
            let location = runtime_ref_field(&runtime, "location").ok_or_else(|| {
                AppError::Internal(format!(
                    "skill component {} has no recorded location",
                    row.component_id
                ))
            })?;
            let slug = std::path::Path::new(location)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "skill component {} has an unusable recorded location",
                        row.component_id
                    ))
                })?;
            // `false` (already gone) is success: uninstall is re-entrant.
            installer
                .remove_materialized(&row.snapshot_id, slug)
                .map(|_removed| ())
                .map_err(|error| AppError::Internal(error.to_string()))
        }
        "preset" => {
            let preset_id = recorded_preset_id(Some(row)).ok_or_else(|| {
                AppError::Internal(format!(
                    "preset component {} has no recorded preset id",
                    row.component_id
                ))
            })?;
            presets.delete_preset(preset_id).await
        }
        "connector" => {
            let server_id = runtime_ref_field(&runtime, "mcp_server_id").ok_or_else(|| {
                AppError::Internal(format!(
                    "connector component {} has no recorded mcp_server_id",
                    row.component_id
                ))
            })?;
            mcp.remove(server_id).await
        }
        other => Err(AppError::Internal(format!(
            "cannot release runtime type {other:?} for component {}",
            row.component_id
        ))),
    }
}

/// Move the runtime state a component's enable/disable flag is supposed to mean.
///
/// `skill` is deliberately a no-op: the skill corpus is plain directories with
/// no state to flip, so for a skill the flag stays a catalogue marker
/// (`05-allo-app-server-protocol.md` §4.5). Everything else must actually move,
/// or the catalogue would claim a state the runtime never entered.
async fn set_component_runtime_enabled(
    presets: &dyn PresetRegistrar,
    mcp: &dyn McpRegistrar,
    row: &nomifun_db::PluginSnapshotComponentRow,
    enabled: bool,
) -> Result<(), AppError> {
    let Some(runtime) = runtime_ref(row) else {
        return Err(AppError::Internal(format!(
            "component {} is recorded as installed without a readable runtime_ref",
            row.component_id
        )));
    };
    match runtime.get("type").and_then(|value| value.as_str()).unwrap_or_default() {
        // Catalogue marker only — see the doc comment above.
        "skill" => Ok(()),
        "preset" => {
            let preset_id = recorded_preset_id(Some(row)).ok_or_else(|| {
                AppError::Internal(format!(
                    "preset component {} has no recorded preset id",
                    row.component_id
                ))
            })?;
            presets.set_preset_enabled(preset_id, enabled).await
        }
        "connector" => {
            let server_id = runtime_ref_field(&runtime, "mcp_server_id").ok_or_else(|| {
                AppError::Internal(format!(
                    "connector component {} has no recorded mcp_server_id",
                    row.component_id
                ))
            })?;
            mcp.set_enabled(server_id, enabled).await
        }
        other => Err(AppError::Internal(format!(
            "cannot set the runtime enabled state of type {other:?} for component {}",
            row.component_id
        ))),
    }
}

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

/// The Preset id this component is already bound to, if any.
///
/// Only an `installed` row counts: a row that was uninstalled has had its refs
/// nulled, and a row that was never installed has none. Empty strings are
/// rejected because `mark_components_installed` binds `""` for every runtime
/// type that is not a Preset (`sqlite_plugin_snapshot.rs`), so `""` means
/// "not a preset component", not "a preset with an empty id".
fn recorded_preset_id(row: Option<&nomifun_db::PluginSnapshotComponentRow>) -> Option<&str> {
    let row = row?;
    if row.installed != 1 {
        return None;
    }
    row.preset_id.as_deref().map(str::trim).filter(|id| !id.is_empty())
}

/// Derive an MCP transport JSON from a connector component payload. The
/// importer stores `kind` (`remote-mcp` / `stdio-mcp` / `cli`) plus
/// `transport_summary` (URL or command). Unknown shapes land in `http` only
/// when a URL is present; otherwise the component stays unregistered.
fn connector_transport(payload: &serde_json::Value) -> String {
    // Prefer the structured transport captured at import time: the summary is
    // display-oriented and loses argv entries (a stdio server is useless
    // without them). Fall back to deriving from the summary for older
    // snapshots that predate the structured field.
    if let Some(transport) = payload.get("transport").filter(|value| value.is_object()) {
        return transport.to_string();
    }
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
    fn connector_transport_prefers_structured_transport() {
        // WP-2 B6: stdio argv must survive import → registration; the summary
        // only carries the command.
        let payload = serde_json::json!({
            "kind": "stdio-mcp",
            "transport": {
                "type": "stdio",
                "command": "C:/appexe/bun.exe",
                "args": ["C:/tmp/mock-mcp.mjs", "--flag"],
                "env": { "TOKEN": "x" },
            },
            "transport_summary": "C:/appexe/bun.exe",
        });
        let json = connector_transport(&payload);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "stdio");
        assert_eq!(value["command"], "C:/appexe/bun.exe");
        assert_eq!(value["args"], serde_json::json!(["C:/tmp/mock-mcp.mjs", "--flag"]));
        assert_eq!(value["env"]["TOKEN"], "x");
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
