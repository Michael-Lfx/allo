//! Import orchestration (docs/agent-store/02 §2 flow, §11 result contract).
//!
//! `ImporterService::run_import` performs: locate source → parse manifest →
//! path validation → copy to the versioned immutable cache + digest →
//! component standardization → idempotency/digest-conflict decision →
//! catalog registration. It never executes anything from the source.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomifun_api_types::{
    AppServerCompatibilityTriple, AppServerImportComponent, AppServerImportResult,
};
use nomifun_common::now_ms;
use nomifun_db::{
    IPluginSnapshotRepository, NewPluginSnapshot, NewPluginSnapshotComponent,
};
use serde_json::json;

use crate::compat;
use crate::digest::tree_digest;
use crate::frontmatter::{parse_agent, parse_skill, AgentDoc};
use crate::manifest::{PluginManifest, validate_relative_path};
use crate::models::{
    Component, CompatTriple, SourceKind, component_id, sanitize_slug,
};
use crate::registry::{IdempotencyDecision, decide};
use crate::walk::copy_tree;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("import source does not exist or is not a directory")]
    SourceNotFound,
    #[error("import failed: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub source_path: PathBuf,
    pub source_kind: SourceKind,
    /// Marketplace provenance (roadmap Phase 2): set when the import is the
    /// materialization of a marketplace entry.
    pub marketplace_id: Option<String>,
    pub entry_name: Option<String>,
    /// Source revision (git commit / HTTP marker) that produced this snapshot;
    /// internal traceability only.
    pub source_revision: Option<String>,
}

impl ImportRequest {
    /// Manual import (no marketplace provenance).
    pub fn manual(source_path: PathBuf, source_kind: SourceKind) -> Self {
        Self {
            source_path,
            source_kind,
            marketplace_id: None,
            entry_name: None,
            source_revision: None,
        }
    }

    /// Marketplace-entry import with provenance linkage.
    pub fn from_marketplace(
        source_path: PathBuf,
        source_kind: SourceKind,
        marketplace_id: String,
        entry_name: String,
    ) -> Self {
        Self {
            source_path,
            source_kind,
            marketplace_id: Some(marketplace_id),
            entry_name: Some(entry_name),
            source_revision: None,
        }
    }

    /// Marketplace-entry import with provenance + source revision.
    pub fn from_marketplace_revision(
        source_path: PathBuf,
        source_kind: SourceKind,
        marketplace_id: String,
        entry_name: String,
        source_revision: String,
    ) -> Self {
        Self {
            source_path,
            source_kind,
            marketplace_id: Some(marketplace_id),
            entry_name: Some(entry_name),
            source_revision: Some(source_revision),
        }
    }
}

/// Importer entry point. `snapshot_root` is the versioned immutable cache
/// root (e.g. `{work_dir}/agent-store-imports/`).
#[derive(Clone)]
pub struct ImporterService {
    snapshot_root: PathBuf,
    repo: Arc<dyn IPluginSnapshotRepository>,
}

impl ImporterService {
    pub fn new(snapshot_root: PathBuf, repo: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { snapshot_root, repo }
    }

    /// The immutable snapshot cache root (`{work_dir}/agent-store-imports/`).
    pub fn snapshot_root(&self) -> &std::path::Path {
        &self.snapshot_root
    }

