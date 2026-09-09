//! Agent Store catalog provider seams for the App Server protocol.
//!
//! The App Server protocol layer stays decoupled from the system Skill / MCP
//! services: production adapters are implemented in the composition root
//! (`nomifun-app`), while tests use the fakes below. Providers return
//! `nomifun_common::AppError`, which the protocol layer maps onto its stable
//! wire codes (`not_found`, `connector_unavailable`, ...).

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerAgentDetail, AppServerAgentSummary, AppServerCompatibilityTriple,
    AppServerConnectorDetail, AppServerConnectorProbeResult, AppServerConnectorStatusView,
    AppServerConnectorSummary, AppServerImportDetail, AppServerImportRequest,
    AppServerImportResult, AppServerImportSummary, AppServerInstallRequest, AppServerInstallResult,
    AppServerInstallStatus, AppServerMarketplaceAddRequest, AppServerMarketplaceDetail,
    AppServerMarketplaceEntry, AppServerMarketplaceRefreshResult,
    AppServerMarketplaceRemoveResult, AppServerMarketplaceSummary,
    AppServerModelList, AppServerModelSummary,
    AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDetail, AppServerSkillSummary, AppServerStoreInstallResult,
    AppServerStoreItem, AppServerStoreList, AppServerTeamDetail, AppServerTeamSummary,
};
use nomifun_common::AppError;

/// Read-side Skill catalog (`skill/list`, `skill/get`).
#[async_trait]
pub trait SkillCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError>;
}

/// Read-side Connector catalog + status/probe (`connector/list|get|status|test`).
#[async_trait]
pub trait ConnectorCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerConnectorSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerConnectorDetail, AppError>;
    async fn status(&self, id: &str) -> Result<AppServerConnectorStatusView, AppError>;
    async fn test(&self, id: &str) -> Result<AppServerConnectorProbeResult, AppError>;
}

/// Connector OAuth pass-through. Only states and public errors cross this
/// seam; tokens never do.
#[async_trait]
pub trait ConnectorAuthProvider: Send + Sync {
    async fn auth_status(&self, id: &str) -> Result<AppServerOAuthStatusView, AppError>;
    /// Kick off the browser flow on the trusted host and return immediately.
    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError>;
    async fn logout(&self, id: &str) -> Result<(), AppError>;
}

/// Import pipeline seam (`import/run`, `import/list`, `import/get`). The
/// source path is resolved and copied on the trusted host; providers return
/// only public projections (no absolute source paths, no credentials).
#[async_trait]
pub trait ImportProvider: Send + Sync {
    async fn run(&self, request: AppServerImportRequest) -> Result<AppServerImportResult, AppError>;
    async fn list(&self, limit: u32) -> Result<Vec<AppServerImportSummary>, AppError>;
    async fn get(&self, snapshot_id: &str) -> Result<AppServerImportDetail, AppError>;
}

/// Installer seam (`install/run|status|enable|disable|uninstall`, roadmap
/// Phase 2). Installation registers an imported snapshot's components into
/// the runtime; providers return only public projections.
#[async_trait]
pub trait InstallProvider: Send + Sync {
    /// Install a snapshot: register its components into the runtime.
    async fn install(&self, request: AppServerInstallRequest) -> Result<AppServerInstallResult, AppError>;

    /// Current per-component installation state for a snapshot.
    async fn status(&self, snapshot_id: &str) -> Result<AppServerInstallStatus, AppError>;

