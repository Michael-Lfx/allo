//! App Server ExpertPack export adapter for the composition root
//! (`nomifun-app`), mirroring `app_server_importer.rs` / `app_server_installer.rs`.
//!
//! Produces the portable **definition** a third-party runtime can run itself
//! (`agent/export` · `team/export`, `docs/agent-store/32-expert-pack-export.zh.md`):
//! the persona, the model hints, skill *references*, and — for a team — the
//! roster with every member expanded.
//!
//! What it deliberately does **not** carry, and why:
//!
//! - **Execution semantics.** Team planning, step scheduling, tool/credential
//!   policy and event shapes are enforced by the runtime and are not data
//!   (doc `32` §5 lists what a consumer must implement itself).
//! - **Skill bytes.** A skill is a directory and `skill/files` already serves it;
//!   inlining would create a second source of truth (doc `32` §2).
//! - **Connector credentials, transport or tool schemas.** Identity and switch
//!   only — the schema read face is `connector/get` (`24` §2/§5.3).
//! - **Runtime-internal ids.** `runtime_binding` states the binding instead.
//!
//! The adapter never touches the filesystem and never returns an absolute path.

use std::sync::Arc;

use async_trait::async_trait;

use nomifun_api_types::{
    APP_SERVER_EXPERT_PACK_FORMAT, AppServerExpertConnectorRef, AppServerExpertModel,
    AppServerExpertModelRef, AppServerExpertPack, AppServerExpertPackKind, AppServerExpertPersona,
    AppServerExpertProvenance, AppServerExpertRuntimeBinding, AppServerExpertSkillRef,
    AppServerExpertTeamPack, AppServerExpertToolPolicy, PresetOverrides, PresetTarget,
    ResolvedPresetSnapshot,
};
use nomifun_app_server::agent_store::ExpertExportPolicy;
use nomifun_app_server::{ExpertPackError, ExpertPackProvider, MAX_EXPERT_PACK_BYTES};
use nomifun_common::AppError;
use nomifun_db::{IPluginSnapshotRepository, PluginSnapshotComponentRow};

use crate::app_server_importer::{localized_field, payload, string_array, string_field};

/// The runtime engine every installed expert is pinned to on this host.
///
/// Reported rather than hidden: an expert is a Preset bound to *this* host's
/// runtime agent, which no other runtime has. That is what makes
/// `runtime_binding.portable` false — a fact a consumer should be able to *state*
/// instead of discover.
const EXPERT_RUNTIME: &str = "nomi";

/// Read seam over the Preset service, narrow enough to fake.
///
/// Mirrors `app_server_installer::PresetRegistrar` for the same reason:
/// `nomifun_preset::PresetService` is a concrete struct backed by a database, so
/// without a seam the export rules (which preset states are exportable, which
/// model resolves) could only be tested by booting a real service.
#[async_trait]
pub trait ExpertPresetReader: Send + Sync {
    /// Whether the Preset still exists **and** is switched on.
    ///
    /// `false` for a missing Preset as well as a disabled one; the caller
    /// distinguishes those by whether it had an id at all.
    async fn is_enabled(&self, preset_id: &str) -> Result<bool, AppError>;

    /// The frozen resolution, for the model hints and the revision stamp.
    async fn resolved(&self, preset_id: &str) -> Result<ResolvedPresetSnapshot, AppError>;
}

/// Production reader over `nomifun_preset::PresetService`.
pub struct AppServerExpertPresetReader {
    service: Arc<nomifun_preset::PresetService>,
}