    pub async fn run_import(
        &self,
        request: &ImportRequest,
    ) -> Result<AppServerImportResult, ImportError> {
        // 1. locate the source (02 §2 step 1)
        let source = request
            .source_path
            .canonicalize()
            .map_err(|_| ImportError::SourceNotFound)?;
        if !source.is_dir() {
            return Err(ImportError::SourceNotFound);
        }

        // 2. parse the manifest (step 2); identity problems block (02 §11.1)
        let parsed = match crate::manifest::parse_manifest(&source, request.source_kind) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(blocked_result(
                    request.source_kind,
                    None,
                    &[format!("清单解析失败：{error}")],
                ));
            }
        };

        // 3+4. copy to the immutable cache + digest (steps 3–5)
        let snapshot_id = nomifun_common::generate_id();
        let materialized = self.snapshot_root.join(&snapshot_id);
        std::fs::create_dir_all(&materialized).map_err(|error| {
            ImportError::Internal(format!("create snapshot dir: {error}"))
        })?;
        let files = match copy_tree(&source, &materialized) {
            Ok(files) => files,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&materialized);
                return Ok(blocked_result(
                    request.source_kind,
                    Some((parsed.name().to_owned(), parsed.version().to_owned())),
                    &[error.to_string()],
                ));
            }
        };
        let content_digest = tree_digest(&files);

        // 5–7. component standardization (steps 6–8)
        let plugin_id = match request.source_kind {
            SourceKind::CodeBuddyPlugin => sanitize_slug(parsed.name()),
            _ => format!("market-{}", sanitize_slug(parsed.name())),
        };
        let meta = crate::models::SnapshotMeta {
            plugin_id: plugin_id.clone(),
            name: parsed.name().to_owned(),
            declared_version: parsed.version().to_owned(),
            resolved_revision: None,
            source_kind: request.source_kind,
            source_uri: source.display().to_string(), // internal only
        };
        let mut builder = ComponentBuilder::new(parsed.version().to_owned());
        let mut agents: Vec<(String, AgentDoc)> = Vec::new();
        match (&parsed, request.source_kind) {
            (crate::manifest::ParsedManifest::Plugin(manifest), SourceKind::CodeBuddyPlugin) => {
                build_plugin_components(&source, manifest, &meta, &mut builder, &mut agents);
            }
            (_, SourceKind::WorkBuddySkillMarket) => {
                match &parsed {
                    crate::manifest::ParsedManifest::SingleSkill(dir) => {
                        build_single_skill_components(&source, dir, &meta, &mut builder);
                    }
                    _ => build_skill_market_components(&source, &meta, &mut builder),
                }
            }
            (crate::manifest::ParsedManifest::Market(market), SourceKind::WorkBuddyConnectorMarket) => {
                build_connector_market_components(&market.connectors, &meta, &mut builder);
            }
            (
                crate::manifest::ParsedManifest::Cli(cli, directory_name),
                SourceKind::WorkBuddyCliConnector,
            ) => {
                build_cli_connector_components(&source, cli, directory_name, &meta, &mut builder);
            }
            // Unreachable: market manifests never parse as plugins and vice
            // versa — parse_manifest dispatches by source kind.
            _ => {}
        }
        // Path-safety violations anywhere block the whole snapshot (02 §11.1).
        if !builder.blocking_errors.is_empty() {
            let _ = std::fs::remove_dir_all(&materialized);
            return Ok(blocked_result(
                request.source_kind,
                Some((meta.name.clone(), meta.declared_version.clone())),
                &builder.blocking_errors,
            ));
        }

        // 8. registry decision + persist (step 9, 02 §9 / §11.1)
        let exact = self
            .repo
            .find_by_identity_digest(&plugin_id, &meta.declared_version, &content_digest)
            .await
            .map_err(|error| ImportError::Internal(format!("catalog lookup: {error}")))?;
        let same_identity = self
            .repo
            .list_by_identity(&plugin_id, &meta.declared_version)
            .await
            .map_err(|error| ImportError::Internal(format!("catalog lookup: {error}")))?;
        match decide(exact, &same_identity) {
            IdempotencyDecision::Reuse(existing) => {
                let _ = std::fs::remove_dir_all(&materialized);
                let stored_components = self
                    .repo
                    .get_components(&existing.snapshot_id)
                    .await
                    .map_err(|error| ImportError::Internal(format!("catalog read: {error}")))?;
                return Ok(AppServerImportResult {
                    snapshot_id: existing.snapshot_id,
                    name: existing.name,
                    version: existing.version,
                    source_kind: existing.source_kind,
                    status: "completed".into(),
                    content_digest: existing.content_digest,
                    component_status: AppServerCompatibilityTriple {
                        semantic_status: "compatible_with_adapter".into(),
                        runtime_status: crate::models::RUNTIME_NOT_VERIFIED.into(),
                        distribution_status: crate::models::DIST_LOCAL_ONLY.into(),
                        reasons: vec!["相同 digest 重复导入，复用既有不可变快照（02 §9）".into()],
                    },
                    component_count: stored_components.len(),
                    imported_at: existing.imported_at,
                    reused: true,
                    warnings: vec![],
                    errors: vec![],
                });
            }
            IdempotencyDecision::Conflict(existing) => {
                let _ = std::fs::remove_dir_all(&materialized);
                return Ok(blocked_result(
                    request.source_kind,
                    Some((meta.name.clone(), meta.declared_version.clone())),
                    &[format!(
                        "digest 冲突：同一身份与版本（{}@{}) 已存在不同内容（snapshot_id={}），不得覆盖既有不可变快照",
                        existing.plugin_id, existing.declared_version, existing.snapshot_id
                    )],
                ));
            }
            IdempotencyDecision::New => {}
        }

        let status = if builder.errors.is_empty() && builder.warnings.is_empty() {
            "completed"
        } else {
            "completed-with-warnings"
        };
        let payload_jsons = builder
            .components
            .iter()
            .map(|component| {
                serde_json::to_string(&component.payload)
                    .map_err(|error| ImportError::Internal(format!("encode component: {error}")))
            })
            .collect::<Result<Vec<String>, ImportError>>()?;
        let compat_jsons: Vec<String> =
            builder.components.iter().map(|component| component.compatibility.to_json()).collect();
        let components: Vec<NewPluginSnapshotComponent<'_>> = builder
            .components
            .iter()
            .zip(payload_jsons.iter())
            .zip(compat_jsons.iter())
            .map(|((component, payload_json), compatibility_json)| NewPluginSnapshotComponent {
                component_id: &component.component_id,
                kind: &component.kind,
                name: &component.name,
                relative_path: component.relative_path.as_deref(),
                compatibility_json,
                payload_json,
            })
            .collect();
        let row = self
            .repo
            .insert_snapshot_with_components(NewPluginSnapshot {
                snapshot_id: &snapshot_id,
                name: &meta.name,
                version: &meta.declared_version,
                source_kind: request.source_kind.as_str(),
                source_uri: Some(&meta.source_uri),
                plugin_id: &plugin_id,
                declared_version: &meta.declared_version,
                resolved_revision: None,
                content_digest: &content_digest,
                status,
                marketplace_id: request.marketplace_id.as_deref(),
                entry_name: request.entry_name.as_deref(),
                source_revision: request.source_revision.as_deref(),
                components,
            })
            .await
            .map_err(|error| ImportError::Internal(format!("catalog write: {error}")))?;

        builder.warnings.dedup();
        builder.errors.dedup();
        Ok(AppServerImportResult {
            snapshot_id: row.snapshot_id,
            name: row.name,
            version: row.version,
            source_kind: row.source_kind,
            status: row.status,
            content_digest: row.content_digest,
            component_status: to_public_triple(&compat::snapshot_aggregate(&builder.components)),
            component_count: builder.components.len(),
            imported_at: row.imported_at,
            reused: false,
            warnings: builder.warnings,
            errors: builder.errors,
        })
    }
}

