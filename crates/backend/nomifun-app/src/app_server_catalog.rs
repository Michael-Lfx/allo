//! App Server catalog providers backed by the system Skill / MCP services.
//!
//! These adapters sit in the composition root (`nomifun-app`) and map the
//! system Skill corpus / MCP configuration into the public Agent Store shapes
//! consumed by the versioned App Server Protocol. They are deliberately thin:
//! no credentials, internal IDs or filesystem paths cross into the protocol
//! layer.

use async_trait::async_trait;
use std::path::Path;

use nomifun_api_types::{
    AppServerCompatibilityStatus, AppServerConnectorDetail, AppServerConnectorProbeResult,
    AppServerConnectorStatus, AppServerConnectorStatusView, AppServerConnectorSummary,
    AppServerConnectorTool, AppServerModelList, AppServerModelSummary,
    AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDetail, AppServerSkillSummary, McpConnectionTestResult, McpTransport,
};
use nomifun_app_server::{
    agent_store::AgentStoreConfig, ConnectorAuthProvider, ConnectorCatalogProvider,
    ModelCatalogProvider, SkillCatalogProvider,
};
use nomifun_common::{AppError, McpServerStatus};
use nomifun_extension::skill_service::{
    self, SkillListItem, SkillOrigin, SkillPaths, SkillSource,
};
use nomifun_mcp::{McpConfigService, McpConnectionTestService, McpOAuthService};

// ---------------------------------------------------------------------------
// Skill catalog
// ---------------------------------------------------------------------------

/// Read-only Skill catalog over the system skill corpus.
///
/// `source` / `compatibility_status` are derived until the Agent Store
/// Importer/PluginSnapshot layer lands (roadmap Phase 1): builtin skills map
/// to `compatible`, custom/extension content to `compatible-with-adapter`.
#[derive(Clone)]
pub struct AppServerSkillCatalog {
    paths: SkillPaths,
}

impl AppServerSkillCatalog {
    pub fn new(paths: SkillPaths) -> Self {
        Self { paths }
    }

    async fn list_items(&self) -> Result<Vec<SkillListItem>, AppError> {
        skill_service::list_available_skills(&self.paths)
            .await
            .map_err(|error| AppError::Internal(format!("list agent-store skills: {error}")))
    }
}

fn skill_summary(item: SkillListItem, origin: SkillOrigin, writable: bool) -> AppServerSkillSummary {
    let (source, compatibility) = match item.source {
        SkillSource::Builtin => ("builtin", AppServerCompatibilityStatus::Compatible),
        SkillSource::Extension => ("extension", AppServerCompatibilityStatus::CompatibleWithAdapter),
        SkillSource::Custom => ("custom", AppServerCompatibilityStatus::CompatibleWithAdapter),
    };
    let description = item.description.trim();
    AppServerSkillSummary {
        // Public id is the skill name until Importer provides source-qualified
        // opaque ids; the name is unique within this local corpus.
        id: item.name.clone(),
        name: item.name,
        description: if description.is_empty() { None } else { Some(description.to_owned()) },
        version: source.to_owned(),
        source: source.to_owned(),
        // `source: custom` covers every unmanaged-root skill; `origin` is what
        // separates a writable user skill from an installed marketplace
        // product (both `custom`) and is the fact the write face enforces.
        origin: origin.as_str().to_owned(),
        writable,
        compatibility_status: compatibility,
        enabled: true,
        required_connectors: Vec::new(),
    }
}

#[async_trait]
impl SkillCatalogProvider for AppServerSkillCatalog {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError> {
        Ok(self
            .list_items()
            .await?
            .into_iter()
            .map(|item| {
                let origin = skill_service::skill_origin_of(&self.paths, Path::new(&item.location));
                let writable =
                    skill_service::is_writable_skill(&self.paths, &item.name, Path::new(&item.location));
                skill_summary(item, origin, writable)
            })
            .collect())
    }

    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError> {
        let item = self
            .list_items()
            .await?
            .into_iter()
            .find(|skill| skill.name == id)
            .ok_or_else(|| AppError::NotFound(format!("skill {id} not found")))?;
        let origin = skill_service::skill_origin_of(&self.paths, Path::new(&item.location));
        let writable =
            skill_service::is_writable_skill(&self.paths, &item.name, Path::new(&item.location));
        // Public body is a bounded, trimmed summary. Internal routing rules,
        // credentials and the full raw Markdown stay behind the seam.
        //
        // The location is a *directory* for every user-root skill
        // (`scan_skill_dirs` records directories, built-ins record the
        // manifest), so the manifest is resolved rather than assumed.
        let instructions_summary = std::fs::read_to_string(
            skill_service::skill_manifest_path(Path::new(&item.location)),
        )
        .ok()
        .map(|body| body.trim().chars().take(1200).collect::<String>())
        .filter(|body| !body.is_empty());
        Ok(AppServerSkillDetail {
            summary: skill_summary(item, origin, writable),
            mode: "store-agent".into(),
            invocation_policy: "model-auto".into(),
            instructions_summary,
        })
    }
}

