//! Versioned App Server connection and run boundary.
//!
//! Agent Store is a consumer of this protocol. A connection owns a
//! transport-established principal and must complete `initialize` followed by
//! `initialized` before it can invoke application methods.

pub mod agent_store;
pub mod catalog;
pub mod skill_admin;
pub mod workspace_resolver;

pub use agent_store::{
    AgentStoreConfig, AgentStoreConfigPatch, AgentStoreMarketplace, AgentStoreModel,
    AgentStoreProvider,
};
pub use catalog::{
    AgentCatalogProvider, ConnectorAuthProvider, ConnectorCatalogProvider, ImportProvider,
    InstallProvider, MarketplaceProvider, ModelCatalogProvider, SkillCatalogProvider,
    StoreProvider, TeamCatalogProvider,
};
pub use workspace_resolver::{
    FilesystemWorkspaceResolver, ResolvedWorkspace, WorkspaceResolver,
};
pub use skill_admin::{
    SkillAdmin, SkillCreateRequest, SkillWriteProvider, validate_skill_name,
};
/// The field-level patch shape the skill write face accepts. Re-stated here
/// because `WsSkillUpdate` turns its fields into one; it is not part of the
/// published surface either way (WS-only host management face).
use nomifun_extension::skill_service::SkillFieldPatch;

use std::collections::{HashMap, HashSet};
use std::path::Path as FsPath;
use std::sync::{Arc, RwLock};

use sha2::{Digest, Sha256};

use nomifun_db::{
    AppServerIdempotencyCommit, AppServerIdempotencyLookup, AppServerIdempotencyScope,
    IAppServerIdempotencyRepository, IAppServerRunMappingRepository,
    IAppServerWorkspaceRepository, NewAppServerIdempotencyReceipt,
};
use nomifun_db::models::AppServerWorkspaceRow;