    /// Disable installed components (runtime artifacts stay).
    async fn disable(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError>;

    /// Re-enable previously disabled components.
    async fn enable(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError>;

    /// Uninstall: remove runtime artifacts and clear the state (snapshot kept).
    async fn uninstall(&self, snapshot_id: &str, component_ids: &[String]) -> Result<AppServerInstallStatus, AppError>;
}

/// Marketplace seam (`market/add|list|get|remove|auto-update|entries/{e}/import`,
/// roadmap Phase 2). A marketplace is a catalog of discoverable plugins; adding
/// one registers its source, entries are imported through the normal snapshot
/// pipeline, and removal cascades uninstalls when confirmed.
#[async_trait]
pub trait MarketplaceProvider: Send + Sync {
    /// Register a marketplace source on the trusted host (directory in Phase A).
    async fn add(
        &self,
        request: AppServerMarketplaceAddRequest,
    ) -> Result<AppServerMarketplaceSummary, AppError>;

    /// All active marketplaces (registry projection, no entries).
    async fn list(&self) -> Result<Vec<AppServerMarketplaceSummary>, AppError>;

    /// One marketplace with its discovered entries.
    async fn get(&self, marketplace_id: &str) -> Result<AppServerMarketplaceDetail, AppError>;

    /// Remove a marketplace. `cascade` uninstalls snapshots installed from it
    /// (CodeBuddy semantics); snapshots themselves are kept.
    async fn remove(
        &self,
        marketplace_id: &str,
        cascade: bool,
    ) -> Result<AppServerMarketplaceRemoveResult, AppError>;

    /// Toggle auto-update for a marketplace.
    async fn set_auto_update(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<AppServerMarketplaceSummary, AppError>;

    /// Re-fetch the marketplace source and rebuild the entries projection when
    /// the resolved revision changed (freshness short-circuit otherwise).
    async fn refresh(
        &self,
        marketplace_id: &str,
    ) -> Result<AppServerMarketplaceRefreshResult, AppError>;

    /// Import one discovered entry (provenance links the snapshot back).
    async fn import_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerImportResult, AppError>;

    /// Resolve the on-disk root of one entry (internal path, trusted host
    /// only). Used by the store asset endpoint to serve display assets
    /// (avatars) of entries that are not yet imported.
    async fn entry_dir(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<std::path::PathBuf, AppError>;

    /// Resolve the on-disk root of one marketplace (internal path, trusted
    /// host only). Used by the store asset endpoint for market-level assets
    /// (`icons/…`) that live outside any single entry directory.
    async fn market_dir(&self, marketplace_id: &str) -> Result<std::path::PathBuf, AppError>;
}

/// Read-side Agent catalog (`agent/list`, `agent/get`, docs/agent-store/05
/// §4.1): Agent Store AgentDefinitions, never Runtime Agent instances.
#[async_trait]
pub trait AgentCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerAgentSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerAgentDetail, AppError>;
}

/// Unified store catalog seam (`store/list`, `store/install-entry`, roadmap
/// Phase 3): winget-style aggregated catalog over all enabled marketplaces.
/// Items carry display metadata (plugin.json fidelity) and the current local
/// install state; install-entry runs import + runtime registration in one
/// idempotent call so clients only ever see a single "Install" action.
#[async_trait]
pub trait StoreProvider: Send + Sync {
    /// All store items across enabled marketplaces (grouped client-side).
    async fn list(&self) -> Result<AppServerStoreList, AppError>;

    /// Install one marketplace entry: import missing snapshots and register
    /// their components into the runtime. Idempotent — an already-installed
    /// entry returns the current state.
    async fn install_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerStoreInstallResult, AppError>;
}

/// Read-side Team catalog (`team/list`, `team/get`, docs/agent-store/05 §4.2).
#[async_trait]
pub trait TeamCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerTeamSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerTeamDetail, AppError>;
}

/// Public model directory seam (`models/list`, REQ-PAR-05b). Projects the
/// provider registry into provider/model pairs; credentials and endpoints
/// never cross this seam.
#[async_trait]
pub trait ModelCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<AppServerModelList, AppError>;
}

// ---------------------------------------------------------------------------
// Test fakes
// ---------------------------------------------------------------------------

/// In-memory model catalog fake.
pub struct FakeModelCatalog {
    pub models: Vec<AppServerModelSummary>,
}

#[async_trait]
impl ModelCatalogProvider for FakeModelCatalog {
    async fn list(&self) -> Result<AppServerModelList, AppError> {
        Ok(AppServerModelList { items: self.models.clone() })
    }
}

/// In-memory Skill catalog fake.
pub struct FakeSkillCatalog {
    pub skills: Vec<AppServerSkillSummary>,
}

#[async_trait]
impl SkillCatalogProvider for FakeSkillCatalog {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError> {
        Ok(self.skills.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError> {
        let summary = self
            .skills
            .iter()
            .find(|skill| skill.id == id || skill.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("skill {id} not found")))?;
        Ok(AppServerSkillDetail {
            summary,
            mode: "store-agent".into(),
            invocation_policy: "model-auto".into(),
            instructions_summary: None,
        })
    }
}

/// In-memory Connector catalog fake. `auth_status` is derived from a mutable
/// set of authenticated connectors; `probe_fail` forces `test` to fail so the
/// `connected` merge rule can be verified.
pub struct FakeConnectorCatalog {
    pub connectors: Vec<AppServerConnectorSummary>,
    pub auth_required_ids: Vec<String>,
    pub probe_fail_ids: Vec<String>,
}

impl FakeConnectorCatalog {
    fn find(&self, id: &str) -> Result<AppServerConnectorSummary, AppError> {
        self.connectors
            .iter()
            .find(|connector| connector.id == id || connector.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("connector {id} not found")))
    }
}

#[async_trait]
impl ConnectorCatalogProvider for FakeConnectorCatalog {
    async fn list(&self) -> Result<Vec<AppServerConnectorSummary>, AppError> {
        Ok(self.connectors.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerConnectorDetail, AppError> {
        let summary = self.find(id)?;
        Ok(AppServerConnectorDetail {
            summary,
            tool_filter: Some("connector__<name>__<tool>".into()),
            tools: vec![],
            auth_status: None,
            source: "system".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
        })
    }

    async fn status(&self, id: &str) -> Result<AppServerConnectorStatusView, AppError> {
        let summary = self.find(id)?;
        let connector_id = summary.id.clone();
        let authenticated = !self.auth_required_ids.contains(&connector_id);
        let status = if !authenticated {
            nomifun_api_types::AppServerConnectorStatus::AuthorizationRequired
        } else if self.probe_fail_ids.contains(&connector_id) {
            nomifun_api_types::AppServerConnectorStatus::Error
        } else {
            nomifun_api_types::AppServerConnectorStatus::Connected
        };
        Ok(AppServerConnectorStatusView {
            connector_id,
            status,
            auth_status: Some(if authenticated {
                AppServerOAuthStatusView { state: "authenticated".into(), error: None }
            } else {
                AppServerOAuthStatusView { state: "not_authenticated".into(), error: None }
            }),
            last_error: if self.probe_fail_ids.contains(&summary.id) {
                Some("mock probe failed".into())
            } else {
                None
            },
        })
    }

    async fn test(&self, id: &str) -> Result<AppServerConnectorProbeResult, AppError> {
        let summary = self.find(id)?;
        let connector_id = summary.id.clone();
        let failed = self.probe_fail_ids.contains(&connector_id);
        Ok(AppServerConnectorProbeResult {
            connector_id,
            success: !failed,
            tools: None,
            error: if failed { Some("mock probe failed".into()) } else { None },
            code: if failed { Some("MCP_CONNECTION_FAILED".into()) } else { None },
        })
    }
}

/// In-memory OAuth fake: `start` flips the connector into the authenticated set.
pub struct FakeConnectorAuth {
    pub authenticated: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl ConnectorAuthProvider for FakeConnectorAuth {
    async fn auth_status(&self, id: &str) -> Result<AppServerOAuthStatusView, AppError> {
        let authenticated = self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?.iter().any(|value| value == id);
        Ok(AppServerOAuthStatusView {
            state: if authenticated { "authenticated".into() } else { "not_authenticated".into() },
            error: None,
        })
    }

    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError> {
        self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?.push(id.to_owned());
        Ok(AppServerOAuthStartResult { connector_id: id.to_owned(), state: "started".into(), error: None })
    }

    async fn logout(&self, id: &str) -> Result<(), AppError> {
        let mut authenticated = self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?;
        authenticated.retain(|value| value != id);
        Ok(())
    }
}

/// In-memory import fake: canned run result + history.
pub struct FakeImportProvider {
    pub run_result: AppServerImportResult,
    pub summaries: Vec<AppServerImportSummary>,
    pub detail: Option<AppServerImportDetail>,
}

impl FakeImportProvider {
    pub fn new() -> Self {
        Self {
            run_result: AppServerImportResult {
                snapshot_id: "snap-demo".into(),
                name: "demo".into(),
                version: "1.0.0".into(),
                source_kind: "codebuddy-plugin".into(),
                status: "completed".into(),
                content_digest: "abc".into(),
                component_status: AppServerCompatibilityTriple {
                    semantic_status: "compatible_with_adapter".into(),
                    runtime_status: "not-verified".into(),
                    distribution_status: "local-only".into(),
                    reasons: vec![],
                },
                component_count: 0,
                imported_at: 1,
                reused: false,
                warnings: vec![],
                errors: vec![],
            },
            summaries: vec![],
            detail: None,
        }
    }
}

impl Default for FakeImportProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ImportProvider for FakeImportProvider {
    async fn run(&self, _request: AppServerImportRequest) -> Result<AppServerImportResult, AppError> {
        Ok(self.run_result.clone())
    }

    async fn list(&self, _limit: u32) -> Result<Vec<AppServerImportSummary>, AppError> {
        Ok(self.summaries.clone())
    }

    async fn get(&self, snapshot_id: &str) -> Result<AppServerImportDetail, AppError> {
        self.detail
            .clone()
            .ok_or_else(|| AppError::NotFound(format!("snapshot {snapshot_id} not found")))
    }
}

/// In-memory install fake: canned install result + per-component state.
pub struct FakeInstallProvider {
    pub install_result: AppServerInstallResult,
    pub status: AppServerInstallStatus,
}

impl FakeInstallProvider {
    pub fn new() -> Self {
        Self {
            install_result: AppServerInstallResult {
                snapshot_id: "snap-demo".into(),
                name: "demo".into(),
                version: "1.0.0".into(),
                installed_count: 0,
                skipped: vec![],
                warnings: vec![],
                errors: vec![],
            },
            status: AppServerInstallStatus { snapshot_id: "snap-demo".into(), components: vec![] },
        }
    }
}

impl Default for FakeInstallProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl InstallProvider for FakeInstallProvider {
    async fn install(
        &self,
        _request: AppServerInstallRequest,
    ) -> Result<AppServerInstallResult, AppError> {
        Ok(self.install_result.clone())
    }

    async fn status(&self, snapshot_id: &str) -> Result<AppServerInstallStatus, AppError> {
        if snapshot_id != self.status.snapshot_id {
            return Err(AppError::NotFound(format!("snapshot {snapshot_id} not found")));
        }
        Ok(self.status.clone())
    }

    async fn disable(
        &self,
        _snapshot_id: &str,
        _component_ids: &[String],
    ) -> Result<AppServerInstallStatus, AppError> {
        Ok(self.status.clone())
    }

    async fn enable(
        &self,
        _snapshot_id: &str,
        _component_ids: &[String],
    ) -> Result<AppServerInstallStatus, AppError> {
        Ok(self.status.clone())
    }

    async fn uninstall(
        &self,
        _snapshot_id: &str,
        _component_ids: &[String],
    ) -> Result<AppServerInstallStatus, AppError> {
        Ok(self.status.clone())
    }
}

/// In-memory marketplace fake: one canned market + canned remove result.
pub struct FakeMarketplaceProvider {
    pub added: AppServerMarketplaceSummary,
    pub detail: AppServerMarketplaceDetail,
    pub remove_result: AppServerMarketplaceRemoveResult,
    pub imported: AppServerImportResult,
}

impl FakeMarketplaceProvider {
    pub fn new() -> Self {
        let summary = AppServerMarketplaceSummary {
            marketplace_id: "company-tools".into(),
            name: "company-tools".into(),
            description: Some("team catalog".into()),
            source_kind: "directory".into(),
            version: Some("1.0.0".into()),
            auto_update: false,
            enabled: true,
            entry_count: 2,
            added_at: 1,
        };
        Self {
            detail: AppServerMarketplaceDetail {
                summary: summary.clone(),
                entries: vec![
                    AppServerMarketplaceEntry {
                        name: "formatter".into(),
                        source_kind: "directory".into(),
                        source: "./plugins/formatter".into(),
                        version: Some("2.1.0".into()),
                        description: Some("Automatic code formatting".into()),
                        keywords: vec![],
                        category: None,
                    },
                    AppServerMarketplaceEntry {
                        name: "deploy".into(),
                        source_kind: "github".into(),
                        source: "company/deploy-plugin".into(),
                        version: None,
                        description: None,
                        keywords: vec![],
                        category: None,
                    },
                ],
            },
            added: summary,
            remove_result: AppServerMarketplaceRemoveResult {
                marketplace_id: "company-tools".into(),
                snapshots: vec!["snap-demo".into()],
                uninstalled_components: vec!["wb-company-tools-formatter".into()],
                warnings: vec![],
            },
            imported: AppServerImportResult {
                snapshot_id: "snap-demo".into(),
                name: "formatter".into(),
                version: "2.1.0".into(),
                source_kind: "codebuddy-plugin".into(),
                status: "completed".into(),
                content_digest: "digest-abc".into(),
                component_status: AppServerCompatibilityTriple {
                    semantic_status: "compatible_with_adapter".into(),
                    runtime_status: "not-verified".into(),
                    distribution_status: "local-only".into(),
                    reasons: vec![],
                },
                component_count: 3,
                imported_at: 1,
                reused: false,
                warnings: vec![],
                errors: vec![],
            },
        }
    }
}

impl Default for FakeMarketplaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MarketplaceProvider for FakeMarketplaceProvider {
    async fn add(
        &self,
        _request: AppServerMarketplaceAddRequest,
    ) -> Result<AppServerMarketplaceSummary, AppError> {
        Ok(self.added.clone())
    }

    async fn list(&self) -> Result<Vec<AppServerMarketplaceSummary>, AppError> {
        Ok(vec![self.added.clone()])
    }

    async fn get(&self, marketplace_id: &str) -> Result<AppServerMarketplaceDetail, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        Ok(self.detail.clone())
    }

    async fn remove(
        &self,
        marketplace_id: &str,
        _cascade: bool,
    ) -> Result<AppServerMarketplaceRemoveResult, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        Ok(self.remove_result.clone())
    }

    async fn set_auto_update(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<AppServerMarketplaceSummary, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        let mut summary = self.added.clone();
        summary.auto_update = enabled;
        Ok(summary)
    }

    async fn refresh(
        &self,
        marketplace_id: &str,
    ) -> Result<AppServerMarketplaceRefreshResult, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        Ok(AppServerMarketplaceRefreshResult {
            marketplace_id: marketplace_id.to_owned(),
            changed: true,
            resolved_revision: "abc123def".into(),
            entry_count: self.detail.entries.len(),
            warnings: vec![],
        })
    }