// ---------------------------------------------------------------------------
// Connector catalog + status + probe
// ---------------------------------------------------------------------------

fn transport_kind(transport: &McpTransport) -> (&'static str, String) {
    match transport {
        McpTransport::Stdio { command, args, .. } => {
            let mut summary = String::from(command);
            if let Some(rendered) = args
                .first()
                .map(|arg| format!(" {arg}"))
            {
                summary.push_str(&rendered);
            }
            ("stdio-mcp", summary)
        }
        McpTransport::Sse { url, .. } | McpTransport::Http { url, .. } => {
            ("remote-mcp", url.clone())
        }
    }
}

fn transport_url(transport: &McpTransport) -> Option<String> {
    match transport {
        McpTransport::Stdio { .. } => None,
        McpTransport::Sse { url, .. } | McpTransport::Http { url, .. } => Some(url.clone()),
    }
}

fn auth_mode_for(transport: &McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio { .. } => "none",
        McpTransport::Sse { .. } | McpTransport::Http { .. } => "oauth",
    }
}

fn summary_status(enabled: bool, last_test: McpServerStatus, auth_mode: &str) -> AppServerConnectorStatus {
    if !enabled {
        return AppServerConnectorStatus::Installed;
    }
    match last_test {
        McpServerStatus::Error => AppServerConnectorStatus::Error,
        McpServerStatus::Connected => AppServerConnectorStatus::Connected,
        _ if auth_mode == "oauth" => AppServerConnectorStatus::AuthorizationRequired,
        _ => AppServerConnectorStatus::Configured,
    }
}

fn connector_summary(
    id: String,
    name: String,
    description: Option<String>,
    enabled: bool,
    transport: &McpTransport,
    last_test: McpServerStatus,
) -> AppServerConnectorSummary {
    let (kind, transport_summary) = transport_kind(transport);
    let auth_mode = auth_mode_for(transport);
    AppServerConnectorSummary {
        id,
        name,
        description,
        kind: kind.to_owned(),
        transport_summary,
        auth_mode: auth_mode.to_owned(),
        enabled,
        status: summary_status(enabled, last_test, auth_mode),
    }
}

fn probe_status(
    enabled: bool,
    authenticated: bool,
    auth_mode: &str,
    last_test: McpServerStatus,
) -> AppServerConnectorStatus {
    if !enabled {
        return AppServerConnectorStatus::Installed;
    }
    match last_test {
        McpServerStatus::Error => AppServerConnectorStatus::Error,
        _ if auth_mode == "oauth" && !authenticated => AppServerConnectorStatus::AuthorizationRequired,
        McpServerStatus::Connected => AppServerConnectorStatus::Connected,
        _ => AppServerConnectorStatus::Configured,
    }
}

/// Connector catalog over the system MCP configuration service.
#[derive(Clone)]
pub struct AppServerConnectorCatalog {
    config: McpConfigService,
    connection_test: McpConnectionTestService,
    oauth: McpOAuthService,
}

impl AppServerConnectorCatalog {
    pub fn new(
        config: McpConfigService,
        connection_test: McpConnectionTestService,
        oauth: McpOAuthService,
    ) -> Self {
        Self { config, connection_test, oauth }
    }

    async fn get_server(&self, connector_id: &str) -> Result<nomifun_api_types::McpServerResponse, AppError> {
        let parsed = nomifun_api_types::McpServerId::parse(connector_id)
            .map_err(|error| AppError::BadRequest(format!("invalid connector id: {error}")))?;
        self.config.get_server(&parsed).await.map_err(AppError::from)
    }