// ---------------------------------------------------------------------------
// component building
// ---------------------------------------------------------------------------

struct ComponentBuilder {
    version: String,
    components: Vec<Component>,
    /// snapshot-relative paths already claimed (component id uniqueness)
    claimed: HashSet<String>,
    warnings: Vec<String>,
    errors: Vec<String>,
    /// Identity/path-safety violations that block the whole snapshot.
    blocking_errors: Vec<String>,
}

impl ComponentBuilder {
    fn new(version: String) -> Self {
        Self {
            version,
            components: Vec::new(),
            claimed: HashSet::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            blocking_errors: Vec::new(),
        }
    }

    fn push(&mut self, component: Component) {
        // Agent and Skill definitions share one component-id namespace per
        // plugin. Real expert plugins frequently name both the agent and its
        // companion skill after the plugin (`aihot` + `skills/aihot`), which
        // collides on `wb-<plugin>-aihot`. Skills yield to the agent: on a
        // collision the skill gets a `-skill` suffix so nothing is lost.
        if self.claimed.contains(&component.component_id) {
            let original_id = component.component_id.clone();
            let original_name = component.name.clone();
            if component.kind == crate::models::KIND_SKILL {
                let mut disambiguated = component;
                disambiguated.component_id = format!("{original_id}-skill");
                if !self.claimed.contains(&disambiguated.component_id) {
                    self.warn(format!(
                        "组件 id 冲突，技能已加后缀消歧：{}（{}）",
                        disambiguated.component_id, disambiguated.name
                    ));
                    self.claimed.insert(disambiguated.component_id.clone());
                    self.components.push(disambiguated);
                    return;
                }
                self.errors.push(format!(
                    "组件 id 冲突，已跳过：{original_id}（{original_name}）"
                ));
                return;
            }
            self.errors.push(format!(
                "组件 id 冲突，已跳过：{original_id}（{original_name}）"
            ));
            return;
        }
        self.claimed.insert(component.component_id.clone());
        self.components.push(component);
    }

    fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    fn error(&mut self, message: String) {
        self.errors.push(message);
    }

    /// Path-safety violations block the whole snapshot (02 §11.1).
    fn block(&mut self, message: String) {
        self.blocking_errors.push(message);
    }
}

/// Attach plugin-level display metadata (`plugin.json`) to an agent payload.
/// The market card is the primary display surface, so plugin manifest values
/// WIN over agent-frontmatter values (`plugin.json` is the market's source of
/// truth for display name/profession; the frontmatter only carries the agent
/// definition's own presentation when no manifest field exists).
fn attach_plugin_display(payload: &mut serde_json::Value, manifest: &PluginManifest) {
    if let Some(text) = &manifest.display_name {
        payload["display_name"] = serde_json::to_value(text).unwrap_or_default();
    }
    if let Some(text) = &manifest.profession {
        payload["profession"] = serde_json::to_value(text).unwrap_or_default();
    }
    if let Some(text) = &manifest.display_description {
        payload["display_description"] = serde_json::to_value(text).unwrap_or_default();
    }
    if let Some(prompt) = &manifest.default_init_prompt {
        payload["default_init_prompt"] = serde_json::to_value(prompt).unwrap_or_default();
    }
    if !manifest.quick_prompts.is_empty() {
        payload["quick_prompts"] = serde_json::to_value(&manifest.quick_prompts).unwrap_or_default();
    }
    if !manifest.tags.is_empty() {
        payload["tags"] = serde_json::to_value(&manifest.tags).unwrap_or_default();
    }
    if let Some(avatar) = &manifest.avatar {
        payload["avatar"] = json!(avatar);
    }
    if let Some(expert_type) = &manifest.expert_type {
        payload["expert_type"] = json!(expert_type);
    }
    if let Some(category_id) = &manifest.category_id {
        payload["category_id"] = json!(category_id);
    }
}