    async fn import_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerImportResult, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        if !self.detail.entries.iter().any(|entry| entry.name == entry_name) {
            return Err(AppError::NotFound(format!("entry {entry_name} not found")));
        }
        Ok(self.imported.clone())
    }

    async fn entry_dir(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<std::path::PathBuf, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        if !self.detail.entries.iter().any(|entry| entry.name == entry_name) {
            return Err(AppError::NotFound(format!("entry {entry_name} not found")));
        }
        Ok(std::path::PathBuf::from("/tmp/market/company-tools/plugins/formatter"))
    }

    async fn market_dir(&self, marketplace_id: &str) -> Result<std::path::PathBuf, AppError> {
        if marketplace_id != self.added.marketplace_id {
            return Err(AppError::NotFound(format!("marketplace {marketplace_id} not found")));
        }
        Ok(std::path::PathBuf::from("/tmp/market/company-tools"))
    }
}

/// In-memory Agent catalog fake (05 §4.1 shapes).
pub struct FakeAgentCatalog {
    pub agents: Vec<AppServerAgentSummary>,
}

#[async_trait]
impl AgentCatalogProvider for FakeAgentCatalog {
    async fn list(&self) -> Result<Vec<AppServerAgentSummary>, AppError> {
        Ok(self.agents.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerAgentDetail, AppError> {
        let summary = self
            .agents
            .iter()
            .find(|agent| agent.id == id || agent.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("agent {id} not found")))?;
        Ok(AppServerAgentDetail {
            summary,
            effort: None,
            max_turns: None,
            disallowed_tools: vec![],
            memory: None,
            background: None,
            isolation: None,
            permission_mode_ignored: false,
            display_description: None,
            quick_prompts: vec![],
            tags: vec![],
            default_init_prompt: None,
            expert_type: None,
            category_id: None,
        })
    }
}

/// In-memory Team catalog fake (05 §4.2 shapes).
pub struct FakeTeamCatalog {
    pub teams: Vec<AppServerTeamSummary>,
}

#[async_trait]
impl TeamCatalogProvider for FakeTeamCatalog {
    async fn list(&self) -> Result<Vec<AppServerTeamSummary>, AppError> {
        Ok(self.teams.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerTeamDetail, AppError> {
        let summary = self
            .teams
            .iter()
            .find(|team| team.id == id || team.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("team {id} not found")))?;
        Ok(AppServerTeamDetail {
            summary,
            planner_policy: "planned".into(),
            routing_constraints: vec![],
            workflow_limits: serde_json::json!({ "max_parallel": 4 }),
            team_runtime_capabilities: vec![
                "fixed_members".into(),
                "planning_context".into(),
                "planned_dag".into(),
                "local_parallel".into(),
                "retry".into(),
                "replan".into(),
                "events".into(),
                "artifacts".into(),
            ],
        })
    }
}

/// In-memory store fake: canned item list + install result.
pub struct FakeStoreProvider {
    pub items: Vec<AppServerStoreItem>,
    pub install_result: AppServerStoreInstallResult,
}

impl Default for FakeStoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeStoreProvider {
    pub fn new() -> Self {
        Self {
            items: vec![AppServerStoreItem {
                id: "company-tools/formatter".into(),
                marketplace_id: "company-tools".into(),
                marketplace_name: "company-tools".into(),
                entry_name: "formatter".into(),
                kind: "agent".into(),
                name: "formatter".into(),
                display_name: None,
                profession: None,
                description: Some("Automatic code formatting".into()),
                display_description: None,
                tags: vec![],
                quick_prompts: vec![],
                avatar_url: None,
                version: "2.1.0".into(),
                source_kind: "directory".into(),
                installed: false,
                update_available: false,
                snapshot_id: None,
                installed_version: None,
            }],
            install_result: AppServerStoreInstallResult {
                marketplace_id: "company-tools".into(),
                entry_name: "formatter".into(),
                snapshot_id: "snap-demo".into(),
                version: "2.1.0".into(),
                reused: false,
                installed_count: 3,
                warnings: vec![],
                errors: vec![],
            },
        }
    }
}

#[async_trait]
impl StoreProvider for FakeStoreProvider {
    async fn list(&self) -> Result<AppServerStoreList, AppError> {
        Ok(AppServerStoreList { items: self.items.clone() })
    }