use axum::{
    Extension, Router,
    extract::{
        Json, Path, Query, State, WebSocketUpgrade,
        rejection::JsonRejection,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
};
pub use nomifun_agent_execution::AgentRuntimeAdapter;
use nomifun_agent_execution::{
    AgentRunPlan, AgentRunReceipt, AgentRunResult, AgentRunSteerRequest, AgentRunView,
};
use nomifun_auth::CurrentUser;
use nomifun_common::{MessagePosition, MessageType, ProviderWithModel, UserId, generate_id};
use nomifun_api_types::{
    AppServerAgentDetail, AppServerAgentSummary, AppServerConfigMemoryView,
    AppServerConfigProviderView, AppServerConfigView,
    AppServerConnectorDetail,
    AppServerConnectorProbeResult, AppServerConnectorStatusView, AppServerConnectorSummary,
    AppServerImportDetail, AppServerImportRequest, AppServerImportResult,
    AppServerImportSummary, AppServerInstallRequest, AppServerInstallResult,
    AppServerInstallStatus, AppServerMarketplaceAddRequest, AppServerMarketplaceDetail,
    AppServerMarketplaceRefreshResult, AppServerMarketplaceRemoveResult,
    AppServerMarketplaceSummary, AppServerModelList, AppServerModelSummary,
    AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDeleteResult, AppServerSkillDetail, AppServerSkillSummary,
    AppServerStoreInstallResult,
    AppServerStoreList, AppServerTeamDetail, AppServerTeamSummary,
    AnswerExecutionDecisionRequest, CreateProviderRequest, ListMessagesQuery, MessageResponse,
    PresetOverrides, PresetSource,
    PresetTarget, SendMessageRequest,
};
use nomifun_conversation::{ConversationService, IdempotentMessageDelivery};
use nomifun_preset::PresetService;
use nomifun_realtime::{BroadcastEventBus, UserEventEnvelope};
use nomifun_system::ProviderService;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

/// App Server wire protocol version, negotiated by `initialize`.
pub const PROTOCOL_VERSION: &str = "2026-08-26";
const CONNECTION_HEADER: &str = "x-app-server-connection-id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalTransport {
    Http,
    Stdio,
    WebSocket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPrincipal {
    principal_id: String,
    user_id: UserId,
    transport: LocalTransport,
}

impl LocalPrincipal {
    /// The transport creates this identity from an already authenticated user.
    /// Request JSON never supplies or replaces the principal.
    pub fn from_authenticated_user(user_id: UserId, transport: LocalTransport) -> Self {
        // The authenticated user is the durable idempotency principal. The
        // transport remains part of the connection, but must not change replay
        // scope across HTTP and WebSocket reconnects.
        Self {
            principal_id: user_id.as_str().to_owned(),
            user_id,
            transport,
        }
    }

    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    pub fn user_id(&self) -> &UserId {
        &self.user_id
    }

    pub fn transport(&self) -> LocalTransport {
        self.transport
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthContextView {
    pub principal_id: String,
    pub issuer: &'static str,
    pub audience: &'static str,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    principal: LocalPrincipal,
    scopes: Vec<String>,
}

impl AuthContext {
    fn new(principal: LocalPrincipal, scopes: Vec<String>) -> Self {
        Self { principal, scopes }
    }

    pub fn principal(&self) -> &LocalPrincipal {
        &self.principal
    }

    pub fn view(&self) -> AuthContextView {
        AuthContextView {
            principal_id: self.principal.principal_id.clone(),
            issuer: "local-agent-store",
            audience: "agent-store",
            scopes: self.scopes.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPhase {
    AwaitingInitialize,
    AwaitingInitialized,
    Ready,
}

/// Which protocol capabilities this connection may advertise.
///
/// Computed from the injected services at connection-open time, so the
/// `initialize` response reflects the actual runtime surface (Agent runtime,
/// event source, Skill catalog, Connector catalog, OAuth) instead of a static
/// feature flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilityAvailability {
    pub runtime: bool,
    pub events: bool,
    pub skills: bool,
    pub connectors: bool,
    pub oauth: bool,
    pub imports: bool,
    pub installs: bool,
    pub marketplaces: bool,
    pub agents: bool,
    pub teams: bool,
    pub store: bool,
    pub models: bool,
}

impl CapabilityAvailability {
    pub fn from_state(state: &AppServerRouterState) -> Self {
        Self {
            runtime: state.runtime.is_some(),
            events: state.event_bus.is_some(),
            skills: state.skills.is_some(),
            connectors: state.connectors.is_some(),
            oauth: state.connector_auth.is_some(),
            imports: state.imports.is_some(),
            installs: state.installs.is_some(),
            marketplaces: state.markets.is_some(),
            agents: state.agent_catalog.is_some(),
            teams: state.team_catalog.is_some(),
            store: state.store.is_some(),
            models: state.models.is_some(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionState {
    connection_id: String,
    phase: ConnectionPhase,
    context: Option<AuthContext>,
    availability: CapabilityAvailability,
    client_id: Option<String>,
}

impl ConnectionState {
    pub fn new(principal: LocalPrincipal, runtime_available: bool) -> Self {
        Self::with_capabilities(
            principal,
            CapabilityAvailability { runtime: runtime_available, ..Default::default() },
        )
    }

    pub fn with_events(
        principal: LocalPrincipal,
        runtime_available: bool,
        events_available: bool,
    ) -> Self {
        Self::with_capabilities(
            principal,
            CapabilityAvailability {
                runtime: runtime_available,
                events: events_available,
                ..Default::default()
            },
        )
    }

    pub fn with_capabilities(principal: LocalPrincipal, availability: CapabilityAvailability) -> Self {
        let connection_id = generate_id();
        Self {
            connection_id,
            phase: ConnectionPhase::AwaitingInitialize,
            availability,
            client_id: None,
            context: Some(AuthContext::new(
                principal,
                vec!["catalog:read".into(), "run:read".into(), "run:write".into()],
            )),
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn phase(&self) -> ConnectionPhase {
        self.phase
    }

    fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    fn belongs_to(&self, user_id: &UserId) -> bool {
        self.context
            .as_ref()
            .is_some_and(|context| context.principal.user_id() == user_id)
    }

    pub fn initialize(&mut self, request: InitializeRequest) -> Result<InitializeResult, ProtocolError> {
        if self.phase != ConnectionPhase::AwaitingInitialize {
            return Err(ProtocolError::AlreadyInitialized);
        }
        if request.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedProtocolVersion(request.protocol_version));
        }
        if request.client.name.trim().is_empty() || request.client.version.trim().is_empty() {
            return Err(ProtocolError::InvalidRequest("client name and version are required".into()));
        }
        self.client_id = Some(format!("{}@{}", request.client.name.trim(), request.client.version.trim()));
        self.phase = ConnectionPhase::AwaitingInitialized;
        let context = self.context.as_ref().expect("principal is set at construction");
        Ok(InitializeResult {
            protocol_version: PROTOCOL_VERSION,
            server: ServerInfo {
                name: "allo-agent-store",
                version: env!("CARGO_PKG_VERSION"),
            },
            auth_context: context.view(),
            capabilities: Capabilities::from_availability(self.availability),
            connection_id: self.connection_id.clone(),
        })
    }

    pub fn mark_initialized(&mut self) -> Result<(), ProtocolError> {
        if self.phase != ConnectionPhase::AwaitingInitialized {
            return Err(ProtocolError::InvalidLifecycle);
        }
        self.phase = ConnectionPhase::Ready;
        Ok(())
    }

    pub fn require_ready(&self) -> Result<&AuthContext, ProtocolError> {
        if self.phase != ConnectionPhase::Ready {
            return Err(ProtocolError::NotInitialized);
        }
        Ok(self.context.as_ref().expect("ready context exists"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeRequest {
    pub protocol_version: String,
    pub client: ClientInfo,
    #[serde(default)]
    pub auth: Option<AuthRequest>,
    #[serde(default)]
    pub capabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthRequest {
    pub mode: Option<String>,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientCapabilities {
    #[serde(default)]
    pub events: bool,
    #[serde(default)]
    pub approvals: bool,
    #[serde(default)]
    pub team_runtime: bool,
    #[serde(default)]
    pub artifacts: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitializeResult {
    pub protocol_version: &'static str,
    pub server: ServerInfo,
    pub auth_context: AuthContextView,
    pub capabilities: Capabilities,
    pub connection_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Capabilities {
    pub agents: bool,
    pub teams: bool,
    pub team_runtime: bool,
    pub skills: bool,
    pub connectors: bool,
    pub run_notifications: bool,
    pub approvals: bool,
    pub artifacts: bool,
    pub oauth: bool,
    pub imports: bool,
    pub installs: bool,
    pub marketplaces: bool,
    pub store: bool,
    pub models: bool,
}

impl Capabilities {
    fn from_availability(availability: CapabilityAvailability) -> Self {
        Self {
            agents: availability.runtime || availability.agents,
            teams: availability.teams,
            team_runtime: false,
            skills: availability.skills,
            connectors: availability.connectors,
            run_notifications: availability.runtime && availability.events,
            // Derived from the runtime, exactly like `run_notifications`: the
            // approval answer path (`run/answer-decision`) rides the same
            // runtime seam, so a connection without a runtime must not advertise
            // a capability whose only method would answer
            // `runtime_unavailable` (the client would wait for an answer that
            // can never be accepted).
            approvals: availability.runtime,
            artifacts: false,
            oauth: availability.oauth,
            imports: availability.imports,
            installs: availability.installs,
            marketplaces: availability.marketplaces,
            store: availability.store,
            models: availability.models,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("protocol version unsupported: {0}")]
    UnsupportedProtocolVersion(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("connection is not initialized")]
    NotInitialized,
    #[error("connection lifecycle state is invalid")]
    InvalidLifecycle,
    #[error("connection is already initialized")]
    AlreadyInitialized,
    #[error("connection not found")]
    ConnectionNotFound,
    #[error("connection principal does not match the authenticated caller")]
    PrincipalMismatch,
    #[error("idempotency key was already used for a different request")]
    IdempotencyConflict,
}

#[derive(Debug, Clone)]
pub struct AppServerError {
    code: &'static str,
    message: String,
    status: StatusCode,
    retryable: bool,
    details: Option<serde_json::Value>,
}

impl AppServerError {
    fn new(code: &'static str, message: impl Into<String>, status: StatusCode, retryable: bool) -> Self {
        Self { code, message: message.into(), status, retryable, details: None }
    }

    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Whether this error carries the shared `not_found` code. Used by the
    /// skill write face to tell "the id is gone" from "the read failed".
    fn is_not_found(&self) -> bool {
        self.code == "not_found"
    }

    fn from_app_error(error: nomifun_common::AppError) -> Self {
        let status = error.status_code();
        let (code, retryable) = match &error {
            nomifun_common::AppError::NotFound(_) => ("not_found", false),
            nomifun_common::AppError::BadRequest(_) => ("invalid_request", false),
            nomifun_common::AppError::Unauthorized(_) => ("unauthenticated", false),
            nomifun_common::AppError::Forbidden(message) if message.contains("compatibility") => ("compatibility_blocked", false),
            nomifun_common::AppError::Forbidden(message) if message.contains("policy") => ("policy_denied", false),
            nomifun_common::AppError::Forbidden(_) => ("policy_denied", false),
            nomifun_common::AppError::Conflict(message)
                if message.contains("settled execution cannot be cancelled")
                    || message.contains("cannot be cancelled") => ("run_not_resumable", false),
            nomifun_common::AppError::Conflict(_) => ("conflict", false),
            nomifun_common::AppError::RateLimited => ("connector_unavailable", true),
            nomifun_common::AppError::ProviderUnavailable(_) => ("runtime_unavailable", true),
            nomifun_common::AppError::BadGateway(_) | nomifun_common::AppError::Timeout(_) => ("runtime_unavailable", true),
            nomifun_common::AppError::Internal(_) => ("internal_error", true),
            nomifun_common::AppError::UnprocessableEntity(_) => ("invalid_request", false),
            nomifun_common::AppError::ManagedFreeModelsDisabled(_) => ("runtime_unavailable", true),
            nomifun_common::AppError::CloudOtpInvalidCode => ("invalid_request", false),
            nomifun_common::AppError::ConversationDelete(_) => ("conflict", false),
            nomifun_common::AppError::ProviderInUse(_) => ("conflict", false),
            nomifun_common::AppError::WorkspacePathEdgeWhitespace(_)
            | nomifun_common::AppError::WorkspacePathEdgeWhitespaceRuntimeUnsupported(_) => {
                ("workspace_denied", false)
            }
        };
        let details = error.error_details();
        let mapped = Self::new(code, error.to_string(), status, retryable);
        if let Some(details) = details {
            mapped.with_details(details)
        } else {
            mapped
        }
    }
}

impl From<ProtocolError> for AppServerError {
    fn from(error: ProtocolError) -> Self {
        match error {
            ProtocolError::UnsupportedProtocolVersion(version) => Self::new(
                "protocol_version_unsupported",
                format!("unsupported protocol version: {version}"),
                StatusCode::BAD_REQUEST,
                false,
            ),
            ProtocolError::InvalidRequest(message) => {
                Self::new("invalid_request", message, StatusCode::BAD_REQUEST, false)
            }
            ProtocolError::NotInitialized | ProtocolError::InvalidLifecycle => {
                Self::new("not_initialized", error.to_string(), StatusCode::FORBIDDEN, false)
            }
            ProtocolError::AlreadyInitialized => {
                Self::new("conflict", error.to_string(), StatusCode::CONFLICT, false)
            }
            ProtocolError::ConnectionNotFound => {
                Self::new("not_found", error.to_string(), StatusCode::NOT_FOUND, false)
            }
            ProtocolError::PrincipalMismatch => {
                Self::new("policy_denied", error.to_string(), StatusCode::FORBIDDEN, false)
            }
            ProtocolError::IdempotencyConflict => {
                Self::new("idempotency_conflict", error.to_string(), StatusCode::CONFLICT, false)
            }
        }
    }
}

impl From<nomifun_common::AppError> for AppServerError {
    fn from(error: nomifun_common::AppError) -> Self {
        Self::from_app_error(error)
    }
}

impl AppServerError {
    fn into_wire_error(self, request_id: Option<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "message": self.message,
            "retryable": self.retryable,
            "details": self.details.unwrap_or_else(|| serde_json::json!({})),
            "request_id": request_id,
        })
    }
}

impl IntoResponse for AppServerError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status;
        let body = self.into_wire_error(None);
        (status, Json(body)).into_response()
    }
}

#[derive(Debug, Clone)]
enum IdempotencyRecord {
    AgentRun {
        fingerprint: String,
        receipt: AgentRunReceipt,
    },
    Cancel {
        fingerprint: String,
        view: AgentRunView,
    },
}

#[derive(Clone, Default)]
pub struct AppServerRegistry {
    connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
    idempotency: Arc<RwLock<HashMap<String, IdempotencyRecord>>>,
    idempotency_gate: Arc<tokio::sync::Mutex<()>>,
}

impl AppServerRegistry {
    pub fn open(&self, principal: LocalPrincipal, runtime_available: bool) -> ConnectionState {
        self.open_with_events(principal, runtime_available, false)
    }

    pub fn open_with_events(
        &self,
        principal: LocalPrincipal,
        runtime_available: bool,
        events_available: bool,
    ) -> ConnectionState {
        self.open_with_capabilities(
            principal,
            CapabilityAvailability {
                runtime: runtime_available,
                events: events_available,
                ..Default::default()
            },
        )
    }

    pub fn open_with_capabilities(
        &self,
        principal: LocalPrincipal,
        availability: CapabilityAvailability,
    ) -> ConnectionState {
        let state = ConnectionState::with_capabilities(principal, availability);
        let connection_id = state.connection_id.clone();
        self.connections
            .write()
            .expect("App Server connection registry lock is not poisoned")
            .insert(connection_id, state.clone());
        state
    }

    pub fn initialize(
        &self,
        connection_id: &str,
        request: InitializeRequest,
    ) -> Result<InitializeResult, ProtocolError> {
        let mut connections = self
            .connections
            .write()
            .expect("App Server connection registry lock is not poisoned");
        let state = connections
            .get_mut(connection_id)
            .ok_or(ProtocolError::ConnectionNotFound)?;
        state.initialize(request)
    }

    pub fn close(&self, connection_id: &str) -> bool {
        self.connections
            .write()
            .expect("App Server connection registry lock is not poisoned")
            .remove(connection_id)
            .is_some()
    }

    pub fn mark_initialized(
        &self,
        connection_id: &str,
        user_id: &UserId,
    ) -> Result<(), ProtocolError> {
        let mut connections = self
            .connections
            .write()
            .expect("App Server connection registry lock is not poisoned");
        let state = connections
            .get_mut(connection_id)
            .ok_or(ProtocolError::ConnectionNotFound)?;
        if !state.belongs_to(user_id) {
            return Err(ProtocolError::PrincipalMismatch);
        }
        state.mark_initialized()
    }

    pub fn ready_idempotency_context(
        &self,
        connection_id: &str,
        user_id: &UserId,
    ) -> Result<(String, String), ProtocolError> {
        let connections = self
            .connections
            .read()
            .expect("App Server connection registry lock is not poisoned");
        let state = connections
            .get(connection_id)
            .ok_or(ProtocolError::ConnectionNotFound)?;
        if !state.belongs_to(user_id) {
            return Err(ProtocolError::PrincipalMismatch);
        }
        state.require_ready()?;
        let context = state.context.as_ref().expect("ready context exists");
        let client_id = state
            .client_id()
            .ok_or_else(|| ProtocolError::InvalidRequest("client identity is missing".into()))?;
        Ok((context.principal.principal_id().to_owned(), client_id.to_owned()))
    }

    pub async fn idempotency_lock(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.idempotency_gate.clone().lock_owned().await
    }

    pub fn existing_idempotent_run(
        &self,
        scope: &str,
        fingerprint: &str,
    ) -> Result<Option<AgentRunReceipt>, ProtocolError> {
        let records = self
            .idempotency
            .read()
            .expect("App Server idempotency registry lock is not poisoned");
        match records.get(scope) {
            Some(IdempotencyRecord::AgentRun { fingerprint: existing_fingerprint, receipt })
                if existing_fingerprint == fingerprint => Ok(Some(receipt.clone())),
            Some(IdempotencyRecord::AgentRun { .. })
            | Some(IdempotencyRecord::Cancel { .. }) => Err(ProtocolError::IdempotencyConflict),
            None => Ok(None),
        }
    }

    pub fn remember_idempotent_run(
        &self,
        scope: &str,
        fingerprint: String,
        receipt: AgentRunReceipt,
    ) -> Result<AgentRunReceipt, ProtocolError> {
        let mut records = self
            .idempotency
            .write()
            .expect("App Server idempotency registry lock is not poisoned");
        if let Some(existing) = records.get(scope) {
            return match existing {
                IdempotencyRecord::AgentRun { fingerprint: existing_fingerprint, receipt }
                    if existing_fingerprint == &fingerprint => Ok(receipt.clone()),
                IdempotencyRecord::AgentRun { .. } | IdempotencyRecord::Cancel { .. } => {
                    Err(ProtocolError::IdempotencyConflict)
                }
            };
        }
        records.insert(
            scope.to_owned(),
            IdempotencyRecord::AgentRun { fingerprint, receipt: receipt.clone() },
        );
        Ok(receipt)
    }

    pub fn existing_idempotent_cancel(
        &self,
        scope: &str,
        fingerprint: &str,
    ) -> Result<Option<AgentRunView>, ProtocolError> {
        let records = self
            .idempotency
            .read()
            .expect("App Server idempotency registry lock is not poisoned");
        match records.get(scope) {
            Some(IdempotencyRecord::Cancel { fingerprint: existing_fingerprint, view })
                if existing_fingerprint == fingerprint => Ok(Some(view.clone())),
            Some(IdempotencyRecord::AgentRun { .. })
            | Some(IdempotencyRecord::Cancel { .. }) => Err(ProtocolError::IdempotencyConflict),
            None => Ok(None),
        }
    }

    pub fn remember_idempotent_cancel(
        &self,
        scope: &str,
        fingerprint: String,
        view: AgentRunView,
    ) -> Result<AgentRunView, ProtocolError> {
        let mut records = self
            .idempotency
            .write()
            .expect("App Server idempotency registry lock is not poisoned");
        if let Some(existing) = records.get(scope) {
            return match existing {
                IdempotencyRecord::Cancel { fingerprint: existing_fingerprint, view }
                    if existing_fingerprint == &fingerprint => Ok(view.clone()),
                IdempotencyRecord::AgentRun { .. } | IdempotencyRecord::Cancel { .. } => {
                    Err(ProtocolError::IdempotencyConflict)
                }
            };
        }
        records.insert(
            scope.to_owned(),
            IdempotencyRecord::Cancel { fingerprint, view: view.clone() },
        );
        Ok(view)
    }

    pub fn require_ready(
        &self,
        connection_id: &str,
        user_id: &UserId,
    ) -> Result<AuthContextView, ProtocolError> {
        let connections = self
            .connections
            .read()
            .expect("App Server connection registry lock is not poisoned");
        let state = connections
            .get(connection_id)
            .ok_or(ProtocolError::ConnectionNotFound)?;
        if !state.belongs_to(user_id) {
            return Err(ProtocolError::PrincipalMismatch);
        }
        Ok(state.require_ready()?.view())
    }
}

#[derive(Clone)]
pub struct AppServerRouterState {
    pub registry: AppServerRegistry,
    /// Preset-backed Agent Execution adapter. This remains separate from
    /// interactive chat, which owns durable Conversation rows directly.
    pub runtime: Option<AgentRuntimeAdapter>,
    /// Shared production Conversation module for persistent, preset-optional
    /// Nomi chat. It is deliberately injected as one deep module so App Server
    /// never reaches into repositories or agent factories itself.
    pub conversation_service: Option<ConversationService>,
    pub conversation_runtime_registry: Option<Arc<dyn nomifun_ai_agent::AgentRuntimeRegistry>>,
    pub preset_service: Option<Arc<PresetService>>,
    /// Durable response receipts. Production must inject this repository when
    /// an idempotency key is accepted.
    pub idempotency: Option<Arc<dyn IAppServerIdempotencyRepository>>,
    /// Durable public-to-internal run identity mapping.
    pub run_mappings: Option<Arc<dyn IAppServerRunMappingRepository>>,
    /// Owner-scoped workspace registry and filesystem authority.
    pub workspaces: Option<Arc<dyn IAppServerWorkspaceRepository>>,
    pub workspace_resolver: Option<Arc<dyn WorkspaceResolver>>,
    /// Owner-scoped event source used by the App Server WebSocket.
    pub event_bus: Option<Arc<BroadcastEventBus>>,
    /// Provider CRUD + encrypted credential storage, shared with the system
    /// routes so App Server chats can register providers read from the local
    /// agent-store config.
    pub provider_service: Option<Arc<ProviderService>>,
    /// Optional override for the agent-store config location. `None` resolves
    /// `~/.agent-store/config.toml` on every request.
    pub agent_store_config_path: Option<std::path::PathBuf>,
    /// Agent Store Skill catalog provider. `None` keeps the `skills`
    /// capability off and returns `unsupported_operation` for `skill/*`.
    pub skills: Option<Arc<dyn SkillCatalogProvider>>,
    /// Agent Store Skill write face (`skill/create|update|delete`, `16` R17).
    /// `None` keeps the write methods off (`unsupported_operation`); the read
    /// catalog stays available either way. Host management surface: no HTTP
    /// binding and no counterpart in the published SDK package.
    pub skill_writes: Option<Arc<dyn SkillWriteProvider>>,
    /// Agent Store Connector catalog provider. `None` keeps the `connectors`
    /// capability off and returns `unsupported_operation` for `connector/*`.
    pub connectors: Option<Arc<dyn ConnectorCatalogProvider>>,
    /// Connector OAuth pass-through. `None` keeps the `oauth` capability off.
    pub connector_auth: Option<Arc<dyn ConnectorAuthProvider>>,
    /// Agent Store Importer/PluginSnapshot provider. `None` keeps the
    /// `imports` capability off.
    pub imports: Option<Arc<dyn ImportProvider>>,
    /// Agent Store Installer provider (roadmap Phase 2). `None` keeps the
    /// `installs` capability off.
    pub installs: Option<Arc<dyn InstallProvider>>,
    /// Agent Store Marketplace provider (roadmap Phase 2). `None` keeps the
    /// `marketplaces` capability off.
    pub markets: Option<Arc<dyn MarketplaceProvider>>,
    /// Agent Store AgentDefinition catalog (05 §4.1). `None` keeps the
    /// `agents` catalog off; `agent/run` still works off the runtime.
    pub agent_catalog: Option<Arc<dyn AgentCatalogProvider>>,
    /// Agent Store Team catalog (05 §4.2). `None` keeps `teams` off.
    pub team_catalog: Option<Arc<dyn TeamCatalogProvider>>,
    /// Unified store catalog (winget-style): aggregated items over all enabled
    /// marketplaces with install state + one-click install. `None` keeps the
    /// `store` capability off.
    pub store: Option<Arc<dyn StoreProvider>>,
    /// Public model directory (REQ-PAR-05b). `None` keeps `models/list` and
    /// the `models` capability off.
    pub models: Option<Arc<dyn ModelCatalogProvider>>,
    /// Agent Store immutable snapshot root (`{work_dir}/agent-store-imports`).
    /// `Some` enables the public asset endpoint for snapshot-attached display
    /// assets (avatars etc.); `None` keeps it off.
    pub snapshot_assets_root: Option<std::path::PathBuf>,
}

impl Default for AppServerRouterState {
    fn default() -> Self {
        Self {
            registry: AppServerRegistry::default(),
            runtime: None,
            conversation_service: None,
            conversation_runtime_registry: None,
            preset_service: None,
            idempotency: None,
            run_mappings: None,
            workspaces: None,
            workspace_resolver: None,
            event_bus: None,
            provider_service: None,
            agent_store_config_path: None,
            skills: None,
            skill_writes: None,
            connectors: None,
            connector_auth: None,
            imports: None,
            installs: None,
            markets: None,
            agent_catalog: None,
            team_catalog: None,
            store: None,
            models: None,
            snapshot_assets_root: None,
        }
    }
}

pub fn app_server_routes(state: AppServerRouterState) -> Router {
    Router::new()
        .route("/api/app-server/initialize", post(initialize))
        .route("/api/app-server/initialized", post(initialized))
        .route("/api/app-server/ping", post(ping))
        .route("/api/app-server/ws", get(websocket))
        .route("/api/app-server/workspaces", post(workspace_register).get(workspace_list))
        .route(
            "/api/app-server/workspaces/{workspace_id}",
            delete(workspace_delete),
        )
        .route("/api/app-server/conversations", post(conversation_create).get(conversation_list))
        .route("/api/app-server/conversations/{conversation_id}", get(conversation_get).delete(conversation_delete))
        .route(
            "/api/app-server/conversations/{conversation_id}/messages",
            get(conversation_messages).post(conversation_send),
        )
        .route("/api/app-server/conversations/{conversation_id}/cancel", post(conversation_cancel))
        .route("/api/app-server/agent/run", post(agent_run))
        .route("/api/app-server/run/{run_id}", get(run_get))
        .route("/api/app-server/run/{run_id}/result", get(run_result))
        .route("/api/app-server/run/{run_id}/plan", get(run_plan))
        .route("/api/app-server/run/{run_id}/events", get(run_events))
        .route("/api/app-server/run/{run_id}/cancel", post(run_cancel))
        .route("/api/app-server/run/{run_id}/steer", post(run_steer))
        .route(
            "/api/app-server/run/{run_id}/answer-decision",
            post(run_answer_decision),
        )
        // Agent Store Skill catalog
        .route("/api/app-server/skills", get(list_skills_route))
        .route("/api/app-server/skills/{skill_id}", get(get_skill_route))
        // Agent Store Connector catalog / status / probe / OAuth
        .route("/api/app-server/connectors", get(list_connectors_route))
        .route("/api/app-server/models", get(list_models_route))
        .route("/api/app-server/connectors/{connector_id}", get(get_connector_route))
        .route(
            "/api/app-server/connectors/{connector_id}/status",
            get(connector_status_route),
        )
        .route(
            "/api/app-server/connectors/{connector_id}/test",
            post(connector_test_route),
        )
        .route(
            "/api/app-server/connectors/{connector_id}/auth-start",
            post(connector_auth_start_route),
        )
        .route(
            "/api/app-server/connectors/{connector_id}/auth-status",
            get(connector_auth_status_route),
        )
        .route(
            "/api/app-server/connectors/{connector_id}/auth-logout",
            post(connector_auth_logout_route),
        )
        // Agent Store Importer (roadmap Phase 1)
        .route("/api/app-server/imports", post(run_import_route).get(list_imports_route))
        .route("/api/app-server/imports/{snapshot_id}", get(get_import_route))
        // Agent Store Installer (roadmap Phase 2)
        .route("/api/app-server/installs", post(run_install_route))
        .route("/api/app-server/installs/{snapshot_id}", get(install_status_route))
        .route(
            "/api/app-server/installs/{snapshot_id}/disable",
            post(install_disable_route),
        )
        .route(
            "/api/app-server/installs/{snapshot_id}/enable",
            post(install_enable_route),
        )
        .route(
            "/api/app-server/installs/{snapshot_id}/uninstall",
            post(install_uninstall_route),
        )
        // Agent Store Marketplaces (roadmap Phase 2)
        .route("/api/app-server/markets", post(market_add_route).get(market_list_route))
        .route("/api/app-server/markets/{marketplace_id}", get(market_get_route))
        .route("/api/app-server/markets/{marketplace_id}/remove", post(market_remove_route))
        .route(
            "/api/app-server/markets/{marketplace_id}/auto-update",
            post(market_auto_update_route),
        )
        .route(
            "/api/app-server/markets/{marketplace_id}/refresh",
            post(market_refresh_route),
        )
        .route(
            "/api/app-server/markets/{marketplace_id}/entries/{entry_name}/import",
            post(market_entry_import_route),
        )
        // Agent Store unified store catalog (winget-style)
        .route("/api/app-server/store", get(store_list_route))
        .route(
            "/api/app-server/store/{marketplace_id}/entries/{entry_name}/install",
            post(store_install_entry_route),
        )
        .with_state(state)
}

/// Public (no-login) display-asset routes for the Agent Store: snapshot
/// avatars and store entry avatars / market icons. They are referenced by
/// plain `<img>` tags, which cannot carry the app-server connection header
/// or an Authorization header, so they must not sit behind the owner auth
/// middleware. Strict path + MIME whitelists stay in the handlers.
pub fn app_server_public_routes(state: AppServerRouterState) -> Router {
    Router::new()
        // Snapshot-attached public display assets (avatars etc.), served from
        // the immutable snapshot with strict path + MIME whitelist.
        .route(
            "/api/app-server/imports/{snapshot_id}/assets/{*asset_path}",
            get(snapshot_asset_route),
        )
        // Store entry display assets (avatars / market icons)
        .route(
            "/api/app-server/store/{marketplace_id}/entries/{entry_name}/assets/{*asset_path}",
            get(store_asset_route),
        )
        .with_state(state)
}

async fn initialize(
    State(state): State<AppServerRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<InitializeRequest>, JsonRejection>,
) -> Result<(HeaderMap, Json<InitializeResult>), AppServerError> {
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let connection = state
        .registry
        .open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id, LocalTransport::Http),
            CapabilityAvailability::from_state(&state),
        );
    let result = state.registry.initialize(connection.connection_id(), request)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        CONNECTION_HEADER,
        connection
            .connection_id()
            .parse()
            .expect("generated connection id is a valid header value"),
    );
    Ok((response_headers, Json(result)))
}

#[derive(Debug, Serialize)]
struct WorkspaceRegistration {
    id: String,
}

/// Owner-scoped public workspace projection. `canonical_path` is only ever
/// returned to the authenticated owner's own connection (this owner's row);
/// it never crosses into another owner's response or an event payload.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceView {
    pub workspace_id: String,
    /// Stable display label derived from the directory basename. Not a path.
    pub name: String,
    pub canonical_path: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCreateRequest {
    /// Absolute local directory path chosen by the owner. The server validates
    /// and canonicalizes it; the client never supplies a bare relative path or
    /// an opaque server-internal identity here.
    pub path: String,
}

fn workspace_display_name(root_path: &str) -> String {
    FsPath::new(root_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| root_path.to_owned())
}

fn workspace_view(row: AppServerWorkspaceRow) -> WorkspaceView {
    WorkspaceView {
        workspace_id: row.workspace_id,
        name: workspace_display_name(&row.root_path),
        canonical_path: row.root_path,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Provision a workspace below the server-controlled registry and persist its
/// owner-scoped identity. The client never supplies a filesystem path.
async fn workspace_register(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<WorkspaceRegistration>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.require_ready(connection_id, &user.id)?;
    let resolver = state.workspace_resolver.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied",
        "workspace resolver is unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        true,
    ))?;
    let repository = state.workspaces.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied",
        "workspace registry is unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        true,
    ))?;
    let workspace_id = generate_id();
    let resolved = resolver.ensure(&workspace_id)?;
    repository
        .register(
            user.id.as_str(),
            resolved.workspace_id(),
            &resolved.path().to_string_lossy(),
        )
        .await
        .map_err(db_error)?;
    Ok(Json(WorkspaceRegistration {
        id: resolved.workspace_id().to_owned(),
    }))
}

/// List the authenticated owner's active workspaces (never foreign or revoked
/// rows). `canonical_path` is returned only because this endpoint is already
/// owner-scoped; it is not exposed in events or cross-owner responses.
async fn workspace_list(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<WorkspaceView>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let repository = state.workspaces.as_ref().ok_or_else(|| {
        AppServerError::new(
            "workspace_denied",
            "workspace registry is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let rows = repository
        .list_active(user.id.as_str())
        .await
        .map_err(db_error)?;
    Ok(Json(rows.into_iter().map(workspace_view).collect()))
}

async fn workspace_create_impl(
    state: &AppServerRouterState,
    user: &CurrentUser,
    path: &str,
) -> Result<WorkspaceView, AppServerError> {
    let resolver = state.workspace_resolver.as_ref().ok_or_else(|| {
        AppServerError::new(
            "workspace_denied",
            "workspace resolver is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let repository = state.workspaces.as_ref().ok_or_else(|| {
        AppServerError::new(
            "workspace_denied",
            "workspace registry is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    // Validate + canonicalize the user-chosen directory first. Nothing below
    // may interpret the request string again; the canonical result is the only
    // path ever persisted.
    let resolved = resolver.resolve_user_path(path)?;
    let row = repository
        .ensure_default(user.id.as_str(), &resolved.path().to_string_lossy())
        .await
        .map_err(db_error)?;
    Ok(workspace_view(row))
}

/// Result of revoking (soft-deleting) an owner's active workspace.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceRevokeResult {
    pub workspace_id: String,
    /// `false` when the workspace is foreign, missing, or already revoked
    /// (idempotent no-op).
    pub revoked: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceRevokeParams {
    workspace_id: String,
}

/// Revoke (soft-delete) an owner's active workspace. Shared by the WS method
/// and the HTTP `DELETE` route. Sessions keep their `extra.workspace_id`;
/// re-registering the same root re-activates the same `workspace_id` so those
/// sessions become visible again.
async fn workspace_revoke_impl(
    state: &AppServerRouterState,
    user: &CurrentUser,
    workspace_id: &str,
) -> Result<WorkspaceRevokeResult, AppServerError> {
    if workspace_id.trim().is_empty() {
        return Err(AppServerError::new(
            "invalid_request",
            "workspace_id must not be empty",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let repository = state.workspaces.as_ref().ok_or_else(|| {
        AppServerError::new(
            "workspace_denied",
            "workspace registry is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let revoked = repository
        .revoke(user.id.as_str(), workspace_id)
        .await
        .map_err(db_error)?;
    Ok(WorkspaceRevokeResult {
        workspace_id: workspace_id.to_owned(),
        revoked,
    })
}

async fn workspace_delete(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(workspace_id): Path<String>,
) -> Result<Json<WorkspaceRevokeResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(
        workspace_revoke_impl(&state, &user, &workspace_id).await?,
    ))
}

async fn initialized(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.mark_initialized(connection_id, &user.id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn ping(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AuthContextView>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    Ok(Json(state.registry.require_ready(connection_id, &user.id)?))
}

// ---------------------------------------------------------------------------
// Agent Store Skill / Connector catalog (HTTP mirror of the WS methods)
// ---------------------------------------------------------------------------

fn skill_catalog_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn SkillCatalogProvider>, AppServerError> {
    state.skills.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "skill catalog is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

fn connector_catalog_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ConnectorCatalogProvider>, AppServerError> {
    state.connectors.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "connector catalog is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

fn model_catalog_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ModelCatalogProvider>, AppServerError> {
    state.models.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "model catalog is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

fn connector_auth_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ConnectorAuthProvider>, AppServerError> {
    state.connector_auth.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "connector OAuth is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

async fn list_skills_impl(
    state: &AppServerRouterState,
) -> Result<Vec<AppServerSkillSummary>, AppServerError> {
    skill_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn get_skill_impl(
    state: &AppServerRouterState,
    skill_id: &str,
) -> Result<AppServerSkillDetail, AppServerError> {
    skill_catalog_provider(state)?.get(skill_id).await.map_err(AppServerError::from)
}

/// Write face seam (`skill/create|update|delete`). A host that wires the read
/// catalog but not the write face answers `unsupported_operation` — never a
/// silent no-op.
fn skill_write_provider(state: &AppServerRouterState) -> Result<Arc<dyn SkillWriteProvider>, AppServerError> {
    state.skill_writes.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "skill writes are not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

/// `skill/create`: validate the name, refuse to shadow an existing id, write
/// through the provider, then answer with `skill/get`'s view of the new skill
/// (read back from disk — never the request echoed).
async fn execute_skill_create(
    state: &AppServerRouterState,
    params: WsSkillCreate,
) -> Result<AppServerSkillDetail, AppServerError> {
    let writes = skill_write_provider(state)?;
    // Every answer below is produced by re-reading through the read catalog;
    // without it a write could land on disk with no readable reply, so the read
    // face is required *before* anything is touched.
    skill_catalog_provider(state)?;
    let name = params.name.clone();
    validate_skill_name(&name).map_err(AppServerError::from)?;
    writes
        .create_skill(params.into_request())
        .await
        .map_err(AppServerError::from)?;
    get_skill_impl(state, &name).await
}

/// `skill/update`: merge the named fields into a writable skill's `SKILL.md`,
/// then re-read it through the read face.
async fn execute_skill_update(
    state: &AppServerRouterState,
    params: WsSkillUpdate,
) -> Result<AppServerSkillDetail, AppServerError> {
    let writes = skill_write_provider(state)?;
    skill_catalog_provider(state)?;
    validate_skill_name(&params.skill_id).map_err(AppServerError::from)?;
    if !params.names_any_field() {
        return Err(AppServerError::new(
            "invalid_request",
            "skill/update needs at least one field to change",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    writes
        .update_skill(&params.skill_id, &params.field_patch())
        .await
        .map_err(AppServerError::from)?;
    get_skill_impl(state, &params.skill_id).await
}

/// `skill/copy`: derive a new user skill from an existing one, then answer with
/// `skill/get`'s view of the **new** skill (read back from disk).
async fn execute_skill_copy(
    state: &AppServerRouterState,
    params: WsSkillCopy,
) -> Result<AppServerSkillDetail, AppServerError> {
    let writes = skill_write_provider(state)?;
    skill_catalog_provider(state)?;
    validate_skill_name(&params.skill_id).map_err(AppServerError::from)?;
    validate_skill_name(&params.new_name).map_err(AppServerError::from)?;
    writes
        .copy_skill(&params.skill_id, &params.new_name)
        .await
        .map_err(AppServerError::from)?;
    get_skill_impl(state, &params.new_name).await
}

/// `skill/delete`: delete a writable skill, then re-read the id so the caller
/// learns what — if anything — is visible there afterwards (a user skill may
/// have been shadowing a same-name built-in).
async fn execute_skill_delete(
    state: &AppServerRouterState,
    params: WsSkillDelete,
) -> Result<AppServerSkillDeleteResult, AppServerError> {
    let writes = skill_write_provider(state)?;
    skill_catalog_provider(state)?;
    validate_skill_name(&params.skill_id).map_err(AppServerError::from)?;
    writes
        .delete_skill(&params.skill_id)
        .await
        .map_err(AppServerError::from)?;
    let revealed_origin = match get_skill_impl(state, &params.skill_id).await {
        Ok(detail) => Some(detail.summary.origin),
        Err(error) if error.is_not_found() => None,
        // The delete landed but its verification read failed. Answering
        // "nothing is visible under this id" would be a claim this server did
        // not verify, so the read error is surfaced instead. A client that
        // retries the delete gets `not_found`, so no second delete can happen.
        Err(error) => return Err(error),
    };
    Ok(AppServerSkillDeleteResult {
        skill_id: params.skill_id,
        deleted: true,
        revealed_origin,
    })
}

async fn list_connectors_impl(
    state: &AppServerRouterState,
) -> Result<Vec<AppServerConnectorSummary>, AppServerError> {
    connector_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn list_models_impl(
    state: &AppServerRouterState,
) -> Result<AppServerModelList, AppServerError> {
    model_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn get_connector_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<AppServerConnectorDetail, AppServerError> {
    connector_catalog_provider(state)?.get(connector_id).await.map_err(AppServerError::from)
}

async fn connector_status_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<AppServerConnectorStatusView, AppServerError> {
    connector_catalog_provider(state)?
        .status(connector_id)
        .await
        .map_err(AppServerError::from)
}

async fn connector_test_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<AppServerConnectorProbeResult, AppServerError> {
    connector_catalog_provider(state)?
        .test(connector_id)
        .await
        .map_err(AppServerError::from)
}

async fn connector_auth_status_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<AppServerOAuthStatusView, AppServerError> {
    connector_auth_provider(state)?
        .auth_status(connector_id)
        .await
        .map_err(AppServerError::from)
}

async fn connector_auth_start_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<AppServerOAuthStartResult, AppServerError> {
    connector_auth_provider(state)?
        .auth_start(connector_id)
        .await
        .map_err(AppServerError::from)
}

async fn connector_auth_logout_impl(
    state: &AppServerRouterState,
    connector_id: &str,
) -> Result<(), AppServerError> {
    connector_auth_provider(state)?
        .logout(connector_id)
        .await
        .map_err(AppServerError::from)
}

// ---------------------------------------------------------------------------
// Agent Store Importer / Agent / Team catalog impls
// ---------------------------------------------------------------------------

fn import_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ImportProvider>, AppServerError> {
    state.imports.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "import pipeline is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

fn agent_catalog_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn AgentCatalogProvider>, AppServerError> {
    state.agent_catalog.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "agent catalog is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

fn team_catalog_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn TeamCatalogProvider>, AppServerError> {
    state.team_catalog.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "team catalog is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

async fn run_import_impl(
    state: &AppServerRouterState,
    request: AppServerImportRequest,
) -> Result<AppServerImportResult, AppServerError> {
    import_provider(state)?.run(request).await.map_err(AppServerError::from)
}

async fn list_imports_impl(
    state: &AppServerRouterState,
    limit: u32,
) -> Result<Vec<AppServerImportSummary>, AppServerError> {
    import_provider(state)?.list(limit).await.map_err(AppServerError::from)
}

async fn get_import_impl(
    state: &AppServerRouterState,
    snapshot_id: &str,
) -> Result<AppServerImportDetail, AppServerError> {
    import_provider(state)?.get(snapshot_id).await.map_err(AppServerError::from)
}

// --- install seam (roadmap Phase 2) ----------------------------------------

fn install_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn InstallProvider>, AppServerError> {
    state.installs.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "install pipeline is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

async fn install_impl(
    state: &AppServerRouterState,
    request: AppServerInstallRequest,
) -> Result<AppServerInstallResult, AppServerError> {
    install_provider(state)?.install(request).await.map_err(AppServerError::from)
}

async fn install_status_impl(
    state: &AppServerRouterState,
    snapshot_id: &str,
) -> Result<AppServerInstallStatus, AppServerError> {
    install_provider(state)?.status(snapshot_id).await.map_err(AppServerError::from)
}

async fn install_disable_impl(
    state: &AppServerRouterState,
    snapshot_id: &str,
    component_ids: &[String],
) -> Result<AppServerInstallStatus, AppServerError> {
    install_provider(state)?
        .disable(snapshot_id, component_ids)
        .await
        .map_err(AppServerError::from)
}

async fn install_enable_impl(
    state: &AppServerRouterState,
    snapshot_id: &str,
    component_ids: &[String],
) -> Result<AppServerInstallStatus, AppServerError> {
    install_provider(state)?
        .enable(snapshot_id, component_ids)
        .await
        .map_err(AppServerError::from)
}

async fn install_uninstall_impl(
    state: &AppServerRouterState,
    snapshot_id: &str,
    component_ids: &[String],
) -> Result<AppServerInstallStatus, AppServerError> {
    install_provider(state)?
        .uninstall(snapshot_id, component_ids)
        .await
        .map_err(AppServerError::from)
}

// --- marketplace seam (roadmap Phase 2) -------------------------------------

fn marketplace_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn MarketplaceProvider>, AppServerError> {
    state.markets.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "marketplace pipelines are not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

/// Register marketplace sources declared under `[default_marketplaces.*]` in
/// the agent-store config. Idempotent (same source returns the existing row).
/// Failures are non-fatal: a broken default source is reported as a warning and
/// the rest keeps working. Network reach is bounded by `tokio::time::timeout`.
///
/// Returns `true` when every source is registered (or there is nothing to
/// register) and `false` when at least one source could not be fetched; the
/// caller uses that to decide whether a later attempt should retry.
///
/// When the config file is absent (fresh install), the builtin public mirror
/// is registered instead, so a new user can browse the store before touching
/// any config. A config file that exists but declares no `default_marketplaces`
/// also falls back. Explicit user markets always win over builtin ones.
async fn ensure_default_marketplaces(state: &AppServerRouterState) -> bool {
    let provider = match marketplace_provider(state) {
        Ok(provider) => provider,
        // Nothing to register: no provider means the store surface is off.
        Err(_) => return true,
    };
    // Only an *explicitly injected* config path enables default marketplace
    // auto-registration (production passes the agent-store config; tests keep
    // `None` so the host user's personal sources never leak into test state).
    let Some(path) = state.agent_store_config_path.clone() else {
        return true;
    };
    // Load the user config; a missing/unreadable file falls back to the
    // builtin public mirror, an existing file drives the source list.
    let sources: Vec<(String, String, String)> = match AgentStoreConfig::load(&path) {
        Ok(config) if !config.default_marketplaces.is_empty() => config
            .default_marketplaces
            .iter()
            .filter_map(|(id, entry)| {
                let (kind, source) = entry.resolved()?;
                Some((id.clone(), kind, source))
            })
            .collect(),
        Ok(_) => AgentStoreConfig::builtin_default_marketplaces(),
        Err(_) => AgentStoreConfig::builtin_default_marketplaces(),
    };
    let mut complete = true;
    for (marketplace_id, source_kind, source) in sources {
        let request = AppServerMarketplaceAddRequest {
            name: Some(marketplace_id.clone()),
            source_kind: match source_kind.as_str() {
                "github" => nomifun_api_types::AppServerMarketplaceSourceKind::Github,
                "git" => nomifun_api_types::AppServerMarketplaceSourceKind::Git,
                "directory" => nomifun_api_types::AppServerMarketplaceSourceKind::Directory,
                _ => nomifun_api_types::AppServerMarketplaceSourceKind::Url,
            },
            source,
        };
        // Best effort: default sources are convenience, never a hard failure.
        // Timeout-bounded so a dead source cannot hold the warm-up forever.
        // A full-tree HTTP mirror (hundreds of skill dirs, thousands of
        // assets) is the common worst case and takes 1–3 minutes over a
        // public mirror at BATCH=32, so the bound is 600s; a genuinely dead
        // source still fails fast per-request (15s client timeout) and the
        // staging guard reclaims the partial tree.
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            provider.add(request),
        )
        .await;
        if !matches!(outcome, Ok(Ok(_))) {
            complete = false;
        }
    }
    complete
}

/// Default-marketplace warm-up state (D-SDK-1 ①). Two flags because "in flight"
/// and "finished" are different questions:
/// - `RUNNING` is set when a warm-up task starts and cleared when it ends, so
///   `marketplaces_warming()` tells a client the truth;
/// - `DONE` latches only on a **complete** run and stops later calls from
///   re-spawning. An incomplete run leaves it clear, so the next store/market
///   call retries.
static DEFAULT_MARKETPLACES_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static DEFAULT_MARKETPLACES_DONE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `true` while the built-in default marketplaces are still registering in the
/// background (D-SDK-1 ①). The store projection surfaces it so a client can
/// tell an incomplete catalog from a genuinely empty one.
///
/// Deliberately *not* "has a warm-up ever run": a completed run reports
/// `false`, otherwise `markets_pending` would be permanently true.
pub fn marketplaces_warming() -> bool {
    DEFAULT_MARKETPLACES_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

/// Start the default-marketplace registration **off the request path**.
///
/// Before this (D-SDK-1), `store/list` / `market/list` / `market/get` awaited
/// `ensure_default_marketplaces` inline: on a fresh data dir that is a
/// full-tree HTTP mirror of the builtin markets (~90s measured), so the first
/// call of a cold install paid it — and the SDK's default temp data dir paid it
/// again on every spawn. A request now answers from whatever is already
/// registered (possibly empty) while the mirroring continues in the
/// background; clients re-list later.
pub fn warm_default_marketplaces(state: &AppServerRouterState) {
    use std::sync::atomic::Ordering;
    if DEFAULT_MARKETPLACES_DONE.load(Ordering::SeqCst) {
        return; // an earlier run already registered everything
    }
    if DEFAULT_MARKETPLACES_RUNNING.swap(true, Ordering::SeqCst) {
        return; // already running
    }
    let state = state.clone();
    tokio::spawn(async move {
        let complete = ensure_default_marketplaces(&state).await;
        // Latch a complete run only: an incomplete one stays clear so the next
        // store/market call retries. (This crate has no logging seam; the
        // caller-visible effect is the retry itself.)
        DEFAULT_MARKETPLACES_DONE.store(complete, Ordering::SeqCst);
        DEFAULT_MARKETPLACES_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// One auto-update sweep per process.
static MARKETPLACES_AUTO_UPDATE_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The declared sweep cadence, or `None` when the host did not opt in.
fn auto_update_cadence(state: &AppServerRouterState) -> Option<std::time::Duration> {
    let config = AgentStoreConfig::load(state.agent_store_config_path.as_deref()?).ok()?;
    config.marketplace?.cadence()
}

/// Start the background auto-update sweep for official marketplaces
/// (doc 21 D7 ①).
///
/// Two gates, both deliberate:
/// 1. `[marketplace] auto_update_interval_hours` must be present in the host
///    config — the table alone is not consent, and `0` reads as off;
/// 2. only marketplaces the provider reports as both `auto_update`-on **and**
///    official are swept (`18` §7: V1 never polls third-party sources).
///
/// Every sweep goes through the ordinary `refresh` path, so the revision /
/// ETag short-circuit in `18` §5.2 still decides whether anything is actually
/// downloaded. A failing marketplace is skipped for that tick — never retried
/// in a tight loop, which is what would turn a dead mirror into a hot loop.
pub fn start_marketplace_auto_update(state: &AppServerRouterState) {
    use std::sync::atomic::Ordering;
    if MARKETPLACES_AUTO_UPDATE_STARTED.swap(true, Ordering::SeqCst) {
        return; // already sweeping
    }
    let Some(cadence) = auto_update_cadence(state) else {
        // Leave the guard clear: nothing to run, and a later caller with a
        // config path still gets its chance.
        MARKETPLACES_AUTO_UPDATE_STARTED.store(false, Ordering::SeqCst);
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cadence);
        // `interval` yields its first tick immediately; skip it so startup
        // traffic stays with the warm-up rather than racing it.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Ok(provider) = marketplace_provider(&state) else {
                continue;
            };
            let targets = provider.auto_update_targets().await.unwrap_or_default();
            for marketplace_id in targets {
                let _ = provider.refresh(&marketplace_id).await;
            }
        }
    });
}

async fn market_add_impl(
    state: &AppServerRouterState,
    request: AppServerMarketplaceAddRequest,
) -> Result<AppServerMarketplaceSummary, AppServerError> {
    marketplace_provider(state)?.add(request).await.map_err(AppServerError::from)
}

async fn market_list_impl(
    state: &AppServerRouterState,
) -> Result<Vec<AppServerMarketplaceSummary>, AppServerError> {
    warm_default_marketplaces(state);
    marketplace_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn market_get_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
) -> Result<AppServerMarketplaceDetail, AppServerError> {
    warm_default_marketplaces(state);
    marketplace_provider(state)?.get(marketplace_id).await.map_err(AppServerError::from)
}

async fn market_remove_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
    cascade: bool,
) -> Result<AppServerMarketplaceRemoveResult, AppServerError> {
    marketplace_provider(state)?
        .remove(marketplace_id, cascade)
        .await
        .map_err(AppServerError::from)
}

async fn market_auto_update_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
    enabled: bool,
) -> Result<AppServerMarketplaceSummary, AppServerError> {
    marketplace_provider(state)?
        .set_auto_update(marketplace_id, enabled)
        .await
        .map_err(AppServerError::from)
}

async fn market_refresh_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
) -> Result<AppServerMarketplaceRefreshResult, AppServerError> {
    marketplace_provider(state)?
        .refresh(marketplace_id)
        .await
        .map_err(AppServerError::from)
}

async fn market_entry_import_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
    entry_name: &str,
) -> Result<AppServerImportResult, AppServerError> {
    marketplace_provider(state)?
        .import_entry(marketplace_id, entry_name)
        .await
        .map_err(AppServerError::from)
}

// --- store seam (winget-style unified catalog, roadmap Phase 3) -------------

fn store_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn StoreProvider>, AppServerError> {
    state.store.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "store pipeline is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

async fn store_list_impl(
    state: &AppServerRouterState,
) -> Result<AppServerStoreList, AppServerError> {
    warm_default_marketplaces(state);
    store_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn store_install_entry_impl(
    state: &AppServerRouterState,
    marketplace_id: &str,
    entry_name: &str,
) -> Result<AppServerStoreInstallResult, AppServerError> {
    store_provider(state)?
        .install_entry(marketplace_id, entry_name)
        .await
        .map_err(AppServerError::from)
}

async fn list_agents_impl(
    state: &AppServerRouterState,
) -> Result<Vec<AppServerAgentSummary>, AppServerError> {
    agent_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn get_agent_impl(
    state: &AppServerRouterState,
    agent_id: &str,
) -> Result<AppServerAgentDetail, AppServerError> {
    agent_catalog_provider(state)?.get(agent_id).await.map_err(AppServerError::from)
}

async fn list_teams_impl(
    state: &AppServerRouterState,
) -> Result<Vec<AppServerTeamSummary>, AppServerError> {
    team_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn get_team_impl(
    state: &AppServerRouterState,
    team_id: &str,
) -> Result<AppServerTeamDetail, AppServerError> {
    team_catalog_provider(state)?.get(team_id).await.map_err(AppServerError::from)
}

const IMPORT_HISTORY_LIMIT: u32 = 50;

async fn run_import_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AppServerImportRequest>, JsonRejection>,
) -> Result<Json<AppServerImportResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    Ok(Json(run_import_impl(&state, request).await?))
}

async fn list_imports_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<AppServerImportSummary>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_imports_impl(&state, IMPORT_HISTORY_LIMIT).await?))
}

async fn get_import_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(snapshot_id): Path<String>,
) -> Result<Json<AppServerImportDetail>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(get_import_impl(&state, &snapshot_id).await?))
}

/// Serve a public display asset from an immutable snapshot (avatar images
/// declared by `plugin.json`). The snapshot id and asset path are validated:
/// the resolved path must stay under the snapshot root and the extension
/// must be on the public whitelist. No directory listings, no raw prompt
/// files (`SKILL.md`/`*.md` are never served here).
async fn snapshot_asset_route(
    State(state): State<AppServerRouterState>,
    Path((snapshot_id, asset_path)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppServerError> {
    // Public display content (avatars from imported snapshots) referenced by
    // plain `<img>` tags: no connection header, no owner auth.
    let root = state.snapshot_assets_root.as_ref().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "snapshot assets are not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })?;
    let ext = asset_path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let content_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => {
            return Err(AppServerError::new(
                "invalid_asset",
                "asset type is not served",
                StatusCode::BAD_REQUEST,
                false,
            ));
        }
    };
    // Snapshot ids are opaque UUIDv7; asset path must be a safe relative path.
    let Some(snapshot_dir) = snapshot_dir_name(&snapshot_id) else {
        return Err(AppServerError::new(
            "invalid_asset",
            "invalid snapshot id",
            StatusCode::BAD_REQUEST,
            false,
        ));
    };
    let base = root.join(&snapshot_dir);
    let canonical_base = std::fs::canonicalize(&base).map_err(|_| {
        AppServerError::new("not_found", "snapshot not found", StatusCode::NOT_FOUND, false)
    })?;
    let target = base.join(&asset_path);
    let canonical_target = std::fs::canonicalize(&target).map_err(|_| {
        AppServerError::new("not_found", "asset not found", StatusCode::NOT_FOUND, false)
    })?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(AppServerError::new(
            "invalid_asset",
            "asset path escapes the snapshot",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    if !canonical_target.is_file() {
        return Err(AppServerError::new(
            "not_found",
            "asset not found",
            StatusCode::NOT_FOUND,
            false,
        ));
    }
    let bytes = tokio::fs::read(&canonical_target).await.map_err(|_| {
        AppServerError::new("not_found", "asset not found", StatusCode::NOT_FOUND, false)
    })?;
    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], bytes))
}

/// Serve a public display asset from a marketplace entry that is not
/// imported yet (store card avatar). The entry directory is resolved through
/// the marketplace seam; the path and MIME whitelist mirror the snapshot
/// asset endpoint. Skips the whitelist for svg (allowed) and rejects
/// non-whitelisted types; prompt files are never served.
async fn store_asset_route(
    State(state): State<AppServerRouterState>,
    Path((marketplace_id, entry_name, asset_path)): Path<(String, String, String)>,
) -> Result<impl IntoResponse, AppServerError> {
    // Assets are public display content (avatars / icons) referenced by
    // plain `<img>` tags, which cannot carry the app-server connection
    // header. Only the auth middleware (`Extension<CurrentUser>`) guards
    // the route; the connection lifecycle does not apply to stateless
    // asset GETs.
    let ext = asset_path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let content_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => {
            return Err(AppServerError::new(
                "invalid_asset",
                "asset type is not served",
                StatusCode::BAD_REQUEST,
                false,
            ));
        }
    };
    let provider = marketplace_provider(&state)?;
    // Entry-level assets (plugin.json `avatar`, e.g. `avatars/expert.png`);
    // market-level assets (`icons/<id>.svg`) live on the market root instead.
    let mut bases = vec![provider.entry_dir(&marketplace_id, &entry_name).await?];
    if !bases[0].join(&asset_path).is_file() {
        if let Ok(market) = provider.market_dir(&marketplace_id).await {
            bases.push(market);
        }
    }
    for base in bases {
        let target = base.join(&asset_path);
        let canonical_base = match std::fs::canonicalize(&base) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let canonical_target = match std::fs::canonicalize(&target) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !canonical_target.starts_with(&canonical_base) {
            return Err(AppServerError::new(
                "invalid_asset",
                "asset path escapes the entry",
                StatusCode::BAD_REQUEST,
                false,
            ));
        }
        if !canonical_target.is_file() {
            continue;
        }
        let bytes = tokio::fs::read(&canonical_target).await.map_err(|_| {
            AppServerError::new("not_found", "asset not found", StatusCode::NOT_FOUND, false)
        })?;
        return Ok(([(axum::http::header::CONTENT_TYPE, content_type)], bytes));
    }
    Err(AppServerError::new(
        "not_found",
        "asset not found",
        StatusCode::NOT_FOUND,
        false,
    ))
}

/// Validate a snapshot id is a bare UUIDv7-like opaque id (36 chars, hex+dash)
/// so it cannot be used as a path traversal vector.
fn snapshot_dir_name(snapshot_id: &str) -> Option<String> {
    if snapshot_id.len() == 36
        && snapshot_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
        && snapshot_id.chars().filter(|c| *c == '-').count() == 4
    {
        Some(snapshot_id.to_owned())
    } else {
        None
    }
}

// --- install route handlers (roadmap Phase 2) ------------------------------

async fn run_install_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AppServerInstallRequest>, JsonRejection>,
) -> Result<Json<AppServerInstallResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    Ok(Json(install_impl(&state, request).await?))
}

async fn install_status_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(snapshot_id): Path<String>,
) -> Result<Json<AppServerInstallStatus>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(install_status_impl(&state, &snapshot_id).await?))
}

async fn install_disable_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(snapshot_id): Path<String>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<AppServerInstallStatus>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(value) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let component_ids: Vec<String> = value
        .get("component_ids")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    Ok(Json(install_disable_impl(&state, &snapshot_id, &component_ids).await?))
}

async fn install_enable_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(snapshot_id): Path<String>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<AppServerInstallStatus>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(value) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let component_ids: Vec<String> = value
        .get("component_ids")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    Ok(Json(install_enable_impl(&state, &snapshot_id, &component_ids).await?))
}

async fn install_uninstall_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(snapshot_id): Path<String>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<AppServerInstallStatus>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(value) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let component_ids: Vec<String> = value
        .get("component_ids")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    Ok(Json(install_uninstall_impl(&state, &snapshot_id, &component_ids).await?))
}

async fn market_add_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AppServerMarketplaceAddRequest>, JsonRejection>,
) -> Result<Json<AppServerMarketplaceSummary>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(request) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    Ok(Json(market_add_impl(&state, request).await?))
}

async fn market_list_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<AppServerMarketplaceSummary>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(market_list_impl(&state).await?))
}

async fn market_get_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(marketplace_id): Path<String>,
) -> Result<Json<AppServerMarketplaceDetail>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(market_get_impl(&state, &marketplace_id).await?))
}

async fn market_remove_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(marketplace_id): Path<String>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<AppServerMarketplaceRemoveResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(value) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let cascade = value
        .get("cascade")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    Ok(Json(market_remove_impl(&state, &marketplace_id, cascade).await?))
}

async fn market_auto_update_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(marketplace_id): Path<String>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<AppServerMarketplaceSummary>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(value) =
        body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let enabled = value
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    Ok(Json(market_auto_update_impl(&state, &marketplace_id, enabled).await?))
}

async fn market_refresh_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(marketplace_id): Path<String>,
) -> Result<Json<AppServerMarketplaceRefreshResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(market_refresh_impl(&state, &marketplace_id).await?))
}

async fn market_entry_import_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path((marketplace_id, entry_name)): Path<(String, String)>,
) -> Result<Json<AppServerImportResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(market_entry_import_impl(&state, &marketplace_id, &entry_name).await?))
}

async fn store_list_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AppServerStoreList>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(store_list_impl(&state).await?))
}

async fn store_install_entry_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path((marketplace_id, entry_name)): Path<(String, String)>,
) -> Result<Json<AppServerStoreInstallResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(store_install_entry_impl(&state, &marketplace_id, &entry_name).await?))
}

