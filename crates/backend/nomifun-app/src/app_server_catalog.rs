//! App Server catalog providers backed by the system Skill / MCP services.
//!
//! These adapters sit in the composition root (`nomifun-app`) and map the
//! system Skill corpus / MCP configuration into the public Agent Store shapes
//! consumed by the versioned App Server Protocol. They are deliberately thin:
//! no credentials, internal IDs or filesystem paths cross into the protocol
//! layer.

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerCompatibilityStatus, AppServerConnectorDetail, AppServerConnectorProbeResult,
    AppServerConnectorStatus, AppServerConnectorStatusView, AppServerConnectorSummary,
    AppServerConnectorTool, AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDetail, AppServerSkillSummary, McpConnectionTestResult, McpTransport,
};
use nomifun_app_server::{ConnectorAuthProvider, ConnectorCatalogProvider, SkillCatalogProvider};
use nomifun_common::{AppError, McpServerStatus};
use nomifun_extension::skill_service::{self, SkillListItem, SkillPaths, SkillSource};
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

fn skill_summary(item: SkillListItem) -> AppServerSkillSummary {
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
            .map(skill_summary)
            .collect())
    }

    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError> {
        let item = self
            .list_items()
            .await?
            .into_iter()
            .find(|skill| skill.name == id)
            .ok_or_else(|| AppError::NotFound(format!("skill {id} not found")))?;
        // Public body is a bounded, trimmed summary. Internal routing rules,
        // credentials and the full raw Markdown stay behind the seam.
        let instructions_summary = std::fs::read_to_string(&item.location)
            .ok()
            .map(|body| body.trim().chars().take(1200).collect::<String>())
            .filter(|body| !body.is_empty());
        Ok(AppServerSkillDetail {
            summary: skill_summary(item),
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