fn build_plugin_components(
    source: &Path,
    manifest: &PluginManifest,
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
    agents_out: &mut Vec<(String, AgentDoc)>,
) {
    // --- agents (02 §5.1) ---
    let agent_dirs = component_dirs(manifest.agents.as_slice(), "agents", source, builder);
    for dir in agent_dirs {
        let root = source.join(&dir);
        let scanned: Vec<(String, String)> = if root.is_file() {
            // Explicit file declaration (`./agents/lead.md`).
            let rel = dir.replace('\\', "/");
            read_optional(&root, &rel, builder)
                .map(|text| vec![(rel, text)])
                .unwrap_or_default()
        } else {
            scan_markdown(source, &dir, builder)
        };
        for (rel, text) in scanned {
            let stem = rel
                .rsplit('/')
                .next()
                .map(|name| name.strip_suffix(".md").unwrap_or(name).to_owned())
                .unwrap_or_default();
            match parse_agent(&text, &rel) {
                Ok(doc) => {
                    let id = component_id(&meta.plugin_id, &stem);
                    let mut payload = doc.to_payload(&id, &builder.version, &rel);
                    payload["source"] = json!(meta.source_kind.as_str());
                    // Plugin-level display metadata: the agent frontmatter
                    // may carry its own displayName/profession; the plugin
                    // manifest fields are the source of truth for the market
                    // card, so they attach whenever present.
                    attach_plugin_display(&mut payload, manifest);
                    let component = Component::new(
                        crate::models::KIND_AGENT,
                        id.clone(),
                        doc.name.clone(),
                        Some(rel.clone()),
                        compat::agent(doc.has_ignored_permission_fields()),
                        payload,
                    );
                    builder.push(component);
                    agents_out.push((stem, doc));
                }
                Err(error) => builder.error(format!("代理文件解析失败 {rel}: {error}")),
            }
        }
    }

    // --- skills (01 §6, 02 §5) ---
    let skill_dirs = component_dirs(manifest.skills.as_slice(), "skills", source, builder);
    for dir in skill_dirs {
        let root = source.join(&dir);
        if root.is_file() {
            // Explicit single-file declaration (`./skills/foo/SKILL.md`).
            let rel = dir.replace('\\', "/");
            let version = builder.version.clone();
            if let Some(text) = read_optional(&root, &rel, builder) {
                skill_from_text(&text, &rel, &meta.plugin_id, &version, builder);
            }
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&root)
            .min_depth(1)
            .max_depth(2)
            .sort_by_file_name()
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
                continue;
            }
            let abs = entry.path();
            let rel = abs
                .strip_prefix(source)
                .ok()
                .and_then(|path| path.to_str())
                .map(|path| path.replace('\\', "/"))
                .unwrap_or_default();
            match std::fs::read_to_string(abs) {
                Ok(text) => {
                    let version = builder.version.clone();
                    skill_from_text(&text, &rel, &meta.plugin_id, &version, builder);
                }
                Err(error) => builder.warn(format!("技能文件不可读 {rel}: {error}")),
            }
        }
    }

    // --- commands (02 §5) ---
    let command_dirs = component_dirs(manifest.commands.as_slice(), "commands", source, builder);
    for dir in command_dirs {
        let root = source.join(&dir);
        let scanned: Vec<(String, String)> = if root.is_file() {
            // Explicit file declaration (`./commands/triage.md`).
            let rel = dir.replace('\\', "/");
            read_optional(&root, &rel, builder)
                .map(|text| vec![(rel, text)])
                .unwrap_or_default()
        } else {
            scan_markdown(source, &dir, builder)
        };
        for (rel, _text) in scanned {
            let stem = rel
                .rsplit('/')
                .next()
                .map(|name| name.strip_suffix(".md").unwrap_or(name).to_owned())
                .unwrap_or_default();
            let id = component_id(&meta.plugin_id, &format!("cmd-{stem}"));
            builder.push(Component::new(
                crate::models::KIND_COMMAND,
                id.clone(),
                stem.clone(),
                Some(rel.clone()),
                compat::command(),
                json!({ "id": id, "version": builder.version, "name": stem, "relative_path": rel }),
            ));
        }
    }

    // --- plugin-level MCP (02 §5 .mcp.json) ---
    let mcp_value = manifest
        .mcp_servers
        .clone()
        .or_else(|| read_json_file(source.join(".mcp.json"), ".mcp.json", builder));
    if let Some(value) = mcp_value {
        build_connector_components(value, ".mcp.json", meta, builder);
    }

    // --- hooks (static import only) ---
    let hooks_value = manifest.hooks.clone().or_else(|| {
        read_json_file(source.join("hooks").join("hooks.json"), "hooks/hooks.json", builder)
            .or_else(|| read_json_file(source.join("hooks.json"), "hooks.json", builder))
    });
    if let Some(value) = hooks_value {
        let id = component_id(&meta.plugin_id, "hooks");
        let events = value
            .as_object()
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        builder.push(Component::new(
            crate::models::KIND_HOOK,
            id.clone(),
            "hooks".into(),
            Some("hooks.json".into()),
            compat::hook(),
            json!({ "id": id, "version": builder.version, "name": "hooks", "events": events, "static_only": true }),
        ));
    }

    // --- LSP (metadata-level) ---
    let lsp_value = manifest.lsp_servers.clone().or_else(|| {
        read_json_file(source.join(".lsp.json"), ".lsp.json", builder)
    });
    if let Some(value) = lsp_value {
        let names: Vec<String> = value
            .as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        let id = component_id(&meta.plugin_id, "lsp");
        builder.push(Component::new(
            crate::models::KIND_LSP,
            id.clone(),
            "lsp".into(),
            Some(".lsp.json".into()),
            compat::lsp(),
            json!({ "id": id, "version": builder.version, "name": "lsp", "servers": names }),
        ));
    }

    // --- userConfig → CredentialSchema (02 §5 / §10, values never imported) ---
    if let Some(value) = manifest.user_config.as_ref() {
        if let Some(fields) = value.as_object() {
            let mut schema_fields: Vec<serde_json::Value> = Vec::new();
            for (key, schema) in fields {
                let schema_type = schema
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("string")
                    .to_owned();
                let sensitive = is_sensitive_field(key, &schema_type, schema);
                if schema.get("default").is_some() || schema.get("value").is_some() {
                    builder.warn(format!(
                        "userConfig[{key}] 的值未导入（[REDACTED]），由用户后续经安全存储提供（02 §10）"
                    ));
                }
                schema_fields.push(json!({
                    "key": key,
                    "type": schema_type,
                    "required": schema.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                    "sensitive": sensitive,
                }));
            }
            if !schema_fields.is_empty() {
                let id = component_id(&meta.plugin_id, "credentials");
                builder.push(Component::new(
                    crate::models::KIND_CREDENTIAL,
                    id.clone(),
                    "userConfig".into(),
                    None,
                    compat::credential(),
                    json!({ "id": id, "version": builder.version, "name": "userConfig", "fields": schema_fields }),
                ));
            }
        }
    }

    // --- dependencies (02 §8) ---
    for (index, dependency) in manifest.dependencies.iter().enumerate() {
        let fallback = format!("dependency-{index}");
        let name = dependency
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(&fallback);
        let slug = sanitize_slug(name);
        let id = component_id(&meta.plugin_id, &format!("dep-{slug}"));
        builder.push(Component::new(
            crate::models::KIND_DEPENDENCY,
            id.clone(),
            name.to_owned(),
            None,
            compat::dependency(),
            json!({ "id": id, "version": builder.version, "name": name, "declared": dependency }),
        ));
    }

    // --- bin/ / scripts/: static import only (TC-IMP-008) ---
    for dir in ["bin", "scripts"] {
        let path = source.join(dir);
        if !path.is_dir() {
            continue;
        }
        let count = walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count();
        let id = component_id(&meta.plugin_id, dir);
        builder.push(Component::new(
            crate::models::KIND_SCRIPT,
            id.clone(),
            dir.to_owned(),
            Some(dir.to_owned()),
            compat::script(),
            json!({ "id": id, "version": builder.version, "name": dir, "file_count": count, "executable": false }),
        ));
    }

    // --- teamInfo → AgentTeamDefinition (02 §6) ---
    if let Some(team_info) = manifest.team_info.clone() {
        let mut by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (stem, doc) in agents_out.iter() {
            by_name.insert(stem.clone(), component_id(&meta.plugin_id, stem));
            by_name.insert(doc.name.clone(), component_id(&meta.plugin_id, stem));
        }
        let mut member_ids = Vec::new();
        let mut missing = Vec::new();
        for member in &team_info.member_agents {
            match by_name.get(member) {
                Some(id) => member_ids.push(id.clone()),
                None => missing.push(member.clone()),
            }
        }
        let lead_id = by_name
            .get(&team_info.lead_agent)
            .cloned()
            .unwrap_or_default();
        if lead_id.is_empty() {
            missing.push(team_info.lead_agent.clone());
        }
        let all_resolved = missing.is_empty();
        for name in &missing {
            builder.warn(format!("Team 成员文件缺失/不可解析：{name}（02 §6 step 3）"));
        }
        let id = component_id(&meta.plugin_id, "team");
        builder.push(Component::new(
            crate::models::KIND_TEAM,
            id.clone(),
            meta.name.clone(),
            None,
            compat::team(all_resolved),
            json!({
                "id": id,
                "version": builder.version,
                "name": meta.name,
                "description": manifest.description,
                "source": meta.source_kind.as_str(),
                "lead_agent_id": lead_id,
                "member_agent_ids": member_ids,
                "planner_policy": "planned",
                "adaptation_policy": "fixed",
                "plan_gate": "automatic",
                "coordination_policy": "leader_planned",
                "workflow_limits": { "max_parallel": 4 },
                "team_runtime_capabilities": [
                    "fixed_members", "planning_context", "planned_dag", "local_parallel",
                    "retry", "replan", "events", "artifacts",
                ],
            }),
        ));
    }
}