    async fn install_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerStoreInstallResult, AppError> {
        if self
            .items
            .iter()
            .any(|item| item.marketplace_id == marketplace_id && item.entry_name == entry_name)
        {
            Ok(self.install_result.clone())
        } else {
            Err(AppError::NotFound(format!("entry {entry_name} not found")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skill() -> AppServerSkillSummary {
        AppServerSkillSummary {
            id: "builtin:demo".into(),
            name: "demo".into(),
            description: Some("a demo skill".into()),
            version: "builtin".into(),
            source: "builtin".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
            enabled: true,
            required_connectors: vec![],
        }
    }

    fn sample_connector() -> AppServerConnectorSummary {
        AppServerConnectorSummary {
            id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            name: "playwright".into(),
            description: None,
            kind: "stdio-mcp".into(),
            transport_summary: "npx @playwright/mcp".into(),
            auth_mode: "none".into(),
            enabled: true,
            status: nomifun_api_types::AppServerConnectorStatus::Connected,
        }
    }

    #[tokio::test]
    async fn skill_catalog_lists_and_gets_by_id_or_name() {
        let catalog = FakeSkillCatalog { skills: vec![sample_skill()] };
        let listed = catalog.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(catalog.get("demo").await.unwrap().summary.name, "demo");
        assert!(matches!(
            catalog.get("missing").await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn connector_status_never_reports_connected_after_a_failed_probe() {
        let catalog = FakeConnectorCatalog {
            connectors: vec![sample_connector()],
            auth_required_ids: vec![],
            probe_fail_ids: vec!["0190f5fe-7c00-7a00-8000-000000000001".into()],
        };
        let status = catalog.status("playwright").await.unwrap();
        assert_eq!(status.status.as_str(), "error");
        assert_ne!(status.status.as_str(), "connected");
    }

    #[tokio::test]
    async fn connector_status_requires_authorization_when_auth_missing() {
        let catalog = FakeConnectorCatalog {
            connectors: vec![sample_connector()],
            auth_required_ids: vec!["0190f5fe-7c00-7a00-8000-000000000001".into()],
            probe_fail_ids: vec![],
        };
        let status = catalog.status("playwright").await.unwrap();
        assert_eq!(status.status.as_str(), "authorization_required");
    }

    #[tokio::test]
    async fn connector_auth_start_then_status_then_logout_roundtrip() {
        let auth = FakeConnectorAuth { authenticated: std::sync::Mutex::new(vec![]) };
        let started = auth.auth_start("playwright").await.unwrap();
        assert_eq!(started.state, "started");
        assert_eq!(auth.auth_status("playwright").await.unwrap().state, "authenticated");
        auth.logout("playwright").await.unwrap();
        assert_eq!(auth.auth_status("playwright").await.unwrap().state, "not_authenticated");
    }
}