    async fn oauth_authenticated(&self, transport: &McpTransport) -> Option<bool> {
        let url = transport_url(transport)?;
        match self.oauth.check_oauth_status(&url).await {
            Ok(status) => Some(status.authenticated),
            Err(_) => None,
        }
    }
}

#[async_trait]
impl ConnectorCatalogProvider for AppServerConnectorCatalog {
    async fn list(&self) -> Result<Vec<AppServerConnectorSummary>, AppError> {
        let servers = self.config.list_servers().await.map_err(AppError::from)?;
        Ok(servers
            .into_iter()
            .map(|server| {
                let id = server.mcp_server_id.as_str().to_owned();
                connector_summary(
                    id,
                    server.name,
                    server.description,
                    server.enabled,
                    &server.transport,
                    server.last_test_status,
                )
            })
            .collect())
    }

    async fn get(&self, id: &str) -> Result<AppServerConnectorDetail, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        let transport = server.transport;
        let auth_mode = auth_mode_for(&transport);
        let summary = connector_summary(
            connector_id.clone(),
            server.name.clone(),
            server.description.clone(),
            server.enabled,
            &transport,
            server.last_test_status,
        );
        let auth_status = if auth_mode == "oauth" {
            self.oauth_authenticated(&transport).await.map(|authenticated| AppServerOAuthStatusView {
                state: if authenticated { "authenticated".into() } else { "not_authenticated".into() },
                error: None,
            })
        } else {
            None
        };
        Ok(AppServerConnectorDetail {
            summary,
            tool_filter: Some(format!("connector__{connector_id}__<tool>")),
            tools: server
                .tools
                .unwrap_or_default()
                .into_iter()
                .map(|tool| AppServerConnectorTool {
                    name: tool.name,
                    description: tool.description,
                })
                .collect(),
            auth_status,
            source: if server.builtin { "builtin".into() } else { "system".into() },
            compatibility_status: AppServerCompatibilityStatus::Compatible,
        })
    }

    async fn status(&self, id: &str) -> Result<AppServerConnectorStatusView, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        let transport = server.transport;
        let auth_mode = auth_mode_for(&transport);
        let authenticated = if auth_mode == "oauth" {
            self.oauth_authenticated(&transport).await.unwrap_or(false)
        } else {
            false
        };
        let status = probe_status(server.enabled, authenticated, auth_mode, server.last_test_status);
        Ok(AppServerConnectorStatusView {
            connector_id,
            status,
            auth_status: if auth_mode == "oauth" {
                Some(AppServerOAuthStatusView {
                    state: if authenticated { "authenticated".into() } else {
                        if status == AppServerConnectorStatus::AuthorizationRequired {
                            "not_authenticated".into()
                        } else {
                            "reauthorization_required".into()
                        }
                    },
                    error: None,
                })
            } else {
                None
            },
            last_error: if status == AppServerConnectorStatus::Error {
                Some("last connection test failed".into())
            } else {
                None
            },
        })
    }

    async fn test(&self, id: &str) -> Result<AppServerConnectorProbeResult, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        let transport = nomifun_mcp::McpServerTransport::from(server.transport);
        let result = self.connection_test.test_connection(&server.name, &transport).await;
        self.config
            .persist_test_result(&server.mcp_server_id, &result)
            .await
            .map_err(AppError::from)?;
        Ok(probe_result(connector_id, result))
    }
}