fn build_skill_market_components(
    source: &Path,
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
) {
    let root = source.join("skills");
    if !root.is_dir() {
        builder.warn("技能市场缺少 skills/ 目录".into());
        return;
    }
    for entry in walkdir::WalkDir::new(&root)
        .min_depth(1)
        .max_depth(1)
        .sort_by_file_name()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.is_file() {
            builder.warn(format!(
                "技能目录缺少 SKILL.md：{}",
                entry.file_name().to_string_lossy()
            ));
            continue;
        }
        let rel = format!(
            "skills/{}/SKILL.md",
            entry.file_name().to_string_lossy()
        );
        match std::fs::read_to_string(&skill_md) {
            Ok(text) => match parse_skill(&text, &rel) {
                Ok(doc) => {
                    let slug = entry.file_name().to_string_lossy().to_string();
                    let id = component_id(&meta.plugin_id, &slug);
                    builder.push(Component::new(
                        crate::models::KIND_SKILL,
                        id.clone(),
                        doc.name.clone(),
                        Some(rel.clone()),
                        compat::skill(),
                        json!({
                            "id": id,
                            "version": builder.version,
                            "name": doc.name,
                            "slug": slug,
                            "description": doc.description,
                            "mode": "store-agent",
                            "invocation_policy": "model-auto",
                            "instructions_ref": rel,
                            "relative_path": rel,
                            "has_arguments_note": doc.has_arguments_note,
                        }),
                    ));
                }
                Err(error) => builder.error(format!("技能文件解析失败 {rel}: {error}")),
            },
            Err(error) => builder.warn(format!("技能文件不可读 {rel}: {error}")),
        }
    }
}