async fn list_skills_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<AppServerSkillSummary>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_skills_impl(&state).await?))
}

async fn get_skill_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(skill_id): Path<String>,
) -> Result<Json<AppServerSkillDetail>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(get_skill_impl(&state, &skill_id).await?))
}

async fn list_connectors_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<AppServerConnectorSummary>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_connectors_impl(&state).await?))
}

async fn list_models_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AppServerModelList>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_models_impl(&state).await?))
}

async fn get_connector_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorDetail>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(get_connector_impl(&state, &connector_id).await?))
}

async fn connector_status_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorStatusView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(connector_status_impl(&state, &connector_id).await?))
}

async fn connector_test_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorProbeResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(connector_test_impl(&state, &connector_id).await?))
}

async fn connector_auth_start_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerOAuthStartResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(connector_auth_start_impl(&state, &connector_id).await?))
}

async fn connector_auth_status_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerOAuthStatusView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(connector_auth_status_impl(&state, &connector_id).await?))
}

async fn connector_auth_logout_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    connector_auth_logout_impl(&state, &connector_id).await?;
    Ok(Json(serde_json::json!({ "connector_id": connector_id, "logged_out": true })))
}

async fn agent_run(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AgentRunRequest>, JsonRejection>,
) -> Result<Json<AgentRunReceipt>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    let (principal_id, client_id) = state.registry.ready_idempotency_context(connection_id, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let receipt = execute_agent_run(&state, &user, &principal_id, &client_id, request).await?;
    Ok(Json(receipt))
}

async fn execute_agent_run(
    state: &AppServerRouterState,
    user: &CurrentUser,
    _principal_id: &str,
    client_id: &str,
    request: AgentRunRequest,
) -> Result<AgentRunReceipt, AppServerError> {
    let goal = request.normalized_goal()?;
    if request.workspace.is_some() && request.work_dir.is_some() {
        return Err(AppServerError::new(
            "workspace_denied",
            "workspace and work_dir cannot be supplied together",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let fingerprint_request = AgentRunRequest {
        idempotency_key: None,
        ..request.clone()
    };
    let fingerprint = request_fingerprint(&fingerprint_request)?;
    let scope = request.idempotency_key.as_ref().map(|key| AppServerIdempotencyScope {
        principal_id: user.id.as_str().to_owned(),
        client_id: client_id.to_owned(),
        method: "agent/run".to_owned(),
        idempotency_key: key.clone(),
    });
    // Serialize keyed mutations in this process even when the receipt store is
    // durable. The database still arbitrates across processes; this gate closes
    // the common same-process lookup/start/commit race.
    let _idempotency_guard = if scope.is_some() {
        Some(state.registry.idempotency_lock().await)
    } else {
        None
    };
    if let Some(scope) = scope.as_ref() {
        if let Some(repository) = state.idempotency.as_ref() {
            if let Some(receipt) = load_idempotent_response::<AgentRunReceipt>(repository, scope, &fingerprint).await? {
                return Ok(receipt);
            }
        } else if let Some(receipt) = state.registry.existing_idempotent_run(&scope_key(scope), &fingerprint)? {
            return Ok(receipt);
        }
    }

    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let resolved_work_dir = resolve_workspace_for_run(state, user, request.workspace.as_ref()).await?;
    if resolved_work_dir.is_none() && request.work_dir.is_some() && state.workspace_resolver.is_some() {
        return Err(AppServerError::new(
            "workspace_denied",
            "raw work_dir is not accepted by the registered workspace policy",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let work_dir = resolved_work_dir
        .map(|workspace| workspace.path().to_string_lossy().into_owned())
        .or(request.work_dir.clone());
    let preset_service = state.preset_service.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server Preset service is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let mut resolved_preset_id = request.preset_id.clone();
    let mut overrides = PresetOverrides::default();
    // Structured `@` mentions resolve against the runtime seams and project
    // into the frozen snapshot (docs/agent-store/05 §4.7):
    // - an agent mention selects the installed preset for the definition
    //   (replacing the legacy `agent_id`/`preset_id` field);
    // - skill mentions mount into `included_skills`;
    // - connector mentions attach MCP servers (validated enabled below).
    apply_mentions(state, &mut resolved_preset_id, &mut overrides, &request.mentions).await?;
    let preset = preset_service.get(&resolved_preset_id).await?;
    validate_agent_store_preset_source(preset.source, preset.source_key.as_deref(), Some(&preset.name))?;
    let mut snapshot = preset_service
        .resolve(
            &resolved_preset_id,
            PresetTarget::ExecutionStep,
            None,
            overrides.clone(),
        )
        .await?;
    // Installed agent-store presets are created without a model binding
    // (the definition payload has no model mandate). A run without a
    // resolved model is rejected at the runtime boundary, so fall back to
    // the owner's first enabled provider/model when the preset left the
    // model unbound. The fallback must keep every mention override
    // (`include_skills`, `mcp_server_ids`, ...): re-resolving from
    // `PresetOverrides::default()` silently drops them.
    if snapshot.resolved_model.is_none() {
        if let Some(model) = default_run_model(state).await? {
            let retry = preset_service
                .resolve(
                    &resolved_preset_id,
                    PresetTarget::ExecutionStep,
                    None,
                    with_default_model(overrides, &model),
                )
                .await?;
            snapshot = retry;
        }
    }
    validate_nomi_runtime_type(snapshot.resolved_agent_type.as_deref())?;
    // Preset MCP references must exist and be enabled before the run starts.
    // The attempt runner projects them into the attempt conversation later;
    // silently dropping a missing/disabled connector would hide a pinned
    // dependency, so reject up-front with `connector_unavailable`.
    if !snapshot.mcp_server_ids.is_empty() {
        let connectors = connector_catalog_provider(state)?;
        for mcp_server_id in &snapshot.mcp_server_ids {
            let detail = connectors.get(mcp_server_id).await.map_err(AppServerError::from)?;
            if !detail.summary.enabled {
                return Err(AppServerError::new(
                    "connector_unavailable",
                    format!("connector {mcp_server_id} referenced by the preset is disabled"),
                    StatusCode::BAD_REQUEST,
                    false,
                ));
            }
        }
    }
    let mut receipt = runtime
        .start_agent_run(user.id.as_str(), snapshot, goal, work_dir, request.steps)
        .await
        .map_err(AgentRuntimeAdapter::map_error)?;
    let internal_run_id = receipt.run_id.clone();
    receipt.run_id = match map_public_run_id(state, user.id.as_str(), &internal_run_id).await {
        Ok(public_run_id) => public_run_id,
        Err(error) => {
            // A runtime execution without a public mapping is not reachable
            // through this protocol. Cancel it best-effort before surfacing the
            // mapping failure so a retry cannot leave active orphan work.
            let _ = runtime
                .cancel_run(user.id.as_str(), &internal_run_id, receipt.version)
                .await;
            return Err(error);
        }
    };
    if let Some(scope) = scope {
        if let Some(repository) = state.idempotency.as_ref() {
            return commit_idempotent_response(repository, scope, fingerprint, &receipt).await;
        }
        return Ok(state.registry.remember_idempotent_run(&scope_key(&scope), fingerprint, receipt)?);
    }
    Ok(receipt)
}

fn scope_key(scope: &AppServerIdempotencyScope) -> String {
    format!("{}:{}:{}:{}", scope.principal_id, scope.client_id, scope.method, scope.idempotency_key)
}

fn db_error(error: nomifun_db::DbError) -> AppServerError {
    AppServerError::from(nomifun_common::AppError::from(error))
}

async fn load_idempotent_response<T: DeserializeOwned>(
    repository: &Arc<dyn IAppServerIdempotencyRepository>,
    scope: &AppServerIdempotencyScope,
    fingerprint: &str,
) -> Result<Option<T>, AppServerError> {
    match repository.lookup(scope, fingerprint).await.map_err(db_error)? {
        AppServerIdempotencyLookup::Missing => Ok(None),
        AppServerIdempotencyLookup::Conflict(_) => Err(AppServerError::from(ProtocolError::IdempotencyConflict)),
        AppServerIdempotencyLookup::Replay(row) => serde_json::from_str(&row.response_json)
            .map(Some)
            .map_err(|error| AppServerError::new("internal_error", format!("stored idempotency response is invalid: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)),
    }
}

async fn commit_idempotent_response<T: Serialize + DeserializeOwned + Clone>(
    repository: &Arc<dyn IAppServerIdempotencyRepository>,
    scope: AppServerIdempotencyScope,
    fingerprint: String,
    response: &T,
) -> Result<T, AppServerError> {
    let response_json = serde_json::to_string(response).map_err(|error| AppServerError::new(
        "internal_error", format!("failed to persist idempotency response: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true,
    ))?;
    let receipt = NewAppServerIdempotencyReceipt {
        scope,
        request_fingerprint: fingerprint,
        response_json,
        created_at: nomifun_common::now_ms(),
    };
    match repository.commit_completed(&receipt).await.map_err(db_error)? {
        AppServerIdempotencyCommit::Conflict(_) => Err(AppServerError::from(ProtocolError::IdempotencyConflict)),
        AppServerIdempotencyCommit::Stored(row) | AppServerIdempotencyCommit::Replay(row) => serde_json::from_str(&row.response_json)
            .map_err(|error| AppServerError::new("internal_error", format!("stored idempotency response is invalid: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)),
    }
}

async fn map_public_run_id(
    state: &AppServerRouterState,
    owner_id: &str,
    internal_run_id: &str,
) -> Result<String, AppServerError> {
    let repository = state.run_mappings.as_ref().ok_or_else(|| {
        AppServerError::new(
            "internal_error",
            "App Server public run mapping is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    repository
        .create_mapping(internal_run_id, owner_id)
        .await
        .map(|row| row.public_run_id)
        .map_err(db_error)
}

async fn resolve_internal_run_id(
    state: &AppServerRouterState,
    owner_id: &str,
    public_run_id: &str,
) -> Result<String, AppServerError> {
    let repository = state.run_mappings.as_ref().ok_or_else(|| {
        AppServerError::new(
            "internal_error",
            "App Server public run mapping is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    repository
        .get_by_public_id(public_run_id, owner_id)
        .await
        .map_err(db_error)?
        .map(|row| row.execution_id)
        .ok_or_else(|| AppServerError::new("not_found", "run not found", StatusCode::NOT_FOUND, false))
}

async fn resolve_workspace_for_run(
    state: &AppServerRouterState,
    user: &CurrentUser,
    workspace: Option<&WorkspaceRef>,
) -> Result<Option<ResolvedWorkspace>, AppServerError> {
    let Some(workspace) = workspace else { return Ok(None); };
    let registry = state.workspaces.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied", "workspace registry is unavailable", StatusCode::SERVICE_UNAVAILABLE, true,
    ))?;
    let resolver = state.workspace_resolver.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied", "workspace resolver is unavailable", StatusCode::SERVICE_UNAVAILABLE, true,
    ))?;
    let row = registry.get(user.id.as_str(), &workspace.id).await.map_err(db_error)?
        .ok_or_else(|| AppServerError::new("workspace_denied", "workspace is not registered for this owner", StatusCode::FORBIDDEN, false))?;
    if row.status != nomifun_db::models::APP_SERVER_WORKSPACE_STATUS_ACTIVE {
        return Err(AppServerError::new("workspace_denied", "workspace is revoked", StatusCode::FORBIDDEN, false));
    }
    let resolved = resolver.resolve_registered(&row.workspace_id, &row.root_path)?;
    let canonical = std::fs::canonicalize(resolved.path()).map_err(|_| AppServerError::new(
        "workspace_denied", "workspace cannot be resolved", StatusCode::FORBIDDEN, false,
    ))?;
    if canonical.to_string_lossy() != row.root_path {
        return Err(AppServerError::new("workspace_denied", "workspace registration changed", StatusCode::FORBIDDEN, false));
    }
    Ok(Some(resolved))
}

/// Public model selection on the wire. Unlike the internal `ProviderWithModel`,
/// `provider_id` is intentionally a plain string here: the App Server accepts
/// either a registered provider UUID (used verbatim) or a `[providers.<key>]`
/// name from `~/.agent-store/config.toml`, which is resolved/registered
/// server-side. Validating UUID-ness during deserialization would reject the
/// config-key convenience surface before resolution can run.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationModelRef {
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub use_model: Option<String>,
}

impl ConversationModelRef {
    fn into_provider_with_model(self) -> ProviderWithModel {
        ProviderWithModel {
            provider_id: self.provider_id,
            model: self.model,
            use_model: self.use_model,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCreateRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// Explicit model selection. When absent, the App Server resolves a
    /// default from `~/.agent-store/config.toml` (falls back to a structured
    /// error when the file has no `default_model`).
    #[serde(default)]
    pub model: Option<ConversationModelRef>,
    #[serde(default)]
    pub workspace: Option<WorkspaceRef>,
    /// OpenAI-style reasoning effort (`low` / `medium` / `high` / `xhigh`)
    /// applied to the conversation's Nomi runtime via `extra.reasoning_effort`.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationUpdateRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// New model selection for the existing Nomi conversation. Uses the same
    /// resolution as `conversation/create` (registered UUID passthrough or
    /// `~/.agent-store/config.toml` provider key).
    #[serde(default)]
    pub model: Option<ConversationModelRef>,
    /// New OpenAI-style reasoning effort for the existing conversation.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationModelOption {
    pub name: String,
    pub display_name: Option<String>,
    pub context_limit: Option<i64>,
    // ── W9（R14）：models.dev 目录事实 ──────────────────────────────────────
    // 只用**已缓存**的目录（`resolve_catalog_capabilities` 不触网）：没有条目
    // （provider 未映射 / 模型不在目录里）就整组缺席，界面上不猜、不显示 0。
    /// 每百万 token 输入价（USD）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_input: Option<f64>,
    /// 每百万 token 输出价（USD）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_output: Option<f64>,
    /// 目录里的上下文窗口（与配置里的 `context_limit` 是两个来源，故分开命名）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_context_window: Option<u64>,
    /// 目录是否声明支持图片输入（发送前兼容性校验用得上，见 W10）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderModelOption {
    pub name: String,
    pub models: Vec<ConversationModelOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationModelSelection {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationModelOptions {
    pub default: Option<ConversationModelSelection>,
    pub providers: Vec<ProviderModelOption>,
    pub reasoning_efforts: Vec<&'static str>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSendRequest {
    pub content: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConversationMessagesQuery {
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    page_size: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationView {
    pub conversation_id: String,
    pub name: String,
    pub model: ProviderWithModel,
    pub status: String,
    pub created_at: i64,
    pub modified_at: i64,
    pub is_processing: bool,
    /// Opaque owner-scoped workspace this chat lives in. Absent for legacy
    /// chats created before workspace lineage was recorded (the UI groups them
    /// under "未归类" rather than guessing from the absolute path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Last server-measured context occupancy. `None` = no `TurnCompleted`
    /// with usage has been recorded; the UI shows "unknown", never a guessed
    /// percentage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsageView>,
}

/// Measured context occupancy for one App Server chat. `percent` is computed
/// server-side and clamped to 0..=100; it is `None` when the engine reported
/// no effective window (`window_tokens == 0`).
#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageView {
    pub used_tokens: i64,
    pub window_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    pub updated_at: i64,
    pub source: &'static str,
}

fn context_usage_view(row: nomifun_db::models::AppServerContextUsageRow) -> ContextUsageView {
    let percent = (row.window_tokens > 0).then(|| {
        let raw = row.context_tokens as f64 / row.window_tokens as f64 * 100.0;
        raw.clamp(0.0, 100.0)
    });
    ContextUsageView {
        used_tokens: row.context_tokens,
        window_tokens: row.window_tokens,
        percent,
        updated_at: row.updated_at,
        source: "measured",
    }
}

/// Additive per-turn token accounting for one completed turn (W9 / R14).
///
/// `input_tokens` / `output_tokens` are the runtime's own report for THIS turn
/// (the `TurnCompleted` event the engine already emits), so the field names
/// mirror the Run-side `TurnUsage` and one accounting vocabulary covers both
/// surfaces. It is deliberately **not** derived from [`ContextUsageView`]:
/// context occupancy is a gauge (the last request's prompt size) and cannot
/// express what one turn cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnUsageView {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Project the runtime's `TurnCompleted` metrics onto the public per-turn
/// usage. `None` — the key stays off the wire entirely — when the frame is
/// missing either side or reported no tokens at all: an unreported turn must
/// stay "unknown", never arrive as a zero that reads as "this turn was free".
fn turn_usage_view(data: &serde_json::Value) -> Option<TurnUsageView> {
    let input_tokens = data.get("input_tokens").and_then(serde_json::Value::as_u64)?;
    let output_tokens = data.get("output_tokens").and_then(serde_json::Value::as_u64)?;
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    Some(TurnUsageView {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationMessageView {
    pub message_id: String,
    pub conversation_id: String,
    pub role: &'static str,
    pub content: serde_json::Value,
    pub message_type: String,
    pub status: Option<String>,
    pub created_at: i64,
}

/// Paginated `conversation/messages` response. `has_more` is the exact
/// server-computed flag (keyset window): `true` when an older page still
/// exists. The client should drive "load older" off this flag instead of the
/// legacy "page came back full-sized" heuristic.
#[derive(Debug, Serialize)]
struct ConversationMessagesPage {
    items: Vec<ConversationMessageView>,
    has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSendReceipt {
    pub conversation_id: String,
    pub message_id: String,
    pub turn_id: Option<String>,
    pub accepted: bool,
    pub replayed: bool,
    pub completed: bool,
    pub result_ok: Option<bool>,
    pub result_text: Option<String>,
    pub result_error: Option<String>,
    pub result_error_code: Option<String>,
    pub result_error_retryable: Option<bool>,
}

fn conversation_service(state: &AppServerRouterState) -> Result<&ConversationService, AppServerError> {
    state.conversation_service.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server conversation runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })
}

fn conversation_runtime_registry(
    state: &AppServerRouterState,
) -> Result<&Arc<dyn nomifun_ai_agent::AgentRuntimeRegistry>, AppServerError> {
    state.conversation_runtime_registry.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server conversation runtime registry is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })
}

fn conversation_workspace_id(extra: &serde_json::Value) -> Option<String> {
    extra
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Project a conversation together with its workspace lineage and the latest
/// measured context occupancy (best-effort: a missing usage row or a read
/// failure simply leaves `context_usage` absent).
async fn project_conversation_view(
    state: &AppServerRouterState,
    conversation: nomifun_api_types::ConversationResponse,
) -> Result<ConversationView, AppServerError> {
    let workspace_id = conversation_workspace_id(&conversation.extra);
    let context_usage = match conversation_service(state) {
        Ok(service) => match service
            .get_app_server_context_usage(&conversation.conversation_id)
            .await
        {
            Ok(Some(row)) => Some(context_usage_view(row)),
            Ok(None) | Err(_) => None,
        },
        Err(_) => None,
    };
    project_conversation(conversation, workspace_id, context_usage)
}

fn project_conversation(
    conversation: nomifun_api_types::ConversationResponse,
    workspace_id: Option<String>,
    context_usage: Option<ContextUsageView>,
) -> Result<ConversationView, AppServerError> {
    let model = conversation.model.ok_or_else(|| {
        AppServerError::new(
            "internal_error",
            "App Server chat is missing its Nomi model selection",
            StatusCode::INTERNAL_SERVER_ERROR,
            true,
        )
    })?;
    Ok(ConversationView {
        conversation_id: conversation.conversation_id,
        name: conversation.name,
        model,
        status: serde_json::to_value(conversation.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "pending".to_owned()),
        created_at: conversation.created_at,
        modified_at: conversation.modified_at,
        is_processing: conversation.runtime.is_some_and(|runtime| runtime.is_processing),
        workspace_id,
        context_usage,
    })
}

fn project_message(message: MessageResponse) -> Option<ConversationMessageView> {
    if message.hidden {
        return None;
    }
    let role = match message.position {
        Some(MessagePosition::Right) => "user",
        Some(MessagePosition::Left) => "assistant",
        _ => "activity",
    };
    let message_type = serde_json::to_value(message.r#type)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    // Internal model state rows (agent prepared/error heartbeats) are not chat
    // content and must never appear in the public history projection.
    if matches!(message.r#type, MessageType::AgentStatus) {
        return None;
    }
    Some(ConversationMessageView {
        message_id: message.message_id,
        conversation_id: message.conversation_id,
        role,
        content: message.content,
        message_type,
        status: message
            .status
            .and_then(|status| serde_json::to_value(status).ok())
            .and_then(|value| value.as_str().map(str::to_owned)),
        created_at: message.created_at,
    })
}

fn project_delivery(conversation_id: &str, delivery: IdempotentMessageDelivery) -> ConversationSendReceipt {
    ConversationSendReceipt {
        conversation_id: conversation_id.to_owned(),
        message_id: delivery.message_id,
        turn_id: delivery.turn_id,
        // A completed/replayed delivery is still a successful acceptance of
        // the caller's idempotent operation; `completed` distinguishes whether
        // there is live work left to stream.
        accepted: true,
        replayed: delivery.replayed,
        completed: delivery.completed,
        result_ok: delivery.result_ok,
        result_text: delivery.result_text,
        result_error: delivery.result_error,
        result_error_code: delivery.result_error_code,
        result_error_retryable: delivery.result_error_retryable,
    }
}

async fn resolved_chat_workspace(
    state: &AppServerRouterState,
    user: &CurrentUser,
    workspace: Option<&WorkspaceRef>,
) -> Result<ResolvedWorkspace, AppServerError> {
    if let Some(resolved) = resolve_workspace_for_run(state, user, workspace).await? {
        return Ok(resolved);
    }

    let resolver = state.workspace_resolver.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied", "workspace resolver is unavailable", StatusCode::SERVICE_UNAVAILABLE, true,
    ))?;
    let repository = state.workspaces.as_ref().ok_or_else(|| AppServerError::new(
        "workspace_denied", "workspace registry is unavailable", StatusCode::SERVICE_UNAVAILABLE, true,
    ))?;
    let workspace_id = generate_id();
    let resolved = resolver.ensure(&workspace_id)?;
    repository
        .register(
            user.id.as_str(),
            resolved.workspace_id(),
            &resolved.path().to_string_lossy(),
        )
        .await
        .map_err(db_error)?;
    Ok(resolved)
}

async fn create_conversation_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    request: ConversationCreateRequest,
) -> Result<ConversationView, AppServerError> {
    let workspace = resolved_chat_workspace(state, user, request.workspace.as_ref()).await?;
    let model = resolve_app_server_model(
        state,
        request.model.map(ConversationModelRef::into_provider_with_model),
    )
    .await?;
    let reasoning_effort = normalize_reasoning_effort(request.reasoning_effort)?;
    let workspace_id = workspace.workspace_id().to_owned();
    let conversation = conversation_service(state)?
        .create_app_server_nomi_chat(
            user.id.as_str(),
            request.name,
            model,
            workspace.path().to_string_lossy().into_owned(),
            Some(workspace_id),
            reasoning_effort,
        )
        .await
        .map_err(AppServerError::from)?;
    project_conversation_view(state, conversation).await
}

/// Validate the public OpenAI-style reasoning effort vocabulary. Empty values
/// are treated as "no explicit effort" (the model/provider default applies).
fn normalize_reasoning_effort(
    value: Option<String>,
) -> Result<Option<String>, AppServerError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if matches!(trimmed, "low" | "medium" | "high" | "xhigh") {
        return Ok(Some(trimmed.to_owned()));
    }
    Err(AppServerError::new(
        "invalid_request",
        format!(
            "reasoning_effort must be one of low/medium/high/xhigh, got '{trimmed}'"
        ),
        StatusCode::BAD_REQUEST,
        false,
    ))
}

/// models.dev catalog facts projected onto one directory entry (W9 / R14).
///
/// Reads the **cached** registry only — `resolve_catalog_capabilities` never
/// fetches, so this cannot turn a model listing into a network call. An
/// unmapped platform (`MergePolicy::Never`, e.g. the bundled `mimo` provider)
/// or an unknown model yields [`CatalogFacts::default`] and the wire fields
/// stay absent instead of being filled with zeros.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct CatalogFacts {
    cost_input: Option<f64>,
    cost_output: Option<f64>,
    catalog_context_window: Option<u64>,
    supports_vision: Option<bool>,
}

fn catalog_model_facts(
    client: &nomifun_models_dev::ModelsDevClient,
    provider: &str,
    model: &str,
) -> CatalogFacts {
    match nomifun_models_dev::resolve_catalog_capabilities(client, provider, model) {
        Some(capabilities) => CatalogFacts {
            cost_input: capabilities.cost_input,
            cost_output: capabilities.cost_output,
            catalog_context_window: capabilities.context_window,
            supports_vision: Some(capabilities.supports_vision),
        },
        None => CatalogFacts::default(),
    }
}

/// Public model catalog for the chat UI: the configured providers/models from
/// `~/.agent-store/config.toml` plus the default selection and the supported
/// reasoning-effort vocabulary. Never exposes credentials.
fn conversation_model_options(state: &AppServerRouterState) -> ConversationModelOptions {
    let config = load_agent_store_config(state, None).ok();
    let mut providers = Vec::new();
    let mut default = None;
    if let Some(config) = config.as_ref() {
        default = config.default_selection().map(|(provider, model)| ConversationModelSelection {
            provider,
            model,
        });
        let mut keys = config.providers.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        for provider_key in keys {
            let mut names = config.models_for_provider(&provider_key);
            names.sort();
            let display_names = config.display_names_for_provider(&provider_key);
            let context_limits = config.context_limits_for_provider(&provider_key);
            let catalog = nomifun_models_dev::default_client();
            providers.push(ProviderModelOption {
                name: provider_key.clone(),
                models: names
                    .into_iter()
                    .map(|name| {
                        let facts = catalog_model_facts(&catalog, &provider_key, &name);
                        ConversationModelOption {
                            display_name: display_names.get(&name).cloned(),
                            context_limit: context_limits.get(&name).copied(),
                            cost_input: facts.cost_input,
                            cost_output: facts.cost_output,
                            catalog_context_window: facts.catalog_context_window,
                            supports_vision: facts.supports_vision,
                            name,
                        }
                    })
                    .collect(),
            });
        }
    }
    ConversationModelOptions {
        default,
        providers,
        reasoning_efforts: vec!["low", "medium", "high", "xhigh"],
    }
}

/// Apply a public model/name/reasoning-effort update to an existing App Server
/// chat. The Nomi runtime is recycled at the next turn boundary when the model
/// or effort changed.
async fn update_conversation_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    request: ConversationUpdateRequest,
) -> Result<ConversationView, AppServerError> {
    let service = conversation_service(state)?;
    service
        .get_app_server_chat(user.id.as_str(), &request.conversation_id)
        .await
        .map_err(AppServerError::from)?;
    let model = match request.model {
        Some(model) => Some(
            resolve_app_server_model(state, Some(model.into_provider_with_model())).await?,
        ),
        None => None,
    };
    let reasoning_effort = normalize_reasoning_effort(request.reasoning_effort)?;
    let extra = reasoning_effort.map(|effort| serde_json::json!({ "reasoning_effort": effort }));
    let conversation = service
        .update(
            user.id.as_str(),
            &request.conversation_id,
            nomifun_api_types::UpdateConversationRequest {
                name: request.name,
                pinned: None,
                model,
                delegation_policy: None,
                execution_model_pool: None,
                decision_policy: None,
                execution_template_id: None,
                extra,
            },
            conversation_runtime_registry(state)?,
        )
        .await
        .map_err(AppServerError::from)?;
    project_conversation_view(state, conversation).await
}

/// Resolve the conversation model selection for an App Server chat.
///
/// Priority:
/// 1. An explicit model whose `provider_id` is already registered is used
///    verbatim (the normal Allo configuration path).
/// 2. An explicit model whose `provider_id` names a `[providers.<key>]` entry
///    in `~/.agent-store/config.toml` registers that provider (idempotently,
///    credentials encrypted) and rewrites the selection to the registered
///    provider UUID.
/// 3. No model: `config.toml`'s `default_model` is used the same way.
async fn resolve_app_server_model(
    state: &AppServerRouterState,
    model: Option<ProviderWithModel>,
) -> Result<ProviderWithModel, AppServerError> {
    if let Some(model) = model.as_ref() {
        if let Some(provider_service) = state.provider_service.as_ref() {
            let registered = provider_service
                .list()
                .await
                .map_err(AppServerError::from)?
                .into_iter()
                .any(|row| row.provider_id == model.provider_id);
            if registered {
                return Ok(model.clone());
            }
        }
    }

    let config = load_agent_store_config(state, model.as_ref())?;
    let (provider_key, model_name) = match model.as_ref() {
        Some(model) => (model.provider_id.clone(), model.model.clone()),
        None => config.default_selection().ok_or_else(|| {
            AppServerError::new(
                "invalid_request",
                "model is required when ~/.agent-store/config.toml defines no default_model",
                StatusCode::BAD_REQUEST,
                false,
            )
        })?,
    };
    let provider_service = state.provider_service.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "provider service is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let provider_id =
        ensure_agent_store_provider(provider_service, &config, &provider_key, Some(&model_name))
            .await?;
    Ok(ProviderWithModel {
        provider_id,
        model: model_name,
        use_model: None,
    })
}

fn load_agent_store_config(
    state: &AppServerRouterState,
    model: Option<&ProviderWithModel>,
) -> Result<AgentStoreConfig, AppServerError> {
    let path = state
        .agent_store_config_path
        .clone()
        .or_else(AgentStoreConfig::default_path);
    let Some(path) = path else {
        return Err(model_selection_error(model));
    };
    if !path.is_file() {
        return Err(model_selection_error(model));
    }
    AgentStoreConfig::load(&path).map_err(|error| {
        AppServerError::new(
            "config_unavailable",
            format!("failed to read agent-store config: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
            false,
        )
    })
}

/// ---------------------------------------------------------------------------
/// Host settings file: `config/get` · `config/set`
/// ---------------------------------------------------------------------------
///
/// Provider / default-model configuration stays **host management surface**
/// (`16` §6): the two methods are additive wire methods with no counterpart in
/// the published client package, and the webui reaches them through its own
/// host-only helpers. The boundaries that hold in both directions:
///
/// - the file's location comes from the host (`agent_store_config_path`, else
///   `~/.agent-store/config.toml`) — no request can name a path;
/// - credentials never cross: the read view has no `api_key` / `base_url` field
///   and the write whitelist (`AgentStoreConfigPatch`) cannot express them;
/// - the owner scope is the connection principal, checked by the dispatch arm
///   (`require_ready`) before any of this runs, and there is no owner/path
///   parameter that could widen it.

fn config_unavailable(message: impl Into<String>) -> AppServerError {
    AppServerError::new(
        "config_unavailable",
        message,
        StatusCode::SERVICE_UNAVAILABLE,
        false,
    )
}

/// Resolve the host's config file location.
fn agent_store_config_file(
    state: &AppServerRouterState,
) -> Result<std::path::PathBuf, AppServerError> {
    state
        .agent_store_config_path
        .clone()
        .or_else(AgentStoreConfig::default_path)
        .ok_or_else(|| {
            config_unavailable("no home directory to resolve ~/.agent-store/config.toml from")
        })
}

/// Project the parsed file into its wire view. Sorted, credential-free, and no
/// fact that is not actually in the file.
fn config_view(config: &AgentStoreConfig, exists: bool) -> AppServerConfigView {
    let mut names: Vec<&String> = config.providers.keys().collect();
    names.sort();
    let providers = names
        .into_iter()
        .map(|name| AppServerConfigProviderView {
            name: name.clone(),
            enabled: config.providers[name].enabled.unwrap_or(true),
            models: config.models_for_provider(name),
        })
        .collect();
    AppServerConfigView {
        exists,
        default_model: config
            .default_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        providers,
        // Absent table → `None` (a client must be able to tell "not configured"
        // from "explicitly off"). A present table with no `distill_enabled` key
        // still reports `Some { distill_enabled: None }` for the same reason.
        memory: config.memory.as_ref().map(|memory| AppServerConfigMemoryView {
            distill_enabled: memory.distill_enabled,
        }),
    }
}

/// `config/get`: the settings file as a wire view.
///
/// A **missing** file is a normal answer (`exists: false`, empty defaults) —
/// never an error, never a fabricated default. An unreadable or unparseable
/// file *is* an error: silently answering with defaults would hide a broken
/// hand-edit behind a settings screen that looks healthy.
fn execute_config_get(state: &AppServerRouterState) -> Result<AppServerConfigView, AppServerError> {
    let path = agent_store_config_file(state)?;
    match std::fs::read_to_string(&path) {
        Ok(source) => {
            let config = AgentStoreConfig::from_source(&source).map_err(|error| {
                config_unavailable(format!("failed to read {}: {error}", path.display()))
            })?;
            Ok(config_view(&config, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(config_view(&AgentStoreConfig::default(), false))
        }
        Err(error) => Err(config_unavailable(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

/// `config/set`: validate the whitelisted patch, rewrite **only** the key it
/// names (comments and every other key survive), then answer with the file
/// re-read from disk — the caller sees the stored value, not an optimistic echo.
fn execute_config_set(
    state: &AppServerRouterState,
    patch: AgentStoreConfigPatch,
) -> Result<AppServerConfigView, AppServerError> {
    let path = agent_store_config_file(state)?;
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        // A host that never created the file yet gets one; that is the whole
        // point of the write path.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(config_unavailable(format!(
                "failed to read {}: {error}",
                path.display()
            )))
        }
    };

    let parsed = AgentStoreConfig::from_source(&source)
        .map_err(|error| config_unavailable(format!("failed to read {}: {error}", path.display())))?;
    if !patch.names_any_key() {
        return Err(AppServerError::new(
            "invalid_request",
            "config/set needs at least one whitelisted key (default_model, memory.distill_enabled)",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    // Each named key is written through its own minimal-change edit, so a
    // request that names two keys rewrites exactly those two and nothing else.
    let mut edited = source.clone();
    if patch.default_model.is_some() {
        let value = patch.validated_default_model(&parsed).map_err(|message| {
            AppServerError::new("invalid_request", message, StatusCode::BAD_REQUEST, false)
        })?;
        edited = AgentStoreConfig::with_default_model(&edited, &value)
            .map_err(|error| config_unavailable(format!("failed to edit {}: {error}", path.display())))?;
    }
    if let Some(distill_enabled) = patch
        .memory
        .as_ref()
        .and_then(|memory| memory.distill_enabled)
    {
        // The `[memory]` write is deliberately unconditional on parsing: the
        // value is a bool, and the host really consumes it at startup
        // (`apps/agent-store` → `set_distill_host_override`), so there is no
        // "resolution will fail later" case to refuse here.
        edited = AgentStoreConfig::with_distill_enabled(&edited, distill_enabled)
            .map_err(|error| config_unavailable(format!("failed to edit {}: {error}", path.display())))?;
    }
    write_config_source(&path, &edited)?;
    execute_config_get(state)
}

/// Minimal, atomic write: a sibling temp file replaces the target (`rename`
/// overwrites on Windows too), so an interrupted save can never leave a
/// half-written config file behind.
fn write_config_source(path: &std::path::Path, source: &str) -> Result<(), AppServerError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            config_unavailable(format!(
                "failed to create {}: {error}",
                parent.display()
            ))
        })?;
    }
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, source).map_err(|error| {
        config_unavailable(format!("failed to write {}: {error}", temp.display()))
    })?;
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        config_unavailable(format!("failed to write {}: {error}", path.display()))
    })
}

fn model_selection_error(model: Option<&ProviderWithModel>) -> AppServerError {
    match model {
        Some(model) => AppServerError::new(
            "provider_not_found",
            format!(
                "provider '{}' is not registered and no matching provider exists in \
                 ~/.agent-store/config.toml",
                model.provider_id
            ),
            StatusCode::BAD_REQUEST,
            false,
        ),
        None => AppServerError::new(
            "invalid_request",
            "model is required (no ~/.agent-store/config.toml file was found)",
            StatusCode::BAD_REQUEST,
            false,
        ),
    }
}

/// Register (idempotently) the provider named `provider_key` from the
/// agent-store config, returning its canonical provider UUID. Registration
/// reuses `ProviderService::create` so encryption and `provider_models`
/// reconciliation are identical to the normal Allo provider UI path. A
/// previously registered provider with the same `name` is reused as-is.
async fn ensure_agent_store_provider(
    provider_service: &ProviderService,
    config: &AgentStoreConfig,
    provider_key: &str,
    requested_model: Option<&str>,
) -> Result<String, AppServerError> {
    let provider_cfg = config.providers.get(provider_key).ok_or_else(|| {
        AppServerError::new(
            "provider_not_found",
            format!(
                "no [providers.{provider_key}] entry in ~/.agent-store/config.toml"
            ),
            StatusCode::BAD_REQUEST,
            false,
        )
    })?;
    let api_key = provider_cfg
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppServerError::new(
                "config_unavailable",
                format!("[providers.{provider_key}] is missing api_key"),
                StatusCode::UNPROCESSABLE_ENTITY,
                false,
            )
        })?;
    let base_url = provider_cfg
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppServerError::new(
                "config_unavailable",
                format!("[providers.{provider_key}] is missing base_url"),
                StatusCode::UNPROCESSABLE_ENTITY,
                false,
            )
        })?;

    // Idempotency: one provider row per agent-store provider key.
    let existing = provider_service
        .list()
        .await
        .map_err(AppServerError::from)?
        .into_iter()
        .find(|row| row.name == provider_key);
    if let Some(existing) = existing {
        return Ok(existing.provider_id);
    }

    let mut models = config.models_for_provider(provider_key);
    if let Some(requested) = requested_model.map(str::trim).filter(|value| !value.is_empty()) {
        if !models.iter().any(|model| model == requested) {
            models.push(requested.to_owned());
        }
    }
    if models.is_empty() {
        return Err(AppServerError::new(
            "config_unavailable",
            format!(
                "no [models.\"{provider_key}/<model>\"] entries exist to register provider '{provider_key}'"
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            false,
        ));
    }

    let created = provider_service
        .create(CreateProviderRequest {
            provider_id: None,
            platform: provider_cfg
                .r#type
                .clone()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "custom".to_owned()),
            name: provider_key.to_owned(),
            base_url: base_url.to_owned(),
            api_key: api_key.to_owned(),
            models,
            enabled: provider_cfg.enabled.unwrap_or(true),
            capabilities: Vec::new(),
            model_context_limits: Some(config.context_limits_for_provider(provider_key)),
            model_protocols: None,
            model_descriptions: Some(config.display_names_for_provider(provider_key)),
            model_enabled: None,
            model_health: None,
            bedrock_config: None,
            is_full_url: false,
            sort_order: None,
        })
        .await
        .map_err(AppServerError::from)?;
    Ok(created.provider_id)
}

async fn get_conversation_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation_id: &str,
) -> Result<ConversationView, AppServerError> {
    let conversation = conversation_service(state)?
        .get_app_server_chat(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)?;
    project_conversation_view(state, conversation).await
}

async fn list_conversation_messages_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation_id: &str,
    query: ConversationMessagesQuery,
) -> Result<ConversationMessagesPage, AppServerError> {
    let service = conversation_service(state)?;
    // Verify the marker + ownership first; message listing alone would only
    // prove row ownership and could otherwise cross this public projection.
    service
        .get_app_server_chat(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)?;
    let result = service
        .list_messages(
            user.id.as_str(),
            conversation_id,
            ListMessagesQuery {
                page: query.page,
                page_size: Some(query.page_size.unwrap_or(100).clamp(1, 200)),
                order: Some("asc".to_owned()),
                content_mode: None,
                cursor: query.cursor,
                day: None,
            },
        )
        .await
        .map_err(AppServerError::from)?;
    Ok(ConversationMessagesPage {
        items: result.items.into_iter().filter_map(project_message).collect(),
        has_more: result.has_more,
    })
}

async fn send_conversation_message_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation_id: &str,
    request: ConversationSendRequest,
) -> Result<ConversationSendReceipt, AppServerError> {
    if request.content.trim().is_empty() {
        return Err(AppServerError::new(
            "invalid_request",
            "content must not be empty",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let service = conversation_service(state)?;
    service
        .get_app_server_chat(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)?;
    let delivery = service
        .send_message_with_idempotency_key(
            user.id.as_str(),
            conversation_id,
            &request.idempotency_key,
            SendMessageRequest {
                content: request.content,
                files: Vec::new(),
                inject_skills: Vec::new(),
                hidden: false,
                origin: None,
                channel_platform: None,
            },
            conversation_runtime_registry(state)?,
        )
        .await
        .map_err(AppServerError::from)?;
    Ok(project_delivery(conversation_id, delivery))
}

async fn conversation_create(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<ConversationCreateRequest>, JsonRejection>,
) -> Result<Json<ConversationView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    Ok(Json(create_conversation_for_user(&state, &user, request).await?))
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConversationListQuery {
    #[serde(default)]
    limit: Option<u32>,
}

async fn conversation_list(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ConversationListQuery>,
) -> Result<Json<Vec<ConversationView>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let conversations = conversation_service(&state)?
        .list_app_server_chats(user.id.as_str(), query.limit.unwrap_or(100))
        .await
        .map_err(AppServerError::from)?;
    let mut views = Vec::with_capacity(conversations.len());
    for conversation in conversations {
        views.push(project_conversation_view(&state, conversation).await?);
    }
    Ok(Json(views))
}

async fn conversation_get(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ConversationView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(get_conversation_for_user(&state, &user, &conversation_id).await?))
}

async fn conversation_messages(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(conversation_id): Path<String>,
    Query(query): Query<ConversationMessagesQuery>,
) -> Result<Json<ConversationMessagesPage>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(
        list_conversation_messages_for_user(&state, &user, &conversation_id, query).await?,
    ))
}

async fn conversation_send(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(conversation_id): Path<String>,
    body: Result<Json<ConversationSendRequest>, JsonRejection>,
) -> Result<Json<ConversationSendReceipt>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    Ok(Json(
        send_conversation_message_for_user(&state, &user, &conversation_id, request).await?,
    ))
}

async fn conversation_cancel(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ConversationView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let service = conversation_service(&state)?;
    service
        .get_app_server_chat(user.id.as_str(), &conversation_id)
        .await
        .map_err(AppServerError::from)?;
    service
        .cancel(user.id.as_str(), &conversation_id, conversation_runtime_registry(&state)?)
        .await
        .map_err(AppServerError::from)?;
    Ok(Json(get_conversation_for_user(&state, &user, &conversation_id).await?))
}

/// Shared deletion path for App Server chats: verify marker + ownership, then
/// delegate to the authoritative `ConversationService::delete` (which owns
/// stop/orphan fences, retained-execution refusal and dependent-row cleanup).
async fn delete_conversation_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation_id: &str,
) -> Result<(), AppServerError> {
    let service = conversation_service(state)?;
    service
        .get_app_server_chat(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)?;
    service
        .delete(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)
}

async fn conversation_delete(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(conversation_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    delete_conversation_for_user(&state, &user, &conversation_id).await?;
    Ok(Json(serde_json::json!({
        "conversation_id": conversation_id,
        "deleted": true
    })))
}

async fn get_run_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    run_id: &str,
) -> Result<AgentRunView, AppServerError> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut view = runtime
        .get_run(user.id.as_str(), &internal_run_id)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    view.run_id = run_id.to_owned();
    Ok(view)
}

async fn run_get(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AgentRunView>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.require_ready(connection_id, &user.id)?;
    let view = get_run_for_user(&state, &user, &run_id).await?;
    Ok(Json(view))
}

async fn run_result(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AgentRunResult>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.require_ready(connection_id, &user.id)?;
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| AppServerError::new("runtime_unavailable", "App Server runtime is unavailable", StatusCode::SERVICE_UNAVAILABLE, true))?;
    let internal_run_id = resolve_internal_run_id(&state, user.id.as_str(), &run_id).await?;
    let mut result = runtime
        .get_result(user.id.as_str(), &internal_run_id)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    result.run_id = run_id;
    Ok(Json(result))
}

/// `GET /api/app-server/run/:run_id/plan` — the HTTP binding of the `run/plan`
/// WS arm (W4 / W6, D-W6-1). Same executor, same owner scope, no extra policy.
async fn run_plan(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<AgentRunPlan>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.require_ready(connection_id, &user.id)?;
    let plan = get_run_plan_for_user(&state, &user, &run_id).await?;
    Ok(Json(plan))
}

/// Shared by the HTTP handler and the WS arm: resolve the public run id inside
/// the caller's owner scope, project the plan, then hand the **public** id back.
async fn get_run_plan_for_user(
    state: &AppServerRouterState,
    user: &CurrentUser,
    run_id: &str,
) -> Result<AgentRunPlan, AppServerError> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut plan = runtime
        .plan(user.id.as_str(), &internal_run_id)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    plan.run_id = run_id.to_owned();
    Ok(plan)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunEventsQuery {
    #[serde(default)]
    after_sequence: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn run_events(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQuery>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<nomifun_agent_execution::AgentRunEvent>>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    state.registry.require_ready(connection_id, &user.id)?;
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| AppServerError::new("runtime_unavailable", "App Server runtime is unavailable", StatusCode::SERVICE_UNAVAILABLE, true))?;
    let internal_run_id = resolve_internal_run_id(&state, user.id.as_str(), &run_id).await?;
    let mut events = runtime
        .list_events(
            user.id.as_str(),
            &internal_run_id,
            query.after_sequence,
            query.limit,
        )
        .await
        .map_err(AgentRuntimeAdapter::map_error)?;
    for event in &mut events {
        event.run_id = run_id.clone();
    }
    Ok(Json(events))
}

async fn run_cancel(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<CancelRunRequest>, JsonRejection>,
) -> Result<Json<AgentRunView>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    let (principal_id, client_id) = state.registry.ready_idempotency_context(connection_id, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let view = execute_cancel_run(
        &state,
        &user,
        &principal_id,
        &client_id,
        &run_id,
        request,
    )
    .await?;
    Ok(Json(view))
}

async fn run_steer(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AgentRunSteerRequest>, JsonRejection>,
) -> Result<Json<AgentRunView>, AppServerError> {
    let _connection_id = connection_id(&headers)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let view = execute_steer_run(&state, &user, &run_id, request).await?;
    Ok(Json(view))
}

/// `POST /api/app-server/run/:run_id/answer-decision` — the HTTP binding of the
/// `run/answer-decision` WS arm. Same executor, same gate, no extra policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerDecisionRequest {
    pub step_id: String,
    pub attempt_id: String,
    pub answer: String,
    pub expected_execution_version: i64,
    pub expected_step_version: i64,
    pub expected_attempt_version: i64,
}