fn probe_result(connector_id: String, result: McpConnectionTestResult) -> AppServerConnectorProbeResult {
    AppServerConnectorProbeResult {
        connector_id,
        success: result.success,
        tools: result.tools.map(|tools| {
            tools
                .into_iter()
                .map(|tool| AppServerConnectorTool {
                    name: tool.name,
                    description: tool.description,
                })
                .collect()
        }),
        error: result.error,
        code: result.code.map(|code| code.as_str().to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Connector OAuth pass-through
// ---------------------------------------------------------------------------

/// OAuth pass-through: the trusted host owns the browser flow; clients only
/// see a start acknowledgement and poll `auth_status`.
#[derive(Clone)]
pub struct AppServerConnectorAuth {
    config: McpConfigService,
    oauth: McpOAuthService,
}

impl AppServerConnectorAuth {
    pub fn new(config: McpConfigService, oauth: McpOAuthService) -> Self {
        Self { config, oauth }
    }

    async fn remote_url(&self, connector_id: &str) -> Result<String, AppError> {
        let server = {
            // resolve through the shared config service (also validates id)
            let parsed = nomifun_api_types::McpServerId::parse(connector_id)
                .map_err(|error| AppError::BadRequest(format!("invalid connector id: {error}")))?;
            self.config.get_server(&parsed).await.map_err(AppError::from)?
        };
        transport_url(&server.transport)
            .ok_or_else(|| AppError::BadRequest("OAuth is not supported for stdio connectors".into()))
    }
}

#[async_trait]
impl ConnectorAuthProvider for AppServerConnectorAuth {
    async fn auth_status(&self, id: &str) -> Result<AppServerOAuthStatusView, AppError> {
        let url = self.remote_url(id).await?;
        let status = self.oauth.check_oauth_status(&url).await.map_err(AppError::from)?;
        Ok(AppServerOAuthStatusView {
            state: if status.authenticated { "authenticated".into() } else { "not_authenticated".into() },
            error: None,
        })
    }

    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError> {
        let url = self.remote_url(id).await?;
        // The PKCE browser flow runs on the trusted host (server-side
        // callback + encrypted token storage). Spawn it so a single
        // connection never blocks on the callback window; clients poll
        // `auth_status` until `authenticated`.
        let oauth = self.oauth.clone();
        tokio::spawn(async move {
            match oauth.login(&url).await {
                Ok(result) if result.success => {
                    tracing::info!(url = %url, "MCP OAuth login completed");
                }
                Ok(result) => {
                    tracing::warn!(
                        url = %url,
                        error = result.error.as_deref().unwrap_or("unknown"),
                        "MCP OAuth login failed"
                    );
                }
                Err(error) => {
                    tracing::warn!(url = %url, %error, "MCP OAuth login task failed");
                }
            }
        });
        Ok(AppServerOAuthStartResult {
            connector_id: id.to_owned(),
            state: "started".into(),
            error: None,
        })
    }

    async fn logout(&self, id: &str) -> Result<(), AppError> {
        let url = self.remote_url(id).await?;
        self.oauth.logout(&url).await.map_err(AppError::from)
    }
}

// ---------------------------------------------------------------------------
// Public model directory (REQ-PAR-05b)
// ---------------------------------------------------------------------------

/// Read-only model directory over the system provider registry. Projects
/// enabled providers into `provider/model` rows; API keys, base URLs and
/// health internals are dropped at this boundary.
#[derive(Clone)]
pub struct AppServerModelCatalog {
    providers: std::sync::Arc<nomifun_system::ProviderService>,
    /// `~/.agent-store/config.toml` path when the host declares one. Providers
    /// declared there but not yet registered in the DB still surface in
    /// `models/list` (WP-7 模型选择器联动: the picker must show the full
    /// catalog a fresh install has, not just what an earlier run/turn already
    /// lazily registered).
    agent_store_config_path: Option<std::path::PathBuf>,
}

impl AppServerModelCatalog {
    pub fn new(
        providers: std::sync::Arc<nomifun_system::ProviderService>,
        agent_store_config_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            providers,
            agent_store_config_path,
        }
    }
}

#[async_trait]
impl ModelCatalogProvider for AppServerModelCatalog {
    async fn list(&self) -> Result<AppServerModelList, AppError> {
        let db_providers = self.providers.list().await.map_err(AppError::from)?;

        // `~/.agent-store/config.toml` is the model-provider source of truth:
        // providers declared there but not yet registered in the DB (fresh
        // install — nothing registered until the first agent/run lazily
        // registers one) still surface in `models/list`, so the picker shows
        // the full catalog the runtime can actually resolve.
        let config = self
            .agent_store_config_path
            .as_deref()
            .and_then(AgentStoreConfig::load_ok);
        let known: std::collections::HashSet<String> = db_providers
            .iter()
            .map(|provider| provider.name.clone())
            .collect();
        let mut config_only: Vec<(String, Vec<(String, Option<String>, Option<i64>)>)> = Vec::new();
        if let Some(config) = config.as_ref() {
            let mut keys: Vec<String> = config
                .providers
                .iter()
                .filter(|(_, cfg)| cfg.enabled.unwrap_or(true))
                .filter(|(key, _)| !known.contains(*key))
                .map(|(key, _)| key.clone())
                .collect();
            keys.sort();
            for key in keys {
                let limits = config.context_limits_for_provider(&key);
                let names = config.display_names_for_provider(&key);
                let models: Vec<(String, Option<String>, Option<i64>)> = config
                    .models_for_provider(&key)
                    .into_iter()
                    .map(|model| (names.get(&model).cloned(), limits.get(&model).copied(), model))
                    .map(|(display_name, context_limit, model)| (model, display_name, context_limit))
                    .collect();
                if !models.is_empty() {
                    config_only.push((key, models));
                }
            }
        }

        // Default selection mirrors the `agent/run` fallback: the config's
        // `default_model` when declared, else the first enabled provider with
        // a model.
        let default = config
            .as_ref()
            .and_then(AgentStoreConfig::default_selection)
            .or_else(|| {
                db_providers
                    .iter()
                    .find(|provider| provider.enabled && !provider.models.is_empty())
                    .map(|provider| (provider.name.clone(), provider.models[0].clone()))
            });

        let mut items = Vec::new();
        for provider in &db_providers {
            if !provider.enabled {
                continue;
            }
            for model in &provider.models {
                if provider
                    .model_enabled
                    .as_ref()
                    .and_then(|enabled| enabled.get(model))
                    == Some(&false)
                {
                    continue;
                }
                items.push(AppServerModelSummary {
                    provider_id: provider.provider_id.clone(),
                    provider_name: provider.name.clone(),
                    model: model.clone(),
                    display_name: provider
                        .model_descriptions
                        .as_ref()
                        .and_then(|descriptions| descriptions.get(model))
                        .cloned(),
                    is_default: default
                        .as_ref()
                        .is_some_and(|(key, default_model)| key == &provider.name && default_model == model),
                });
            }
        }
        for (provider_name, models) in config_only {
            for (model, display_name, _context_limit) in models {
                let is_default = default
                    .as_ref()
                    .is_some_and(|(key, default_model)| key == &provider_name && default_model == &model);
                items.push(AppServerModelSummary {
                    provider_id: provider_name.clone(),
                    provider_name: provider_name.clone(),
                    model,
                    display_name,
                    is_default,
                });
            }
        }
        Ok(AppServerModelList { items })
    }
}
#[cfg(test)]
mod model_catalog_tests {
    use super::*;

    const CONFIG_BODY: &str = r#"
default_model = "mimo/mimo-v2.5"

[providers.mimo]
type = "openai"
api_key = "sk-test"
base_url = "https://mimo.example"

[models."mimo/mimo-v2.5"]
provider = "mimo"
model = "mimo-v2.5"
display_name = "MiMo V2.5"
max_context_size = 1000000
"#;

    fn write_config(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, CONFIG_BODY).expect("write config");
        path
    }

    async fn service() -> std::sync::Arc<nomifun_system::ProviderService> {
        let db = nomifun_db::init_database_memory().await.unwrap();
        let pool = db.pool().clone();
        std::mem::forget(db);
        std::sync::Arc::new(nomifun_system::ProviderService::new(
            std::sync::Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
            std::sync::Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool)),
            [0x42; 32],
        ))
    }

    #[tokio::test]
    async fn models_list_merges_unregistered_agent_store_config_providers() {
        let dir = std::env::temp_dir().join(format!("allo-wp7-{}", nomifun_common::generate_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let config_path = write_config(&dir);
        let catalog = AppServerModelCatalog::new(service().await, Some(config_path));

        // DB is empty (fresh install) — the config provider must still surface.
        let list = catalog.list().await.expect("list ok");
        assert_eq!(list.items.len(), 1, "config-only provider projects: {:?}", list.items);
        let entry = &list.items[0];
        assert_eq!(entry.provider_name, "mimo");
        assert_eq!(entry.model, "mimo-v2.5");
        assert_eq!(entry.display_name.as_deref(), Some("MiMo V2.5"));
        assert!(entry.is_default, "config default_model marks the entry");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn models_list_falls_back_to_db_default_when_config_has_none() {
        let dir = std::env::temp_dir().join(format!("allo-wp7-{}", nomifun_common::generate_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // No default_model: strip it from the fixture.
        let body = CONFIG_BODY.replacen("default_model = \"mimo/mimo-v2.5\"\n", "", 1);
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, body).expect("write config");
        let catalog = AppServerModelCatalog::new(service().await, Some(config_path));

        let list = catalog.list().await.expect("list ok");
        assert_eq!(list.items.len(), 1);
        // No DB provider and no config default → nothing flagged default.
        assert!(!list.items[0].is_default);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- Skill read face: origin / writable / bounded body (`16` R17) -------

    /// Temp `SkillPaths` for the skill read face. Every root is inside a fresh
    /// directory, so a real disk layout can be laid out per test.
    fn temp_skill_paths() -> (std::path::PathBuf, SkillPaths) {
        let dir = std::env::temp_dir().join(format!("allo-skill-read-{}", nomifun_common::generate_id()));
        std::fs::create_dir_all(&dir).expect("temp skill dir");
        let paths = SkillPaths {
            data_dir: dir.clone(),
            user_skills_dir: dir.join("skills"),
            cron_skills_dir: dir.join("cron/skills"),
            builtin_skills_dir: dir.join("builtin-skills"),
            builtin_rules_dir: dir.join("rules"),
            preset_rules_dir: dir.join("preset-rules"),
            preset_skills_dir: dir.join("preset-skills"),
            catalog_roots: Default::default(),
        };
        (dir, paths)
    }

    fn write_manifest(dir: &std::path::Path, name: &str, description: &str, body: &str) {
        std::fs::create_dir_all(dir).expect("skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        )
        .expect("skill manifest");
    }

    /// The read face is what a UI keys its write buttons off, so the three
    /// ownership classes the write face distinguishes must already be
    /// distinguishable here — and the bounded body must stay bounded.
    #[tokio::test]
    async fn skill_read_face_reports_origin_and_writability_per_layout() {
        let (dir, paths) = temp_skill_paths();
        write_manifest(
            &paths.builtin_skills_dir.join("builtin-skill"),
            "builtin-skill",
            "a built-in",
            "builtin body",
        );
        write_manifest(
            &paths
                .user_skills_dir
                .join("agent-store")
                .join("0190f5fe-7c00-7a00-8000-000000000301")
                .join("market-skill"),
            "market-skill",
            "an installed product",
            "market body",
        );
        write_manifest(
            &paths.user_skills_dir.join("shared").join("shared-skill"),
            "shared-skill",
            "a companion shared skill",
            "shared body",
        );
        let long_body = "x".repeat(3000);
        write_manifest(
            &paths.user_skills_dir.join("user-skill"),
            "user-skill",
            "a user skill",
            &long_body,
        );

        let catalog = AppServerSkillCatalog::new(paths.clone());
        let listed = catalog.list().await.expect("list");
        let by_name = |name: &str| {
            listed
                .iter()
                .find(|skill| skill.name == name)
                .unwrap_or_else(|| panic!("{name} must be listed: {:?}", listed.iter().map(|s| &s.name).collect::<Vec<_>>()))
                .clone()
        };

        // Only the flat user-root skill is writable, and each class reports the
        // owner it actually has — `source` alone calls three of these "custom".
        let user = by_name("user-skill");
        assert_eq!(user.source, "custom");
        assert_eq!(user.origin, "user");
        assert!(user.writable);
        let market = by_name("market-skill");
        assert_eq!(market.source, "custom");
        assert_eq!(market.origin, "marketplace");
        assert!(!market.writable);
        let shared = by_name("shared-skill");
        assert_eq!(shared.origin, "shared");
        assert!(!shared.writable);
        let builtin = by_name("builtin-skill");
        assert_eq!(builtin.source, "builtin");
        assert_eq!(builtin.origin, "builtin");
        assert!(!builtin.writable);

        // `skill/get` reads the manifest of a *user* skill, whose recorded
        // location is a directory, and still bounds the body to 1200 chars.
        let detail = catalog.get("user-skill").await.expect("get user skill");
        assert_eq!(detail.summary.origin, "user");
        assert!(detail.summary.writable);
        let summary = detail.instructions_summary.expect("bounded summary");
        assert_eq!(summary.chars().count(), 1200);
        assert!(!summary.contains(&"x".repeat(1201)));
        // The built-in path was already a manifest path and keeps working.
        let builtin_detail = catalog.get("builtin-skill").await.expect("get builtin");
        assert_eq!(
            builtin_detail.instructions_summary.as_deref(),
            Some("---\nname: builtin-skill\ndescription: a built-in\n---\n\nbuiltin body")
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