/// Single skill directory (`skills/<slug>/`): the root holds the SKILL.md
/// directly; identity = directory name. Distinct from the market root, which
/// scans `skills/*/SKILL.md` one level deep.
fn build_single_skill_components(
    source: &Path,
    directory_name: &str,
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
) {
    let skill_md = source.join("SKILL.md");
    if !skill_md.is_file() {
        builder.warn("技能目录缺少 SKILL.md".into());
        return;
    }
    let rel = format!("{directory_name}/SKILL.md");
    match std::fs::read_to_string(&skill_md) {
        Ok(text) => {
            let slug = directory_name.to_owned();
            let id = component_id(&meta.plugin_id, &slug);
            match parse_skill(&text, &rel) {
                Ok(doc) => {
                    builder.push(Component::new(
                        crate::models::KIND_SKILL,
                        id.clone(),
                        doc.name.clone(),
                        Some(rel.clone()),
                        compat::skill(),
                        json!({
                            "id": id,
                            "version": builder.version,
                            "name": doc.name,
                            "slug": slug,
                            "description": doc.description,
                            "mode": "store-agent",
                            "invocation_policy": "model-auto",
                            "instructions_ref": rel,
                            "relative_path": rel,
                            "has_arguments_note": doc.has_arguments_note,
                        }),
                    ));
                }
                Err(error) => builder.error(format!("技能文件解析失败 {rel}: {error}")),
            }
        }
        Err(error) => builder.warn(format!("技能文件不可读 {rel}: {error}")),
    }
}

fn build_connector_market_components(
    entries: &[serde_json::Value],
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
) {
    for (index, entry) in entries.iter().enumerate() {
        let fallback = format!("connector-{index}");
        // Market index rows carry an ASCII `id` (unique) plus a display
        // `name` that is often Chinese (「企业微信」). The opaque component id
        // and the tool namespace must stay ASCII-unique; the display name may
        // be localized freely.
        let id_value = entry
            .get("id")
            .and_then(|value| value.as_str())
            .map(sanitize_slug)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| {
                sanitize_slug(
                    entry
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or(&fallback),
                )
            });
        let slug = if id_value.is_empty() { fallback.clone() } else { id_value };
        let name = entry
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(&slug)
            .to_owned();
        let id = component_id(&meta.plugin_id, &slug);
        let kind = entry
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("remote-mcp")
            .to_owned();
        let transport = entry
            .get("url")
            .and_then(|value| value.as_str())
            .or_else(|| entry.get("command").and_then(|value| value.as_str()))
            .unwrap_or("")
            .to_owned();
        let auth_mode = entry
            .get("auth")
            .or_else(|| entry.get("authMode"))
            .and_then(|value| value.as_str())
            .unwrap_or("none")
            .to_owned();
        builder.push(Component::new(
            crate::models::KIND_CONNECTOR,
            id.clone(),
            name.clone(),
            None,
            compat::connector(),
            json!({
                "id": id,
                "version": builder.version,
                "name": name,
                "kind": kind,
                "transport_summary": transport,
                "auth_mode": auth_mode,
                "tool_filter": format!("connector__{slug}__<tool>"),
            }),
        ));
    }
}