impl AppServerExpertPresetReader {
    pub fn new(service: Arc<nomifun_preset::PresetService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl ExpertPresetReader for AppServerExpertPresetReader {
    async fn is_enabled(&self, preset_id: &str) -> Result<bool, AppError> {
        match self.service.get(preset_id).await {
            Ok(preset) => Ok(preset.enabled),
            // A hand-deleted Preset is "off", not "broken": the same reading
            // `install` takes when its recorded id no longer resolves.
            Err(AppError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn resolved(&self, preset_id: &str) -> Result<ResolvedPresetSnapshot, AppError> {
        // `ExecutionStep` rather than `Conversation`: this describes the
        // definition as a **participant** resolves it, which is exactly what
        // `team_run` does for every member. Default overrides on purpose — the
        // Definition's Skill and Connector bindings are applied at run time by
        // the caller, they are not part of the frozen definition.
        self.service
            .resolve(
                preset_id,
                PresetTarget::ExecutionStep,
                None,
                PresetOverrides::default(),
            )
            .await
    }
}

/// Composition-root expert export provider.
#[derive(Clone)]
pub struct AppServerExpertExport {
    repo: Arc<dyn IPluginSnapshotRepository>,
    presets: Arc<dyn ExpertPresetReader>,
    policy: ExpertExportPolicy,
}

impl AppServerExpertExport {
    pub fn new(
        repo: Arc<dyn IPluginSnapshotRepository>,
        presets: Arc<dyn ExpertPresetReader>,
        policy: ExpertExportPolicy,
    ) -> Self {
        Self { repo, presets, policy }
    }

    /// The host's gate, evaluated **before** any read.
    ///
    /// First for the same reason `connector/call` gates first: a refused request
    /// must not touch the database, and `policy_denied` must be
    /// distinguishable from `not_found`.
    fn admit(&self, id: &str) -> Result<(), ExpertPackError> {
        self.policy.decide(id).map_err(ExpertPackError::PolicyDenied)
    }

    async fn rows(&self, kind: &str) -> Result<Vec<PluginSnapshotComponentRow>, ExpertPackError> {
        self.repo
            .list_components_by_kind(kind)
            .await
            .map_err(|error| ExpertPackError::Internal(format!("list {kind} components: {error}")))
    }

    /// The snapshot's content digest, for provenance.
    ///
    /// Read best-effort: a missing snapshot row means the pack loses a
    /// reconciliation aid, which is not a reason to refuse a definition that is
    /// otherwise readable.
    async fn content_digest(&self, snapshot_id: &str) -> String {
        match self.repo.get_by_snapshot_id(snapshot_id).await {
            Ok(Some(row)) => row.content_digest,
            Ok(None) => String::new(),
            Err(error) => {
                tracing::warn!(
                    snapshot_id,
                    "expert export could not read the snapshot digest: {error}"
                );
                String::new()
            }
        }
    }

    /// One AgentDefinition row as a pack. Callers have already admitted the id.
    async fn agent_pack(
        &self,
        row: &PluginSnapshotComponentRow,
        content_digest: &str,
    ) -> Result<AppServerExpertPack, ExpertPackError> {
        let value = payload(row);
        let preset_id = recorded_preset_id(row).ok_or_else(|| {
            ExpertPackError::NotInstalled(format!(
                "expert {} is not installed; run install/* before exporting it",
                row.component_id
            ))
        })?;
        if !self
            .presets
            .is_enabled(preset_id)
            .await
            .map_err(|error| ExpertPackError::Internal(format!("preset lookup: {error}")))?
        {
            return Err(ExpertPackError::Disabled(format!(
                "expert {} is disabled; enable it before exporting it",
                row.component_id
            )));
        }
        let snapshot = self
            .presets
            .resolved(preset_id)
            .await
            .map_err(|error| ExpertPackError::Internal(format!("preset resolve: {error}")))?;

        let declared_model = string_field(&value, "model");
        let resolved_model = snapshot.resolved_model.as_ref().map(|model| AppServerExpertModelRef {
            provider_id: model.provider_id.clone(),
            model: model.model.clone(),
        });

        Ok(AppServerExpertPack {
            pack_format: APP_SERVER_EXPERT_PACK_FORMAT,
            kind: AppServerExpertPackKind::Agent,
            id: row.component_id.clone(),
            version: string_field(&value, "version").unwrap_or_default(),
            name: row.name.clone(),
            display_name: localized_field(&value, "display_name"),
            description: string_field(&value, "description"),
            persona: AppServerExpertPersona {
                instructions: string_field(&value, "instructions").unwrap_or_default(),
                memory: string_field(&value, "memory"),
                background: string_field(&value, "background"),
            },
            model: AppServerExpertModel {
                declared: declared_model,
                resolved: resolved_model,
                effort: string_field(&value, "effort"),
                max_turns: value
                    .get("max_turns")
                    .and_then(serde_json::Value::as_u64)
                    .map(|turns| turns as u32),
            },
            skills: skill_refs(&value),
            // Always empty for an agent, and faithfully so: this format has no
            // agent-level connector dependency (`02` §5.1 — a plugin-level
            // `mcpServers` is a plugin capability, explicitly not a per-agent
            // grant). Mapping one here would invent authority the import never
            // established.
            connectors: Vec::new(),
            tool_policy: AppServerExpertToolPolicy {
                tools: string_array(&value, "tools"),
                disallowed_tools: string_array(&value, "disallowed_tools"),
            },
            team: None,
            provenance: AppServerExpertProvenance {
                source: string_field(&value, "source").unwrap_or_else(|| "imported".into()),
                snapshot_id: row.snapshot_id.clone(),
                content_digest: content_digest.to_owned(),
                preset_id: Some(preset_id.to_owned()),
                preset_revision: Some(snapshot.preset_revision),
            },
            runtime_binding: AppServerExpertRuntimeBinding {
                runtime: EXPERT_RUNTIME.to_owned(),
                portable: false,
            },
        })
    }

    /// The Connectors this Team's own snapshot installed and left **enabled**.
    ///
    /// Read through `list_installation_state` (the same projection the installer
    /// writes, and the same source `team/get` uses) rather than the manifest, so
    /// a Connector that was uninstalled or switched off is not reported as a
    /// dependency. The member Agents' `mcpServers` are deliberately not consulted
    /// — see `AppServerTeamDetail::connectors`.
    async fn team_connectors(
        &self,
        snapshot_id: &str,
    ) -> Result<Vec<AppServerExpertConnectorRef>, ExpertPackError> {
        let rows = self
            .repo
            .list_installation_state(Some(snapshot_id))
            .await
            .map_err(|error| ExpertPackError::Internal(format!("installation state: {error}")))?;
        let mut connectors = Vec::new();
        for row in rows {
            if row.kind != "connector" || row.installed != 1 || row.disabled != 0 {
                continue;
            }
            let Some(server_id) = row
                .runtime_ref
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|value| {
                    value
                        .get("mcp_server_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
            else {
                tracing::warn!(
                    snapshot_id,
                    component_id = %row.component_id,
                    "installed connector component has no readable mcp_server_id; not exportable"
                );
                continue;
            };
            if connectors.iter().any(|existing: &AppServerExpertConnectorRef| existing.id == server_id) {
                continue;
            }
            connectors.push(AppServerExpertConnectorRef {
                id: server_id,
                // The snapshot component's name, which is what the curated
                // Definition called it. The registered MCP row may have been
                // renamed by hand since; the definition is the honest answer here.
                name: row.name.clone(),
                enabled: true,
            });
        }
        Ok(connectors)
    }

    /// Refuse a pack that serializes past the cap, naming both numbers.
    fn enforce_size(pack: &AppServerExpertPack) -> Result<(), ExpertPackError> {
        let size = serde_json::to_vec(pack).map(|bytes| bytes.len() as u64).unwrap_or(0);
        if size > MAX_EXPERT_PACK_BYTES {
            return Err(ExpertPackError::TooLarge {
                size,
                limit: MAX_EXPERT_PACK_BYTES,
            });
        }
        Ok(())
    }
}

#[async_trait]
impl ExpertPackProvider for AppServerExpertExport {
    async fn export_agent(&self, agent_id: &str) -> Result<AppServerExpertPack, ExpertPackError> {
        self.admit(agent_id)?;
        let row = self
            .rows("agent")
            .await?
            .into_iter()
            .find(|row| row.component_id == agent_id)
            .ok_or_else(|| ExpertPackError::NotFound(format!("agent {agent_id} not found")))?;
        let digest = self.content_digest(&row.snapshot_id).await;
        let pack = self.agent_pack(&row, &digest).await?;
        Self::enforce_size(&pack)?;
        Ok(pack)
    }

    async fn export_team(&self, team_id: &str) -> Result<AppServerExpertPack, ExpertPackError> {
        self.admit(team_id)?;
        let row = self
            .rows("team")
            .await?
            .into_iter()
            .find(|row| row.component_id == team_id)
            .ok_or_else(|| ExpertPackError::NotFound(format!("team {team_id} not found")))?;
        let value = payload(&row);
        let digest = self.content_digest(&row.snapshot_id).await;

        // Roster order and de-duplication mirror `team_run::resolve_team_members`
        // exactly: the leader first, then each declared member, and an Agent
        // listed twice occupies one slot. A pack whose order disagreed with the
        // engine's would make "who is the leader" a consumer-side guess.
        let lead_agent_id = string_field(&value, "lead_agent_id").unwrap_or_default();
        let declared_members = string_array(&value, "member_agent_ids");
        let mut order: Vec<String> = Vec::new();
        for id in std::iter::once(lead_agent_id.clone()).chain(declared_members.iter().cloned()) {
            if id.trim().is_empty() || order.contains(&id) {
                continue;
            }
            order.push(id);
        }

        let agents = self.rows("agent").await?;
        let mut members = Vec::with_capacity(order.len());
        for member_id in &order {
            // A member the operator denied cannot be omitted silently, and a
            // partial roster is exactly what doc `32` §6.4 refuses to emit.
            self.admit(member_id)?;
            let member_row = agents
                .iter()
                .find(|row| &row.component_id == member_id)
                .ok_or_else(|| {
                    ExpertPackError::NotFound(format!(
                        "team {team_id} lists member {member_id}, which is not in the catalog"
                    ))
                })?;
            members.push(self.agent_pack(member_row, &digest).await?);
        }

        let pack = AppServerExpertPack {
            pack_format: APP_SERVER_EXPERT_PACK_FORMAT,
            kind: AppServerExpertPackKind::Team,
            id: row.component_id.clone(),
            version: string_field(&value, "version").unwrap_or_default(),
            name: row.name.clone(),
            display_name: localized_field(&value, "display_name"),
            description: string_field(&value, "description"),
            // A team has no persona of its own — the members do. The team's own
            // Preset exists so `team/run` and `conversation/create` have
            // something resolvable; it is not part of the Definition, so nothing
            // is read from it here and `preset_revision` stays absent.
            persona: AppServerExpertPersona {
                instructions: string_field(&value, "instructions").unwrap_or_default(),
                memory: None,
                background: None,
            },
            model: AppServerExpertModel {
                declared: None,
                resolved: None,
                effort: None,
                max_turns: None,
            },
            skills: Vec::new(),
            connectors: self.team_connectors(&row.snapshot_id).await?,
            // A team declares no tool surface of its own: its members' policies
            // are what the runtime applies, and each member pack carries its own.
            tool_policy: AppServerExpertToolPolicy {
                tools: Vec::new(),
                disallowed_tools: Vec::new(),
            },
            team: Some(AppServerExpertTeamPack {
                lead_agent_id,
                member_agent_ids: declared_members,
                planner_policy: string_field(&value, "planner_policy")
                    .unwrap_or_else(|| "planned".into()),
                routing_constraints: string_array(&value, "routing_constraints"),
                workflow_limits: value
                    .get("workflow_limits")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
                team_runtime_capabilities: string_array(&value, "team_runtime_capabilities"),
                members,
            }),
            provenance: AppServerExpertProvenance {
                source: string_field(&value, "source").unwrap_or_else(|| "imported".into()),
                snapshot_id: row.snapshot_id.clone(),
                content_digest: digest,
                preset_id: recorded_preset_id(&row).map(str::to_owned),
                preset_revision: None,
            },
            runtime_binding: AppServerExpertRuntimeBinding {
                runtime: EXPERT_RUNTIME.to_owned(),
                portable: false,
            },
        };
        Self::enforce_size(&pack)?;
        Ok(pack)
    }
}

/// The Preset the installer recorded on this component, if any.
///
/// The recorded id is the only admissible evidence of installation — the same
/// rule `app_server_installer::recorded_preset_id` applies (adopting a Preset by
/// *name* would let two snapshots with the same display name rebind each other).
fn recorded_preset_id(row: &PluginSnapshotComponentRow) -> Option<&str> {
    row.preset_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// The Definition's declared Skills, by reference.
///
/// `id` and `name` are the same string because `skill/list` publishes the Skill
/// *name* as its id (`24` §2 records that asymmetry against `agent/list`, which
/// publishes component ids).
fn skill_refs(value: &serde_json::Value) -> Vec<AppServerExpertSkillRef> {
    string_array(value, "skills")
        .into_iter()
        .filter(|name| !name.trim().is_empty())
        .map(|name| AppServerExpertSkillRef {
            id: name.clone(),
            name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Adapter-level tests over a real in-memory SQLite snapshot repository.
    //!
    //! The protocol layer's tests (`nomifun-app-server`) pin the seam, the
    //! capability bit and the wire codes; these pin what the *adapter* reads:
    //! that the persona is the payload body verbatim, that an export is
    //! deterministic, that a roster expands the way `team/run` expands it, and
    //! that nothing outside the definition's identity reaches the pack.

    use super::*;
    use nomifun_app_server::agent_store::AgentStoreExpertExport;
    use nomifun_db::{ComponentRuntimeRef, NewPluginSnapshot, NewPluginSnapshotComponent};
    use std::collections::HashMap;

    /// The `plugin_snapshots.snapshot_id` CHECK requires a UUIDv7 *shape*
    /// (36 chars, lowercase, `7` version nibble), so this cannot be a friendly
    /// name — see `migrations/059_agent_store_imports.sql`.
    const SNAPSHOT: &str = "0190f5fe-7c00-7a00-8000-00000000de01";
    const DIGEST: &str = "digest-demo";
    const AGENT_ID: &str = "wb-demo-lead";
    const MEMBER_ID: &str = "wb-demo-qa";
    const TEAM_ID: &str = "wb-demo-team";
    const CONNECTOR_ID: &str = "wb-demo-github";
    const MCP_SERVER_ID: &str = "mcp-github-1";
    const LEAD_PRESET: &str = "preset-lead";
    const MEMBER_PRESET: &str = "preset-qa";
    /// Stands in for a connector's credential. The pack reports connector
    /// *identity* only (doc `24` §2/§5.3), and this constant is what keeps that
    /// claim honest: it is planted in the connector's own payload and must never
    /// turn up in an export.
    const SECRET_SENTINEL: &str = "SECRET-e7f1-must-not-leak";

    /// Preset reader stub. Anything absent from `enabled` reads as switched off,
    /// which is what a hand-deleted Preset looks like from the adapter's side.
    struct FakePresets {
        enabled: HashMap<String, bool>,
    }

    impl FakePresets {
        fn with(enabled: &[&str]) -> Self {
            Self {
                enabled: enabled.iter().map(|id| ((*id).to_owned(), true)).collect(),
            }
        }
    }

    #[async_trait]
    impl ExpertPresetReader for FakePresets {
        async fn is_enabled(&self, preset_id: &str) -> Result<bool, AppError> {
            Ok(self.enabled.get(preset_id).copied().unwrap_or(false))
        }

        async fn resolved(&self, preset_id: &str) -> Result<ResolvedPresetSnapshot, AppError> {
            Ok(ResolvedPresetSnapshot {
                preset_id: preset_id.to_owned(),
                preset_revision: 7,
                preset_name: format!("agent-store: {preset_id}"),
                target: PresetTarget::ExecutionStep,
                routing_description: None,
                // Deliberately different from the payload body: if the adapter
                // ever read the persona from the Preset instead of the snapshot,
                // the fidelity test would catch it.
                instructions: "PRESET COPY, NOT THE PERSONA".to_owned(),
                resolved_agent_id: Some("0190f5fe-7c00-7a00-8000-000000000114".to_owned()),
                resolved_agent_type: None,
                resolved_agent_backend: None,
                resolved_model: Some(nomifun_api_types::ModelPreference {
                    provider_id: Some("018f1234-5678-7abc-8def-012345678990".to_owned()),
                    model: "gpt-5".to_owned(),
                    required: true,
                }),
                reasoning_effort: None,
                included_skills: vec![],
                excluded_auto_skills: vec![],
                knowledge_policy: nomifun_api_types::PresetKnowledgePolicy::default(),
                knowledge_base_ids: vec![],
                mcp_server_ids: vec![],
                warnings: vec![],
            })
        }
    }

    async fn new_repo() -> Arc<dyn IPluginSnapshotRepository> {
        let db = nomifun_db::init_database_memory().await.expect("in-memory db");
        Arc::new(nomifun_db::SqlitePluginSnapshotRepository::new(db.pool().clone()))
    }

    /// One snapshot holding a team, its two members and a connector.
    ///
    /// The team lists the leader and a duplicate member on purpose: `team/run`
    /// de-duplicates and puts the leader first, and a pack whose order disagreed
    /// would make "who leads" a consumer-side guess.
    async fn seeded_repo(persona: &str) -> Arc<dyn IPluginSnapshotRepository> {
        let repo = new_repo().await;
        let lead = serde_json::json!({
            "id": AGENT_ID,
            "version": "1.0.0",
            "name": "Lead",
            "description": "lead",
            "skills": ["release-notes"],
            "tools": ["read_file"],
            "disallowed_tools": ["shell"],
            "model": "gpt-5-mini",
            "effort": "high",
            "max_turns": 40,
            "source": "codebuddy-plugin",
            "instructions": persona,
            "display_name": { "zh": "组长", "en": "Lead" },
            "memory": "keep notes",
            "avatar": "avatars/lead.png",
        })
        .to_string();
        let member = serde_json::json!({
            "id": MEMBER_ID,
            "version": "1.0.0",
            "name": "QA",
            "source": "codebuddy-plugin",
            "instructions": "qa persona",
        })
        .to_string();
        let team = serde_json::json!({
            "id": TEAM_ID,
            "version": "2.0.0",
            "name": "Demo team",
            "description": "demo",
            "source": "codebuddy-plugin",
            "lead_agent_id": AGENT_ID,
            "member_agent_ids": [MEMBER_ID, AGENT_ID, MEMBER_ID],
            "planner_policy": "planned",
            "routing_constraints": ["prefer_frontend"],
            "workflow_limits": { "max_parallel": 4 },
            "team_runtime_capabilities": ["planned_dag", "local_parallel"],
        })
        .to_string();
        let connector = serde_json::json!({
            "id": CONNECTOR_ID,
            "name": "github",
            "transport": {
                "type": "stdio",
                "command": "npx",
                "env": { "TOKEN": SECRET_SENTINEL },
            },
        })
        .to_string();

        let components = vec![
            NewPluginSnapshotComponent {
                component_id: AGENT_ID,
                kind: "agent",
                name: "Lead",
                relative_path: Some("agents/lead.md"),
                compatibility_json: "{}",
                payload_json: &lead,
            },
            NewPluginSnapshotComponent {
                component_id: MEMBER_ID,
                kind: "agent",
                name: "QA",
                relative_path: Some("agents/qa.md"),
                compatibility_json: "{}",
                payload_json: &member,
            },
            NewPluginSnapshotComponent {
                component_id: TEAM_ID,
                kind: "team",
                name: "Demo team",
                relative_path: Some("plugin.json"),
                compatibility_json: "{}",
                payload_json: &team,
            },
            NewPluginSnapshotComponent {
                component_id: CONNECTOR_ID,
                kind: "connector",
                name: "github",
                relative_path: Some("mcp.json"),
                compatibility_json: "{}",
                payload_json: &connector,
            },
        ];

        repo.insert_snapshot_with_components(NewPluginSnapshot {
            snapshot_id: SNAPSHOT,
            name: "demo",
            version: "1.0.0",
            source_kind: "codebuddy-plugin",
            source_uri: Some("internal/traceability/only"),
            plugin_id: "demo",
            declared_version: "1.0.0",
            resolved_revision: None,
            content_digest: DIGEST,
            status: "completed",
            marketplace_id: None,
            entry_name: None,
            source_revision: None,
            components,
        })
        .await
        .expect("insert snapshot");
        repo
    }

    /// Register components as installed, the way the installer does: an
    /// agent/team becomes a Preset (so `preset_id` is the recorded id), a
    /// connector becomes an MCP server row.
    async fn install(repo: &Arc<dyn IPluginSnapshotRepository>, refs: &[(&str, &str, &str)]) {
        let borrowed: Vec<ComponentRuntimeRef<'_>> = refs
            .iter()
            .map(|(component_id, runtime_type, location)| ComponentRuntimeRef {
                component_id: *component_id,
                runtime_type: *runtime_type,
                location: *location,
                mcp_server_id: (*runtime_type == "connector").then_some(*location),
            })
            .collect();
        repo.mark_components_installed(&borrowed, 1)
            .await
            .expect("mark installed");
    }

    fn exporter(
        repo: Arc<dyn IPluginSnapshotRepository>,
        presets: FakePresets,
        policy: ExpertExportPolicy,
    ) -> AppServerExpertExport {
        AppServerExpertExport::new(repo, Arc::new(presets), policy)
    }

    /// The standard fixture: everything installed and enabled.
    async fn ready(persona: &str) -> AppServerExpertExport {
        let repo = seeded_repo(persona).await;
        install(
            &repo,
            &[
                (AGENT_ID, "preset", LEAD_PRESET),
                (MEMBER_ID, "preset", MEMBER_PRESET),
                (TEAM_ID, "preset", "preset-team"),
                (CONNECTOR_ID, "connector", MCP_SERVER_ID),
            ],
        )
        .await;
        exporter(
            repo,
            FakePresets::with(&[LEAD_PRESET, MEMBER_PRESET, "preset-team"]),
            ExpertExportPolicy::allow_all(),
        )
    }

    #[tokio::test]
    async fn persona_is_the_payload_body_verbatim_even_when_long() {
        // `skill/get` summarises at ~1200 characters. The persona must not
        // inherit that habit, and the Preset's own `instructions` is a different
        // string on purpose — so reading the wrong source fails here.
        let persona = format!("PERSONA-START {}", "x".repeat(1300));
        let provider = ready(&persona).await;
        let pack = provider.export_agent(AGENT_ID).await.expect("export");

        assert_eq!(pack.persona.instructions, persona);
        assert!(pack.persona.instructions.len() > 1200);
        assert_eq!(pack.persona.memory.as_deref(), Some("keep notes"));
        assert_eq!(pack.pack_format, APP_SERVER_EXPERT_PACK_FORMAT);
        assert_eq!(pack.kind, AppServerExpertPackKind::Agent);
        assert_eq!(pack.version, "1.0.0");
        assert_eq!(pack.model.declared.as_deref(), Some("gpt-5-mini"));
        assert_eq!(pack.model.resolved.as_ref().unwrap().model, "gpt-5");
        assert_eq!(pack.model.effort.as_deref(), Some("high"));
        assert_eq!(pack.model.max_turns, Some(40));
        assert_eq!(pack.tool_policy.disallowed_tools, vec!["shell".to_owned()]);
        assert_eq!(pack.provenance.content_digest, DIGEST);
        assert_eq!(pack.provenance.preset_revision, Some(7));
        assert_eq!(pack.provenance.preset_id.as_deref(), Some(LEAD_PRESET));
        assert_eq!(pack.runtime_binding.runtime, "nomi");
        assert!(!pack.runtime_binding.portable);
        // Skills ride as references with id == name.
        assert_eq!(pack.skills.len(), 1);
        assert_eq!(pack.skills[0].name, "release-notes");
        assert_eq!(pack.skills[0].id, "release-notes");
        // An agent has no connector surface, and that is faithful rather than a
        // gap: this format has no agent-level connector dependency (02 §5.1).
        assert!(pack.connectors.is_empty());
        assert!(pack.team.is_none());
    }

    #[tokio::test]
    async fn export_is_byte_identical_across_calls() {
        // No timestamp, and every list ordered: a consumer keys a cache on
        // `content_digest` and diffs two exports to see what changed upstream.
        let provider = ready("stable persona").await;
        let first = serde_json::to_string(&provider.export_agent(AGENT_ID).await.unwrap()).unwrap();
        let second = serde_json::to_string(&provider.export_agent(AGENT_ID).await.unwrap()).unwrap();
        assert_eq!(first, second);

        let team_first = serde_json::to_string(&provider.export_team(TEAM_ID).await.unwrap()).unwrap();
        let team_second = serde_json::to_string(&provider.export_team(TEAM_ID).await.unwrap()).unwrap();
        assert_eq!(team_first, team_second);
    }

    #[tokio::test]
    async fn the_roster_expands_leader_first_and_without_duplicates() {
        let provider = ready("persona").await;
        let pack = provider.export_team(TEAM_ID).await.expect("export team");
        let team = pack.team.as_ref().expect("team section");

        assert_eq!(pack.kind, AppServerExpertPackKind::Team);
        // The declared list is reported verbatim; the expansion is what is
        // de-duplicated. `team/run` does the same, so a consumer reading
        // `members` sees exactly the participants the engine would build.
        assert_eq!(
            team.member_agent_ids,
            vec![MEMBER_ID.to_owned(), AGENT_ID.to_owned(), MEMBER_ID.to_owned()]
        );
        let ids: Vec<&str> = team.members.iter().map(|member| member.id.as_str()).collect();
        assert_eq!(ids, vec![AGENT_ID, MEMBER_ID], "leader first, duplicate dropped");
        assert_eq!(team.routing_constraints, vec!["prefer_frontend".to_owned()]);
        assert_eq!(team.workflow_limits["max_parallel"], 4);
        // The team's own Preset is not part of the definition: no revision stamp,
        // and no model — the members carry theirs.
        assert_eq!(pack.provenance.preset_revision, None);
        assert!(pack.model.resolved.is_none());
        assert!(pack.skills.is_empty());
        assert_eq!(pack.provenance.preset_id.as_deref(), Some("preset-team"));
    }

    #[tokio::test]
    async fn a_team_with_an_uninstalled_member_fails_as_a_whole_and_names_it() {
        let repo = seeded_repo("persona").await;
        // The leader is installed, the member is not.
        install(&repo, &[(AGENT_ID, "preset", LEAD_PRESET)]).await;
        let provider = exporter(
            repo,
            FakePresets::with(&[LEAD_PRESET]),
            ExpertExportPolicy::allow_all(),
        );

        let error = provider.export_team(TEAM_ID).await.expect_err("partial roster");
        match error {
            ExpertPackError::NotInstalled(message) => {
                assert!(
                    message.contains(MEMBER_ID),
                    "a ten-member roster must not answer `some member is missing`: {message}"
                );
            }
            other => panic!("expected NotInstalled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn installation_state_decides_not_installed_and_disabled() {
        let repo = seeded_repo("persona").await;
        // Never installed: there is no Preset, so there is nothing runnable to
        // describe.
        let untouched = exporter(
            repo.clone(),
            FakePresets::with(&[LEAD_PRESET]),
            ExpertExportPolicy::allow_all(),
        );
        assert!(matches!(
            untouched.export_agent(AGENT_ID).await,
            Err(ExpertPackError::NotInstalled(_))
        ));

        install(&repo, &[(AGENT_ID, "preset", LEAD_PRESET)]).await;

        // Which signal means "switched off"? The **Preset**, not the component
        // row's `disabled` column. `install/disable` writes both (`set_preset_enabled`
        // plus `set_components_disabled`, `app_server_installer.rs:353-358` and
        // `:956-963`), so in practice they agree — but only the Preset is what the
        // engine honours: every run path resolves through `PresetService`, which
        // refuses a disabled Preset. Reading the row here would accept an expert
        // `agent/run` would reject, so the adapter reads what the run path reads
        // (`team_run.rs:192-207` does the same).
        repo.set_components_disabled(&[AGENT_ID], true)
            .await
            .expect("disable the row");
        let row_flag_only = exporter(
            repo.clone(),
            FakePresets::with(&[LEAD_PRESET]),
            ExpertExportPolicy::allow_all(),
        );
        assert!(
            row_flag_only.export_agent(AGENT_ID).await.is_ok(),
            "the row's flag is bookkeeping; the Preset is what the engine resolves"
        );

        // With the Preset off — the state `install/disable` actually produces —
        // the export refuses and says which of the two it is.
        let preset_off = exporter(repo, FakePresets::with(&[]), ExpertExportPolicy::allow_all());
        assert!(matches!(
            preset_off.export_agent(AGENT_ID).await,
            Err(ExpertPackError::Disabled(_))
        ));
    }

    #[tokio::test]
    async fn a_switched_off_preset_reports_disabled_not_missing() {
        // Installed, but the Preset itself is off (which is what `install/disable`
        // does, and what a hand-edit can do). "You turned this off" and "there is
        // no such thing" must not collapse into one answer.
        let repo = seeded_repo("persona").await;
        install(&repo, &[(AGENT_ID, "preset", LEAD_PRESET)]).await;
        // `FakePresets::with(&[])` = the reader sees it as switched off.
        let provider = exporter(repo, FakePresets::with(&[]), ExpertExportPolicy::allow_all());
        assert!(matches!(
            provider.export_agent(AGENT_ID).await,
            Err(ExpertPackError::Disabled(_))
        ));
    }

    #[tokio::test]
    async fn the_gate_runs_before_any_read() {
        // An EMPTY repository plus a denied id: reaching the database first would
        // answer `not_found`, so `policy_denied` here proves the gate is first.
        let repo = new_repo().await;
        let denied = ExpertExportPolicy::from_declared(&AgentStoreExpertExport {
            enabled: None,
            deny: Some(vec![AGENT_ID.to_owned()]),
        });
        let provider = exporter(repo.clone(), FakePresets::with(&[]), denied);
        assert!(matches!(
            provider.export_agent(AGENT_ID).await,
            Err(ExpertPackError::PolicyDenied(_))
        ));

        // The whole face can be switched off with one key.
        let off = ExpertExportPolicy::from_declared(&AgentStoreExpertExport {
            enabled: Some(false),
            deny: None,
        });
        let provider = exporter(repo, FakePresets::with(&[]), off);
        assert!(is_policy_denied(provider.export_team(TEAM_ID).await));
    }

    fn is_policy_denied(result: Result<AppServerExpertPack, ExpertPackError>) -> bool {
        matches!(result, Err(ExpertPackError::PolicyDenied(_)))
    }

    #[tokio::test]
    async fn an_unknown_id_is_not_found_and_the_message_names_it() {
        let provider = ready("persona").await;
        match provider.export_agent("wb-nope").await {
            Err(ExpertPackError::NotFound(message)) => assert!(message.contains("wb-nope")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn connectors_come_from_the_install_state_not_the_manifest() {
        let provider = ready("persona").await;
        let pack = provider.export_team(TEAM_ID).await.expect("export");
        let connectors = &pack.connectors;
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].id, MCP_SERVER_ID);
        assert_eq!(connectors[0].name, "github");
        assert!(connectors[0].enabled);

        // Switched off after install: no longer a dependency, and not silently
        // reported as one.
        let repo = seeded_repo("persona").await;
        install(
            &repo,
            &[
                (AGENT_ID, "preset", LEAD_PRESET),
                (MEMBER_ID, "preset", MEMBER_PRESET),
                (TEAM_ID, "preset", "preset-team"),
                (CONNECTOR_ID, "connector", MCP_SERVER_ID),
            ],
        )
        .await;
        repo.set_components_disabled(&[CONNECTOR_ID], true)
            .await
            .expect("disable connector");
        let provider = exporter(
            repo,
            FakePresets::with(&[LEAD_PRESET, MEMBER_PRESET, "preset-team"]),
            ExpertExportPolicy::allow_all(),
        );
        let pack = provider.export_team(TEAM_ID).await.expect("export");
        assert!(pack.connectors.is_empty());
    }

    #[tokio::test]
    async fn a_pack_carries_no_connector_credential_and_no_absolute_path() {
        // The connector's own payload holds a stdio `command` and an env token.
        // The pack reports identity only (doc `24` §2/§5.3), and the adversarial
        // part of this test is that the sentinel is planted, not assumed absent.
        let provider = ready("persona").await;
        for pack in [
            provider.export_agent(AGENT_ID).await.unwrap(),
            provider.export_team(TEAM_ID).await.unwrap(),
        ] {
            let json = serde_json::to_string(&pack).unwrap();
            assert!(!json.contains(SECRET_SENTINEL), "credential value leaked");
            assert!(!json.contains("npx"), "transport command leaked");
            assert!(!json.contains("\"env\""), "connector env leaked");
            assert!(!json.contains("internal/traceability"), "source_uri leaked");
            assert!(!json.contains("avatars/lead.png"), "asset path leaked");
        }
    }
}