async fn run_answer_decision(
    State(state): State<AppServerRouterState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<AnswerDecisionRequest>, JsonRejection>,
) -> Result<Json<AgentRunView>, AppServerError> {
    let _connection_id = connection_id(&headers)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let view = execute_answer_decision(
        &state,
        &user,
        &run_id,
        &request.step_id,
        &request.attempt_id,
        AnswerExecutionDecisionRequest {
            answer: request.answer,
            expected_execution_version: request.expected_execution_version,
            expected_step_version: request.expected_step_version,
            expected_attempt_version: request.expected_attempt_version,
        },
    )
    .await?;
    Ok(Json(view))
}

async fn execute_cancel_run(
    state: &AppServerRouterState,
    user: &CurrentUser,
    _principal_id: &str,
    client_id: &str,
    run_id: &str,
    request: CancelRunRequest,
) -> Result<AgentRunView, AppServerError> {
    let fingerprint_request = CancelFingerprintRequest {
        run_id,
        expected_version: request.expected_version,
        command_id: request.command_id.as_deref(),
    };
    let fingerprint = request_fingerprint(&fingerprint_request)?;
    let scope = request.idempotency_key.as_ref().map(|key| AppServerIdempotencyScope {
        principal_id: user.id.as_str().to_owned(),
        client_id: client_id.to_owned(),
        method: "run/cancel".to_owned(),
        idempotency_key: key.clone(),
    });
    // Serialize keyed mutations in this process even when the receipt store is
    // durable. The database still arbitrates across processes; this gate closes
    // the common same-process lookup/start/commit race.
    let _idempotency_guard = if scope.is_some() {
        Some(state.registry.idempotency_lock().await)
    } else {
        None
    };
    if let Some(scope) = scope.as_ref() {
        if let Some(repository) = state.idempotency.as_ref() {
            if let Some(view) = load_idempotent_response::<AgentRunView>(repository, scope, &fingerprint).await? {
                return Ok(view);
            }
        } else if let Some(view) = state.registry.existing_idempotent_cancel(&scope_key(scope), &fingerprint)? {
            return Ok(view);
        }
    }
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut view = runtime
        .cancel_run(user.id.as_str(), &internal_run_id, request.expected_version)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    view.run_id = run_id.to_owned();
    if let Some(scope) = scope {
        if let Some(repository) = state.idempotency.as_ref() {
            return commit_idempotent_response(repository, scope, fingerprint, &view).await;
        }
        return Ok(state.registry.remember_idempotent_cancel(&scope_key(&scope), fingerprint, view)?);
    }
    Ok(view)
}

async fn execute_steer_run(
    state: &AppServerRouterState,
    user: &CurrentUser,
    run_id: &str,
    request: AgentRunSteerRequest,
) -> Result<AgentRunView, AppServerError> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut view = runtime
        .steer_run(user.id.as_str(), &internal_run_id, &request.text, request.expected_version)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    view.run_id = run_id.to_owned();
    Ok(view)
}

/// Answer a pending decision on one attempt of a public Run.
///
/// `runtime.answer_decision` is a straight pass-through to the engine's single
/// answer gate (owner scope + three-way CAS + `WaitingInput` only + non-empty
/// answer), so this function adds no policy: it only resolves the public
/// `run_id` to the internal execution id inside the caller's owner scope. The
/// desktop confirmation route's `always_allow` flag deliberately has no
/// counterpart here.
async fn execute_answer_decision(
    state: &AppServerRouterState,
    user: &CurrentUser,
    run_id: &str,
    step_id: &str,
    attempt_id: &str,
    request: AnswerExecutionDecisionRequest,
) -> Result<AgentRunView, AppServerError> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut view = runtime
        .answer_decision(
            user.id.as_str(),
            &internal_run_id,
            step_id,
            attempt_id,
            request,
        )
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    view.run_id = run_id.to_owned();
    Ok(view)
}

#[derive(Debug, Serialize)]
struct CancelFingerprintRequest<'a> {
    run_id: &'a str,
    expected_version: i64,
    command_id: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRunRequest {
    /// `agent_id` is the public protocol name; `preset_id` remains accepted for
    /// compatibility with the initial runtime spike.
    #[serde(rename = "agent_id", alias = "preset_id")]
    pub preset_id: String,
    #[serde(default)]
    pub agent_version: Option<String>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub work_dir: Option<String>,
    #[serde(default)]
    pub workspace: Option<WorkspaceRef>,
    #[serde(default)]
    pub steps: Option<Vec<nomifun_api_types::PlannedExecutionStep>>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Structured Agent Store references resolved from the composer `@`
    /// mentions. The client never sends raw text; it sends resolved catalog
    /// refs and the server enforces the runtime seam for each kind
    /// (docs/agent-store/05 §4.7).
    #[serde(default)]
    pub mentions: Vec<MentionRef>,
}

/// One resolved `@` mention reference. `id` is an opaque catalog id
/// (`wb-<plugin>-<slug>` for agent/skill, MCP server id for connector).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MentionRef {
    pub kind: MentionKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentionKind {
    /// `@expert`: AgentDefinition with an installed preset (`preset_id`).
    Agent,
    /// `@skill`: SkillDefinition mounted into the run context.
    Skill,
    /// `@connector` / `@mcp`: configured MCP server attached to the run.
    Connector,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRef {
    pub id: String,
}

impl AgentRunRequest {
    fn normalized_goal(&self) -> Result<String, AppServerError> {
        if !self.goal.trim().is_empty() {
            return Ok(self.goal.trim().to_owned());
        }
        let Some(input) = self.input.as_ref() else {
            return Err(AppServerError::new(
                "invalid_request",
                "goal or input.text is required",
                StatusCode::BAD_REQUEST,
                false,
            ));
        };
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                AppServerError::new(
                    "invalid_request",
                    "input.text is required when goal is omitted",
                    StatusCode::BAD_REQUEST,
                    false,
                )
            })?;
        Ok(text.to_owned())
    }
}

/// Resolve structured `@` mentions into run-level seams. Each kind enforces
/// its own runtime contract:
/// - `agent`: resolve the AgentDefinition id to its installed `preset_id`
///   (only one agent mention is allowed — two definitions cannot both be the
///   run's lead);
/// - `skill`: mount the definition id into `include_skills` (the resolver
///   freezes it into `included_skills` on the snapshot);
/// - `connector`: attach the MCP server id via `mcp_server_ids` (validated
///   enabled by the caller's connector catalog check).
async fn apply_mentions(
    state: &AppServerRouterState,
    resolved_preset_id: &mut String,
    overrides: &mut PresetOverrides,
    mentions: &[MentionRef],
) -> Result<(), AppServerError> {
    for mention in mentions {
        match mention.kind {
            MentionKind::Agent => {
                let agent = agent_catalog_provider(state)?
                    .get(&mention.id)
                    .await
                    .map_err(AppServerError::from)?;
                let Some(preset_id) = agent.summary.preset_id.as_deref() else {
                    return Err(AppServerError::new(
                        "agent_not_installed",
                        format!("agent {} is not installed; run install/* before starting it", mention.id),
                        StatusCode::BAD_REQUEST,
                        false,
                    ));
                };
                if mentions.iter().filter(|m| m.kind == MentionKind::Agent).count() > 1 {
                    return Err(AppServerError::new(
                        "invalid_mentions",
                        "only one agent may be mentioned per run",
                        StatusCode::BAD_REQUEST,
                        false,
                    ));
                }
                if !resolved_preset_id.is_empty() && *resolved_preset_id != preset_id {
                    return Err(AppServerError::new(
                        "invalid_mentions",
                        "the agent mention conflicts with the explicit agent_id",
                        StatusCode::BAD_REQUEST,
                        false,
                    ));
                }
                *resolved_preset_id = preset_id.to_owned();
            }
            MentionKind::Skill => {
                // Skill ids stay as source-qualified catalog refs; the preset
                // resolver freezes them into `included_skills`.
                if !overrides.include_skills.contains(&mention.id) {
                    overrides.include_skills.push(mention.id.clone());
                }
            }
            MentionKind::Connector => {
                if let Some(ids) = overrides.mcp_server_ids.as_mut() {
                    if !ids.contains(&mention.id) {
                        ids.push(mention.id.clone());
                    }
                } else {
                    overrides.mcp_server_ids = Some(vec![mention.id.clone()]);
                }
            }
        }
    }
    Ok(())
}

/// First enabled provider/model pair for the run model fallback. Returns
/// `None` when no provider is available; the run then fails at the runtime
/// boundary with the standard `InvalidSnapshot` (no silently wrong model).
async fn default_run_model(
    state: &AppServerRouterState,
) -> Result<Option<nomifun_api_types::ModelPreference>, AppServerError> {
    let Some(provider_service) = state.provider_service.as_ref() else {
        return Ok(None);
    };
    let providers = provider_service.list().await.map_err(AppServerError::from)?;
    Ok(providers
        .into_iter()
        .find(|provider| provider.enabled && !provider.models.is_empty())
        .map(|provider| nomifun_api_types::ModelPreference {
            provider_id: Some(provider.provider_id),
            model: provider.models[0].clone(),
            required: true,
        }))
}

/// Merge the owner's default model into an existing override set. Mention
/// overrides (`include_skills`, `mcp_server_ids`, ...) must survive the model
/// fallback: re-resolving from a fresh `PresetOverrides` silently drops them
/// and the run loses every skill/connector the caller mentioned (WP-2 B5).
fn with_default_model(
    base: PresetOverrides,
    model: &nomifun_api_types::ModelPreference,
) -> PresetOverrides {
    PresetOverrides {
        model: Some(model.model.clone()),
        provider_id: model.provider_id.clone(),
        ..base
    }
}