/// Build components for a single CLI connector directory (`cli.json` +
/// `skills/`), the layout used by real CodeBuddy connector markets (wecom /
/// feishu / tmeet / …).
///
/// - one `connector` component (kind `cli`) describing runtime + auth
///   lifecycle derived from `cli.json`;
/// - one `skill` component per `skills/<dir>/SKILL.md` (02 §5); a missing
///   `skills/` directory degrades to a warning, never a block.
fn build_cli_connector_components(
    source: &Path,
    cli: &crate::manifest::CliManifest,
    directory_name: &str,
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
) {
    let id = component_id(&meta.plugin_id, directory_name);
    let runtime_type = cli
        .runtime
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_owned();
    let runtime_version = cli
        .runtime
        .as_ref()
        .and_then(|value| value.get("version"))
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let version_check = crate::manifest::platform_summary(&cli.version_check)
        .unwrap_or_default();
    let init_summary = crate::manifest::platform_summary(&cli.init).unwrap_or_default();
    let auth_summary = crate::manifest::platform_summary(&cli.auth).unwrap_or_default();
    let status_summary = crate::manifest::platform_summary(&cli.status).unwrap_or_default();
    let auth_domain = cli.auth_url_domain.clone().unwrap_or_default();
    let auth_mode = if auth_summary.is_empty() {
        "none".to_owned()
    } else {
        "cli-auth".to_owned()
    };
    builder.push(Component::new(
        crate::models::KIND_CONNECTOR,
        id.clone(),
        directory_name.to_owned(),
        None,
        compat::connector(),
        json!({
            "id": id,
            "version": builder.version,
            "name": directory_name,
            "kind": "cli",
            "runtime": {
                "type": runtime_type,
                "version": runtime_version,
            },
            "transport_summary": if init_summary.is_empty() {
                format!("cli:{}", directory_name)
            } else {
                init_summary.clone()
            },
            "auth_mode": auth_mode,
            "auth": {
                "init": init_summary,
                "status": status_summary,
                "status_match": cli.status_match.clone(),
                "domain": auth_domain,
            },
            "version_check": version_check,
            "tool_filter": format!("connector__{directory_name}__<tool>"),
        }),
    ));

    // skills/ scan (one SKILL.md per subdirectory; shallow, mirrors the
    // plugin skills branch). Depends on `source.join("skills")` — the dir is
    // copied verbatim by walk.rs.
    let skills_root = source.join("skills");
    if !skills_root.is_dir() {
        builder.warn("CLI 连接器缺少 skills/ 目录".into());
        return;
    }
    for entry in walkdir::WalkDir::new(&skills_root)
        .min_depth(1)
        .max_depth(2)
        .sort_by_file_name()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
            continue;
        }
        let abs = entry.path();
        let rel = abs
            .strip_prefix(source)
            .ok()
            .and_then(|path| path.to_str())
            .map(|path| path.replace('\\', "/"))
            .unwrap_or_default();
        match std::fs::read_to_string(abs) {
            Ok(text) => {
                let version = builder.version.clone();
                skill_from_text(&text, &rel, &meta.plugin_id, &version, builder);
            }
            Err(error) => builder.warn(format!("技能文件不可读 {rel}: {error}")),
        }
    }
}

fn build_connector_components(
    value: serde_json::Value,
    source_rel: &str,
    meta: &crate::models::SnapshotMeta,
    builder: &mut ComponentBuilder,
) {
    let servers = value
        .get("mcpServers")
        .and_then(|value| value.as_object())
        .or_else(|| value.as_object());
    let Some(servers) = servers else {
        builder.error("MCP 配置格式无法识别（缺少 mcpServers 对象）".into());
        return;
    };
    for (name, config) in servers {
        let url = config
            .get("url")
            .and_then(|value| value.as_str())
            .map(|value| value.to_owned());
        let command = config
            .get("command")
            .and_then(|value| value.as_str())
            .map(|value| value.to_owned());
        let kind = if url.is_some() { "remote-mcp" } else { "stdio-mcp" };
        let transport_summary = url
            .clone()
            .unwrap_or_else(|| command.clone().unwrap_or_else(|| name.clone()));
        let id = component_id(&meta.plugin_id, &format!("mcp-{}", sanitize_slug(name)));
        builder.push(Component::new(
            crate::models::KIND_CONNECTOR,
            id.clone(),
            name.clone(),
            Some(source_rel.to_owned()),
            compat::connector(),
            json!({
                "id": id,
                "version": builder.version,
                "name": name,
                "kind": kind,
                "transport_summary": transport_summary,
                "auth_mode": "oauth",
                "tool_filter": format!("connector__{name}__<tool>"),
            }),
        ));
    }
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

fn is_sensitive_field(key: &str, schema_type: &str, schema: &serde_json::Value) -> bool {
    let lower = key.to_ascii_lowercase();
    if ["api", "token", "secret", "password", "apikey"].iter().any(|part| lower.contains(part)) {
        return true;
    }
    if ["apikey", "secret", "token", "oauth", "password"]
        .iter()
        .any(|part| schema_type.to_ascii_lowercase().contains(part))
    {
        return true;
    }
    schema.get("sensitive").and_then(|value| value.as_bool()).unwrap_or(false)
}

/// Manifest-declared component roots: directories or individual files.
///
/// Real CodeBuddy markets declare both shapes: a directory
/// (`"./agents"`) that is scanned recursively, and explicit file paths
/// (`"./agents/lead.md"`) that are imported one-by-one. V1 (02 §4) treats
/// every declared root as a scan target; files are imported directly.
fn component_dirs(
    declared: &[String],
    default_dir: &str,
    source: &Path,
    builder: &mut ComponentBuilder,
) -> Vec<String> {
    let explicitly_declared = !declared.is_empty();
    let mut out: Vec<String> = Vec::new();
    let list: Vec<String> = if explicitly_declared {
        declared.to_vec()
    } else {
        vec![format!("./{default_dir}")]
    };
    for raw in list {
        match validate_relative_path(&raw) {
            Ok(dir) if dir.is_empty() => out.push(String::new()),
            Ok(dir) => {
                let path = source.join(&dir);
                if path.is_file() || path.is_dir() {
                    out.push(dir);
                } else if explicitly_declared {
                    builder.warn(format!("清单声明的目录/文件不存在：{dir}"));
                }
            }
            Err(error) => builder.block(format!("忽略不安全路径 {raw}: {error}")),
        }
    }
    out
}

/// Read a single declared component file; missing files degrade to a warning
/// rather than blocking (02 §11.1).
fn read_optional(path: &Path, rel: &str, builder: &mut ComponentBuilder) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            builder.warn(format!("组件文件不可读 {rel}: {error}"));
            None
        }
    }
}

