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
    AppServerCompatibilityStatus, AppServerConnectorCredential, AppServerConnectorDetail,
    AppServerConnectorProbeResult, AppServerConnectorStatus, AppServerConnectorStatusView,
    AppServerConnectorSummary, AppServerCredentialMode, AppServerConnectorTool,
    AppServerLocalizedText, AppServerModelList, AppServerModelSummary, AppServerOAuthStartResult,
    AppServerOAuthStatusView, AppServerSkillDetail, AppServerSkillSummary, McpConnectionTestResult,
    McpToolResponse, McpTransport, oauth_state,
};
use nomifun_app_server::{
    agent_store::AgentStoreConfig, ConnectorAuthProvider, ConnectorCatalogProvider,
    ModelCatalogProvider, SkillCatalogProvider, MAX_CONNECTOR_TOOLS_BYTES,
};
use nomifun_common::{AppError, McpServerStatus};
use nomifun_extension::skill_service::{
    self, SkillListItem, SkillOrigin, SkillPaths, SkillSource,
};
use nomifun_mcp::{McpConfigService, McpConnectionTestService, McpOAuthService, sanitize_oauth_error};

// ---------------------------------------------------------------------------
// Skill catalog
// ---------------------------------------------------------------------------

/// Read-only Skill catalog over the system skill corpus.
///
/// `source` / `compatibility_status` are derived until the Agent Store
#[derive(Clone)]
pub struct AppServerSkillCatalog {
    paths: SkillPaths,
    /// Marketplace icon resolver for installed products (installed panel).
    assets: Option<crate::app_server_entry_assets::AppServerEntryAssets>,
}

impl AppServerSkillCatalog {
    pub fn new(paths: SkillPaths) -> Self {
        Self { paths, assets: None }
    }

    /// Wire marketplace icon resolution (composition root).
    pub fn with_assets(
        mut self,
        assets: crate::app_server_entry_assets::AppServerEntryAssets,
    ) -> Self {
        self.assets = Some(assets);
        self
    }

    async fn list_items(&self) -> Result<Vec<SkillListItem>, AppError> {
        skill_service::list_available_skills(&self.paths)
            .await
            .map_err(|error| AppError::Internal(format!("list agent-store skills: {error}")))
    }

    /// Avatar URL for one skill: marketplace products borrow their market
    /// entry icon; builtin / user skills have none.
    async fn avatar_for(&self, location: &str) -> Option<String> {
        let assets = self.assets.as_ref()?;
        let (snapshot_id, slug) = managed_snapshot_provenance(&self.paths, Path::new(location))?;
        assets.avatar_for_snapshot_slug(&snapshot_id, Some(&slug)).await
    }

    /// Localized tags for one skill: marketplace products read the tags their
    /// market index row declared (stamped into the snapshot payload, `44`);
    /// builtin / user skills have none.
    async fn tags_for(&self, location: &str) -> Option<Vec<AppServerLocalizedText>> {
        let assets = self.assets.as_ref()?;
        let (snapshot_id, slug) = managed_snapshot_provenance(&self.paths, Path::new(location))?;
        assets.tags_for_skill(&snapshot_id, Some(&slug)).await
    }
}

fn skill_summary(
    item: SkillListItem,
    origin: SkillOrigin,
    writable: bool,
    avatar_url: Option<String>,
    tags: Vec<AppServerLocalizedText>,
) -> AppServerSkillSummary {
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
        avatar_url,
        tags,
    }
}

/// Marketplace provenance of an installed skill, read from its on-disk
/// location (`{user_skills_dir}/agent-store/<snapshot_id>/<slug>`):
/// `Some((snapshot_id, slug))` for a market product, `None` for builtin and
/// user skills (which have no marketplace entry to borrow an icon from).
fn managed_snapshot_provenance(
    paths: &SkillPaths,
    location: &Path,
) -> Option<(String, String)> {
    let relative = location.strip_prefix(&paths.user_skills_dir).ok()?;
    let mut components = relative.components();
    let marker = components.next()?.as_os_str().to_str()?;
    if marker != "agent-store" {
        return None;
    }
    let snapshot_id = components.next()?.as_os_str().to_str()?.to_owned();
    let slug = components.next()?.as_os_str().to_str()?.to_owned();
    Some((snapshot_id, slug))
}