fn request_fingerprint<T: Serialize>(request: &T) -> Result<String, AppServerError> {    let bytes = serde_json::to_vec(request).map_err(|error| {
        AppServerError::new(
            "invalid_request",
            format!("request cannot be fingerprinted: {error}"),
            StatusCode::BAD_REQUEST,
            false,
        )
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn validate_agent_store_preset_source(
    source: PresetSource,
    source_key: Option<&str>,
    preset_name: Option<&str>,
) -> Result<(), nomifun_common::AppError> {
    let legacy_builtin = source == PresetSource::Builtin && source_key == Some("builtin-office");
    // Agent Store installs register user presets named `agent-store: <name>`.
    // Only those user presets may start runs; arbitrary user presets stay
    // out of the App Server compatibility surface.
    let agent_store_user =
        source == PresetSource::User && preset_name.is_some_and(|name| name.starts_with("agent-store: "));
    if !legacy_builtin && !agent_store_user {
        return Err(nomifun_common::AppError::Forbidden(
            "App Server compatibility policy only allows the builtin-office Preset and installed agent-store Presets".into(),
        ));
    }
    Ok(())
}

fn validate_nomi_runtime_type(runtime_type: Option<&str>) -> Result<(), nomifun_common::AppError> {
    if runtime_type != Some("nomi") {
        return Err(nomifun_common::AppError::Forbidden(
            "App Server compatibility policy only allows Presets resolved to a Nomi Runtime Agent".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRunRequest {
    pub expected_version: i64,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

fn connection_id(headers: &HeaderMap) -> Result<&str, nomifun_common::AppError> {
    headers
        .get(CONNECTION_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| nomifun_common::AppError::BadRequest("app-server connection id is required".into()))
}

async fn websocket(
    State(state): State<AppServerRouterState>,
    Extension(user): Extension<CurrentUser>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| handle_websocket(socket, state, user))
}

async fn handle_websocket(socket: WebSocket, state: AppServerRouterState, user: CurrentUser) {
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<String>(128);
    let outbound_gate = Arc::new(tokio::sync::Mutex::new(()));
    let writer = tokio::spawn(async move {
        while let Some(text) = outbound_rx.recv().await {
            if socket_sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let connection = state.registry.open_with_capabilities(
        LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
        CapabilityAvailability::from_state(&state),
    );
    let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
    let event_task = state.event_bus.clone().map(|event_bus| {
        tokio::spawn(forward_app_server_events(
            event_bus.subscribe_user(),
            state.clone(),
            user.id.clone(),
            subscriptions.clone(),
            outbound_gate.clone(),
            outbound_tx.clone(),
        ))
    });

    while let Some(message) = socket_receiver.next().await {
        let message = match message {
            Ok(message) => message,
            Err(_) => break,
        };
        let Message::Text(text) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            continue;
        };
        let request: WsRequest = match serde_json::from_str(&text) {
            Ok(request) => request,
            Err(error) => {
                if !send_ws_output(
                    &outbound_gate,
                    &outbound_tx,
                    ws_error(
                        None,
                        AppServerError::from(ProtocolError::InvalidRequest(error.to_string())),
                    ),
                )
                .await
                {
                    break;
                }
                continue;
            }
        };
        let request_id = request.id.clone();
        if request.jsonrpc != "2.0" {
            let output = ws_error(
                request_id,
                AppServerError::from(ProtocolError::InvalidRequest(
                    "jsonrpc must be \"2.0\"".into(),
                )),
            );
            if !send_ws_output(&outbound_gate, &outbound_tx, output).await {
                break;
            }
            continue;
        }
        // Serialize request dispatch with asynchronous event delivery. This
        // keeps a subscribe response ahead of an event observed immediately
        // after the subscription is installed.
        let _outbound_guard = outbound_gate.lock().await;
        let response = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            request.method.as_str(),
            request.params,
            request_id.clone(),
        )
        .await;
        if request_id.is_none() {
            continue;
        }
        let output = match response {
            Ok(value) => value,
            Err(error) => ws_error(request_id, error),
        };
        if outbound_tx.send(output.to_string()).await.is_err() {
            break;
        }
    }

    let connection_id = connection.connection_id().to_owned();
    state.registry.close(&connection_id);
    drop(outbound_tx);
    if let Some(event_task) = event_task {
        event_task.abort();
        let _ = event_task.await;
    }
    let _ = writer.await;
}

async fn send_ws_output(
    outbound_gate: &Arc<tokio::sync::Mutex<()>>,
    outbound_tx: &mpsc::Sender<String>,
    output: serde_json::Value,
) -> bool {
    let _guard = outbound_gate.lock().await;
    outbound_tx.send(output.to_string()).await.is_ok()
}

#[derive(Default)]
struct WsSubscriptions {
    runs: HashSet<String>,
    conversations: HashSet<String>,
    /// Notification sequence is connection-local. History is the durable,
    /// authoritative replay source; this cursor only deduplicates best-effort
    /// live notifications for one subscribed conversation.
    conversation_sequences: HashMap<String, i64>,
}

fn conversation_event_sequence(
    subscriptions: &Arc<RwLock<WsSubscriptions>>,
    conversation_id: &str,
) -> Option<i64> {
    let mut subscriptions = subscriptions.write().ok()?;
    if !subscriptions.conversations.contains(conversation_id) {
        return None;
    }
    let sequence = subscriptions
        .conversation_sequences
        .entry(conversation_id.to_owned())
        .or_insert(0);
    *sequence += 1;
    Some(*sequence)
}

/// Project first-party Conversation events into a small, stable App Server
/// notification contract. The implementation intentionally does not relay raw
/// agent/runtime payloads: opaque runtime/session/tool identifiers and backend
/// transport fields remain behind the module seam.
fn project_conversation_notification(
    event: &nomifun_api_types::WebSocketMessage<serde_json::Value>,
    subscriptions: &Arc<RwLock<WsSubscriptions>>,
) -> Option<serde_json::Value> {
    let conversation_id = event
        .data
        .get("conversation_id")
        .and_then(serde_json::Value::as_str)?;
    // Hidden relay entries are internal bookkeeping/transcript material, not
    // chat UI content. Do not even advance the public sequence for them.
    if event
        .data
        .get("hidden")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let sequence = conversation_event_sequence(subscriptions, conversation_id)?;
    let message_id = event
        .data
        .get("msg_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let (event_type, payload) = match event.name.as_str() {
        "message.userCreated" => (
            "message.created",
            serde_json::json!({
                "message_id": message_id,
                "role": "user",
                "content": event.data.get("content").cloned().unwrap_or(serde_json::Value::Null),
                "created_at": event.data.get("created_at").cloned().unwrap_or(serde_json::Value::Null),
            }),
        ),
        "message.stream" => {
            let kind = event
                .data
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("activity");
            match kind {
                "content" | "text" => (
                    "message.delta",
                    serde_json::json!({
                        "message_id": message_id,
                        "content": event.data.pointer("/data/content").cloned().unwrap_or(serde_json::Value::Null),
                        "replace": event.data.get("replace").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    }),
                ),
                "thinking" => (
                    "message.thinking",
                    serde_json::json!({
                        "message_id": message_id,
                        "content": event.data.pointer("/data/content").cloned().unwrap_or(serde_json::Value::Null),
                        "subject": event.data.pointer("/data/subject").cloned().unwrap_or(serde_json::Value::Null),
                        "status": event.data.pointer("/data/status").cloned().unwrap_or(serde_json::Value::Null),
                        "duration": event.data.pointer("/data/duration").cloned().unwrap_or(serde_json::Value::Null),
                        "replace": event.data.get("replace").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    }),
                ),
                "tips" => (
                    "message.tips",
                    serde_json::json!({
                        "message_id": message_id,
                        "content": event.data.pointer("/data/content").cloned().unwrap_or(serde_json::Value::Null),
                        "tip_type": event.data.pointer("/data/type").cloned().unwrap_or(serde_json::Value::Null),
                        "created_at": event.data.get("created_at").cloned().unwrap_or(serde_json::Value::Null),
                    }),
                ),
                "tool_call" => (
                    "message.tool",
                    serde_json::json!({
                        "message_id": message_id,
                        "name": event.data.pointer("/data/name").cloned().unwrap_or(serde_json::Value::Null),
                        "status": event.data.pointer("/data/status").cloned().unwrap_or(serde_json::Value::Null),
                        // The live row must render what the reloaded row renders:
                        // without these, the same tool call showed no arguments
                        // while streaming and gained them after a reload.
                        // Opaque ids (`call_id`, session ids) stay behind the seam.
                        "args": event.data.pointer("/data/args").cloned().unwrap_or(serde_json::Value::Null),
                        "output": event.data.pointer("/data/output").cloned().unwrap_or(serde_json::Value::Null),
                    }),
                ),
                "error" => (
                    "message.error",
                    serde_json::json!({
                        "message_id": message_id,
                        "message": event.data.pointer("/data/message").cloned().unwrap_or(serde_json::Value::Null),
                        "code": event.data.pointer("/data/code").cloned().unwrap_or(serde_json::Value::Null),
                        "retryable": event.data.pointer("/data/retryable").cloned().unwrap_or(serde_json::Value::Null),
                    }),
                ),
                // W9（R14）：把运行时的逐轮用量带上 wire。引擎的 `TurnCompleted`
                // 事件本来就带本轮的 `input_tokens` / `output_tokens`，此前在这个投影里
                // 被降级成「活动标记」、载荷整段丢掉，于是客户端只有**会话级**占用
                // （`context.usage`），算不出「本轮花了多少」。这里**原样**带上运行时已经
                // 给出的用量（additive：缺一侧或整轮没上报就整段不出现），`kind` 与
                // `message.activity` 关系不变——R31 的「收尾中」标记与降噪规则照旧。
                "turn_completed" => {
                    let payload = match turn_usage_view(&event.data["data"]) {
                        Some(usage) => serde_json::json!({
                            "message_id": message_id,
                            "kind": kind,
                            "usage": usage,
                        }),
                        None => serde_json::json!({ "message_id": message_id, "kind": kind }),
                    };
                    ("message.activity", payload)
                }
                _ => (
                    "message.activity",
                    serde_json::json!({ "message_id": message_id, "kind": kind }),
                ),
            }
        }
        "turn.started" => (
            "turn.status",
            serde_json::json!({
                "turn_id": event.data.get("turn_id").cloned().unwrap_or(serde_json::Value::Null),
                "status": "running",
            }),
        ),
        "turn.completed" => (
            "turn.status",
            serde_json::json!({
                "turn_id": event.data.get("turn_id").cloned().unwrap_or(serde_json::Value::Null),
                "status": "completed",
            }),
        ),
        "context.usage" => (
            "context.usage",
            serde_json::json!({
                "context_usage": event.data.get("context_usage").cloned().unwrap_or(serde_json::Value::Null),
            }),
        ),
        _ => return None,
    };

    Some(ws_notification(
        "conversation/event",
        serde_json::json!({
            "conversation_id": conversation_id,
            "sequence": sequence,
            "event_type": event_type,
            "payload": payload,
        }),
    ))
}

async fn forward_app_server_events(
    mut receiver: tokio::sync::broadcast::Receiver<UserEventEnvelope>,
    state: AppServerRouterState,
    owner_id: UserId,
    subscriptions: Arc<RwLock<WsSubscriptions>>,
    outbound_gate: Arc<tokio::sync::Mutex<()>>,
    outbound_tx: mpsc::Sender<String>,
) {
    loop {
        let envelope = match receiver.recv().await {
            Ok(envelope) => envelope,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let (run_ids, conversation_ids) = subscriptions
                    .read()
                    .map(|subscriptions| {
                        (
                            subscriptions.runs.iter().cloned().collect::<Vec<_>>(),
                            subscriptions.conversations.iter().cloned().collect::<Vec<_>>(),
                        )
                    })
                    .unwrap_or_default();
                if !run_ids.is_empty()
                    && !send_ws_output(
                        &outbound_gate,
                        &outbound_tx,
                        ws_notification(
                            "run/resync-required",
                            serde_json::json!({
                                "run_ids": run_ids,
                                "reason": "event_stream_lagged"
                            }),
                        ),
                    )
                    .await
                {
                    break;
                }
                if !conversation_ids.is_empty()
                    && !send_ws_output(
                        &outbound_gate,
                        &outbound_tx,
                        ws_notification(
                            "conversation/resync-required",
                            serde_json::json!({
                                "conversation_ids": conversation_ids,
                                "reason": "event_stream_lagged"
                            }),
                        ),
                    )
                    .await
                {
                    break;
                }
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        if envelope.user_id != owner_id.as_str() {
            continue;
        }
        if let Some(notification) = project_conversation_notification(&envelope.event, &subscriptions) {
            if !send_ws_output(&outbound_gate, &outbound_tx, notification).await {
                break;
            }
            continue;
        }
        if envelope.event.name != "agentExecution.changed" {
            continue;
        }
        let Some(internal_run_id) = envelope.event.data.get("execution_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(repository) = state.run_mappings.as_ref() else {
            continue;
        };
        let mapping = match repository.get_by_execution_id(internal_run_id, owner_id.as_str()).await {
            Ok(Some(mapping)) => mapping,
            Ok(None) | Err(_) => continue,
        };
        let public_run_id = mapping.public_run_id;
        let subscribed = subscriptions
            .read()
            .map(|subscriptions| subscriptions.runs.contains(&public_run_id))
            .unwrap_or(false);
        if !subscribed {
            continue;
        }
        let sequence = envelope
            .event
            .data
            .get("sequence")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        let notification = match state.runtime.as_ref() {
            Some(runtime) => match runtime
                .list_events(owner_id.as_str(), internal_run_id, Some(sequence.saturating_sub(1)), Some(1))
                .await
            {
                Ok(mut events) if events.first().is_some_and(|event| event.sequence == sequence) => {
                    let mut event = events.remove(0);
                    event.run_id = public_run_id.clone();
                    ws_notification(
                        "event",
                        serde_json::to_value(event).unwrap_or_else(|_| serde_json::json!({
                            "run_id": public_run_id,
                            "sequence": sequence,
                            "event_type": "run.resync_required",
                            "payload": {}
                        })),
                    )
                }
                _ => ws_notification(
                    "event",
                    serde_json::json!({
                        "run_id": public_run_id,
                        "sequence": sequence,
                        "event_type": "run.resync_required",
                        "payload": {"reason": "event_unavailable"}
                    }),
                ),
            },
            None => continue,
        };
        if !send_ws_output(&outbound_gate, &outbound_tx, notification).await {
            break;
        }
    }
}

async fn dispatch_connection_request(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    subscriptions: &Arc<RwLock<WsSubscriptions>>,
    method: &str,
    params: serde_json::Value,
    request_id: Option<serde_json::Value>,
) -> Result<serde_json::Value, AppServerError> {
    match method {
        "initialize" => {
            let params = parse_ws_params::<InitializeRequest>(params)?;
            let result = state
                .registry
                .initialize(connection.connection_id(), params)?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode initialize result: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "initialized" => {
            state
                .registry
                .mark_initialized(connection.connection_id(), &user.id)?;
            Ok(ws_response(request_id, serde_json::json!({ "ok": true })))
        }
        "ping" => {
            let context = state.registry.require_ready(connection.connection_id(), &user.id)?;
            Ok(ws_response(
                request_id,
                serde_json::to_value(context).map_err(|error| {
                    AppServerError::new(
                        "internal_error",
                        format!("failed to encode auth context: {error}"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                        true,
                    )
                })?,
            ))
        }
        "conversation/delete" => {
            let params = parse_ws_params::<WsConversationQuery>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            delete_conversation_for_user(state, user, &params.conversation_id).await?;
            if let Ok(mut subscriptions) = subscriptions.write() {
                subscriptions.conversations.remove(&params.conversation_id);
                subscriptions.conversation_sequences.remove(&params.conversation_id);
            }
            Ok(ws_response(request_id, serde_json::json!({
                "conversation_id": params.conversation_id,
                "deleted": true
            })))
        }
        "workspace/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let repository = state.workspaces.as_ref().ok_or_else(|| {
                AppServerError::new(
                    "workspace_denied",
                    "workspace registry is unavailable",
                    StatusCode::SERVICE_UNAVAILABLE,
                    true,
                )
            })?;
            let rows = repository
                .list_active(user.id.as_str())
                .await
                .map_err(db_error)?;
            Ok(ws_response(request_id, serde_json::to_value(rows.into_iter().map(workspace_view).collect::<Vec<_>>()).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode workspaces: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "workspace/create" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WorkspaceCreateRequest>(params)?;
            let view = workspace_create_impl(state, user, &params.path).await?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode workspace: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "workspace/revoke" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WorkspaceRevokeParams>(params)?;
            let result = workspace_revoke_impl(state, user, &params.workspace_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode workspace revoke result: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/create" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<ConversationCreateRequest>(params)?;
            let conversation = create_conversation_for_user(state, user, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(conversation).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversation: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/model-options" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let options = conversation_model_options(state);
            Ok(ws_response(request_id, serde_json::to_value(options).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode model options: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/update" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<ConversationUpdateRequest>(params)?;
            let conversation = update_conversation_for_user(state, user, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(conversation).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversation: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<ConversationListQuery>(params)?;
            let conversations = conversation_service(state)?
                .list_app_server_chats(user.id.as_str(), params.limit.unwrap_or(100))
                .await
                .map_err(AppServerError::from)?;
            let mut views = Vec::with_capacity(conversations.len());
            for conversation in conversations {
                views.push(project_conversation_view(state, conversation).await?);
            }
            Ok(ws_response(request_id, serde_json::to_value(views).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversations: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/get" => {
            let params = parse_ws_params::<WsConversationQuery>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let conversation = get_conversation_for_user(state, user, &params.conversation_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(conversation).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversation: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/messages" => {
            let params = parse_ws_params::<WsConversationMessages>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let messages = list_conversation_messages_for_user(
                state,
                user,
                &params.conversation_id,
                ConversationMessagesQuery { page: params.page, page_size: params.page_size, cursor: params.cursor },
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(messages).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode messages: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/send" => {
            let params = parse_ws_params::<WsConversationSend>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let receipt = send_conversation_message_for_user(
                state,
                user,
                &params.conversation_id,
                ConversationSendRequest { content: params.content, idempotency_key: params.idempotency_key },
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(receipt).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversation send receipt: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/cancel" => {
            let params = parse_ws_params::<WsConversationQuery>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let service = conversation_service(state)?;
            service
                .get_app_server_chat(user.id.as_str(), &params.conversation_id)
                .await
                .map_err(AppServerError::from)?;
            service
                .cancel(user.id.as_str(), &params.conversation_id, conversation_runtime_registry(state)?)
                .await
                .map_err(AppServerError::from)?;
            let conversation = get_conversation_for_user(state, user, &params.conversation_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(conversation).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode conversation: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "conversation/subscribe" => {
            let params = parse_ws_params::<WsConversationQuery>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let _ = get_conversation_for_user(state, user, &params.conversation_id).await?;
            subscriptions
                .write()
                .map_err(|_| AppServerError::new("internal_error", "subscription state unavailable", StatusCode::INTERNAL_SERVER_ERROR, true))?
                .conversations
                .insert(params.conversation_id.clone());
            Ok(ws_response(request_id, serde_json::json!({"conversation_id": params.conversation_id, "subscribed": true})))
        }
        "conversation/unsubscribe" => {
            let params = parse_ws_params::<WsConversationQuery>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let _ = get_conversation_for_user(state, user, &params.conversation_id).await?;
            let mut subscriptions = subscriptions
                .write()
                .map_err(|_| AppServerError::new("internal_error", "subscription state unavailable", StatusCode::INTERNAL_SERVER_ERROR, true))?;
            subscriptions.conversations.remove(&params.conversation_id);
            subscriptions.conversation_sequences.remove(&params.conversation_id);
            Ok(ws_response(request_id, serde_json::json!({"conversation_id": params.conversation_id, "subscribed": false})))
        }
        "agent/run" => {
            let (principal_id, client_id) = state.registry.ready_idempotency_context(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<AgentRunRequest>(params)?;
            let receipt = execute_agent_run(state, user, &principal_id, &client_id, params).await?;
            Ok(ws_response(
                request_id,
                serde_json::to_value(receipt).map_err(|error| {
                    AppServerError::new(
                        "internal_error",
                        format!("failed to encode run receipt: {error}"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                        true,
                    )
                })?,
            ))
        }
        "run/get" => {
            let params = parse_ws_params::<WsRunQuery>(params)?;
            let view = ws_get_run(state, connection, user, &params.run_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode run view: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/result" => {
            let params = parse_ws_params::<WsRunQuery>(params)?;
            let result = ws_get_result(state, connection, user, &params.run_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode run result: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/plan" => {
            // W4 / W6（D-W6-1）：计划与步骤的权威快照。与 `run/get` 同一条 owner
            // 解析路径，HTTP 臂共用 `get_run_plan_for_user`。
            let params = parse_ws_params::<WsRunQuery>(params)?;
            let plan = ws_get_plan(state, connection, user, &params.run_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(plan).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode run plan: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/events" => {
            let params = parse_ws_params::<WsRunEvents>(params)?;
            let events = ws_list_events(state, connection, user, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(events).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode run events: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/cancel" => {
            let params = parse_ws_params::<WsCancelRun>(params)?;
            let view = ws_cancel_run(state, connection, user, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode cancelled run: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/steer" => {
            let params = parse_ws_params::<WsSteerRun>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let view = execute_steer_run(state, user, &params.run_id, AgentRunSteerRequest {
                text: params.text,
                expected_version: params.expected_version,
                command_id: params.command_id,
                idempotency_key: params.idempotency_key,
            })
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode steered run: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/answer-decision" => {
            let params = parse_ws_params::<WsAnswerDecision>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let view = execute_answer_decision(
                state,
                user,
                &params.run_id,
                &params.step_id,
                &params.attempt_id,
                AnswerExecutionDecisionRequest {
                    answer: params.answer,
                    expected_execution_version: params.expected_execution_version,
                    expected_step_version: params.expected_step_version,
                    expected_attempt_version: params.expected_attempt_version,
                },
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new(
                    "internal_error",
                    format!("failed to encode answered run: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    true,
                )
            })?))
        }
        "run/subscribe" => {
            let params = parse_ws_params::<WsRunSubscription>(params)?;
            let _ = ws_get_run(state, connection, user, &params.run_id).await?;
            subscriptions
                .write()
                .map_err(|_| AppServerError::new("internal_error", "subscription state unavailable", StatusCode::INTERNAL_SERVER_ERROR, true))?
                .runs
                .insert(params.run_id.clone());
            Ok(ws_response(
                request_id,
                serde_json::json!({"run_id": params.run_id, "subscribed": true}),
            ))
        }
        "run/unsubscribe" => {
            let params = parse_ws_params::<WsRunSubscription>(params)?;
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            // Keep unsubscribe owner-scoped just like subscribe. A caller must
            // present a public mapping belonging to this user before changing
            // the connection's local subscription set.
            let _ = resolve_internal_run_id(state, user.id.as_str(), &params.run_id).await?;
            subscriptions
                .write()
                .map_err(|_| AppServerError::new("internal_error", "subscription state unavailable", StatusCode::INTERNAL_SERVER_ERROR, true))?
                .runs
                .remove(&params.run_id);
            Ok(ws_response(
                request_id,
                serde_json::json!({"run_id": params.run_id, "subscribed": false}),
            ))
        }
        // ---------------- Agent Store Skill catalog ----------------
        "skill/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let skills = list_skills_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(skills).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skills: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "skill/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillQuery>(params)?;
            let skill = get_skill_impl(state, &params.skill_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(skill).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Skill write face (host management surface) ---------
        // `16` R17 / W12: the store's own CRUD over *user* skills. WebSocket
        // only and deliberately absent from the published SDK package — a
        // third-party consumer must not be able to write files into this
        // host's skill tree (`16` §6), same judgement as `config/*`. All three
        // go through the same owner gate as every other method, take their
        // root from the host (never from a parameter), and answer from a
        // re-read of the catalog instead of echoing the request.
        "skill/create" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillCreate>(params)?;
            let skill = execute_skill_create(state, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(skill).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "skill/update" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillUpdate>(params)?;
            let skill = execute_skill_update(state, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(skill).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "skill/delete" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillDelete>(params)?;
            let result = execute_skill_delete(state, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill delete result: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "skill/copy" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillCopy>(params)?;
            let skill = execute_skill_copy(state, params).await?;
            Ok(ws_response(request_id, serde_json::to_value(skill).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Connector catalog ----------------
        "connector/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let connectors = list_connectors_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(connectors).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connectors: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Host settings file (config/get · config/set) ---------
        // Provider / default-model config is host management surface, not a
        // third-party SDK capability (`16` §6): additive wire methods with no
        // counterpart in the published client package. Both go through the same
        // owner gate as every other method (`require_ready`), take their path
        // from the host (never from a parameter) and cannot express a credential.
        "config/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            parse_ws_params::<WsConfigQuery>(params)?;
            let view = execute_config_get(state)?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode config view: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "config/set" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<AgentStoreConfigPatch>(params)?;
            let view = execute_config_set(state, params)?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode config view: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Public model directory ----------------
        "models/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let models = list_models_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(models).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode models: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let connector = get_connector_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(connector).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/status" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let status = connector_status_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/test" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let result = connector_test_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector probe: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Connector OAuth ----------------
        "connector/auth/status" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let status = connector_auth_status_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode oauth status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/auth/start" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let result = connector_auth_start_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode oauth start: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/auth/logout" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            connector_auth_logout_impl(state, &params.connector_id).await?;
            Ok(ws_response(request_id, serde_json::json!({ "connector_id": params.connector_id, "logged_out": true })))
        }
        // ---------------- Agent Store Agent catalog (05 §4.1) ----------------
        "agent/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let agents = list_agents_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(agents).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode agents: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "agent/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsAgentQuery>(params)?;
            let agent = get_agent_impl(state, &params.agent_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(agent).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode agent: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Team catalog (05 §4.2) ----------------
        "team/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let teams = list_teams_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(teams).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode teams: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "team/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsTeamQuery>(params)?;
            let team = get_team_impl(state, &params.team_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(team).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode team: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store unified store catalog (Phase 3) ----------------
        "store/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let store = store_list_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(store).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode store: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "store/install-entry" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsStoreEntry>(params)?;
            let result = store_install_entry_impl(state, &params.marketplace_id, &params.entry_name).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode store install: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Importer (05 §4.4; same impls as the HTTP routes) ----------------
        "import/run" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let request = parse_ws_params::<AppServerImportRequest>(params)?;
            let result = run_import_impl(state, request).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode import: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "import/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let imports = list_imports_impl(state, IMPORT_HISTORY_LIMIT).await?;
            Ok(ws_response(request_id, serde_json::to_value(imports).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode imports: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "import/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSnapshotQuery>(params)?;
            let detail = get_import_impl(state, &params.snapshot_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(detail).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode import: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Installer (05 §4.5; same impls as the HTTP routes) ----------------
        "install/run" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let request = parse_ws_params::<AppServerInstallRequest>(params)?;
            let result = install_impl(state, request).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode install: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "install/status" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSnapshotQuery>(params)?;
            let status = install_status_impl(state, &params.snapshot_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode install status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "install/disable" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsInstallComponents>(params)?;
            let status = install_disable_impl(state, &params.snapshot_id, &params.component_ids).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode install status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "install/enable" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsInstallComponents>(params)?;
            let status = install_enable_impl(state, &params.snapshot_id, &params.component_ids).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode install status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "install/uninstall" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsInstallComponents>(params)?;
            let status = install_uninstall_impl(state, &params.snapshot_id, &params.component_ids).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode install status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Marketplaces (05 §4.6; same impls as the HTTP routes) ----------------
        "market/add" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let request = parse_ws_params::<AppServerMarketplaceAddRequest>(params)?;
            let market = market_add_impl(state, request).await?;
            Ok(ws_response(request_id, serde_json::to_value(market).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplace: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let markets = market_list_impl(state).await?;
            Ok(ws_response(request_id, serde_json::to_value(markets).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplaces: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsMarketplaceQuery>(params)?;
            let market = market_get_impl(state, &params.marketplace_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(market).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplace: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/remove" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsMarketRemove>(params)?;
            let result = market_remove_impl(state, &params.marketplace_id, params.cascade.unwrap_or(true)).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplace remove: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/auto-update" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsMarketAutoUpdate>(params)?;
            let market = market_auto_update_impl(state, &params.marketplace_id, params.enabled.unwrap_or(true)).await?;
            Ok(ws_response(request_id, serde_json::to_value(market).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplace: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/refresh" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsMarketplaceQuery>(params)?;
            let result = market_refresh_impl(state, &params.marketplace_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode marketplace refresh: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "market/entry-import" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsStoreEntry>(params)?;
            let result = market_entry_import_impl(state, &params.marketplace_id, &params.entry_name).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode market entry import: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        _ => Err(AppServerError::from(ProtocolError::InvalidRequest(
            "unknown App Server method".into(),
        ))),
    }
}

fn parse_ws_params<T>(params: serde_json::Value) -> Result<T, AppServerError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(params).map_err(|error| {
        AppServerError::from(ProtocolError::InvalidRequest(error.to_string()))
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConversationQuery {
    conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillQuery {
    skill_id: String,
}

/// `skill/create` (`16` R17 / W12).
///
/// Structured fields, not raw Markdown: the extension primitive assembles the
/// canonical frontmatter, so the stored document's `name` always equals the id
/// it is addressed by. `deny_unknown_fields` is load-bearing — a request
/// carrying `api_key` / `env` / `token` is `invalid_request` instead of being
/// silently dropped (R22 keeps its credential gate), and no field can name a
/// path.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillCreate {
    name: String,
    description: String,
    #[serde(default)]
    when_to_use: Option<String>,
    #[serde(default)]
    allowed_tools: Option<String>,
    #[serde(default)]
    paths: Option<String>,
    #[serde(default)]
    body: String,
}

impl WsSkillCreate {
    fn into_request(self) -> SkillCreateRequest {
        SkillCreateRequest {
            name: self.name,
            description: self.description,
            when_to_use: self.when_to_use,
            allowed_tools: self.allowed_tools,
            paths: self.paths,
            body: self.body,
        }
    }
}

/// `skill/update`: a field-level edit of one writable skill.
///
/// Every field is optional and `None` means "leave it alone"; the server merges
/// the named fields into the document it reads from disk. Two consequences are
/// deliberate:
/// - the caller never has to send the whole file, which matters because
///   `skill/get` caps the body it exposes (a full-document round trip is not
///   even possible through the read face);
/// - `name` is not patchable — the public id *is* the frontmatter name, so
///   renaming is not an edit (`skill/copy` derives a new skill instead).
///
/// `deny_unknown_fields`: `api_key` / `env` / `token` are `invalid_request`,
/// never silently dropped (R22 keeps its gate).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillUpdate {
    skill_id: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    when_to_use: Option<String>,
    #[serde(default)]
    allowed_tools: Option<String>,
    #[serde(default)]
    paths: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

impl WsSkillUpdate {
    /// True when the request names at least one field to change. A patch that
    /// names nothing is refused rather than answered with an unchanged skill.
    fn names_any_field(&self) -> bool {
        self.description.is_some()
            || self.when_to_use.is_some()
            || self.allowed_tools.is_some()
            || self.paths.is_some()
            || self.body.is_some()
    }

    fn field_patch(&self) -> SkillFieldPatch<'_> {
        SkillFieldPatch {
            description: self.description.as_deref(),
            when_to_use: self.when_to_use.as_deref(),
            allowed_tools: self.allowed_tools.as_deref(),
            paths: self.paths.as_deref(),
            body: self.body.as_deref(),
        }
    }
}

/// `skill/delete`: one writable skill id.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillDelete {
    skill_id: String,
}

/// `skill/copy`: derive a new user skill from an existing one.
///
/// The source may live in **any** origin (built-in, marketplace install, shared,
/// companion, draft, user) — copying a read-only skill into the user root is the
/// only way to make it editable, which is why this method exists. The target is
/// always `{user_skills_dir}/{new_name}`: no field can name a path, and
/// `new_name` must be free in every origin (`conflict` otherwise).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillCopy {
    skill_id: String,
    new_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsAgentQuery {
    agent_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsTeamQuery {
    team_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsStoreEntry {
    marketplace_id: String,
    entry_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConnectorQuery {
    connector_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSnapshotQuery {
    snapshot_id: String,
}

/// `config/get` takes no parameters — `deny_unknown_fields` on an empty struct
/// is what makes `{"path": "…"}` a hard `invalid_request` instead of a silently
/// ignored field.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConfigQuery {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsInstallComponents {
    snapshot_id: String,
    #[serde(default)]
    component_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsMarketplaceQuery {
    marketplace_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsMarketRemove {
    marketplace_id: String,
    #[serde(default)]
    cascade: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsMarketAutoUpdate {
    marketplace_id: String,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConversationMessages {
    conversation_id: String,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    page_size: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConversationSend {
    conversation_id: String,
    content: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsRunQuery {
    run_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsRunSubscription {
    run_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsRunEvents {
    run_id: String,
    #[serde(default)]
    after_sequence: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsCancelRun {
    run_id: String,
    expected_version: i64,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSteerRun {
    run_id: String,
    text: String,
    expected_version: i64,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

/// `run/answer-decision` params.
///
/// The three `expected_*_version` fields are the engine's CAS tokens, not a
/// client-supplied convenience: `answer-decision` refuses to apply an answer
/// once any of them moved. `deny_unknown_fields` is load-bearing here — a
/// desktop-style `always_allow` / approve-all flag is not part of this method
/// and must fail fast instead of being silently ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsAnswerDecision {
    run_id: String,
    step_id: String,
    attempt_id: String,
    answer: String,
    expected_execution_version: i64,
    expected_step_version: i64,
    expected_attempt_version: i64,
}

async fn ws_get_run(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    run_id: &str,
) -> Result<AgentRunView, AppServerError> {
    state.registry.require_ready(connection.connection_id(), &user.id)?;
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new("runtime_unavailable", "App Server runtime is unavailable", StatusCode::SERVICE_UNAVAILABLE, true)
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut view = runtime
        .get_run(user.id.as_str(), &internal_run_id)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    view.run_id = run_id.to_owned();
    Ok(view)
}

async fn ws_get_plan(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    run_id: &str,
) -> Result<AgentRunPlan, AppServerError> {
    state
        .registry
        .require_ready(connection.connection_id(), &user.id)?;
    get_run_plan_for_user(state, user, run_id).await
}

async fn ws_get_result(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    run_id: &str,
) -> Result<AgentRunResult, AppServerError> {
    state.registry.require_ready(connection.connection_id(), &user.id)?;
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new("runtime_unavailable", "App Server runtime is unavailable", StatusCode::SERVICE_UNAVAILABLE, true)
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), run_id).await?;
    let mut result = runtime
        .get_result(user.id.as_str(), &internal_run_id)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    result.run_id = run_id.to_owned();
    Ok(result)
}

async fn ws_list_events(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    params: WsRunEvents,
) -> Result<Vec<nomifun_agent_execution::AgentRunEvent>, AppServerError> {
    state.registry.require_ready(connection.connection_id(), &user.id)?;
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new("runtime_unavailable", "App Server runtime is unavailable", StatusCode::SERVICE_UNAVAILABLE, true)
    })?;
    let internal_run_id = resolve_internal_run_id(state, user.id.as_str(), &params.run_id).await?;
    let mut events = runtime
        .list_events(user.id.as_str(), &internal_run_id, params.after_sequence, params.limit)
        .await
        .map_err(AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    for event in &mut events {
        event.run_id = params.run_id.clone();
    }
    Ok(events)
}

async fn ws_cancel_run(
    state: &AppServerRouterState,
    connection: &ConnectionState,
    user: &CurrentUser,
    params: WsCancelRun,
) -> Result<AgentRunView, AppServerError> {
    let (principal_id, client_id) = state
        .registry
        .ready_idempotency_context(connection.connection_id(), &user.id)?;
    execute_cancel_run(
        state,
        user,
        &principal_id,
        &client_id,
        &params.run_id,
        CancelRunRequest {
            expected_version: params.expected_version,
            command_id: params.command_id,
            idempotency_key: params.idempotency_key,
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

fn ws_response(id: Option<serde_json::Value>, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn ws_notification(method: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

fn ws_error(id: Option<serde_json::Value>, error: AppServerError) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.clone(),
        "error": error.into_wire_error(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use nomifun_api_types::WebSocketMessage;
    use nomifun_realtime::UserEventSink;
    use tower::ServiceExt;

    fn request() -> InitializeRequest {
        InitializeRequest {
            protocol_version: PROTOCOL_VERSION.into(),
            client: ClientInfo {
                name: "test-client".into(),
                version: "0.1.0".into(),
            },
            auth: None,
            capabilities: ClientCapabilities::default(),
        }
    }

    #[test]
    fn initialize_binds_transport_principal_and_never_accepts_request_identity() {
        let principal = LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::Stdio);
        let expected_principal_id = principal.principal_id().to_owned();
        let mut state = ConnectionState::new(principal, true);
        let result = state.initialize(request()).unwrap();
        assert_eq!(result.auth_context.principal_id, expected_principal_id);
        assert_eq!(state.phase(), ConnectionPhase::AwaitingInitialized);
    }

    #[test]
    fn business_calls_require_initialized_notification() {
        let mut state = ConnectionState::new(LocalPrincipal::from_authenticated_user(
            UserId::new(),
            LocalTransport::WebSocket,
        ), true);
        state.initialize(request()).unwrap();
        assert!(matches!(
            state.require_ready(),
            Err(ProtocolError::NotInitialized)
        ));
        state.mark_initialized().unwrap();
        assert!(state.require_ready().is_ok());
    }

    #[test]
    fn unknown_principal_field_is_rejected_by_the_wire_contract() {
        let value = serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "client": {"name":"client","version":"1"},
            "principal_id": "forged"
        });
        assert!(serde_json::from_value::<InitializeRequest>(value).is_err());
    }

    #[test]
    fn app_server_preset_source_policy_covers_builtin_and_installed_agent_store() {
        assert!(validate_agent_store_preset_source(PresetSource::Builtin, Some("builtin-office"), Some("Office")).is_ok());
        // Installed agent-store presets are user presets named `agent-store: <name>`.
        assert!(validate_agent_store_preset_source(PresetSource::User, None, Some("agent-store: software-engineer")).is_ok());
        // Arbitrary user presets stay outside the compatibility surface.
        assert!(validate_agent_store_preset_source(PresetSource::User, None, Some("my-personal-preset")).is_err());
        assert!(validate_agent_store_preset_source(PresetSource::User, Some("builtin-office"), Some("Office")).is_err());
        assert!(validate_agent_store_preset_source(PresetSource::Extension, Some("builtin-office"), Some("Office")).is_err());
        assert!(validate_agent_store_preset_source(PresetSource::Builtin, Some("other-preset"), Some("Other")).is_err());
    }

    #[test]
    fn app_server_only_allows_nomi_runtime_presets() {
        assert!(validate_nomi_runtime_type(Some("nomi")).is_ok());
        assert!(validate_nomi_runtime_type(Some("acp")).is_err());
        assert!(validate_nomi_runtime_type(None).is_err());
    }

    #[test]
    fn initialize_exposes_only_single_agent_runtime_capabilities() {
        let mut state = ConnectionState::new(
            LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::Http),
            true,
        );
        let result = state.initialize(request()).unwrap();
        assert!(result.capabilities.agents);
        assert!(!result.capabilities.teams);
        assert!(!result.capabilities.team_runtime);
        assert!(!result.capabilities.skills);
        assert!(!result.capabilities.connectors);
        assert!(!result.capabilities.run_notifications);
        // Approvals are derived from the runtime (same seam as the answer path).
        assert!(result.capabilities.approvals);
        assert!(!result.capabilities.artifacts);
        assert!(!result.capabilities.oauth);
    }

    #[test]
    fn approvals_capability_follows_the_runtime_seam() {
        // Only the runtime gates the answer path, so a runtime-less connection
        // must not advertise it...
        let mut without_runtime = ConnectionState::new(
            LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::Http),
            false,
        );
        assert!(!without_runtime.initialize(request()).unwrap().capabilities.approvals);
        let runtime_less_state = AppServerRouterState::default();
        assert!(!CapabilityAvailability::from_state(&runtime_less_state).runtime);

        // ...while every connection that owns one does, on both transports.
        for transport in [LocalTransport::Http, LocalTransport::WebSocket] {
            let mut state = ConnectionState::new(
                LocalPrincipal::from_authenticated_user(UserId::new(), transport),
                true,
            );
            assert!(
                state.initialize(request()).unwrap().capabilities.approvals,
                "approvals must follow the runtime on {transport:?}"
            );
        }
    }

    /// The `run/answer-decision` params are exactly the engine's answer contract:
    /// three CAS tokens, no desktop-only switches, nothing optional.
    #[test]
    fn answer_decision_params_are_exactly_the_cas_contract() {
        let canonical = serde_json::json!({
            "run_id": "0190f5fe-7c00-7a00-8000-000000000010",
            "step_id": "0190f5fe-7c00-7a00-8000-000000000011",
            "attempt_id": "0190f5fe-7c00-7a00-8000-000000000012",
            "answer": "approved",
            "expected_execution_version": 4,
            "expected_step_version": 5,
            "expected_attempt_version": 6,
        });
        let parsed: WsAnswerDecision = serde_json::from_value(canonical.clone()).unwrap();
        assert_eq!(parsed.step_id, "0190f5fe-7c00-7a00-8000-000000000011");
        assert_eq!(parsed.expected_execution_version, 4);
        assert_eq!(parsed.expected_step_version, 5);
        assert_eq!(parsed.expected_attempt_version, 6);

        let http_body = |mut body: serde_json::Value| {
            body.as_object_mut().unwrap().remove("run_id");
            body
        };

        // The desktop confirmation route's `always_allow` (and every other
        // approve-all / CAS-skipping switch) is not part of this method. It must
        // fail fast rather than be silently dropped on the floor.
        for flag in ["always_allow", "approve_all", "yolo", "skip_cas", "confirmation_mode"] {
            let mut ws = canonical.clone();
            ws[flag] = serde_json::json!(true);
            assert!(
                serde_json::from_value::<WsAnswerDecision>(ws).is_err(),
                "{flag} must be rejected by the run/answer-decision params"
            );
            let mut http = http_body(canonical.clone());
            http[flag] = serde_json::json!(true);
            assert!(
                serde_json::from_value::<AnswerDecisionRequest>(http).is_err(),
                "{flag} must be rejected by the HTTP binding"
            );
        }

        // All three CAS tokens are mandatory — there is no "answer whatever is
        // current" shortcut.
        for missing in [
            "expected_execution_version",
            "expected_step_version",
            "expected_attempt_version",
        ] {
            let mut ws = canonical.clone();
            ws.as_object_mut().unwrap().remove(missing);
            assert!(
                serde_json::from_value::<WsAnswerDecision>(ws).is_err(),
                "{missing} must be required by run/answer-decision"
            );
        }

        // The HTTP body is the same contract minus the path parameter.
        assert!(serde_json::from_value::<AnswerDecisionRequest>(http_body(canonical)).is_ok());
    }

    #[tokio::test]
    async fn run_answer_decision_requires_ready_then_a_runtime() {
        let state = AppServerRouterState::default();
        let user = CurrentUser {
            id: UserId::new(),
            username: "operator".into(),
        };
        let connection = state.registry.open(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            false,
        );
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        let params = serde_json::json!({
            "run_id": "run_01",
            "step_id": "0190f5fe-7c00-7a00-8000-000000000011",
            "attempt_id": "0190f5fe-7c00-7a00-8000-000000000012",
            "answer": "approved",
            "expected_execution_version": 1,
            "expected_step_version": 1,
            "expected_attempt_version": 1,
        });
        let request_id = Some(serde_json::json!("req-1"));

        let error = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "run/answer-decision",
            params.clone(),
            request_id.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "not_initialized");

        state
            .registry
            .initialize(connection.connection_id(), request())
            .unwrap();
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .unwrap();
        let error = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "run/answer-decision",
            params.clone(),
            request_id.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.code, "runtime_unavailable",
            "the arm must refuse before resolving a public run id when no runtime backs it"
        );

        let mut with_desktop_flag = params;
        with_desktop_flag["always_allow"] = serde_json::json!(true);
        let error = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "run/answer-decision",
            with_desktop_flag,
            request_id,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "invalid_request");
    }

    #[test]
    fn run_notifications_requires_an_event_source_on_the_connection() {
        let mut state = ConnectionState::with_events(
            LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::WebSocket),
            true,
            true,
        );
        let result = state.initialize(request()).unwrap();
        assert!(result.capabilities.agents);
        assert!(result.capabilities.run_notifications);
        assert!(!result.capabilities.teams);
        assert!(!result.capabilities.skills);
        assert!(!result.capabilities.connectors);
        assert!(result.capabilities.approvals);
    }

    #[test]
    fn agent_run_accepts_public_agent_id_and_input_text() {
        let request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "agent_id": "0190f5fe-7c00-7a00-8000-000000000004",
            "input": {"text": "inspect the repository"},
            "idempotency_key": "client-op-01"
        }))
        .unwrap();
        assert_eq!(request.preset_id, "0190f5fe-7c00-7a00-8000-000000000004");
        assert_eq!(request.normalized_goal().unwrap(), "inspect the repository");
    }

    #[test]
    fn agent_run_wire_accepts_structured_mentions() {
        let request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "agent_id": "0190f5fe-7c00-7a00-8000-000000000004",
            "goal": "summarize the repo",
            "mentions": [
                {"kind": "agent", "id": "wb-demo-software-architect"},
                {"kind": "skill", "id": "wb-demo-release-notes"},
                {"kind": "connector", "id": "0190f5fe-7c00-7a00-8000-000000000020"}
            ]
        }))
        .unwrap();
        assert_eq!(request.mentions.len(), 3);
        assert_eq!(request.mentions[1].kind, MentionKind::Skill);
        assert_eq!(request.mentions[2].id, "0190f5fe-7c00-7a00-8000-000000000020");
        // Omitted mentions default to the empty vec (backward compatible).
        let bare: AgentRunRequest =
            serde_json::from_value(serde_json::json!({"agent_id": "x"})).unwrap();
        assert!(bare.mentions.is_empty());
    }

    #[test]
    fn agent_run_wire_rejects_unknown_mention_kind() {
        assert!(serde_json::from_value::<AgentRunRequest>(serde_json::json!({
            "agent_id": "x",
            "mentions": [{"kind": "team", "id": "wb-t"}]
        }))
        .is_err());
    }

    #[tokio::test]
    async fn apply_mentions_resolves_agent_to_installed_preset() {
        let mut state = AppServerRouterState::default();
        state.agent_catalog = Some(Arc::new(crate::catalog::FakeAgentCatalog {
            agents: vec![nomifun_api_types::AppServerAgentSummary {
                id: "wb-demo-software-architect".into(),
                version: "1.0.0".into(),
                name: "software-architect".into(),
                preset_id: Some("0190f5fe-7c00-7a00-8000-000000000030".into()),
                description: None,
                skills: vec![],
                connectors: vec![],
                model_summary: None,
                tool_policy_summary: None,
                source: "imported".into(),
                compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::CompatibleWithAdapter,
                display_name: None,
                profession: None,
                avatar_url: None,
            }],
        }));
        let mut preset_id = String::new();
        let mut overrides = PresetOverrides::default();
        apply_mentions(
            &state,
            &mut preset_id,
            &mut overrides,
            &[
                MentionRef { kind: MentionKind::Agent, id: "wb-demo-software-architect".into() },
                MentionRef { kind: MentionKind::Skill, id: "wb-demo-release-notes".into() },
                MentionRef { kind: MentionKind::Connector, id: "0190f5fe-7c00-7a00-8000-000000000020".into() },
            ],
        )
        .await
        .unwrap();
        assert_eq!(preset_id, "0190f5fe-7c00-7a00-8000-000000000030");
        assert_eq!(overrides.include_skills, vec!["wb-demo-release-notes"]);
        assert_eq!(
            overrides.mcp_server_ids,
            Some(vec!["0190f5fe-7c00-7a00-8000-000000000020".into()])
        );
    }

    #[test]
    fn default_model_fallback_keeps_mention_overrides() {
        // WP-2 B5: the owner-default model fallback re-resolves the preset,
        // and that second resolve must not drop the mention overrides.
        let base = PresetOverrides {
            include_skills: vec!["legacy:hello".into()],
            mcp_server_ids: Some(vec!["0190f5fe-7c00-7a00-8000-000000000020".into()]),
            ..Default::default()
        };
        let model = nomifun_api_types::ModelPreference {
            provider_id: Some("0190f5fe-7c00-7a00-8000-000000000001".into()),
            model: "mimo-v2.5".into(),
            required: true,
        };
        let merged = with_default_model(base, &model);
        assert_eq!(merged.model.as_deref(), Some("mimo-v2.5"));
        assert_eq!(
            merged.provider_id.as_deref(),
            Some("0190f5fe-7c00-7a00-8000-000000000001")
        );
        assert_eq!(merged.include_skills, vec!["legacy:hello"]);
        assert_eq!(
            merged.mcp_server_ids,
            Some(vec!["0190f5fe-7c00-7a00-8000-000000000020".into()])
        );
    }

    #[tokio::test]
    async fn apply_mentions_rejects_uninstalled_agent() {        let mut state = AppServerRouterState::default();
        state.agent_catalog = Some(Arc::new(crate::catalog::FakeAgentCatalog {
            agents: vec![nomifun_api_types::AppServerAgentSummary {
                id: "wb-demo-uninstalled".into(),
                version: "1.0.0".into(),
                name: "uninstalled".into(),
                preset_id: None,
                description: None,
                skills: vec![],
                connectors: vec![],
                model_summary: None,
                tool_policy_summary: None,
                source: "imported".into(),
                compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::CompatibleWithAdapter,
                display_name: None,
                profession: None,
                avatar_url: None,
            }],
        }));
        let mut preset_id = String::new();
        let mut overrides = PresetOverrides::default();
        let error = apply_mentions(
            &state,
            &mut preset_id,
            &mut overrides,
            &[MentionRef { kind: MentionKind::Agent, id: "wb-demo-uninstalled".into() }],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "agent_not_installed");
    }

    #[test]
    fn snapshot_asset_path_validation_rejects_traversal_and_non_whitelist() {
        // Bare UUID-like opaque ids pass; anything else (traversal candidates)
        // is rejected before touching the filesystem.
        assert!(snapshot_dir_name("0190f5fe-7c00-7a00-8000-000000000004").is_some());
        assert!(snapshot_dir_name("../etc/passwd").is_none());
        assert!(snapshot_dir_name("foo").is_none());
        assert!(snapshot_dir_name("0190f5fe-7c00-7a00-8000-000000000004/../../../").is_none());
    }

    #[test]
    fn idempotency_returns_same_receipt_and_rejects_different_fingerprint() {
        let registry = AppServerRegistry::default();
        let receipt = AgentRunReceipt {
            run_id: "0190f5fe-7c00-7a00-8000-000000000004".into(),
            status: nomifun_agent_execution::AgentRunStatus::Planning,
            version: 1,
            preset_revision: 2,
            content_digest: "sha256:test".into(),
        };
        assert_eq!(
            registry
                .remember_idempotent_run("scope", "fingerprint".into(), receipt.clone())
                .unwrap()
                .run_id,
            receipt.run_id
        );
        assert_eq!(
            registry
                .existing_idempotent_run("scope", "fingerprint")
                .unwrap()
                .unwrap()
                .run_id,
            receipt.run_id
        );
        assert!(matches!(
            registry.existing_idempotent_run("scope", "other"),
            Err(ProtocolError::IdempotencyConflict)
        ));
    }

    #[test]
    fn initialize_accepts_protocol_auth_metadata_without_using_it_as_identity() {
        let request: InitializeRequest = serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "client": {"name": "cli", "version": "1"},
            "auth": {"mode": "local-session", "credential": "opaque-session-reference"},
            "capabilities": {"events": true}
        }))
        .unwrap();
        assert_eq!(request.auth.as_ref().and_then(|auth| auth.mode.as_deref()), Some("local-session"));
        assert!(request.capabilities.events);
    }

    #[test]
    fn websocket_request_rejects_non_jsonrpc_v2() {
        let request: WsRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "1.0",
            "id": "req-1",
            "method": "ping",
            "params": {}
        }))
        .unwrap();
        assert_ne!(request.jsonrpc, "2.0");
    }

    #[test]
    fn websocket_error_uses_the_public_error_contract() {
        let response = ws_error(
            Some(serde_json::json!("req-1")),
            AppServerError::from(ProtocolError::NotInitialized),
        );
        assert_eq!(response["error"]["code"], "not_initialized");
        assert_eq!(response["error"]["retryable"], false);
        assert_eq!(response["error"]["request_id"], "req-1");
        assert_eq!(response["id"], "req-1");
    }

    #[test]
    fn a_different_authenticated_user_cannot_advance_the_connection() {
        let registry = AppServerRegistry::default();
        let owner = UserId::new();
        let other = UserId::new();
        let connection = registry.open(
            LocalPrincipal::from_authenticated_user(owner, LocalTransport::Http),
            true,
        );
        registry.initialize(connection.connection_id(), request()).unwrap();
        assert!(matches!(
            registry.mark_initialized(connection.connection_id(), &other),
            Err(ProtocolError::PrincipalMismatch)
        ));
    }

    #[tokio::test]
    async fn http_business_call_without_connection_returns_structured_protocol_error() {
        let router = app_server_routes(AppServerRouterState::default()).layer(
            axum::Extension(CurrentUser {
                id: UserId::new(),
                username: "test-user".into(),
            }),
        );
        let response = router
            .oneshot(
                axum::http::Request::post("/api/app-server/initialized")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["code"], "invalid_request");
        assert_eq!(payload["retryable"], false);
        assert!(payload.get("request_id").is_some());
    }

    #[tokio::test]
    async fn websocket_dispatch_requires_initialized_before_agent_methods() {
        let state = AppServerRouterState::default();
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let connection = state.registry.open(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            false,
        );
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        let error = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "agent/run",
            serde_json::json!({}),
            Some(serde_json::json!("req-1")),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "not_initialized");
    }

    #[tokio::test]
    async fn http_agent_run_reports_runtime_unavailable_after_handshake() {
        let router = app_server_routes(AppServerRouterState::default()).layer(
            axum::Extension(CurrentUser {
                id: UserId::new(),
                username: "test-user".into(),
            }),
        );
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::post("/api/app-server/initialize")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "protocol_version": PROTOCOL_VERSION,
                            "client": {"name":"test-client","version":"0.1.0"},
                            "capabilities": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let connection_id = response
            .headers()
            .get(CONNECTION_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("initialize returns its connection id")
            .to_owned();

        let response = router
            .clone()
            .oneshot(
                axum::http::Request::post("/api/app-server/initialized")
                    .header(CONNECTION_HEADER, &connection_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .oneshot(
                axum::http::Request::post("/api/app-server/agent/run")
                    .header(CONNECTION_HEADER, &connection_id)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "agent_id": "0190f5fe-7c00-7a00-8000-000000000004",
                            "input": {"text": "inspect the repository"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["code"], "runtime_unavailable");
        assert_eq!(payload["retryable"], true);
    }

    #[tokio::test]
    async fn http_handshake_returns_connection_id_and_requires_same_authenticated_user() {
        let router = app_server_routes(AppServerRouterState::default()).layer(
            axum::Extension(CurrentUser {
                id: UserId::new(),
                username: "test-user".into(),
            }),
        );
        let response = router
            .oneshot(
                axum::http::Request::post("/api/app-server/initialize")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "protocol_version": PROTOCOL_VERSION,
                            "client": {"name":"test-client","version":"0.1.0"},
                            "capabilities": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let connection_id = response
            .headers()
            .get(CONNECTION_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("initialize returns its connection id")
            .to_owned();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["auth_context"]["issuer"], "local-agent-store");
        assert_eq!(payload["auth_context"]["audience"], "agent-store");
        assert_eq!(payload["server"]["name"], "allo-agent-store");
        assert_eq!(payload["connection_id"], connection_id);
    }

    #[tokio::test]
    async fn event_forwarding_drops_foreign_owners_and_unsubscribed_runs() {
        let bus = BroadcastEventBus::new(16);
        let owner = UserId::new();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let task = tokio::spawn(forward_app_server_events(
            bus.subscribe_user(),
            AppServerRouterState::default(),
            owner.clone(),
            subscriptions,
            gate,
            tx,
        ));

        // A different owner's change must never cross this connection.
        bus.send_to_user(
            "other-owner",
            WebSocketMessage::new(
                "agentExecution.changed",
                serde_json::json!({"execution_id": "0190f5fe-7c00-7a00-8000-000000000001", "sequence": 1}),
            ),
        );
        // The owner is not subscribed to any public run yet.
        bus.send_to_user(
            owner.as_str(),
            WebSocketMessage::new(
                "agentExecution.changed",
                serde_json::json!({"execution_id": "0190f5fe-7c00-7a00-8000-000000000001", "sequence": 1}),
            ),
        );

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            rx.try_recv().is_err(),
            "no notification may cross the bridge for foreign or unsubscribed runs"
        );
        task.abort();
    }

    #[tokio::test]
    async fn event_forwarding_lag_reports_resync_for_subscribed_runs() {
        let bus = BroadcastEventBus::new(4);
        let owner = UserId::new();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            runs: HashSet::from(["0190f5fe-7c00-7a00-8000-000000000009".to_owned()]),
            ..Default::default()
        }));
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let task = tokio::spawn(forward_app_server_events(
            bus.subscribe_user(),
            AppServerRouterState::default(),
            owner.clone(),
            subscriptions,
            gate,
            tx,
        ));

        for sequence in 0..32 {
            bus.send_to_user(
                owner.as_str(),
                WebSocketMessage::new(
                    "agentExecution.changed",
                    serde_json::json!({"execution_id": "0190f5fe-7c00-7a00-8000-000000000001", "sequence": sequence}),
                ),
            );
        }

        let message = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("lag notification must arrive")
            .expect("channel stays open");
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "run/resync-required");
        assert_eq!(value["params"]["reason"], "event_stream_lagged");
        assert_eq!(
            value["params"]["run_ids"][0],
            "0190f5fe-7c00-7a00-8000-000000000009"
        );
        task.abort();
    }

    #[tokio::test]
    async fn conversation_event_forwarding_is_owner_scoped_and_sanitizes_payloads() {
        let bus = BroadcastEventBus::new(16);
        let owner = UserId::new();
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let task = tokio::spawn(forward_app_server_events(
            bus.subscribe_user(),
            AppServerRouterState::default(),
            owner.clone(),
            subscriptions,
            gate,
            tx,
        ));

        // A foreign owner and a non-subscribed conversation are both invisible.
        bus.send_to_user(
            "other-owner",
            WebSocketMessage::new("message.stream", serde_json::json!({
                "conversation_id": conversation_id,
                "msg_id": "foreign-message",
                "type": "content",
                "data": {"content": "must not arrive"}
            })),
        );
        bus.send_to_user(
            owner.as_str(),
            WebSocketMessage::new("message.stream", serde_json::json!({
                "conversation_id": "0190f5fe-7c00-7a00-8000-000000000010",
                "msg_id": "other-message",
                "type": "content",
                "data": {"content": "must not arrive"}
            })),
        );
        bus.send_to_user(
            owner.as_str(),
            WebSocketMessage::new("message.stream", serde_json::json!({
                "conversation_id": conversation_id,
                "msg_id": "0190f5fe-7c00-7a00-8000-000000000011",
                "type": "content",
                "data": {"content": "safe"},
                "turn_id": "internal-turn-must-not-leak",
                "session_id": "internal-session-must-not-leak"
            })),
        );

        let message = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("subscribed conversation event must arrive")
            .expect("channel stays open");
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["method"], "conversation/event");
        assert_eq!(value["params"]["conversation_id"], conversation_id);
        assert_eq!(value["params"]["sequence"], 1);
        assert_eq!(value["params"]["event_type"], "message.delta");
        assert_eq!(value["params"]["payload"]["content"], "safe");
        assert!(value["params"].get("turn_id").is_none());
        assert!(value["params"].get("session_id").is_none());
        task.abort();
    }

    #[test]
    fn conversation_thinking_projection_preserves_public_stream_fields() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "thinking",
            "data": {
                "content": "检查项目结构",
                "subject": "正在分析",
                "status": "thinking",
                "duration": null,
            },
            "replace": false,
            "turn_id": "internal-turn",
            "session_id": "internal-session",
        }));

        let notification = project_conversation_notification(&event, &subscriptions).expect("thinking is public");
        assert_eq!(notification["params"]["event_type"], "message.thinking");
        assert_eq!(notification["params"]["payload"]["message_id"], "0190f5fe-7c00-7a00-0000-000000000011");
        assert_eq!(notification["params"]["payload"]["content"], "检查项目结构");
        assert_eq!(notification["params"]["payload"]["subject"], "正在分析");
        assert_eq!(notification["params"]["payload"]["status"], "thinking");
        assert_eq!(notification["params"]["payload"]["replace"], false);
        assert!(notification["params"].get("turn_id").is_none());
        assert!(notification["params"].get("session_id").is_none());

        let done_event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "thinking",
            "data": { "content": "", "status": "done", "duration": 842 },
            "replace": false,
        }));
        let done = project_conversation_notification(&done_event, &subscriptions).expect("thinking completion is public");
        assert_eq!(done["params"]["sequence"], 2);
        assert_eq!(done["params"]["payload"]["status"], "done");
        assert_eq!(done["params"]["payload"]["duration"], 842);
    }

    #[tokio::test]
    async fn event_forwarding_never_leaks_internal_ids_without_public_mapping() {
        let bus = BroadcastEventBus::new(16);
        let owner = UserId::new();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            runs: HashSet::from(["0190f5fe-7c00-7a00-8000-000000000009".to_owned()]),
            ..Default::default()
        }));
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let task = tokio::spawn(forward_app_server_events(
            bus.subscribe_user(),
            AppServerRouterState::default(),
            owner.clone(),
            subscriptions,
            gate,
            tx,
        ));

        bus.send_to_user(
            owner.as_str(),
            WebSocketMessage::new(
                "agentExecution.changed",
                serde_json::json!({"execution_id": "0190f5fe-7c00-7a00-8000-000000000001", "sequence": 1}),
            ),
        );

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            rx.try_recv().is_err(),
            "an unmapped internal execution must stay off the public wire"
        );
        task.abort();
    }

    const AGENT_STORE_TEST_CONFIG: &str = r#"
default_model = "opencode/mimo-v2.5-free"

[providers.opencode]
type = "openai"
api_key = "sk-test-not-a-real-key"
base_url = "https://opencode.ai/zen/v1"

[models."opencode/laguna-s-2.1-free"]
provider = "opencode"
model = "laguna-s-2.1-free"
max_context_size = 256000
display_name = "Laguna S 2.1 Free"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
max_context_size = 200000
display_name = "MiMo V2.5 Free"
"#;

    #[tokio::test]
    async fn app_server_chat_resolves_and_registers_agent_store_providers() {
        let db = nomifun_db::init_database_memory_with_owner(UserId::new())
            .await
            .expect("in-memory db");
        let pool = db.pool().clone();
        let service = ProviderService::new(
            Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
            Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
            [7u8; 32],
        );

        let dir = std::env::temp_dir().join(format!("allo-ab-test-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp config dir");
        let path = dir.join("config.toml");
        std::fs::write(&path, AGENT_STORE_TEST_CONFIG).expect("temp config file");
        let state = AppServerRouterState {
            provider_service: Some(Arc::new(service)),
            agent_store_config_path: Some(path.clone()),
            ..Default::default()
        };

        // 1. No explicit model → default_model is resolved, provider registered.
        let resolved = resolve_app_server_model(&state, None).await.expect("default resolution");
        assert!(
            nomifun_common::validate_uuidv7(&resolved.provider_id).is_ok(),
            "resolved provider must be the canonical registered UUID"
        );
        assert_eq!(resolved.model, "mimo-v2.5-free");

        // 2. Idempotent: a second resolution reuses the same provider row.
        let again = resolve_app_server_model(&state, None).await.expect("repeat resolution");
        assert_eq!(again.provider_id, resolved.provider_id);

        // 3. Registration carries models + context limits into provider_models.
        let providers = state
            .provider_service
            .as_ref()
            .expect("provider service")
            .list()
            .await
            .expect("provider list");
        assert_eq!(providers.len(), 1, "one provider row per agent-store key");
        let provider = &providers[0];
        assert_eq!(provider.name, "opencode");
        assert_eq!(provider.base_url, "https://opencode.ai/zen/v1");
        assert!(provider.enabled);
        assert!(provider.models.contains(&"mimo-v2.5-free".to_owned()));
        assert_eq!(
            provider
                .model_context_limits
                .as_ref()
                .and_then(|limits| limits.get("mimo-v2.5-free")),
            Some(&200_000)
        );
        assert_eq!(
            provider
                .model_descriptions
                .as_ref()
                .and_then(|names| names.get("mimo-v2.5-free").map(String::as_str)),
            Some("MiMo V2.5 Free")
        );

        // 4. An explicit config-key selection registers nothing new and rewrites
        //    the selection to the same provider UUID.
        let keyed = resolve_app_server_model(
            &state,
            Some(ProviderWithModel {
                provider_id: "opencode".to_owned(),
                model: "laguna-s-2.1-free".to_owned(),
                use_model: None,
            }),
        )
        .await
        .expect("config-key resolution");
        assert_eq!(keyed.provider_id, resolved.provider_id);
        assert_eq!(keyed.model, "laguna-s-2.1-free");

        // 5. An unknown provider with no config entry fails with a stable code.
        let error = resolve_app_server_model(
            &state,
            Some(ProviderWithModel {
                provider_id: "0190f5fe-7c00-7a00-8000-000000000099".to_owned(),
                model: "unknown-model".to_owned(),
                use_model: None,
            }),
        )
        .await
        .expect_err("unknown provider must fail");
        assert_eq!(error.code, "provider_not_found");

        // 6. Missing config file keeps the plain "provider not found" surface.
        let missing = AppServerRouterState {
            agent_store_config_path: Some(dir.join("absent.toml")),
            ..state.clone()
        };
        let error = resolve_app_server_model(&missing, None)
            .await
            .expect_err("missing config must fail");
        assert_eq!(error.code, "invalid_request");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conversation_tool_projection_carries_args_and_output_but_hides_ids() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "tool_call",
            "data": {
                "call_id": "nomi-call_internal",
                "name": "Read",
                "args": {"file_path": "C:\\internal\\MEMORY.md"},
                "input": {"file_path": "C:\\internal\\MEMORY.md"},
                "output": "internal output",
                "status": "completed",
            },
            "turn_id": "internal-turn",
        }));

        let notification = project_conversation_notification(&event, &subscriptions).expect("tool_call is public");
        assert_eq!(notification["params"]["event_type"], "message.tool");
        assert_eq!(notification["params"]["payload"]["name"], "Read");
        assert_eq!(notification["params"]["payload"]["status"], "completed");
        // The live row renders exactly what a reloaded row renders — no
        // "arguments appear only after a reload" second look.
        assert_eq!(
            notification["params"]["payload"]["args"]["file_path"],
            "C:\\internal\\MEMORY.md"
        );
        assert_eq!(notification["params"]["payload"]["output"], "internal output");
        // Opaque runtime identifiers stay behind the module seam.
        assert!(notification["params"]["payload"].get("input").is_none());
        assert!(notification["params"]["payload"].get("call_id").is_none());
        assert!(notification["params"].get("turn_id").is_none());
    }

    /// W9（R14）：本轮 token 用量随 `turn_completed` 到达客户端。引擎早就在事件里
    /// 给了逐轮数字，此前被投影丢掉；这里钉住「原样带上、其余运行时指标仍留在
    /// seam 后面」，因为客户端就是靠这三个数（配目录费率）算「本轮花了多少」。
    #[test]
    fn conversation_turn_completed_projection_carries_per_turn_usage() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "turn_completed",
            "data": {
                "elapsed_ms": 15_178,
                "input_tokens": 1_200,
                "output_tokens": 340,
                "cache_creation_tokens": 120,
                "cache_read_tokens": 800,
                "context_tokens": 8_000,
                "context_window": 100_000,
                "stop_reason": "end_turn",
                "context_breakdown": { "conversation": 7_500 },
                "moa": { "slots": [] },
            },
            "hidden": false,
        }));

        let notification = project_conversation_notification(&event, &subscriptions)
            .expect("turn_completed stays public (R31 wrap-up marker)");
        assert_eq!(notification["params"]["event_type"], "message.activity");
        assert_eq!(notification["params"]["payload"]["kind"], "turn_completed");
        assert_eq!(
            notification["params"]["payload"]["usage"]["input_tokens"],
            1_200
        );
        assert_eq!(
            notification["params"]["payload"]["usage"]["output_tokens"],
            340
        );
        assert_eq!(
            notification["params"]["payload"]["usage"]["total_tokens"],
            1_540
        );
        // The projection stays a seam: the remaining runtime metrics (cache
        // detail, context gauge, breakdown, MoA slots, stop reason) are not
        // relayed — the public payload carries the accounting, not the frame.
        for field in [
            "cache_creation_tokens",
            "cache_read_tokens",
            "context_tokens",
            "context_window",
            "context_breakdown",
            "stop_reason",
            "moa",
            "elapsed_ms",
        ] {
            assert!(
                notification["params"]["payload"].get(field).is_none(),
                "{field} must stay behind the seam"
            );
        }
    }

    /// 没有可报的用量时**整段缺席**——不是 `usage: {0, 0}`，更不是拿上下文占用
    /// 顶替。客户端据此保持「本轮未知」，永远不会把 0 当成「这一轮不花钱」。
    #[test]
    fn conversation_turn_completed_projection_omits_unreported_usage() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let frame = |data: serde_json::Value| {
            WebSocketMessage::new("message.stream", serde_json::json!({
                "conversation_id": conversation_id,
                "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
                "type": "turn_completed",
                "data": data,
                "hidden": false,
            }))
        };

        // ① 运行时一个 token 都没报（只有占用与耗时）。
        let silent = frame(serde_json::json!({
            "elapsed_ms": 42,
            "input_tokens": 0,
            "output_tokens": 0,
            "context_tokens": 8_000,
            "context_window": 100_000,
        }));
        // ② 只有单侧——半个账单不算账单。
        let partial = frame(serde_json::json!({ "input_tokens": 1_200 }));
        // ③ 极旧的帧，连 `data` 都没有。
        let absent = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "turn_completed",
        }));

        for (label, event) in [("silent", silent), ("partial", partial), ("absent", absent)] {
            let notification = project_conversation_notification(&event, &subscriptions)
                .unwrap_or_else(|| panic!("{label}: turn_completed stays public"));
            assert_eq!(notification["params"]["payload"]["kind"], "turn_completed");
            assert!(
                notification["params"]["payload"].get("usage").is_none(),
                "{label}: unreported usage must stay off the wire"
            );
        }
    }

    #[test]
    fn conversation_tips_projection_carries_only_public_tip_fields() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "tips",
            "data": {
                "content": "This is a display tip",
                "type": "success",
            },
            "turn_id": "internal-turn",
            "session_id": "internal-session",
        }));

        let notification = project_conversation_notification(&event, &subscriptions).expect("tips are public");
        assert_eq!(notification["params"]["event_type"], "message.tips");
        assert_eq!(notification["params"]["payload"]["message_id"], "0190f5fe-7c00-7a00-0000-000000000011");
        assert_eq!(notification["params"]["payload"]["content"], "This is a display tip");
        assert_eq!(notification["params"]["payload"]["tip_type"], "success");
        assert!(notification["params"]["payload"].get("error").is_none());
        assert!(notification["params"].get("turn_id").is_none());
        assert!(notification["params"].get("session_id").is_none());
    }

    #[test]
    fn conversation_error_projection_exposes_only_public_error_fields() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));
        let event = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "error",
            "data": {
                "message": "The model provider rate limited the request",
                "code": "user_llm_provider_rate_limited",
                "retryable": true,
                "incident_id": "internal-incident-must-not-leak",
                "detail": "internal stack detail",
                "workspacePath": "C:\\internal\\path",
            },
            "turn_id": "internal-turn",
        }));

        let notification = project_conversation_notification(&event, &subscriptions).expect("error is public");
        assert_eq!(notification["params"]["event_type"], "message.error");
        assert_eq!(notification["params"]["payload"]["message"], "The model provider rate limited the request");
        assert_eq!(notification["params"]["payload"]["code"], "user_llm_provider_rate_limited");
        assert_eq!(notification["params"]["payload"]["retryable"], true);
        assert!(notification["params"]["payload"].get("incident_id").is_none());
        assert!(notification["params"]["payload"].get("detail").is_none());
        assert!(notification["params"]["payload"].get("workspacePath").is_none());
        assert!(notification["params"].get("turn_id").is_none());
        assert!(notification["params"].get("session_id").is_none());
    }

    #[test]
    fn conversation_model_ref_accepts_config_keys_and_uuid_providers() {
        // Config-key provider names must survive deserialization so the
        // server-side resolution can register them (regression: the internal
        // ProviderWithModel deserializer rejects non-UUID provider_id).
        let config_key: ConversationCreateRequest =
            serde_json::from_value(serde_json::json!({
                "model": { "provider_id": "opencode", "model": "mimo-v2.5-free" },
                "reasoning_effort": "high",
            }))
            .expect("config-key provider must deserialize");
        let model = config_key.model.expect("model present");
        assert_eq!(model.provider_id, "opencode");
        assert_eq!(model.model, "mimo-v2.5-free");
        assert_eq!(config_key.reasoning_effort.as_deref(), Some("high"));

        let uuid: ConversationUpdateRequest =
            serde_json::from_value(serde_json::json!({
                "conversation_id": "0190f5fe-7c00-7a00-8000-000000000009",
                "model": { "provider_id": "0190f5fe-7c00-7a00-8000-000000000011", "model": "laguna-s-2.1-free" },
            }))
            .expect("registered UUID provider must deserialize");
        assert_eq!(
            uuid.model.expect("model present").provider_id,
            "0190f5fe-7c00-7a00-8000-000000000011"
        );

        assert!(
            serde_json::from_value::<ConversationCreateRequest>(serde_json::json!({
                "model": { "provider_id": "opencode", "model": "mimo-v2.5-free", "extra": true },
            }))
            .is_err(),
            "unknown fields stay rejected"
        );
    }

    #[test]
    fn reasoning_effort_validation_uses_the_public_vocabulary() {
        assert_eq!(
            normalize_reasoning_effort(Some("high".to_owned())).unwrap(),
            Some("high".to_owned())
        );
        assert_eq!(
            normalize_reasoning_effort(Some(" low ".to_owned())).unwrap(),
            Some("low".to_owned())
        );
        assert_eq!(normalize_reasoning_effort(None).unwrap(), None);
        assert_eq!(normalize_reasoning_effort(Some("  ".to_owned())).unwrap(), None);
        let error = normalize_reasoning_effort(Some("ultra".to_owned()))
            .expect_err("unknown effort must fail");
        assert_eq!(error.code, "invalid_request");
    }

    #[test]
    fn catalog_model_facts_read_the_cached_registry_and_stay_absent_when_unknown() {
        let client = nomifun_models_dev::ModelsDevClient::new(
            "http://127.0.0.1:1/api.json",
            std::env::temp_dir().join("nomifun-app-server-models-dev-facts-test.json"),
            None,
        );
        client.seed_cache(serde_json::json!({
            "anthropic": {
                "name": "Anthropic",
                "models": {
                    "claude-sonnet-4-5": {
                        "name": "Claude Sonnet 4.5",
                        "attachment": true,
                        "limit": { "context": 200000 },
                        "cost": { "input": 3.0, "output": 15.0 }
                    }
                }
            }
        }));

        let known = catalog_model_facts(&client, "anthropic", "claude-sonnet-4-5");
        assert_eq!(known.cost_input, Some(3.0));
        assert_eq!(known.cost_output, Some(15.0));
        assert_eq!(known.catalog_context_window, Some(200_000));
        assert_eq!(known.supports_vision, Some(true));

        // An unknown model and an unmapped platform (`mimo` is `MergePolicy::Never`)
        // both yield nothing — the UI must never receive a zero standing in for
        // "unknown".
        assert_eq!(
            catalog_model_facts(&client, "anthropic", "does-not-exist"),
            CatalogFacts::default()
        );
        assert_eq!(
            catalog_model_facts(&client, "mimo", "claude-sonnet-4-5"),
            CatalogFacts::default()
        );

        // Wire shape: absent facts stay off the wire (additive, no nulls).
        let projected = ConversationModelOption {
            name: "claude-sonnet-4-5".to_owned(),
            display_name: None,
            context_limit: None,
            cost_input: known.cost_input,
            cost_output: known.cost_output,
            catalog_context_window: known.catalog_context_window,
            supports_vision: known.supports_vision,
        };
        let value = serde_json::to_value(&projected).expect("serialize projected option");
        assert_eq!(value["cost_input"], serde_json::json!(3.0));
        assert_eq!(value["cost_output"], serde_json::json!(15.0));
        assert_eq!(value["catalog_context_window"], serde_json::json!(200_000));
        assert_eq!(value["supports_vision"], serde_json::json!(true));

        let bare = ConversationModelOption {
            name: "mimo-v2.5".to_owned(),
            display_name: None,
            context_limit: None,
            cost_input: None,
            cost_output: None,
            catalog_context_window: None,
            supports_vision: None,
        };
        let value = serde_json::to_value(&bare).expect("serialize bare option");
        assert!(value.get("cost_input").is_none());
        assert!(value.get("cost_output").is_none());
        assert!(value.get("catalog_context_window").is_none());
        assert!(value.get("supports_vision").is_none());
    }

    #[test]
    fn conversation_model_options_project_the_agent_store_catalog() {
        let dir = std::env::temp_dir().join(format!("allo-mo-test-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp config dir");
        let path = dir.join("config.toml");
        std::fs::write(&path, AGENT_STORE_TEST_CONFIG).expect("temp config file");

        let state = AppServerRouterState {
            agent_store_config_path: Some(path.clone()),
            ..Default::default()
        };
        let options = conversation_model_options(&state);
        let default = options.default.expect("default_model must surface");
        assert_eq!(default.provider, "opencode");
        assert_eq!(default.model, "mimo-v2.5-free");
        assert_eq!(options.reasoning_efforts, vec!["low", "medium", "high", "xhigh"]);
        assert_eq!(options.providers.len(), 1);
        let opencode = &options.providers[0];
        assert_eq!(opencode.name, "opencode");
        let names = opencode.models.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["laguna-s-2.1-free", "mimo-v2.5-free"]);
        let mimo = opencode
            .models
            .iter()
            .find(|entry| entry.name == "mimo-v2.5-free")
            .expect("mimo model entry");
        assert_eq!(mimo.display_name.as_deref(), Some("MiMo V2.5 Free"));
        assert_eq!(mimo.context_limit, Some(200_000));

        // A missing config file degrades to an empty catalog (never errors).
        let missing = AppServerRouterState {
            agent_store_config_path: Some(dir.join("absent.toml")),
            ..state.clone()
        };
        let empty = conversation_model_options(&missing);
        assert!(empty.default.is_none());
        assert!(empty.providers.is_empty());
        assert_eq!(empty.reasoning_efforts, vec!["low", "medium", "high", "xhigh"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_display_name_uses_the_basename_and_falls_back_to_root() {
        assert_eq!(workspace_display_name(r"C:\projects\demo"), "demo");
        assert_eq!(workspace_display_name("/home/user/repo"), "repo");
        assert_eq!(workspace_display_name("C:\\"), "C:\\");
        assert_eq!(workspace_display_name("/"), "/");
    }

    #[test]
    fn conversation_workspace_id_accepts_only_trimmed_strings() {
        assert_eq!(
            conversation_workspace_id(&serde_json::json!({ "workspace_id": "0190f5fe-7c00-7a00-8000-000000000099" })),
            Some("0190f5fe-7c00-7a00-8000-000000000099".to_owned())
        );
        assert_eq!(conversation_workspace_id(&serde_json::json!({ "workspace_id": "" })), None);
        assert_eq!(conversation_workspace_id(&serde_json::json!({ "workspace_id": 5 })), None);
        assert_eq!(conversation_workspace_id(&serde_json::json!({})), None);
    }

    #[test]
    fn context_usage_view_clamps_percent_and_marks_unknown_window() {
        let row = |context_tokens: i64, window_tokens: i64| nomifun_db::models::AppServerContextUsageRow {
            id: 0,
            conversation_id: "0190f5fe-7c00-7a00-8000-000000000099".to_owned(),
            context_tokens,
            window_tokens,
            updated_at: 123,
        };
        let normal = context_usage_view(row(100_000, 200_000));
        assert_eq!(normal.percent, Some(50.0));
        assert_eq!(normal.source, "measured");

        // Over-window occupancy clamps instead of exploding the bar.
        let over = context_usage_view(row(500_000, 200_000));
        assert_eq!(over.percent, Some(100.0));

        // A zero window means "unknown", never a fake percentage.
        let unknown = context_usage_view(row(500, 0));
        assert_eq!(unknown.percent, None);
    }

    #[tokio::test]
    async fn websocket_dispatch_rejects_workspace_and_delete_before_ready() {
        let state = AppServerRouterState::default();
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let connection = state.registry.open(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            false,
        );
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        for (method, params) in [
            ("workspace/list", serde_json::json!({})),
            ("workspace/revoke", serde_json::json!({ "workspace_id": "0190f5fe-7c00-7a00-8000-000000000099" })),
            (
                "conversation/delete",
                serde_json::json!({ "conversation_id": "0190f5fe-7c00-7a00-8000-000000000099" }),
            ),
            // Host settings file: the ready gate runs before any file access.
            ("config/get", serde_json::json!({})),
            ("config/set", serde_json::json!({ "default_model": "opencode/mimo-v2.5-free" })),
            // Skill write face: the ready gate runs before any filesystem access.
            ("skill/create", serde_json::json!({ "name": "demo", "description": "d", "body": "b" })),
            (
                "skill/update",
                serde_json::json!({ "skill_id": "demo", "markdown": "---\nname: demo\ndescription: d\n---\n" }),
            ),
            ("skill/delete", serde_json::json!({ "skill_id": "demo" })),
        ] {
            let error = dispatch_connection_request(
                &state,
                &connection,
                &user,
                &subscriptions,
                method,
                params,
                Some(serde_json::json!("req-1")),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "not_initialized", "{method} must require ready");
        }
    }

    // ---- Host settings file: `config/get` · `config/set` --------------------

    /// Hand-edited host config: the write path must leave every line of this
    /// alone except the one key it was asked to change.
    const COMMENTED_CONFIG: &str = r#"# allo host config — do not reformat
default_model = "opencode/mimo-v2.5-free"   # current pick

[providers.opencode]
type = "openai"
api_key = "sk-live-must-never-reach-the-wire"
base_url = "https://opencode.ai/zen/v1"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
"#;

    fn config_temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("allo-config-{label}-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp config dir");
        dir
    }

    /// Ready connection whose host config path points at `path`.
    fn config_dispatch_state(
        path: &std::path::Path,
    ) -> (AppServerRouterState, CurrentUser, ConnectionState, Arc<RwLock<WsSubscriptions>>) {
        let state = AppServerRouterState {
            agent_store_config_path: Some(path.to_path_buf()),
            ..Default::default()
        };
        let user = CurrentUser { id: UserId::new(), username: "test-user".into() };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state
            .registry
            .initialize(connection.connection_id(), request())
            .expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        (state, user, connection, subscriptions)
    }

    /// Dispatch one method and unwrap the JSON-RPC envelope to its `result`.
    async fn dispatch_config(
        state: &AppServerRouterState,
        connection: &ConnectionState,
        user: &CurrentUser,
        subscriptions: &Arc<RwLock<WsSubscriptions>>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppServerError> {
        dispatch_connection_request(
            state,
            connection,
            user,
            subscriptions,
            method,
            params,
            Some(serde_json::json!("req-1")),
        )
        .await
        .map(|response| response["result"].clone())
    }

    #[tokio::test]
    async fn config_get_reads_the_host_file_without_credentials() {
        let dir = config_temp_dir("get");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        let view = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");

        assert_eq!(view["exists"], serde_json::json!(true));
        assert_eq!(view["default_model"], serde_json::json!("opencode/mimo-v2.5-free"));
        assert_eq!(view["providers"][0]["name"], serde_json::json!("opencode"));
        assert_eq!(view["providers"][0]["enabled"], serde_json::json!(true));
        assert_eq!(view["providers"][0]["models"], serde_json::json!(["mimo-v2.5-free"]));

        // The file carries a credential; the wire view must not.
        let encoded = view.to_string();
        assert!(!encoded.contains("sk-live-must-never-reach-the-wire"), "{encoded}");
        assert!(!encoded.contains("api_key"), "{encoded}");
        assert!(!encoded.contains("base_url"), "{encoded}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_get_on_a_missing_file_answers_defaults_without_error() {
        let dir = config_temp_dir("missing");
        let path = dir.join("absent.toml");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        let view = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("a missing config file is not an error");
        assert_eq!(view["exists"], serde_json::json!(false));
        assert!(view["default_model"].is_null(), "absent must be an explicit null");
        assert_eq!(view["providers"], serde_json::json!([]));

        // A default the runtime could not resolve is refused, not written.
        let error = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "default_model": "opencode/fresh" }),
        )
        .await
        .expect_err("no [providers.opencode] table to resolve against");
        assert_eq!(error.code, "invalid_request");
        assert!(!path.exists(), "a refused write must not create the file");

        // Declaring the provider is what makes a default resolvable; the write
        // then creates the file and answers with the file re-read.
        std::fs::write(&path, "[providers.opencode]\ntype = \"openai\"\n").expect("temp config");
        let view = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "default_model": "opencode/fresh" }),
        )
        .await
        .expect("config/set");
        assert_eq!(view["exists"], serde_json::json!(true));
        assert_eq!(view["default_model"], serde_json::json!("opencode/fresh"));
        assert_eq!(view["providers"][0]["models"], serde_json::json!([]));
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert!(on_disk.contains("default_model = \"opencode/fresh\""), "{on_disk}");
        assert!(on_disk.contains("[providers.opencode]"), "{on_disk}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_toggles_the_memory_distill_switch_and_reads_it_back() {
        let dir = config_temp_dir("memory");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        // A file without `[memory]` reports "not configured" — not `false`.
        let before = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert!(before["memory"].is_null(), "absent table must be null: {before}");

        let view = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "memory": { "distill_enabled": false } }),
        )
        .await
        .expect("config/set");
        assert_eq!(view["memory"]["distill_enabled"], serde_json::json!(false));

        // The value really landed in the host file the launcher reads.
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert!(on_disk.contains("[memory]"), "{on_disk}");
        assert!(on_disk.contains("distill_enabled = false"), "{on_disk}");
        // ...and everything else survived.
        assert!(on_disk.contains("# allo host config — do not reformat"), "{on_disk}");
        // Derived from the fixture rather than typed out: the credential line is
        // asserted byte-for-byte without a secret ever entering this file.
        let credential_line = COMMENTED_CONFIG
            .lines()
            .find(|line| line.starts_with("api_key"))
            .expect("fixture carries a credential line");
        assert!(on_disk.contains(credential_line), "{on_disk}");
        assert!(on_disk.contains("[models.\"opencode/mimo-v2.5-free\"]"), "{on_disk}");
        assert_eq!(on_disk.matches("distill_enabled").count(), 1, "{on_disk}");
        assert!(!dir.join("config.toml.tmp").exists(), "temp file must not survive");

        // Re-read agrees, and flipping it back is a second minimal write.
        let again = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert_eq!(again, view);

        let flipped = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "memory": { "distill_enabled": true } }),
        )
        .await
        .expect("config/set");
        assert_eq!(flipped["memory"]["distill_enabled"], serde_json::json!(true));
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert_eq!(on_disk.matches("distill_enabled").count(), 1, "{on_disk}");
        assert!(on_disk.contains("distill_enabled = true"), "{on_disk}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_takes_no_credential_fields_in_the_memory_patch() {
        let dir = config_temp_dir("memory-credentials");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        for body in [
            // Credential-shaped fields have no slot inside `[memory]` either.
            serde_json::json!({ "memory": { "api_key": "«redacted:sk-…»" } }),
            serde_json::json!({ "memory": { "env": { "TOKEN": "«redacted»" } } }),
            serde_json::json!({ "memory": { "distill_enabled": false, "api_key": "«redacted:sk-…»" } }),
            serde_json::json!({ "memory": { "distill_enabled": "false" } }),
            serde_json::json!({ "memory": {} }),
            serde_json::json!({ "memory": null }),
        ] {
            let error = dispatch_config(&state, &connection, &user, &subscriptions, "config/set", body.clone())
                .await
                .expect_err("must be refused");
            assert_eq!(error.code, "invalid_request", "{body}");
        }

        // Nothing was written: a refused patch never creates `[memory]`.
        assert_eq!(std::fs::read_to_string(&path).expect("config on disk"), COMMENTED_CONFIG);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_rewrites_only_the_target_key_and_preserves_comments() {
        let dir = config_temp_dir("set");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        let view = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "default_model": "opencode/other-model" }),
        )
        .await
        .expect("config/set");
        assert_eq!(view["default_model"], serde_json::json!("opencode/other-model"));

        // On disk: only the target key moved. Comments, credentials, layout and
        // the other tables are untouched, and no temp file is left behind.
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert_eq!(on_disk.matches("default_model").count(), 1, "{on_disk}");
        assert!(on_disk.contains("# allo host config — do not reformat"), "{on_disk}");
        assert!(on_disk.contains("# current pick"), "{on_disk}");
        assert!(on_disk.contains("api_key = \"sk-live-must-never-reach-the-wire\""), "{on_disk}");
        assert!(on_disk.contains("base_url = \"https://opencode.ai/zen/v1\""), "{on_disk}");
        assert!(on_disk.contains("[models.\"opencode/mimo-v2.5-free\"]"), "{on_disk}");
        assert_eq!(on_disk.lines().count(), COMMENTED_CONFIG.lines().count(), "{on_disk}");
        assert!(!dir.join("config.toml.tmp").exists(), "temp file must not survive");

        // A second read agrees with the answer the write handed back.
        let again = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert_eq!(again, view);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_refuses_out_of_envelope_fields_and_values() {
        let dir = config_temp_dir("refuse");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        for body in [
            // Credentials / path / owner / endpoint have no slot in the patch.
            serde_json::json!({ "api_key": "sk-live-write" }),
            serde_json::json!({ "base_url": "http://evil.example" }),
            serde_json::json!({ "path": "C:/elsewhere/config.toml" }),
            serde_json::json!({ "owner": "someone-else" }),
            serde_json::json!({ "default_model": "opencode/mimo-v2.5-free", "api_key": "sk-live-write" }),
            // Values the runtime could not resolve.
            serde_json::json!({ "default_model": "   " }),
            serde_json::json!({ "default_model": "opencode" }),
            serde_json::json!({ "default_model": "/mimo-v2.5-free" }),
            serde_json::json!({ "default_model": "opencode/" }),
            serde_json::json!({ "default_model": "ghost/model" }),
            serde_json::json!({ "default_model": "opencode/line\nbreak" }),
            // Nothing to write at all.
            serde_json::json!({}),
        ] {
            let error = dispatch_config(&state, &connection, &user, &subscriptions, "config/set", body.clone())
                .await
                .expect_err("must be refused");
            assert_eq!(error.code, "invalid_request", "{body}");
        }

        // The refusals left the file byte-identical.
        assert_eq!(std::fs::read_to_string(&path).expect("config on disk"), COMMENTED_CONFIG);
        assert!(!dir.join("config.toml.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_methods_are_owner_scoped_and_take_no_path() {
        let dir = config_temp_dir("scope");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        // A foreign owner's request can never reach this host's file: the same
        // `require_ready` gate every other method goes through, and there is no
        // owner parameter that could widen it.
        let foreign = CurrentUser { id: UserId::new(), username: "someone-else".into() };
        for (method, params) in [
            ("config/get", serde_json::json!({})),
            ("config/set", serde_json::json!({ "default_model": "opencode/mimo-v2.5-free" })),
        ] {
            let error = dispatch_config(&state, &connection, &foreign, &subscriptions, method, params)
                .await
                .expect_err("foreign owner must be refused");
            assert_eq!(error.code, "policy_denied", "{method}");
        }

        // A connection this registry never issued is `not_found`, not a file
        // read: the config face adds no new reachable surface.
        let unknown = ConnectionState::new(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            true,
        );
        let error = dispatch_config(&state, &unknown, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect_err("unknown connection must be refused");
        assert_eq!(error.code, "not_found");

        // `config/get` takes no parameters: a path is a hard rejection.
        let error = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/get",
            serde_json::json!({ "path": "C:/elsewhere/config.toml" }),
        )
        .await
        .expect_err("config/get must not accept a path");
        assert_eq!(error.code, "invalid_request");

        // And the host file is still exactly as it was.
        assert_eq!(std::fs::read_to_string(&path).expect("config on disk"), COMMENTED_CONFIG);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn initialize_exposes_skill_connector_and_oauth_capabilities_when_provided() {
        let mut state = ConnectionState::with_capabilities(
            LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::WebSocket),
            CapabilityAvailability {
                runtime: true,
                events: true,
                skills: true,
                connectors: true,
                oauth: true,
                ..Default::default()
            },
        );
        let result = state.initialize(request()).unwrap();
        assert!(result.capabilities.agents);
        assert!(result.capabilities.skills);
        assert!(result.capabilities.connectors);
        assert!(result.capabilities.oauth);
        assert!(result.capabilities.run_notifications);
        assert!(!result.capabilities.teams);
        assert!(result.capabilities.approvals);
        assert!(!result.capabilities.artifacts);
    }

    fn sample_skill_summary() -> nomifun_api_types::AppServerSkillSummary {
        nomifun_api_types::AppServerSkillSummary {
            id: "demo".into(),
            name: "demo".into(),
            description: Some("a demo skill".into()),
            version: "builtin".into(),
            source: "builtin".into(),
            origin: "builtin".into(),
            writable: false,
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
            enabled: true,
            required_connectors: vec![],
        }
    }

    fn sample_connector_summary() -> nomifun_api_types::AppServerConnectorSummary {
        nomifun_api_types::AppServerConnectorSummary {
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

    fn sample_model_summary() -> nomifun_api_types::AppServerModelSummary {
        nomifun_api_types::AppServerModelSummary {
            provider_id: "0190f5fe-7c00-7a00-8000-000000000002".into(),
            provider_name: "Demo Provider".into(),
            model: "demo-model".into(),
            display_name: Some("Demo Model".into()),
            is_default: true,
        }
    }

    fn ready_catalog_state() -> (AppServerRouterState, CurrentUser, ConnectionState) {
        let state = AppServerRouterState {
            skills: Some(Arc::new(crate::catalog::FakeSkillCatalog {
                skills: vec![sample_skill_summary()],
            })),
            connectors: Some(Arc::new(crate::catalog::FakeConnectorCatalog {
                connectors: vec![sample_connector_summary()],
                auth_required_ids: vec![],
                probe_fail_ids: vec![],
            })),
            connector_auth: Some(Arc::new(crate::catalog::FakeConnectorAuth {
                authenticated: std::sync::Mutex::new(vec![]),
            })),
            models: Some(Arc::new(crate::catalog::FakeModelCatalog {
                models: vec![sample_model_summary()],
            })),
            ..Default::default()
        };
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state
            .registry
            .initialize(connection.connection_id(), request())
            .expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        (state, user, connection)
    }

    #[tokio::test]
    async fn websocket_dispatch_serves_skill_and_connector_catalog_methods() {
        let (state, user, connection) = ready_catalog_state();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));

        let skills = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-1")),
        )
        .await
        .expect("skill/list");
        assert_eq!(skills["result"][0]["name"], "demo");

        let skill = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/get",
            serde_json::json!({ "skill_id": "demo" }),
            Some(serde_json::json!("req-2")),
        )
        .await
        .expect("skill/get");
        // `summary` is flattened onto the detail wire object.
        assert_eq!(skill["result"]["id"], "demo");

        let connectors = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-3")),
        )
        .await
        .expect("connector/list");
        assert_eq!(connectors["result"][0]["name"], "playwright");

        let status = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/status",
            serde_json::json!({ "connector_id": "playwright" }),
            Some(serde_json::json!("req-4")),
        )
        .await
        .expect("connector/status");
        assert_eq!(status["result"]["status"], "connected");
    }

    #[tokio::test]
    async fn websocket_dispatch_serves_model_directory_and_advertises_capability() {
        let (state, user, connection) = ready_catalog_state();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        assert!(CapabilityAvailability::from_state(&state).models);

        let models = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "models/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-models")),
        )
        .await
        .expect("models/list");
        let entry = &models["result"]["items"][0];
        assert_eq!(entry["provider_id"], "0190f5fe-7c00-7a00-8000-000000000002");
        assert_eq!(entry["provider_name"], "Demo Provider");
        assert_eq!(entry["model"], "demo-model");
        assert_eq!(entry["display_name"], "Demo Model");
        assert_eq!(entry["is_default"], true);
        // Credential/endpoint internals must never cross the protocol.
        assert!(entry.get("api_key").is_none());
        assert!(entry.get("base_url").is_none());
    }

    #[tokio::test]
    async fn websocket_dispatch_rejects_catalog_methods_without_providers() {
        let state = AppServerRouterState::default();
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state
            .registry
            .initialize(connection.connection_id(), request())
            .expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));

        for (method, params) in [
            ("skill/list", serde_json::json!({})),
            ("skill/get", serde_json::json!({ "skill_id": "demo" })),
            ("models/list", serde_json::json!({})),
            ("connector/list", serde_json::json!({})),
            ("connector/get", serde_json::json!({ "connector_id": "playwright" })),
            ("connector/status", serde_json::json!({ "connector_id": "playwright" })),
            ("connector/test", serde_json::json!({ "connector_id": "playwright" })),
            (
                "connector/auth/start",
                serde_json::json!({ "connector_id": "playwright" }),
            ),
            ("import/list", serde_json::json!({})),
            ("import/get", serde_json::json!({ "snapshot_id": "snap_01" })),
            ("install/status", serde_json::json!({ "snapshot_id": "snap_01" })),
            (
                "install/disable",
                serde_json::json!({ "snapshot_id": "snap_01", "component_ids": [] }),
            ),
            ("market/list", serde_json::json!({})),
            ("market/get", serde_json::json!({ "marketplace_id": "m1" })),
            (
                "market/remove",
                serde_json::json!({ "marketplace_id": "m1", "cascade": true }),
            ),
            ("store/list", serde_json::json!({})),
        ] {
            let error = dispatch_connection_request(
                &state,
                &connection,
                &user,
                &subscriptions,
                method,
                params,
                Some(serde_json::json!("req-1")),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "unsupported_operation", "{method} must be gated by capability");
        }
    }

    #[tokio::test]
    async fn websocket_dispatch_runs_connector_auth_roundtrip() {
        let (state, user, connection) = ready_catalog_state();
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        let initial = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/auth/status",
            serde_json::json!({ "connector_id": "playwright" }),
            Some(serde_json::json!("req-1")),
        )
        .await
        .expect("auth/status");
        assert_eq!(initial["result"]["state"], "not_authenticated");

        let started = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/auth/start",
            serde_json::json!({ "connector_id": "playwright" }),
            Some(serde_json::json!("req-2")),
        )
        .await
        .expect("auth/start");
        assert_eq!(started["result"]["state"], "started");

        let authenticated = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/auth/status",
            serde_json::json!({ "connector_id": "playwright" }),
            Some(serde_json::json!("req-3")),
        )
        .await
        .expect("auth/status after start");
        assert_eq!(authenticated["result"]["state"], "authenticated");

        let logout = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "connector/auth/logout",
            serde_json::json!({ "connector_id": "playwright" }),
            Some(serde_json::json!("req-4")),
        )
        .await
        .expect("auth/logout");
        assert_eq!(logout["result"]["logged_out"], true);
    }

    #[test]
    fn initialize_advertises_import_agent_and_team_capabilities_when_provided() {
        let mut state = ConnectionState::with_capabilities(
            LocalPrincipal::from_authenticated_user(UserId::new(), LocalTransport::WebSocket),
            CapabilityAvailability {
                runtime: true,
                events: true,
                imports: true,
                installs: true,
                marketplaces: true,
                agents: true,
                teams: true,
                ..Default::default()
            },
        );
        let result = state.initialize(request()).unwrap();
        assert!(result.capabilities.imports);
        assert!(result.capabilities.installs);
        assert!(result.capabilities.marketplaces);
        assert!(result.capabilities.agents);
        assert!(result.capabilities.teams);
        assert!(!result.capabilities.skills, "uninjected capability stays off");
    }

    #[tokio::test]
    async fn install_impls_route_through_the_provider_and_gate() {
        let mut install = crate::catalog::FakeInstallProvider::new();
        install.install_result.installed_count = 3;
        install.install_result.skipped = vec!["wb-demo-hole".into()];
        install.status.components = vec![nomifun_api_types::AppServerInstallComponent {
            id: "wb-demo-skill".into(),
            kind: "skill".into(),
            name: "demo-skill".into(),
            state: nomifun_api_types::AppServerInstallState::Installed,
            runtime_location: Some("/data/skills/agent-store/x/demo-skill/SKILL.md".into()),
            preset_id: None,
        }];
        let state = AppServerRouterState {
            installs: Some(Arc::new(install)),
            ..Default::default()
        };
        let request = nomifun_api_types::AppServerInstallRequest {
            snapshot_id: "snap-demo".into(),
        };
        let result = install_impl(&state, request.clone()).await.unwrap();
        assert_eq!(result.installed_count, 3);
        assert_eq!(result.skipped, vec!["wb-demo-hole"]);

        let status = install_status_impl(&state, "snap-demo").await.unwrap();
        assert_eq!(status.components.len(), 1);
        assert_eq!(status.components[0].state.as_str(), "installed");

        // Uninjected provider keeps the surface closed.
        let bare = AppServerRouterState::default();
        let denied = install_impl(&bare, request).await.unwrap_err();
        assert_eq!(denied.code, "unsupported_operation");
        let denied_status = install_status_impl(&bare, "snap-demo").await.unwrap_err();
        assert_eq!(denied_status.code, "unsupported_operation");
    }

    #[tokio::test]
    async fn market_impls_route_through_the_provider_and_gate() {
        let market = crate::catalog::FakeMarketplaceProvider::new();
        let state = AppServerRouterState {
            markets: Some(Arc::new(market)),
            ..Default::default()
        };
        let request = nomifun_api_types::AppServerMarketplaceAddRequest {
            name: None,
            source_kind: nomifun_api_types::AppServerMarketplaceSourceKind::Directory,
            source: "/tmp/company-tools".into(),
        };
        let summary = market_add_impl(&state, request.clone()).await.unwrap();
        assert_eq!(summary.marketplace_id, "company-tools");
        assert_eq!(summary.entry_count, 2);

        let listed = market_list_impl(&state).await.unwrap();
        assert_eq!(listed.len(), 1);

        let detail = market_get_impl(&state, "company-tools").await.unwrap();
        assert_eq!(detail.entries.len(), 2);
        assert_eq!(detail.entries[0].name, "formatter");
        // Entry provenance is server-projected (doc 16 D-W13-1 ①): the imported
        // entry carries its snapshot and install tally, and the untouched entry
        // carries nothing — a cascade removal reads its impact set from exactly
        // this, before it runs.
        let imported = detail.entries[0]
            .snapshot
            .as_ref()
            .expect("imported entry carries its snapshot");
        assert_eq!(imported.snapshot_id, "0190f5fe-7c00-7a00-8000-0000000000aa");
        assert_eq!(imported.component_count, 2);
        assert_eq!(imported.installed_count, 1);
        assert!(detail.entries[1].snapshot.is_none());

        let removed = market_remove_impl(&state, "company-tools", true).await.unwrap();
        assert_eq!(removed.snapshots, vec!["snap-demo"]);

        let toggled = market_auto_update_impl(&state, "company-tools", true).await.unwrap();
        assert!(toggled.auto_update);

        let refreshed = market_refresh_impl(&state, "company-tools").await.unwrap();
        assert!(refreshed.changed);
        assert_eq!(refreshed.entry_count, 2);
        assert_eq!(refreshed.resolved_revision, "abc123def");

        let imported = market_entry_import_impl(&state, "company-tools", "formatter").await.unwrap();
        assert_eq!(imported.snapshot_id, "snap-demo");

        // Uninjected provider keeps the surface closed.
        let bare = AppServerRouterState::default();
        let denied = market_add_impl(&bare, request).await.unwrap_err();
        assert_eq!(denied.code, "unsupported_operation");
        let denied_list = market_list_impl(&bare).await.unwrap_err();
        assert_eq!(denied_list.code, "unsupported_operation");
    }

    #[tokio::test]
    async fn store_impls_route_through_the_provider_and_gate() {
        let store = crate::catalog::FakeStoreProvider::new();
        let state = AppServerRouterState {
            store: Some(Arc::new(store)),
            ..Default::default()
        };

        let listed = store_list_impl(&state).await.unwrap();
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].id, "company-tools/formatter");
        assert_eq!(listed.items[0].entry_name, "formatter");

        let installed = store_install_entry_impl(&state, "company-tools", "formatter").await.unwrap();
        assert_eq!(installed.snapshot_id, "snap-demo");
        assert_eq!(installed.installed_count, 3);

        // Uninjected provider keeps the surface closed.
        let bare = AppServerRouterState::default();
        let denied = store_list_impl(&bare).await.unwrap_err();
        assert_eq!(denied.code, "unsupported_operation");
        let denied_install = store_install_entry_impl(&bare, "company-tools", "formatter").await.unwrap_err();
        assert_eq!(denied_install.code, "unsupported_operation");
    }

    fn sample_agent_summary() -> nomifun_api_types::AppServerAgentSummary {
        nomifun_api_types::AppServerAgentSummary {
            id: "wb-demo-software-team-lead".into(),
            version: "1.0.0".into(),
            name: "software-team-lead".into(),
            preset_id: None,
            description: Some("lead".into()),
            skills: vec![],
            connectors: vec![],
            model_summary: Some("gpt-5".into()),
            tool_policy_summary: Some("read_file, write_file".into()),
            source: "codebuddy-plugin".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::CompatibleWithAdapter,
            display_name: None,
            profession: None,
            avatar_url: None,
        }
    }

    fn sample_team_summary() -> nomifun_api_types::AppServerTeamSummary {
        nomifun_api_types::AppServerTeamSummary {
            id: "wb-demo-team".into(),
            version: "1.0.0".into(),
            name: "demo".into(),
            description: Some("demo team".into()),
            lead_agent_id: "wb-demo-software-team-lead".into(),
            member_agent_ids: vec!["wb-demo-software-qa-engineer".into()],
            source: "codebuddy-plugin".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::CompatibleWithAdapter,
        }
    }

    #[tokio::test]
    async fn websocket_dispatch_serves_agent_and_team_catalog_methods() {
        let state = AppServerRouterState {
            agent_catalog: Some(Arc::new(crate::catalog::FakeAgentCatalog {
                agents: vec![sample_agent_summary()],
            })),
            team_catalog: Some(Arc::new(crate::catalog::FakeTeamCatalog {
                teams: vec![sample_team_summary()],
            })),
            ..Default::default()
        };
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state
            .registry
            .initialize(connection.connection_id(), request())
            .expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));

        let agents = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "agent/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-1")),
        )
        .await
        .expect("agent/list");
        assert_eq!(agents["result"][0]["id"], "wb-demo-software-team-lead");

        let agent = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "agent/get",
            serde_json::json!({ "agent_id": "wb-demo-software-team-lead" }),
            Some(serde_json::json!("req-2")),
        )
        .await
        .expect("agent/get");
        assert_eq!(agent["result"]["name"], "software-team-lead");

        let teams = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "team/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-3")),
        )
        .await
        .expect("team/list");
        assert_eq!(teams["result"][0]["lead_agent_id"], "wb-demo-software-team-lead");

        let team = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "team/get",
            serde_json::json!({ "team_id": "wb-demo-team" }),
            Some(serde_json::json!("req-4")),
        )
        .await
        .expect("team/get");
        assert_eq!(team["result"]["planner_policy"], "planned");
        assert!(team["result"]["team_runtime_capabilities"].as_array().unwrap().len() >= 8);
    }

    #[tokio::test]
    async fn import_impls_route_through_the_provider_and_gate() {
        let state = AppServerRouterState {
            imports: Some(Arc::new(crate::catalog::FakeImportProvider::new())),
            ..Default::default()
        };
        let request = AppServerImportRequest {
            source_path: "/local/source/demo".into(),
            source_kind: nomifun_api_types::AppServerImportSourceKind::CodeBuddyPlugin,
        };
        let result = run_import_impl(&state, request.clone()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.snapshot_id, "snap-demo");
        assert_eq!(result.source_kind, "codebuddy-plugin");

        let list = list_imports_impl(&state, 10).await.unwrap();
        assert!(list.is_empty());

        let missing = get_import_impl(&state, "nope").await.unwrap_err();
        assert_eq!(missing.code, "not_found");

        // Uninjected provider keeps the surface closed.
        let bare = AppServerRouterState::default();
        let denied = run_import_impl(&bare, request).await.unwrap_err();
        assert_eq!(denied.code, "unsupported_operation");
        let denied_agents = list_agents_impl(&bare).await.unwrap_err();
        assert_eq!(denied_agents.code, "unsupported_operation");
    }

    // ---- Skill write face: `skill/create` · `skill/update` · `skill/delete` --
    //
    // `SkillAdmin` is the production write provider (the host wires it with its
    // own `SkillPaths`), so these tests exercise the real filesystem primitive
    // through the real dispatch arm — a refusal is only "nothing was written"
    // if the temp tree is still byte-identical afterwards, which is what every
    // refusal test below asserts.

    /// Isolated `SkillPaths` under the OS temp dir. `TMP` is deliberately not
    /// touched: Rust's `temp_dir()` reads it and mutating it breaks every other
    /// test in the process.
    fn skill_temp_paths(label: &str) -> (std::path::PathBuf, nomifun_extension::skill_service::SkillPaths) {
        let dir = std::env::temp_dir().join(format!("allo-skill-w12-{label}-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp skill root");
        let paths = nomifun_extension::skill_service::SkillPaths {
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

    /// Write one `SKILL.md` (creating parents) — the on-disk fixtures these
    /// tests classify (built-in, marketplace snapshot, shared, companion).
    fn write_skill_manifest(dir: &std::path::Path, name: &str, description: &str, body: &str) {
        std::fs::create_dir_all(dir).expect("skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        )
        .expect("skill manifest");
    }

    /// Test stand-in for the host's read catalog (`AppServerSkillCatalog`): same
    /// `skill_service::list_available_skills` source and the same id/origin
    /// rules, so the write face's read-back is a real read. The host's own
    /// projection (truncation included) is asserted in `nomifun-app`.
    struct TempSkillCatalog {
        paths: nomifun_extension::skill_service::SkillPaths,
    }

    #[async_trait::async_trait]
    impl SkillCatalogProvider for TempSkillCatalog {
        async fn list(&self) -> Result<Vec<AppServerSkillSummary>, nomifun_common::AppError> {
            let items = nomifun_extension::skill_service::list_available_skills(&self.paths)
                .await
                .map_err(|error| nomifun_common::AppError::Internal(error.to_string()))?;
            Ok(items
                .into_iter()
                .map(|item| {
                    let location = FsPath::new(&item.location);
                    let origin = nomifun_extension::skill_service::skill_origin_of(&self.paths, location);
                    AppServerSkillSummary {
                        id: item.name.clone(),
                        name: item.name.clone(),
                        description: Some(item.description.clone()).filter(|text| !text.trim().is_empty()),
                        version: "custom".into(),
                        source: "custom".into(),
                        origin: origin.as_str().into(),
                        writable: nomifun_extension::skill_service::is_writable_skill(
                            &self.paths,
                            &item.name,
                            location,
                        ),
                        compatibility_status:
                            nomifun_api_types::AppServerCompatibilityStatus::CompatibleWithAdapter,
                        enabled: true,
                        required_connectors: vec![],
                    }
                })
                .collect())
        }

        async fn get(&self, id: &str) -> Result<AppServerSkillDetail, nomifun_common::AppError> {
            let summary = self
                .list()
                .await?
                .into_iter()
                .find(|skill| skill.name == id)
                .ok_or_else(|| nomifun_common::AppError::NotFound(format!("skill {id} not found")))?;
            let location = nomifun_extension::skill_service::list_available_skills(&self.paths)
                .await
                .map_err(|error| nomifun_common::AppError::Internal(error.to_string()))?
                .into_iter()
                .find(|item| item.name == id)
                .map(|item| item.location)
                .expect("summary came from the same list");
            let instructions_summary = std::fs::read_to_string(
                nomifun_extension::skill_service::skill_manifest_path(FsPath::new(&location)),
            )
            .ok()
            .map(|body| body.trim().chars().take(1200).collect::<String>())
            .filter(|body| !body.is_empty());
            Ok(AppServerSkillDetail {
                summary,
                mode: "store-agent".into(),
                invocation_policy: "model-auto".into(),
                instructions_summary,
            })
        }
    }

    /// Ready connection whose catalog and write face both point at `paths`.
    fn skill_dispatch_state(
        paths: nomifun_extension::skill_service::SkillPaths,
    ) -> (AppServerRouterState, CurrentUser, ConnectionState, Arc<RwLock<WsSubscriptions>>) {
        let state = AppServerRouterState {
            skills: Some(Arc::new(TempSkillCatalog { paths: paths.clone() })),
            skill_writes: Some(Arc::new(SkillAdmin::new(paths))),
            ..Default::default()
        };
        let user = CurrentUser { id: UserId::new(), username: "test-user".into() };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state
            .registry
            .initialize(connection.connection_id(), request())
            .expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));
        (state, user, connection, subscriptions)
    }

    /// Dispatch one skill-write method and unwrap to its `result`.
    async fn dispatch_skill_write(
        state: &AppServerRouterState,
        connection: &ConnectionState,
        user: &CurrentUser,
        subscriptions: &Arc<RwLock<WsSubscriptions>>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppServerError> {
        let response = dispatch_connection_request(
            state,
            connection,
            user,
            subscriptions,
            method,
            params,
            Some(serde_json::json!("req-skill")),
        )
        .await?;
        Ok(response["result"].clone())
    }

    fn json_skill_create(name: &str, description: &str, body: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "description": description, "body": body })
    }

    #[tokio::test]
    async fn skill_write_face_is_absent_until_the_host_wires_it() {
        // Read catalog present, write face not wired: the four methods answer
        // `unsupported_operation` instead of pretending anything was written.
        let (_dir, paths) = skill_temp_paths("unwired");
        let state = AppServerRouterState {
            skills: Some(Arc::new(TempSkillCatalog { paths })),
            ..Default::default()
        };
        let user = CurrentUser { id: UserId::new(), username: "test-user".into() };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state.registry.initialize(connection.connection_id(), request()).expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));

        for (method, params) in [
            ("skill/create", json_skill_create("demo", "d", "b")),
            ("skill/update", serde_json::json!({ "skill_id": "demo", "description": "d" })),
            ("skill/delete", serde_json::json!({ "skill_id": "demo" })),
            ("skill/copy", serde_json::json!({ "skill_id": "demo", "new_name": "demo-copy" })),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                method,
                params,
            )
            .await
            .expect_err("unwired write face must refuse");
            assert_eq!(error.code, "unsupported_operation", "{method}");
        }
    }

    /// The write face answers by re-reading through the catalog, so a host that
    /// wires *only* the write provider must be refused **before** anything
    /// lands: otherwise a create could succeed on disk and still answer an
    /// error, and a retry would then hit `conflict` on its own first attempt.
    #[tokio::test]
    async fn skill_write_refuses_before_writing_when_the_read_catalog_is_missing() {
        let (dir, paths) = skill_temp_paths("write-only");
        let state = AppServerRouterState {
            skill_writes: Some(Arc::new(SkillAdmin::new(paths.clone()))),
            ..Default::default()
        };
        let user = CurrentUser { id: UserId::new(), username: "test-user".into() };
        let connection = state.registry.open_with_capabilities(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            CapabilityAvailability::from_state(&state),
        );
        state.registry.initialize(connection.connection_id(), request()).expect("initialize");
        state
            .registry
            .mark_initialized(connection.connection_id(), &user.id)
            .expect("initialized");
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions::default()));

        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/create",
            json_skill_create("demo", "d", "b"),
        )
        .await
        .expect_err("a write face without a read catalog must refuse");
        assert_eq!(error.code, "unsupported_operation");
        assert!(
            !paths.user_skills_dir.exists(),
            "a refused create must not have touched the skill root"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn skill_write_methods_are_owner_scoped_and_take_no_path() {
        let (dir, paths) = skill_temp_paths("scope");
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());

        // A foreign owner cannot reach this host's skill tree, and no parameter
        // can name one: the gate runs before any filesystem access.
        let foreign = CurrentUser { id: UserId::new(), username: "someone-else".into() };
        for (method, params) in [
            ("skill/create", json_skill_create("demo", "d", "b")),
            ("skill/update", serde_json::json!({ "skill_id": "demo", "description": "d" })),
            ("skill/delete", serde_json::json!({ "skill_id": "demo" })),
            ("skill/copy", serde_json::json!({ "skill_id": "demo", "new_name": "demo-copy" })),
        ] {
            let error =
                dispatch_skill_write(&state, &connection, &foreign, &subscriptions, method, params)
                    .await
                    .expect_err("foreign owner must be refused");
            assert_eq!(error.code, "policy_denied", "{method}");
        }

        // An unknown connection is `not_found`, never a write.
        let unknown = ConnectionState::new(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            true,
        );
        let error = dispatch_skill_write(
            &state,
            &unknown,
            &user,
            &subscriptions,
            "skill/create",
            json_skill_create("demo", "d", "b"),
        )
        .await
        .expect_err("unknown connection must be refused");
        assert_eq!(error.code, "not_found");

        // Nothing exists on disk after every refused call.
        assert!(!paths.user_skills_dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_create_writes_then_reads_back_and_rejects_paths_and_credentials() {
        let (dir, paths) = skill_temp_paths("create");
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());
        let body = "## steps\n1. gather\n2. write";

        let created = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/create",
            json_skill_create("weekly-report", "weekly summary", body),
        )
        .await
        .expect("skill/create");

        // The answer is the catalog's view: the id the caller named, a writable
        // user skill, and the body read back through `skill/get` (bounded).
        assert_eq!(created["id"], "weekly-report");
        assert_eq!(created["origin"], "user");
        assert_eq!(created["writable"], true);
        assert_eq!(created["source"], "custom");
        let summary = created["instructions_summary"].as_str().expect("summary");
        assert!(summary.contains("## steps"), "{summary}");
        assert!(summary.len() <= 1200, "summary must stay bounded: {}", summary.len());

        // …and the file is really there, at the canonical user location only.
        let manifest = paths.user_skills_dir.join("weekly-report").join("SKILL.md");
        let on_disk = std::fs::read_to_string(&manifest).expect("manifest on disk");
        assert!(on_disk.contains("name: weekly-report"), "{on_disk}");
        assert!(on_disk.contains("description: weekly summary"), "{on_disk}");
        assert!(on_disk.contains(body), "{on_disk}");

        // `skill/list` reports it as writable so a UI can offer the write action.
        let listed = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/list",
            serde_json::json!({}),
            Some(serde_json::json!("req-list")),
        )
        .await
        .expect("skill/list");
        assert_eq!(listed["result"][0]["name"], "weekly-report");
        assert_eq!(listed["result"][0]["writable"], true);
        assert_eq!(listed["result"][0]["origin"], "user");

        // A long body is truncated by the read face, never handed back whole.
        let long = "x".repeat(3000);
        dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/create",
            json_skill_create("long-skill", "long body", &long),
        )
        .await
        .expect("skill/create long");
        let fetched = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/get",
            serde_json::json!({ "skill_id": "long-skill" }),
            Some(serde_json::json!("req-get")),
        )
        .await
        .expect("skill/get");
        let truncated = fetched["result"]["instructions_summary"].as_str().expect("summary");
        assert_eq!(truncated.chars().count(), 1200);
        assert!(!truncated.contains(&"x".repeat(1201)));

        // Bad names: path traversal, separators, absolute paths, empty, dot
        // names, control characters — all `invalid_request`, all with an
        // unchanged user root.
        let before: Vec<_> = std::fs::read_dir(&paths.user_skills_dir)
            .expect("user root exists")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        for name in [
            "../escape",
            "a/b",
            "a\\b",
            "C:\\skills\\evil",
            "/etc/passwd",
            "",
            ".",
            "..",
            "evil\u{7}name",
            "trailing-space ",
            &"x".repeat(65),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/create",
                json_skill_create(name, "desc", "body"),
            )
            .await
            .expect_err("bad name must be refused");
            assert_eq!(error.code, "invalid_request", "name {name:?}");
        }
        let after: Vec<_> = std::fs::read_dir(&paths.user_skills_dir)
            .expect("user root exists")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(before, after, "a refused name must not create anything");
        assert!(!paths.data_dir.join("escape").exists());
        assert!(!paths.data_dir.join("skills").join("a").exists());

        // Credential-shaped fields are unknown fields: a hard rejection, not a
        // silent drop (R22 keeps its gate).
        for extra in ["api_key", "env", "token"] {
            let mut params = json_skill_create("cred", "desc", "body");
            params[extra] = serde_json::json!("secret-value");
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/create",
                params,
            )
            .await
            .expect_err("credential field must be refused");
            assert_eq!(error.code, "invalid_request", "{extra}");
        }
        assert!(!paths.user_skills_dir.join("cred").exists());
        let mut update = serde_json::json!({
            "skill_id": "weekly-report",
            "markdown": "---\nname: weekly-report\ndescription: d\n---\n"
        });
        update["env"] = serde_json::json!({ "TOKEN": "x" });
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            update,
        )
        .await
        .expect_err("credential field must be refused");
        assert_eq!(error.code, "invalid_request");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_create_conflicts_with_every_existing_source() {
        let (dir, paths) = skill_temp_paths("conflict");
        // Same name from three different owners: built-in, an installed
        // marketplace snapshot, and an existing user skill.
        write_skill_manifest(&paths.builtin_skills_dir.join("builtin-name"), "builtin-name", "b", "b");
        write_skill_manifest(
            &paths
                .user_skills_dir
                .join("agent-store")
                .join("0190f5fe-7c00-7a00-8000-000000000201")
                .join("market-name"),
            "market-name",
            "m",
            "m",
        );
        write_skill_manifest(&paths.user_skills_dir.join("user-name"), "user-name", "u", "u");
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());

        for (name, expected_origin, hint) in [
            ("builtin-name", "builtin", "read-only"),
            ("market-name", "marketplace", "install/uninstall"),
            ("user-name", "user", "skill/update"),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/create",
                json_skill_create(name, "desc", "body"),
            )
            .await
            .expect_err("existing name must conflict");
            assert_eq!(error.code, "conflict", "{name}");
            assert!(
                error.message.contains(&format!("origin={expected_origin}")),
                "{name}: {}",
                error.message
            );
            assert!(error.message.contains(hint), "{name}: {}", error.message);
        }

        // No shadow copy was created for any of them.
        assert!(!paths.user_skills_dir.join("builtin-name").exists());
        assert!(!paths.user_skills_dir.join("market-name").exists());

        // An on-disk directory the scanner does not report (no valid
        // frontmatter) is still a conflict: `create` never merges into it.
        std::fs::create_dir_all(paths.user_skills_dir.join("broken")).expect("broken dir");
        std::fs::write(paths.user_skills_dir.join("broken").join("SKILL.md"), "not frontmatter")
            .expect("broken manifest");
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/create",
            json_skill_create("broken", "desc", "body"),
        )
        .await
        .expect_err("uncatalogued directory must conflict");
        assert_eq!(error.code, "conflict");
        assert_eq!(
            std::fs::read_to_string(paths.user_skills_dir.join("broken").join("SKILL.md"))
                .expect("untouched manifest"),
            "not frontmatter"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_update_and_delete_only_touch_writable_user_skills() {
        let (dir, paths) = skill_temp_paths("update-delete");
        write_skill_manifest(&paths.builtin_skills_dir.join("builtin-name"), "builtin-name", "b", "b");
        let market_dir = paths
            .user_skills_dir
            .join("agent-store")
            .join("0190f5fe-7c00-7a00-8000-000000000202")
            .join("market-name");
        write_skill_manifest(&market_dir, "market-name", "m", "m");
        let shared_dir = paths.user_skills_dir.join("shared").join("shared-name");
        write_skill_manifest(&shared_dir, "shared-name", "s", "s");
        write_skill_manifest(&paths.user_skills_dir.join("user-name"), "user-name", "u", "u");
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());

        let update_body = |name: &str, description: &str| {
            serde_json::json!({ "skill_id": name, "description": description })
        };

        // Read-only origins: `policy_denied`, each naming its own reason, and
        // the bytes on disk are untouched.
        for (name, reason) in [
            ("builtin-name", "built-in skills are read-only"),
            ("market-name", "uninstall them through the installer/marketplace chain"),
            ("shared-name", "companion flow"),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/update",
                update_body(name, "hijacked"),
            )
            .await
            .expect_err("read-only origin must be refused");
            assert_eq!(error.code, "policy_denied", "{name}");
            assert!(error.message.contains(reason), "{name}: {}", error.message);
        }
        let market_before =
            std::fs::read_to_string(market_dir.join("SKILL.md")).expect("market manifest");
        let shared_before =
            std::fs::read_to_string(shared_dir.join("SKILL.md")).expect("shared manifest");
        assert!(!market_before.contains("hijacked"));
        assert!(!shared_before.contains("hijacked"));

        // An unknown id is `not_found`; a name whose directory disagrees with
        // its frontmatter name is refused rather than guessed at.
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            update_body("ghost", "x"),
        )
        .await
        .expect_err("unknown skill");
        assert_eq!(error.code, "not_found");
        write_skill_manifest(&paths.user_skills_dir.join("dir-name"), "other-name", "o", "o");
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            update_body("other-name", "x"),
        )
        .await
        .expect_err("non-canonical layout");
        assert_eq!(error.code, "policy_denied");
        assert!(error.message.contains("disagrees"), "{}", error.message);

        // The write face has no rename path any more, and the old whole-document
        // shape is outside the envelope: `markdown` and `name` (and any other
        // unknown key) are `invalid_request`, not a silent partial update.
        for body in [
            serde_json::json!({
                "skill_id": "user-name",
                "markdown": "---\nname: user-name\ndescription: d\n---\n",
            }),
            serde_json::json!({ "skill_id": "user-name", "name": "someone-else" }),
            // Nothing named at all: refused instead of answering with an
            // unchanged skill.
            serde_json::json!({ "skill_id": "user-name" }),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/update",
                body.clone(),
            )
            .await
            .expect_err("out-of-envelope update");
            assert_eq!(error.code, "invalid_request", "{body}");
        }
        assert_eq!(
            std::fs::read_to_string(paths.user_skills_dir.join("user-name").join("SKILL.md"))
                .expect("untouched user manifest"),
            "---\nname: user-name\ndescription: u\n---\n\nu\n"
        );

        // The writable one is updated, and the answer is the re-read document.
        // Only the named field moves: frontmatter `name`, the body and the other
        // keys of a hand-edited document survive a partial edit.
        let updated = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            update_body("user-name", "updated description"),
        )
        .await
        .expect("skill/update");
        assert_eq!(updated["id"], "user-name");
        assert_eq!(updated["writable"], true);
        assert_eq!(updated["description"], "updated description");
        let after = std::fs::read_to_string(paths.user_skills_dir.join("user-name").join("SKILL.md"))
            .expect("updated manifest");
        assert!(after.contains("name: user-name"), "{after}");
        assert!(after.contains("description: updated description"), "{after}");
        assert!(after.contains("\nu\n") || after.ends_with("u\n"), "body survived: {after}");

        // A second partial edit that names only `when-to-use` leaves the
        // description it did not mention alone.
        let added = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            serde_json::json!({ "skill_id": "user-name", "when_to_use": "when the test runs" }),
        )
        .await
        .expect("skill/update");
        assert_eq!(added["description"], "updated description");
        let after = std::fs::read_to_string(paths.user_skills_dir.join("user-name").join("SKILL.md"))
            .expect("updated manifest");
        assert!(after.contains("when-to-use: when the test runs"), "{after}");
        assert!(after.contains("description: updated description"), "{after}");

        // Clearing an optional key removes its line; clearing the required
        // description is refused.
        dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            serde_json::json!({ "skill_id": "user-name", "when_to_use": "" }),
        )
        .await
        .expect("clear when-to-use");
        let after = std::fs::read_to_string(paths.user_skills_dir.join("user-name").join("SKILL.md"))
            .expect("updated manifest");
        assert!(!after.contains("when-to-use"), "{after}");
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/update",
            serde_json::json!({ "skill_id": "user-name", "description": "   " }),
        )
        .await
        .expect_err("empty description");
        assert_eq!(error.code, "invalid_request");

        // Delete removes the user skill and reports what the id now resolves to
        // — nothing here, so `revealed_origin` is absent.
        let deleted = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/delete",
            serde_json::json!({ "skill_id": "user-name" }),
        )
        .await
        .expect("skill/delete");
        assert_eq!(deleted["skill_id"], "user-name");
        assert_eq!(deleted["deleted"], true);
        assert!(deleted.get("revealed_origin").is_none(), "{deleted}");
        assert!(!paths.user_skills_dir.join("user-name").exists());

        // The read-only trees are still exactly as they were, and deleting them
        // answers `policy_denied` instead of emptying them.
        for name in ["builtin-name", "market-name", "shared-name"] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/delete",
                serde_json::json!({ "skill_id": name }),
            )
            .await
            .expect_err("read-only origin must be refused");
            assert_eq!(error.code, "policy_denied", "{name}");
        }
        assert_eq!(
            std::fs::read_to_string(market_dir.join("SKILL.md")).expect("market manifest"),
            market_before
        );
        assert_eq!(
            std::fs::read_to_string(shared_dir.join("SKILL.md")).expect("shared manifest"),
            shared_before
        );
        assert!(paths.builtin_skills_dir.join("builtin-name").join("SKILL.md").exists());

        // Deleting again is `not_found` — no second delete, no guessing.
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/delete",
            serde_json::json!({ "skill_id": "user-name" }),
        )
        .await
        .expect_err("second delete");
        assert_eq!(error.code, "not_found");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_copy_derives_a_user_skill_from_any_origin() {
        let (dir, paths) = skill_temp_paths("copy");
        write_skill_manifest(&paths.builtin_skills_dir.join("builtin-name"), "builtin-name", "b", "b");
        // A built-in with support files: the copy must carry the whole subtree,
        // not just SKILL.md.
        let support = paths.builtin_skills_dir.join("builtin-name").join("references");
        std::fs::create_dir_all(&support).expect("support dir");
        std::fs::write(support.join("guide.md"), "reference body\n").expect("support file");
        let market_dir = paths
            .user_skills_dir
            .join("agent-store")
            .join("0190f5fe-7c00-7a00-8000-000000000203")
            .join("market-name");
        write_skill_manifest(&market_dir, "market-name", "m", "m");
        write_skill_manifest(&paths.user_skills_dir.join("user-name"), "user-name", "u", "u");
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());

        // A read-only source is copyable — that is the point of the method — and
        // the new skill is writable.
        let copied = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "builtin-name", "new_name": "my-builtin" }),
        )
        .await
        .expect("skill/copy");
        assert_eq!(copied["id"], "my-builtin");
        assert_eq!(copied["origin"], "user");
        assert_eq!(copied["writable"], true);
        assert_eq!(copied["description"], "b");

        // The copy is a real directory with the rewritten identity and the
        // source's support files.
        let on_disk = std::fs::read_to_string(
            paths.user_skills_dir.join("my-builtin").join("SKILL.md"),
        )
        .expect("copied manifest");
        assert!(on_disk.contains("name: my-builtin"), "{on_disk}");
        assert!(!on_disk.contains("name: builtin-name"), "{on_disk}");
        assert_eq!(
            std::fs::read_to_string(
                paths.user_skills_dir.join("my-builtin").join("references").join("guide.md")
            )
            .expect("copied support file"),
            "reference body\n"
        );
        // The source was not touched.
        assert!(std::fs::read_to_string(paths.builtin_skills_dir.join("builtin-name").join("SKILL.md"))
            .expect("builtin manifest")
            .contains("name: builtin-name"));

        // A marketplace install is a copyable source too, and the copy lands in
        // the user root — never back into the snapshot.
        let copied = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "market-name", "new_name": "my-market" }),
        )
        .await
        .expect("skill/copy");
        assert_eq!(copied["id"], "my-market");
        assert!(paths.user_skills_dir.join("my-market").join("SKILL.md").exists());
        assert!(!paths
            .user_skills_dir
            .join("agent-store")
            .join("0190f5fe-7c00-7a00-8000-000000000203")
            .join("my-market")
            .exists());

        // Conflicts: the target name is taken by an existing origin, and by an
        // uncatalogued directory on disk.
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "user-name", "new_name": "builtin-name" }),
        )
        .await
        .expect_err("existing target name must conflict");
        assert_eq!(error.code, "conflict");
        assert!(error.message.contains("origin=builtin"), "{}", error.message);

        // Copying a skill onto itself is a malformed request, not a collision:
        // the caller named the same skill twice.
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "user-name", "new_name": "user-name" }),
        )
        .await
        .expect_err("copy onto itself must be refused");
        assert_eq!(error.code, "invalid_request");
        assert!(error.message.contains("new name"), "{}", error.message);
        std::fs::create_dir_all(paths.user_skills_dir.join("broken")).expect("broken dir");
        std::fs::write(paths.user_skills_dir.join("broken").join("SKILL.md"), "not frontmatter")
            .expect("broken manifest");
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "user-name", "new_name": "broken" }),
        )
        .await
        .expect_err("uncatalogued target directory must conflict");
        assert_eq!(error.code, "conflict");

        // Paths and junk never reach the filesystem, and an unknown source is
        // `not_found` — the same gates as every other write.
        for (skill_id, new_name, expected) in [
            ("user-name", "../escape", "invalid_request"),
            ("user-name", "a/b", "invalid_request"),
            ("user-name", "", "invalid_request"),
            ("ghost", "fresh-name", "not_found"),
        ] {
            let error = dispatch_skill_write(
                &state,
                &connection,
                &user,
                &subscriptions,
                "skill/copy",
                serde_json::json!({ "skill_id": skill_id, "new_name": new_name }),
            )
            .await
            .expect_err("must be refused");
            assert_eq!(error.code, expected, "{new_name}");
        }
        assert!(!paths.user_skills_dir.join("fresh-name").exists());
        // No credential field is expressible here either (R22 gate).
        let error = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/copy",
            serde_json::json!({ "skill_id": "user-name", "new_name": "ok-name", "env": { "TOKEN": "x" } }),
        )
        .await
        .expect_err("unknown field");
        assert_eq!(error.code, "invalid_request");
        assert!(!paths.user_skills_dir.join("ok-name").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn skill_delete_reports_the_builtin_it_unshadows() {
        let (dir, paths) = skill_temp_paths("reveal");
        // A user skill carrying a built-in's name: the read face prefers the
        // user copy, so deleting it must hand the id back to the built-in.
        write_skill_manifest(
            &paths.builtin_skills_dir.join("code-review"),
            "code-review",
            "builtin review",
            "b",
        );
        write_skill_manifest(
            &paths.user_skills_dir.join("code-review"),
            "code-review",
            "my own review",
            "u",
        );
        let (state, user, connection, subscriptions) = skill_dispatch_state(paths.clone());

        let before = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/get",
            serde_json::json!({ "skill_id": "code-review" }),
            None,
        )
        .await
        .expect("skill/get before");
        assert_eq!(before["result"]["origin"], "user");
        assert_eq!(before["result"]["writable"], true);

        let deleted = dispatch_skill_write(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/delete",
            serde_json::json!({ "skill_id": "code-review" }),
        )
        .await
        .expect("skill/delete");
        assert_eq!(deleted["revealed_origin"], "builtin");

        let after = dispatch_connection_request(
            &state,
            &connection,
            &user,
            &subscriptions,
            "skill/get",
            serde_json::json!({ "skill_id": "code-review" }),
            None,
        )
        .await
        .expect("skill/get after");
        assert_eq!(after["result"]["origin"], "builtin");
        assert_eq!(after["result"]["writable"], false);
        assert!(paths.builtin_skills_dir.join("code-review").join("SKILL.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