/// Parse a `SKILL.md` text and push a skill component. Shared by directory
/// scans and explicit file declarations.
fn skill_from_text(
    text: &str,
    rel: &str,
    plugin_id: &str,
    version: &str,
    builder: &mut ComponentBuilder,
) {
    let slug = rel
        .rsplit_once('/')
        .map(|(parent, _)| parent.rsplit('/').next().unwrap_or("skill"))
        .unwrap_or("skill");
    match parse_skill(text, rel) {
        Ok(doc) => {
            let id = component_id(plugin_id, slug);
            let payload = json!({
                "id": id,
                "version": version,
                "name": doc.name,
                "slug": slug,
                "description": doc.description,
                "mode": "store-agent",
                "invocation_policy": "model-auto",
                "instructions_ref": rel,
                "relative_path": rel,
                "has_arguments_note": doc.has_arguments_note,
            });
            builder.push(Component::new(
                crate::models::KIND_SKILL,
                id,
                doc.name,
                Some(rel.to_owned()),
                compat::skill(),
                payload,
            ));
        }
        Err(error) => builder.error(format!("技能文件解析失败 {rel}: {error}")),
    }
}

/// `.md` files under `source/dir` (sorted, shallow recursion).
fn scan_markdown(
    source: &Path,
    dir: &str,
    builder: &mut ComponentBuilder,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let root = if dir.is_empty() { source.to_path_buf() } else { source.join(dir) };
    for entry in walkdir::WalkDir::new(&root).sort_by_file_name() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() || !entry.file_name().to_string_lossy().ends_with(".md") {
            continue;
        }
        let abs = entry.path();
        let rel = abs
            .strip_prefix(source)
            .ok()
            .and_then(|path| path.to_str())
            .map(|path| path.replace('\\', "/"))
            .unwrap_or_default();
        match std::fs::read_to_string(abs) {
            Ok(text) => out.push((rel, text)),
            Err(error) => builder.warn(format!("文件不可读 {rel}: {error}")),
        }
    }
    out
}

fn read_json_file(path: PathBuf, rel: &str, builder: &mut ComponentBuilder) -> Option<serde_json::Value> {
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(value) => Some(value),
            Err(error) => {
                builder.error(format!("JSON 文件解析失败 {rel}: {error}"));
                None
            }
        },
        Err(_) => None,
    }
}

fn blocked_result(
    source_kind: SourceKind,
    identity: Option<(String, String)>,
    errors: &[String],
) -> AppServerImportResult {
    let (name, version) = identity.unwrap_or_else(|| ("unknown".into(), "0".into()));
    AppServerImportResult {
        snapshot_id: nomifun_common::generate_id(),
        name,
        version,
        source_kind: source_kind.as_str().into(),
        status: "blocked".into(),
        content_digest: String::new(),
        component_status: AppServerCompatibilityTriple {
            semantic_status: "blocked".into(),
            runtime_status: crate::models::RUNTIME_NOT_VERIFIED.into(),
            distribution_status: crate::models::DIST_LOCAL_ONLY.into(),
            reasons: errors.to_vec(),
        },
        component_count: 0,
        imported_at: now_ms(),
        reused: false,
        warnings: vec![],
        errors: errors.to_vec(),
    }
}

fn to_public_triple(triple: &CompatTriple) -> AppServerCompatibilityTriple {
    AppServerCompatibilityTriple {
        semantic_status: triple.semantic_status.clone(),
        runtime_status: triple.runtime_status.clone(),
        distribution_status: triple.distribution_status.clone(),
        reasons: triple.reasons.clone(),
    }
}

pub fn component_to_public(component: &Component) -> AppServerImportComponent {
    AppServerImportComponent {
        id: component.component_id.clone(),
        kind: component.kind.clone(),
        name: component.name.clone(),
        compatibility: to_public_triple(&component.compatibility),
        warnings: component.warnings.clone(),
    }
}

// ---------------------------------------------------------------------------
// tests (fixtures live in tests/fixtures; unit behavior here)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_field_detection_never_leaks_values() {
        assert!(is_sensitive_field("apiKey", "string", &json!({})));
        assert!(is_sensitive_field("token", "string", &json!({})));
        assert!(is_sensitive_field("client_secret", "string", &json!({})));
        assert!(is_sensitive_field("webhook_url", "apikey", &json!({})));
        assert!(is_sensitive_field("name", "string", &json!({"sensitive": true})));
        assert!(!is_sensitive_field("name", "string", &json!({})));
        assert!(!is_sensitive_field("temperature", "number", &json!({})));
    }

    #[test]
    fn component_id_is_documented_shape() {
        assert_eq!(
            component_id("software-company", "software-team-lead"),
            "wb-software-company-software-team-lead"
        );
        assert_eq!(component_id("my plugin", "hello world"), "wb-my-plugin-hello-world");
    }
}