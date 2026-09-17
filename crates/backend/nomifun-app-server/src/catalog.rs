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
    AppServerConnectorCallResult, AppServerConnectorDetail, AppServerConnectorProbeResult,
    AppServerConnectorStatusView,
    AppServerConnectorSummary, AppServerImportDetail, AppServerImportRequest,
    AppServerImportResult, AppServerImportSummary, AppServerInstallRequest, AppServerInstallResult,
    AppServerInstallStatus, AppServerMarketplaceAddRequest, AppServerMarketplaceDetail,
    AppServerMarketplaceEntry, AppServerMarketplaceEntrySnapshot, AppServerMarketplaceRefreshResult,
    AppServerMarketplaceRemoveResult, AppServerMarketplaceSummary,
    AppServerModelList,
    AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDetail, AppServerSkillFileList, AppServerSkillSummary, AppServerStoreInstallResult,
    AppServerStoreItem, AppServerStoreList, AppServerTeamDetail, AppServerTeamSummary,
};
use nomifun_common::{AppError, LocalizedVariant};
use std::collections::BTreeMap;

/// Read-side Skill catalog (`skill/list`, `skill/get`).
#[async_trait]
pub trait SkillCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError>;
}

/// One skill file's bytes plus the metadata the wire needs to describe them.
///
/// Not a wire DTO: HTTP hands back the raw body with a `content-type` header,
/// so the content type travels beside the bytes rather than inside them. The
/// WS binding is the one that has to encode, and it does so at its own edge.
pub struct SkillFileBytes {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Largest single file the skill file face will serve.
///
/// A Skill's scripts and references are prose and small assets; anything this
/// large is not something a caller should pull through the protocol. The cap
/// lives at this seam rather than in the adapter because it is a **wire-shape**
/// decision: the size is read from metadata *before* the bytes are read, so an
/// oversized file is refused rather than loaded and then rejected.
pub const MAX_SKILL_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Why a skill file operation failed, at the seam.
///
/// A dedicated type rather than [`AppError`]: the wire distinguishes
/// `response_too_large` from `invalid_request` and `not_found` (`05` §4.3.1),
/// and `AppError` has no way to carry that third case without widening the
/// shared enum for every crate that matches on it. The protocol layer maps
/// these onto stable codes.
#[derive(Debug)]
pub enum SkillFileError {
    NotFound(String),
    InvalidRequest(String),
    /// Refused before reading: `size` is the file's own length.
    TooLarge { size: u64, limit: u64 },
    Internal(String),
}

/// Read-side Skill *file tree* (`skill/files`, `skill/file`).
///
/// Separate from [`SkillCatalogProvider`] because the two answer different
/// questions: the catalog describes a Skill as a catalog entry, this one serves
/// the directory it lives in. `skill/get` can only ever return a bounded
/// summary of the manifest, so without this seam the rest of a Skill's files
/// (`references/`, `scripts/`, `templates/`, `assets/`) have no read face at
/// all — see `docs/agent-store/24-external-agent-skill-and-mcp-access.zh.md`.
///
/// Implementations are responsible for path safety: an implementation that
/// trusts `path` turns this into an arbitrary local file read for any
/// authenticated caller. The protocol layer does not re-check it.
#[async_trait]
pub trait SkillFileProvider: Send + Sync {
    /// Every readable file inside the skill directory, plus its tree digest.
    async fn files(&self, skill_id: &str) -> Result<AppServerSkillFileList, SkillFileError>;