#[async_trait]
impl SkillCatalogProvider for AppServerSkillCatalog {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError> {
        let mut out = Vec::new();
        for item in self.list_items().await? {
            let origin = skill_service::skill_origin_of(&self.paths, Path::new(&item.location));
            let writable =
                skill_service::is_writable_skill(&self.paths, &item.name, Path::new(&item.location));
            let avatar_url = self.avatar_for(&item.location).await;
            let tags = self.tags_for(&item.location).await.unwrap_or_default();
            out.push(skill_summary(item, origin, writable, avatar_url, tags));
        }
        Ok(out)
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
        let avatar_url = self.avatar_for(&item.location).await;
        let tags = self.tags_for(&item.location).await.unwrap_or_default();
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
            summary: skill_summary(item, origin, writable, avatar_url, tags),
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

/// The connector's **transport shape**, projected into the summary's `auth_mode`
/// field. Display-only: it never decides anything. What a connector actually
/// needs is `credential.mode` (`34` §6.1), and the transport guess itself lives
/// in exactly one place — [`crate::app_server_credentials::credential_mode`].
fn transport_auth_shape(transport: &McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio { .. } => "none",
        McpTransport::Sse { .. } | McpTransport::Http { .. } => "oauth",
    }
}

/// Whether this connector offers an OAuth entry point at all.
///
/// The declaration wins: a marketplace connector declaring `server-side` /
/// `mcp` / `oneid-token` / nothing is answered `none` by the importer's mapping
/// (`34` §6.1) and must never grow an authorization button. Only a connector
/// with no declaration falls back to the transport guess — through the *same*
/// function the credential block uses, so the guess exists in one place, not
/// two. A host with no declaration reader wired (tests, no import history)
/// takes that same fallback instead of losing the entry point.
fn needs_auth_for(
    credential: Option<&AppServerConnectorCredential>,
    transport: &McpTransport,
) -> bool {
    match credential {
        Some(credential) => credential.mode == AppServerCredentialMode::Oauth,
        None => crate::app_server_credentials::credential_mode(None, transport)
            == AppServerCredentialMode::Oauth,
    }
}

fn summary_status(
    enabled: bool,
    last_test: McpServerStatus,
    needs_auth: bool,
) -> AppServerConnectorStatus {
    if !enabled {
        return AppServerConnectorStatus::Installed;
    }
    match last_test {
        McpServerStatus::Error => AppServerConnectorStatus::Error,
        McpServerStatus::Connected => AppServerConnectorStatus::Connected,
        _ if needs_auth => AppServerConnectorStatus::AuthorizationRequired,
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
    avatar_url: Option<String>,
    credential: Option<&AppServerConnectorCredential>,
) -> AppServerConnectorSummary {
    let (kind, transport_summary) = transport_kind(transport);
    let needs_auth = needs_auth_for(credential, transport);
    AppServerConnectorSummary {
        id,
        name,
        description,
        kind: kind.to_owned(),
        transport_summary,
        auth_transport: transport_auth_shape(transport).to_owned(),
        enabled,
        status: summary_status(enabled, last_test, needs_auth),
        avatar_url,
        // The credential block is read *before* the summary is built: the status
        // below depends on it, so attaching it afterwards was an ordering bug
        // that let the transport guess outvote the declaration.
        credential: credential.cloned(),
    }
}

fn probe_status(
    enabled: bool,
    authenticated: bool,
    needs_auth: bool,
    last_test: McpServerStatus,
) -> AppServerConnectorStatus {
    if !enabled {
        return AppServerConnectorStatus::Installed;
    }
    match last_test {
        McpServerStatus::Error => AppServerConnectorStatus::Error,
        _ if needs_auth && !authenticated => AppServerConnectorStatus::AuthorizationRequired,
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
    /// Marketplace icon resolver for connectors installed from a market.
    assets: Option<crate::app_server_entry_assets::AppServerEntryAssets>,
    /// Reads the credential declaration an imported connector shipped (`34` §5.2).
    /// Absent in tests and in hosts with no import history: the summary then
    /// simply carries no `credential` block.
    credentials: Option<crate::app_server_credentials::AppServerConnectorCredentials>,
}

impl AppServerConnectorCatalog {
    pub fn new(
        config: McpConfigService,
        connection_test: McpConnectionTestService,
        oauth: McpOAuthService,
    ) -> Self {
        Self { config, connection_test, oauth, assets: None, credentials: None }
    }

    /// Wire the credential declaration reader (composition root).
    pub fn with_credentials(
        mut self,
        credentials: crate::app_server_credentials::AppServerConnectorCredentials,
    ) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// The `credential` block for one server, as `principal` sees it.
    async fn credential_block(
        &self,
        mcp_server_id: &str,
        server: &nomifun_api_types::McpServerResponse,
        principal: Option<&str>,
    ) -> Option<nomifun_api_types::AppServerConnectorCredential> {
        let reader = self.credentials.as_ref()?;
        reader
            .describe(
                mcp_server_id,
                &server.transport,
                server.last_test_status,
                principal,
            )
            .await
    }

    /// Wire marketplace icon resolution (composition root).
    pub fn with_assets(
        mut self,
        assets: crate::app_server_entry_assets::AppServerEntryAssets,
    ) -> Self {
        self.assets = Some(assets);
        self
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

    /// The `authorization_pending` view for a connector whose browser flow is
    /// in flight right now — `None` when none is open.
    ///
    /// An open flow outranks the stored credential at every site that reports
    /// an auth state (`get` / `status` / `auth_status`): a caller watching the
    /// browser window is asking "is it running?", while a stored credential
    /// answers "is the previous one still usable?" — during a re-authorization
    /// the two disagree, and the client polling for progress needs the first
    /// answer. `06` §8 has named this state since the design; the App Server
    /// never reported it, so "a login is in progress" and "no login was ever
    /// started" were the same answer.
    fn pending_auth_view(&self, transport: &McpTransport) -> Option<AppServerOAuthStatusView> {
        let url = transport_url(transport)?;
        self.oauth
            .login_pending(&url)
            .then(|| AppServerOAuthStatusView {
                state: oauth_state::AUTHORIZATION_PENDING.to_owned(),
                error: None,
            })
    }
}

#[async_trait]
impl ConnectorCatalogProvider for AppServerConnectorCatalog {
    async fn list(&self, principal: Option<&str>) -> Result<Vec<AppServerConnectorSummary>, AppError> {
        let servers = self.config.list_servers().await.map_err(AppError::from)?;
        let mut out = Vec::with_capacity(servers.len());
        for server in servers {
            let id = server.mcp_server_id.as_str().to_owned();
            let avatar_url = match self.assets.as_ref() {
                Some(assets) => assets.avatar_for_mcp_server(&id).await,
                None => None,
            };
            let credential = self.credential_block(&id, &server, principal).await;
            out.push(connector_summary(
                id.clone(),
                server.name.clone(),
                server.description.clone(),
                server.enabled,
                &server.transport,
                server.last_test_status,
                avatar_url,
                credential.as_ref(),
            ));
        }
        Ok(out)
    }

    async fn get(
        &self,
        id: &str,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorDetail, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        // Cloned, not moved: the `credential` block below still needs the whole
        // server row (its `values` live inside the transport).
        let transport = server.transport.clone();
        let avatar_url = match self.assets.as_ref() {
            Some(assets) => assets.avatar_for_mcp_server(&connector_id).await,
            None => None,
        };
        let credential = self.credential_block(&connector_id, &server, principal).await;
        let needs_auth = needs_auth_for(credential.as_ref(), &transport);
        let summary = connector_summary(
            connector_id.clone(),
            server.name.clone(),
            server.description.clone(),
            server.enabled,
            &transport,
            server.last_test_status,
            avatar_url,
            credential.as_ref(),
        );
        let auth_status = if needs_auth {
            match self.pending_auth_view(&transport) {
                Some(pending) => Some(pending),
                None => self.oauth_authenticated(&transport).await.map(|authenticated| {
                    AppServerOAuthStatusView {
                        state: if authenticated { "authenticated".into() } else { "not_authenticated".into() },
                        error: None,
                    }
                }),
            }
        } else {
            None
        };
        let (tools, tools_truncated) = connector_tools(server.tools);
        Ok(AppServerConnectorDetail {
            summary,
            tool_filter: Some(format!("connector__{connector_id}__<tool>")),
            tools: tools.unwrap_or_default(),
            tools_truncated,
            auth_status,
            source: if server.builtin { "builtin".into() } else { "system".into() },
            compatibility_status: AppServerCompatibilityStatus::Compatible,
        })
    }

    async fn status(
        &self,
        id: &str,
        _principal: Option<&str>,
    ) -> Result<AppServerConnectorStatusView, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        // Borrowed before the transport moves out below: the credential block
        // reads the whole server row (its `values` live inside the transport).
        let credential = self.credential_block(&connector_id, &server, principal).await;
        let transport = server.transport;
        let needs_auth = needs_auth_for(credential.as_ref(), &transport);
        let authenticated = if needs_auth {
            self.oauth_authenticated(&transport).await.unwrap_or(false)
        } else {
            false
        };
        let status = probe_status(server.enabled, authenticated, needs_auth, server.last_test_status);
        Ok(AppServerConnectorStatusView {
            connector_id,
            status,
            auth_status: if needs_auth {
                match self.pending_auth_view(&transport) {
                    Some(pending) => Some(pending),
                    None => Some(AppServerOAuthStatusView {
                        state: if authenticated { "authenticated".into() } else {
                            if status == AppServerConnectorStatus::AuthorizationRequired {
                                "not_authenticated".into()
                            } else {
                                "reauthorization_required".into()
                            }
                        },
                        error: None,
                    }),
                }
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
        self.test_for(id, None).await
    }

    async fn test_for(
        &self,
        id: &str,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorProbeResult, AppError> {
        let server = self.get_server(id).await?;
        let connector_id = server.mcp_server_id.as_str().to_owned();
        let transport = nomifun_mcp::McpServerTransport::from(server.transport);
        let result = self
            .connection_test
            .test_connection_for(&server.name, &transport, principal)
            .await;
        self.config
            .persist_test_result(&server.mcp_server_id, &result)
            .await
            .map_err(AppError::from)?;
        Ok(probe_result(connector_id, result))
    }

    /// Register a connector the host never imported (`34` §6.5).
    ///
    /// The MCP configuration's own `add_server` is the whole write: it upserts by
    /// name (so re-registering updates rather than duplicating), refuses a builtin
    /// server's name, and persists the row **disabled** — the template is stored as
    /// handed in, credentials and all, and nothing about it is resolved here.
    ///
    /// Returned as this caller's detail, which is the point: the caller immediately
    /// sees the form its own template declared (`mode: token`, the `${secret:NAME}`
    /// names as fields) and which of them are still missing.
    async fn register(
        &self,
        registration: nomifun_api_types::AppServerConnectorRegistration,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorDetail, AppError> {
        let name = registration.name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest(
                "a connector name must not be empty".to_owned(),
            ));
        }
        let created = self
            .config
            .add_server(nomifun_api_types::CreateMcpServerRequest {
                name: name.to_owned(),
                description: registration
                    .description
                    .map(|description| description.trim().to_owned())
                    .filter(|description| !description.is_empty()),
                transport: registration.transport,
                original_json: None,
                builtin: false,
            })
            .await
            .map_err(AppError::from)?;
        let connector_id = created.mcp_server_id.as_str().to_owned();
        self.get(&connector_id, principal).await
    }
}

/// Project upstream tools onto the wire shape (doc `26` §5).
///
/// Names and descriptions always survive; `input_schema` is carried **whole** or
/// not at all, and the running budget is [`MAX_CONNECTOR_TOOLS_BYTES`]. The
/// returned flag is what makes an omission honest instead of silent.
fn connector_tools(
    tools: Option<Vec<McpToolResponse>>,
) -> (Option<Vec<AppServerConnectorTool>>, bool) {
    let Some(tools) = tools else {
        return (None, false);
    };
    let mut budget = MAX_CONNECTOR_TOOLS_BYTES;
    let mut truncated = false;
    let projected = tools
        .into_iter()
        .map(|tool| {
            let input_schema = match tool.input_schema {
                Some(schema) => {
                    // A schema that cannot be serialized cannot travel either, so
                    // it is treated as an omission rather than an error.
                    let size = serde_json::to_vec(&schema)
                        .map(|bytes| bytes.len())
                        .unwrap_or(usize::MAX);
                    if size <= budget {
                        budget -= size;
                        Some(schema)
                    } else {
                        truncated = true;
                        None
                    }
                }
                None => None,
            };
            AppServerConnectorTool {
                name: tool.name,
                description: tool.description,
                input_schema,
            }
        })
        .collect();
    (Some(projected), truncated)
}

fn probe_result(connector_id: String, result: McpConnectionTestResult) -> AppServerConnectorProbeResult {
    let (tools, tools_truncated) = connector_tools(result.tools);
    AppServerConnectorProbeResult {
        connector_id,
        success: result.success,
        tools,
        error: result.error,
        code: result.code.map(|code| code.as_str().to_owned()),
        tools_truncated,
    }
}

// ---------------------------------------------------------------------------
// Connector OAuth pass-through
// ---------------------------------------------------------------------------

/// OAuth pass-through: the trusted host owns the browser flow. `auth_start`
/// reports a failure that happens *before* the browser opens — nothing was
/// shown to the user, so there is nothing to wait for — and acknowledges the
/// rest, which clients follow by polling `auth_status`, whose `error` carries a
/// flow that failed after the browser step.
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
        // An open flow outranks the stored credential — see
        // [`AppServerConnectorCatalog::pending_auth_view`]. This is the method a
        // client polls while the user is in the browser, so it is the one place
        // the distinction has to survive.
        if self.oauth.login_pending(&url) {
            return Ok(AppServerOAuthStatusView {
                state: oauth_state::AUTHORIZATION_PENDING.to_owned(),
                error: None,
            });
        }
        let status = self.oauth.check_oauth_status(&url).await.map_err(AppError::from)?;
        if status.authenticated {
            return Ok(AppServerOAuthStatusView {
                state: "authenticated".into(),
                error: None,
            });
        }
        Ok(AppServerOAuthStatusView {
            state: "not_authenticated".into(),
            // The flow outlives the request that starts it, so for anything
            // that fails after the browser step this is the only channel that
            // can carry the reason to the client.
            error: self.oauth.last_login_error(&url).await,
        })
    }

    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError> {
        let url = self.remote_url(id).await?;
        // One INFO per *requested* login: the attempt count is what makes a
        // server-side throttle readable from a log ("too many OAuth requests"
        // cannot be diagnosed from completions alone).
        tracing::info!(connector_id = id, url = %url, "connector OAuth login requested");
        // Start only the synchronous half first (endpoint discovery, client
        // identity, loopback callback, browser launch). A failure there means
        // nothing was shown to the user, so acknowledging `started` would send
        // the client off to wait for a browser window that never opened.
        let oauth = self.oauth.clone();
        let started = match oauth.begin_login(&url).await {
            Ok(started) => started,
            Err(error) => {
                let message = sanitize_oauth_error(&error);
                tracing::warn!(url = %url, error = %message, "MCP OAuth login failed");
                return Ok(AppServerOAuthStartResult {
                    connector_id: id.to_owned(),
                    state: "error".into(),
                    error: Some(message),
                });
            }
        };

        // The browser is open; the callback wait and the token exchange run on
        // the trusted host's PKCE callback (server-side callback + encrypted
        // token storage) and can take minutes, so a single connection must not
        // block on them. Clients poll `auth_status` — whose `error` carries
        // whatever this task ends up reporting.
        tokio::spawn(async move {
            let result = started.complete().await;
            if result.success {
                tracing::info!(url = %url, "MCP OAuth login completed");
            } else {
                tracing::warn!(
                    url = %url,
                    error = result.error.as_deref().unwrap_or("unknown"),
                    "MCP OAuth login failed"
                );
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

#[cfg(test)]
mod connector_tools_tests {
    use super::*;

    fn tool(name: &str, schema: Option<serde_json::Value>) -> McpToolResponse {
        McpToolResponse {
            name: name.to_owned(),
            description: Some(format!("{name} description")),
            input_schema: schema,
        }
    }

    /// The point of the read face: what the caller sees is what the server said,
    /// byte for byte — not a re-rendering of it.
    #[test]
    fn schemas_are_carried_verbatim() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "url": { "type": "string" } },
            "required": ["url"],
        });
        let (tools, truncated) = connector_tools(Some(vec![
            tool("navigate", Some(schema.clone())),
            tool("click", None),
        ]));
        let tools = tools.expect("tools");
        assert_eq!(tools[0].input_schema.as_ref(), Some(&schema));
        assert_eq!(
            tools[1].input_schema, None,
            "a tool the server published no schema for keeps none"
        );
        assert!(!truncated);
    }

    /// A failed probe reports no tool list at all, which is not an empty list.
    #[test]
    fn an_absent_tool_list_stays_absent() {
        let (tools, truncated) = connector_tools(None);
        assert!(tools.is_none());
        assert!(!truncated);
    }

    /// Oversized schemas are dropped **whole** (never halved) and the flag says
    /// so; names and descriptions are never the thing that gets sacrificed.
    #[test]
    fn oversized_schemas_are_omitted_whole_and_reported() {
        let huge = serde_json::json!({ "description": "x".repeat(MAX_CONNECTOR_TOOLS_BYTES) });
        let (tools, truncated) = connector_tools(Some(vec![
            tool("small", None),
            tool("huge", Some(huge)),
        ]));
        let tools = tools.expect("tools");
        assert_eq!(tools.len(), 2, "every tool keeps its name and description");
        assert!(tools[0].input_schema.is_none());
        assert!(tools[1].input_schema.is_none());
        assert_eq!(tools[1].description.as_deref(), Some("huge description"));
        assert!(truncated);
    }

    /// The budget is spent in list order, so which schema survives is
    /// deterministic rather than dependent on iteration order.
    #[test]
    fn the_budget_is_spent_in_order() {
        let take = MAX_CONNECTOR_TOOLS_BYTES * 3 / 5;
        let first = serde_json::json!({ "description": "y".repeat(take) });
        let second = serde_json::json!({ "description": "y".repeat(take) });
        let (tools, truncated) =
            connector_tools(Some(vec![tool("a", Some(first.clone())), tool("b", Some(second))]));
        let tools = tools.expect("tools");
        assert_eq!(tools[0].input_schema.as_ref(), Some(&first));
        assert!(tools[1].input_schema.is_none());
        assert!(truncated);
    }
}

/// The in-flight half of the connector auth surface.
///
/// `connector/auth/status` is what a client polls while the user is in the
/// browser, so "a login is running" must be distinguishable from "no login was
/// ever started" — and only for the connector that is actually running one.
#[cfg(test)]
mod connector_auth_tests {
    use super::*;
    use std::time::Duration;

    /// A bound socket nobody reads from: the flow parks inside discovery, which
    /// is exactly the window a client polls from. Held by the caller so the port
    /// stays bound.
    async fn silent_endpoint() -> (tokio::net::TcpListener, String) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/mcp", listener.local_addr().expect("addr"));
        (listener, url)
    }

    /// One connector registered over an in-memory database, plus the auth
    /// provider the App Server answers `connector/auth/*` from.
    async fn connector(
        name: &str,
        url: &str,
    ) -> (AppServerConnectorAuth, String, McpOAuthService) {
        let db = nomifun_db::init_database_memory().await.unwrap();
        let pool = db.pool().clone();
        // The database handle owns the pool; leaking it keeps the connection
        // alive for the duration of the test (same shape as the model tests).
        std::mem::forget(db);
        let config = McpConfigService::new(std::sync::Arc::new(
            nomifun_db::SqliteMcpServerRepository::new(pool.clone()),
        ));
        let oauth = McpOAuthService::new(
            std::sync::Arc::new(nomifun_db::SqliteOAuthTokenRepository::new(pool)),
            reqwest::Client::builder().no_proxy().build().expect("client"),
        );
        let created = config
            .add_server(
                serde_json::from_value(serde_json::json!({
                    "name": name,
                    "transport": { "type": "http", "url": url },
                }))
                .expect("request"),
            )
            .await
            .expect("add server");
        let id = created.mcp_server_id.to_string();
        (AppServerConnectorAuth::new(config, oauth.clone()), id, oauth)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auth_status_answers_pending_for_the_connector_being_authorized() {
        let (_endpoint, url) = silent_endpoint().await;
        let (authorized, authorized_id, oauth) = connector("fenbi_mcp", &url).await;
        let (_other, other_url) = silent_endpoint().await;
        let (idle, idle_id, _other_oauth) = connector("other_mcp", &other_url).await;

        // Nothing started yet: both are simply unauthenticated.
        assert_eq!(
            authorized.auth_status(&authorized_id).await.unwrap().state,
            oauth_state::NOT_AUTHENTICATED
        );

        let flow = tokio::spawn({
            let oauth = oauth.clone();
            let url = url.clone();
            async move { oauth.begin_login(&url).await }
        });
        // The mark is taken before discovery issues its first request, so it is
        // observable without racing the (silent) endpoint.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !oauth.login_pending(&url) {
            assert!(tokio::time::Instant::now() < deadline, "the flow never started");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert_eq!(
            authorized.auth_status(&authorized_id).await.unwrap().state,
            oauth_state::AUTHORIZATION_PENDING,
            "the connector with an open browser flow reports that it is running"
        );
        assert_eq!(
            idle.auth_status(&idle_id).await.unwrap().state,
            oauth_state::NOT_AUTHENTICATED,
            "another connector must not borrow that flow's state"
        );

        flow.abort();
        let _ = flow.await;
        assert_eq!(
            authorized.auth_status(&authorized_id).await.unwrap().state,
            oauth_state::NOT_AUTHENTICATED,
            "the state is gone with the flow"
        );
    }

    /// `46` TC-FIX-001 / 003 / 004: the OAuth entry point follows the
    /// declaration. A url-shaped connector that declares `server-side`
    /// (normalized to `none` by the importer) must not grow an authorization
    /// button — the transport guess may never outvote what the market declared.
    /// With no declaration at all the same fallback the credential block uses
    /// applies, so a hand-registered remote server keeps its entry point.
    #[test]
    fn needs_auth_follows_the_declaration_not_the_transport_shape() {
        let url = McpTransport::Http {
            url: "https://example.test/mcp".to_owned(),
            headers: Default::default(),
            values: Default::default(),
        };
        let stdio = McpTransport::Stdio {
            command: "npx".to_owned(),
            args: vec!["@playwright/mcp".to_owned()],
            env: Default::default(),
        };
        let declared = |mode: &str| -> AppServerConnectorCredential {
            serde_json::from_value(serde_json::json!({
                "connector_id": "netdisk",
                "mode": mode,
                "status": "not_required",
            }))
            .expect("a credential block")
        };

        assert!(!needs_auth_for(Some(&declared("none")), &url));
        assert!(needs_auth_for(Some(&declared("oauth")), &url));
        assert!(!needs_auth_for(Some(&declared("token")), &url));
        assert!(needs_auth_for(None, &url));
        assert!(!needs_auth_for(None, &stdio));
    }

    /// `46` TC-FIX-003 at the projection level: a host with **no declaration
    /// reader wired** (no import history, tests) must not lose the fallback.
    /// A hand-registered remote server keeps its authorization entry point; a
    /// stdio one never grows one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_host_without_a_declaration_reader_keeps_the_transport_fallback() {
        let db = nomifun_db::init_database_memory().await.unwrap();
        let pool = db.pool().clone();
        std::mem::forget(db);
        let config = McpConfigService::new(std::sync::Arc::new(
            nomifun_db::SqliteMcpServerRepository::new(pool.clone()),
        ));
        let oauth = McpOAuthService::new(
            std::sync::Arc::new(nomifun_db::SqliteOAuthTokenRepository::new(pool)),
            reqwest::Client::builder().no_proxy().build().expect("client"),
        );
        let connection_test =
            McpConnectionTestService::new(reqwest::Client::builder().no_proxy().build().expect("client"));
        let catalog = AppServerConnectorCatalog::new(config, connection_test, oauth);

        for (name, transport) in [
            ("remote_mcp", serde_json::json!({ "type": "http", "url": "https://mcp.example.com/mcp" })),
            ("local_mcp", serde_json::json!({ "type": "stdio", "command": "npx", "args": ["-y", "some-mcp"] })),
        ] {
            let created = catalog
                .config
                .add_server(serde_json::from_value(serde_json::json!({
                    "name": name,
                    "transport": transport,
                }))
                .expect("request"))
                .await
                .expect("add server");
            // A fresh row is disabled, and a disabled connector reports
            // `installed` before any auth question is asked — enable it so the
            // status under test is the auth decision, not the enablement one.
            catalog
                .config
                .set_server_enabled(&created.mcp_server_id, true)
                .await
                .expect("enable server");
        }

        let summaries = catalog.list(None).await.expect("list");
        let by_name = |name: &str| {
            summaries
                .iter()
                .find(|summary| summary.name == name)
                .unwrap_or_else(|| panic!("{name} must be listed"))
        };
        assert_eq!(
            by_name("remote_mcp").status,
            AppServerConnectorStatus::AuthorizationRequired,
            "an undeclared remote server keeps the OAuth entry point"
        );
        assert_ne!(
            by_name("local_mcp").status,
            AppServerConnectorStatus::AuthorizationRequired,
            "a stdio server never grows one"
        );
        assert!(by_name("local_mcp").credential.is_none());
    }

    /// `46` TC-FIX-001: the declaration decides the entry point, even for a
    /// url-shaped connector. A marketplace entry declaring `server-side` (the
    /// importer normalizes it to `none`) used to be reported
    /// `authorization_required` with a `not_authenticated` `auth_status`, so the
    /// UI grew a button whose flow could only die in discovery — github does not
    /// even publish RFC 8414 metadata. A declared `oauth` entry keeps its entry
    /// point, so the fix removes the false affordance, not the real one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_declared_none_connector_gets_no_authorization_entry_point() {
        let db = nomifun_db::init_database_memory().await.unwrap();
        let pool = db.pool().clone();
        std::mem::forget(db);
        let client = reqwest::Client::builder().no_proxy().build().expect("client");
        let config = McpConfigService::new(std::sync::Arc::new(
            nomifun_db::SqliteMcpServerRepository::new(pool.clone()),
        ));
        let oauth = McpOAuthService::new(
            std::sync::Arc::new(nomifun_db::SqliteOAuthTokenRepository::new(pool.clone())),
            client.clone(),
        );
        let snapshots = std::sync::Arc::new(nomifun_db::SqlitePluginSnapshotRepository::new(pool));
        let catalog = AppServerConnectorCatalog::new(config, McpConnectionTestService::new(client), oauth)
            .with_credentials(crate::app_server_credentials::AppServerConnectorCredentials::new(
                snapshots.clone(),
            ));

        // Both entries are url-shaped; only the declaration differs.
        let mut ids = std::collections::HashMap::new();
        for (name, declared) in [("netdisk", "none"), ("flova", "oauth")] {
            let created = catalog
                .config
                .add_server(
                    serde_json::from_value(serde_json::json!({
                        "name": name,
                        "transport": { "type": "http", "url": format!("https://{name}.example.com/mcp") },
                    }))
                    .expect("request"),
                )
                .await
                .expect("add server");
            catalog
                .config
                .set_server_enabled(&created.mcp_server_id, true)
                .await
                .expect("enable server");
            let mcp_server_id = created.mcp_server_id.as_str().to_owned();

            // The marketplace snapshot that registered this row: its
            // `connector` component carries the normalized `auth_mode`.
            let snapshot_id = nomifun_common::generate_id();
            let component_id = format!("wb-{name}");
            let payload =
                serde_json::json!({ "connector_id": name, "auth_mode": declared }).to_string();
            nomifun_db::IPluginSnapshotRepository::insert_snapshot_with_components(
                snapshots.as_ref(),
                nomifun_db::NewPluginSnapshot {
                    snapshot_id: &snapshot_id,
                    name,
                    version: "1.0.0",
                    source_kind: "codebuddy-connector",
                    source_uri: Some("/local/market"),
                    plugin_id: name,
                    declared_version: "1.0.0",
                    resolved_revision: None,
                    content_digest: "digest",
                    status: "completed",
                    marketplace_id: Some("market"),
                    entry_name: Some(name),
                    source_revision: None,
                    components: vec![nomifun_db::NewPluginSnapshotComponent {
                        component_id: &component_id,
                        kind: "connector",
                        name,
                        relative_path: None,
                        compatibility_json: "{}",
                        payload_json: &payload,
                    }],
                })
                .await
                .expect("insert snapshot");
            nomifun_db::IPluginSnapshotRepository::mark_components_installed(
                snapshots.as_ref(),
                &[nomifun_db::ComponentRuntimeRef {
                    snapshot_id: &snapshot_id,
                    component_id: &component_id,
                    runtime_type: "connector",
                    location: name,
                    mcp_server_id: Some(&mcp_server_id),
                }],
                nomifun_common::now_ms(),
            )
            .await
            .expect("mark installed");
            ids.insert(name, mcp_server_id);
        }

        let summaries = catalog.list(None).await.expect("list");
        let by_name = |name: &str| {
            summaries
                .iter()
                .find(|summary| summary.name == name)
                .unwrap_or_else(|| panic!("{name} must be listed"))
        };
        assert_eq!(
            by_name("netdisk").status,
            AppServerConnectorStatus::Configured,
            "a declared `server-side` connector needs no authorization"
        );
        assert_eq!(
            by_name("netdisk").credential.as_ref().map(|c| c.mode),
            Some(AppServerCredentialMode::None)
        );
        assert_eq!(
            by_name("flova").status,
            AppServerConnectorStatus::AuthorizationRequired,
            "a declared `oauth` connector keeps its entry point"
        );

        let detail = catalog
            .get(ids.get("netdisk").expect("id"), None)
            .await
            .expect("detail");
        assert!(
            detail.auth_status.is_none(),
            "no OAuth state to report for a connector that needs none"
        );
    }
}