    /// One file's bytes, addressed by its skill-relative `path`.
    async fn read(&self, skill_id: &str, path: &str) -> Result<SkillFileBytes, SkillFileError>;
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

/// Largest serialized tool result the call proxy will return.
///
/// A tool result is a payload for a caller to use, not a bulk data channel; and
/// a refusal is better than a silently shortened JSON document, which the
/// caller would parse and believe.
pub const MAX_CONNECTOR_CALL_RESULT_BYTES: usize = 1024 * 1024;

/// Largest total size of the tool **schemas** a connector catalog response will
/// carry (`connector/get` / `connector/test`, doc `26` §5.2).
///
/// Names and descriptions are always kept — they are what a caller chooses by,
/// and they are cheap. Only `input_schema` values are dropped to stay inside
/// this budget, and each one is dropped **whole**: a half-truncated JSON Schema
/// would be parsed and believed, whereas an omitted one is reported through
/// `tools_truncated`. Refusing the whole response instead would turn one
/// outsized connector into a catalog that cannot be listed at all.
pub const MAX_CONNECTOR_TOOLS_BYTES: usize = 1024 * 1024;

/// Why a connector call produced no result, at the seam.
///
/// A dedicated type rather than [`AppError`] for the same reason as
/// [`SkillFileError`]: the wire separates `policy_denied`,
/// `connector_unavailable`, `connector_call_timeout`, `connector_call_failed`
/// and `response_too_large`, and [`AppError`] has no way to carry those without
/// widening the shared enum for every crate that matches on it. The protocol
/// layer maps these onto stable codes.
#[derive(Debug)]
pub enum ConnectorCallError {
    /// The request itself is unusable (e.g. an empty tool name).
    InvalidRequest(String),
    /// No such connector on this host.
    NotFound(String),
    /// The connector is registered but switched off.
    Unavailable(String),
    /// The host's `[connector_proxy]` policy refuses this connector/tool pair.
    PolicyDenied(String),
    /// The call did not finish inside the budget.
    Timeout { seconds: u64 },
    /// The upstream result exceeded [`MAX_CONNECTOR_CALL_RESULT_BYTES`].
    TooLarge { size: usize, limit: usize },
    /// Transport, protocol, or server failure.
    Failed(String),
}

/// The **call proxy** seam (`connector/call`, doc `24` §5).
///
/// Deliberately separate from [`ConnectorCatalogProvider`]: that one *describes*
/// connectors, this one *executes* on them, and only this one can be a security
/// boundary. Everything an MCP call needs — the transport, its credentials, the
/// OAuth token, the child process — stays behind this interface; what crosses it
/// is a tool name, an argument object, and the server's own result.
///
/// Implementations own the two gates, because only they can see the connector
/// row and the host policy:
///
/// 1. the connector must be registered **and enabled**;
/// 2. the `[connector_proxy]` policy must admit the pair — `enabled` is the
///    grant, and the optional `allow` / `deny` lists are the operator's
///    narrowing and subtraction (doc `26` §4).
///
/// A caller may not name a URL, a command, or a header: the addressable surface
/// is one registered connector id.
#[async_trait]
pub trait ConnectorCallProvider: Send + Sync {
    async fn call(
        &self,
        connector_id: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<AppServerConnectorCallResult, ConnectorCallError>;
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

    /// Ids the background auto-update sweep may refresh.
    ///
    /// An internal seam rather than a wire method: eligibility has to look at
    /// the source URI, which never crosses the public protocol (`18` §7). The
    /// scheduler is the only caller (doc 21 D7 ①).
    async fn auto_update_targets(&self) -> Result<Vec<String>, AppError>;

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
    pub models: Vec<nomifun_api_types::AppServerModelSummary>,
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
            tools_truncated: false,
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
            tools_truncated: false,
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
                outcomes: vec![],
            },
            status: AppServerInstallStatus {
                snapshot_id: "snap-demo".into(),
                components: vec![],
                outcomes: vec![],
                errors: vec![],
            },
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
            resolved_revision: None,
            last_checked_at: None,
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
                        // Provenance is part of the entry projection (doc 16
                        // D-W13-1 ①). One imported entry and one untouched
                        // entry, so both shapes stay exercised on the wire.
                        // Localized manifest variants ride along verbatim
                        // (doc 18 §4 / D8=A); the reader picks by UI language.
                        localized: BTreeMap::from([(
                            "description_zh".to_owned(),
                            LocalizedVariant::Text("自动代码格式化".to_owned()),
                        )]),
                        strict: false,
                        blocked_reason: None,
                        snapshot: Some(AppServerMarketplaceEntrySnapshot {
                            snapshot_id: "0190f5fe-7c00-7a00-8000-0000000000aa".into(),
                            name: "formatter".into(),
                            version: "2.1.0".into(),
                            status: "completed".into(),
                            component_count: 2,
                            installed_count: 1,
                            imported_at: 1,
                        }),
                    },
                    AppServerMarketplaceEntry {
                        name: "deploy".into(),
                        source_kind: "github".into(),
                        source: "company/deploy-plugin".into(),
                        version: None,
                        description: None,
                        keywords: vec![],
                        category: None,
                        localized: BTreeMap::new(),
                        strict: false,
                        blocked_reason: None,
                        snapshot: None,
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

    async fn auto_update_targets(&self) -> Result<Vec<String>, AppError> {
        // The fake models a local directory market, which is never an official
        // mirror, so nothing is auto-updatable here.
        Ok(Vec::new())
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
    /// Connectors this fake's snapshots "installed". Empty by default: a Team
    /// with no snapshot-installed Connector binds none, which is the common case.
    pub connectors: Vec<String>,
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
            connectors: self.connectors.clone(),
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
                published_at: None,
                avatar_url: None,
                version: "2.1.0".into(),
                source_kind: "directory".into(),
                installed: false,
                update_available: false,
                snapshot_id: None,
                installed_version: None,
                blocked_reason: None,
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
                outcomes: vec![],
            },
        }
    }
}

#[async_trait]
impl StoreProvider for FakeStoreProvider {
    async fn list(&self) -> Result<AppServerStoreList, AppError> {
        Ok(AppServerStoreList { items: self.items.clone(), markets_pending: false })
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
            origin: "builtin".into(),
            writable: false,
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
            enabled: true,
            required_connectors: vec![],
            avatar_url: None,
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
            avatar_url: None,
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