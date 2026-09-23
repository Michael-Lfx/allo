//! Versioned App Server connection and run boundary.
//!
//! Agent Store is a consumer of this protocol. A connection owns a
//! transport-established principal and must complete `initialize` followed by
//! `initialized` before it can invoke application methods.

pub mod agent_store;
pub mod catalog;
pub mod skill_admin;
mod team_run;
pub mod workspace_resolver;

pub use team_run::TeamRunRequest;

pub use agent_store::{
    AgentStoreConfig, AgentStoreConfigPatch, AgentStoreMarketplace, AgentStoreModel,
    AgentStoreProvider,
};
pub use catalog::{
    AgentCatalogProvider, ConnectorAuthProvider, ConnectorCallError, ConnectorCallProvider,
    ConnectorCatalogProvider, ConnectorCredentialProvider, ExpertPackError, ExpertPackProvider,
    ImportProvider,
    InstallProvider, MarketplaceProvider, ModelCatalogProvider, MAX_CONNECTOR_CALL_RESULT_BYTES,
    MAX_CONNECTOR_TOOLS_BYTES,
    MAX_EXPERT_PACK_BYTES,
    MAX_SKILL_FILE_BYTES,
    SkillCatalogProvider, SkillFileBytes, SkillFileError, SkillFileProvider, StoreProvider,
    TeamCatalogProvider,
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

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

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
    AgentExecutionEngine, AgentRunPlan, AgentRunReceipt, AgentRunResult, AgentRunSteerRequest,
    AgentRunView, TeamRunReceipt,
};
use nomifun_auth::CurrentUser;
use nomifun_common::{MessagePosition, MessageType, ProviderWithModel, UserId, generate_id};
use nomifun_api_types::{
    AppServerAgentDetail, AppServerAgentSummary, AppServerConfigMcpRejectionView,
    AppServerConfigMcpServerView, AppServerConfigMcpView, AppServerConfigMemoryView,
    AppServerConfigProviderView, AppServerConfigView, AppServerMcpSourceView,
    AppServerConnectorCallResult,
    AppServerConnectorCredential,
    AppServerConnectorDetail,
    AppServerConnectorProbeResult, AppServerConnectorStatusView, AppServerConnectorSummary,
    AppServerExpertPack,
    AppServerImportDetail, AppServerImportRequest, AppServerImportResult,
    AppServerImportSummary, AppServerInstallRequest, AppServerInstallResult,
    AppServerInstallStatus, AppServerMarketplaceAddRequest, AppServerMarketplaceDetail,
    AppServerMarketplaceRefreshResult, AppServerMarketplaceRemoveResult,
    AppServerMarketplaceSummary, AppServerModelList, AppServerModelSummary,
    AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDeleteResult, AppServerSkillDetail, AppServerSkillFileList, AppServerSkillSummary,
    AppServerStoreInstallResult,
    AppServerStoreList, AppServerTeamDetail, AppServerTeamSummary,
    AnswerExecutionDecisionRequest, CreateProviderRequest, ListMessagesQuery, McpServerId, MessageResponse,
    PresetOverrides, PresetSource,
    PresetTarget, SendMessageRequest, UpdateProviderModelRequest,
};
use nomifun_conversation::{AppServerChatBindings, ConversationService, IdempotentMessageDelivery};
use nomifun_preset::PresetService;
use nomifun_realtime::{BroadcastEventBus, UserEventEnvelope};
use nomifun_system::{ProviderModelService, ProviderService};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

/// App Server wire protocol version, negotiated by `initialize`.
///
/// Not a version number: a **contract fingerprint**. It must differ from the
/// previous value on any wire change at all, additive included. The shape is
/// `fp-<n>` — a plain counter, so a bump just increments it and no value is ever
/// reused by accident.
///
/// So it is a **label, not a version**: neither a release number nor a date.
/// Until `fp-1` the values were date stamps (kept below as history), and those
/// dates were **not** the day of the change: consecutive changes advanced the
/// stamp a day each, so they ran ahead of the calendar. The list below is keyed
/// by *value* — read it as "which wire change is this?", never as "when did this
/// ship".
///
/// `2026-09-16` carried `store/list`'s `published_at`; `2026-09-17` added the
/// two MCP declaration write methods; `2026-09-18` adds `config/get-mcp`, the
/// editor's read of the same file (`21` D17); `2026-09-19` adds the
/// `conversation/list-changed` notification — the sidebar's half of the
/// conversation list projection (auto-title, rename, delete), which until now
/// only reached the host channel. `2026-09-20` adds the Skill **file tree**
/// read face (`skill/files`, `skill/file`), so a Skill's companions are
/// readable instead of only its 1200-char manifest summary (doc 24 §4).
/// `2026-09-21` adds the connector **call proxy** (`connector/call`), so a third
/// party can run an installed MCP tool while the connection, its headers and its
/// credentials stay on the host (doc 24 §5). **`fp-1` changes the shape only**
/// (date stamp → counter): a `2026-…` value invites being read as a release date,
/// and no wire behaviour changed with the rename. **`fp-2` carries the tools'
/// parameters**: `ConnectorTool.input_schema` plus `tools_truncated` on
/// `ConnectorDetail` and `AppServerConnectorProbeResult`, so a caller that has to
/// name a tool in order to be granted it can read what that tool takes — the
/// counterpart of the `[connector_proxy]` grant moving from one tool at a time to
/// the connector (doc 26 §5). **`fp-3` makes a Skill selectable per turn**:
/// `conversation/send` gains an optional `mentions` list whose only honoured kind
/// is `skill`, so a caller can mount a Skill's instructions for one turn without
/// rewriting the conversation's create-time snapshot (doc 27 阶段 1).
/// **`fp-4` lets a conversation be created as an installed expert**:
/// `conversation/create` gains an optional `agent_id`, whose Definition supplies
/// the chat's preset identity plus its own Skill and Connector fences — frozen at
/// creation, because nothing about a conversation's preset snapshot is mutable
/// afterwards (doc 27 阶段 2a). **`fp-5` opens a Team's Leader the same way**: an
/// optional `team_id` runs the `team/run` orchestration (members, template,
/// fences) but stops before the goal turn, so the client speaks first; the two
/// fields are mutually exclusive (doc 27 阶段 2b). **`fp-6` makes the model and
/// the reasoning level selectable per call**: `conversation/send` and
/// `agent/run` each gain an optional `model` and `reasoning_effort`, and
/// `ConversationView` gains `reasoning_effort` so the value can be read back.
/// The scope is the conversation (send) / the run (agent/run), not "one turn":
/// the Nomi runtime is built from the persisted row, so what a caller passes is
/// a sticky switch that takes effect on that very turn (doc 29 §4).
/// **`fp-7` adds the `zip` marketplace source kind**: `market/add` accepts
/// `source_kind: "zip"` — one archive whose root *is* the market root — and the
/// official bundles move to it, because the old `url` form made a first fetch
/// mirror 14,714 files (611 MiB) for `experts` alone (doc 30).
/// **`fp-8` adds expert definition export**: `agent/export` and `team/export`
/// return a portable `AppServerExpertPack` — the persona, model hints, skill
/// references and (for a team) the roster with every member expanded. No method
/// is *removed* and nothing existing changes shape, but the response carries the
/// Agent Markdown body, which the catalog faces deliberately never do
/// (`frontmatter.rs:114`, `app_server.rs:435`); see doc 32. Both methods are
/// WebSocket-only, like the rest of the agent/team family, so the documented
/// route split moves to `48 / 73` (mapped unchanged, two new unmapped).
/// **`fp-9` adds connector user credentials** (doc 34): a connector that needs a
/// key or token the *user* supplies now has a form — `connector/credential/get`,
/// `set` and `clear`, plus a `credential` block on the connector summary
/// (`mode` / `status` / `missing` / `fields`). Secret values never cross this
/// wire in either direction; `missing` and the block's field list carry key names
/// and marketplace text only. Credentials are written per principal
/// (`<principal>:NAME`), so a shared host no longer resolves one user's token for
/// another. The three methods are mapped, not WebSocket-only, so the documented
/// split moves to `51 / 73`.
pub const PROTOCOL_VERSION: &str = "fp-9";
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
    /// `skill/files` / `skill/file` readiness — the *file tree* seam, which is
    /// wired independently of the catalog (`05` §4.3.1).
    pub skill_files: bool,
    pub connectors: bool,
    /// `connector/call` readiness (the call proxy seam).
    pub connector_calls: bool,
    pub oauth: bool,
    pub imports: bool,
    pub installs: bool,
    pub marketplaces: bool,
    pub agents: bool,
    pub teams: bool,
    /// `team/run` readiness: the Team catalog *and* the execution facade. Both are
    /// required (one resolves the Definition, the other materializes/runs it), so
    /// advertising the capability off either half would promise a method whose only
    /// answer is `unsupported_operation`.
    pub team_runtime: bool,
    pub store: bool,
    pub models: bool,
    /// `agent/export` / `team/export` readiness — the expert **definition**
    /// seam, which is wired independently of the catalogs it reads (doc `32`).
    ///
    /// Reports the seam, not the policy: the `[expert_export]` table is evaluated
    /// per request by the provider (same split as `connector_calls`, which is
    /// wired even on a host whose `[connector_proxy]` is off).
    pub expert_export: bool,
}

impl CapabilityAvailability {
    pub fn from_state(state: &AppServerRouterState) -> Self {
        Self {
            runtime: state.runtime.is_some(),
            events: state.event_bus.is_some(),
            skills: state.skills.is_some(),
            skill_files: state.skill_files.is_some(),
            connectors: state.connectors.is_some(),
            connector_calls: state.connector_calls.is_some(),
            oauth: state.connector_auth.is_some(),
            imports: state.imports.is_some(),
            installs: state.installs.is_some(),
            marketplaces: state.markets.is_some(),
            agents: state.agent_catalog.is_some(),
            teams: state.team_catalog.is_some(),
            team_runtime: state.team_catalog.is_some() && state.engine.is_some(),
            store: state.store.is_some(),
            models: state.models.is_some(),
            expert_export: state.expert_packs.is_some(),
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
    /// Idle deadline for this connection token (`None` = never expires). Set by
    /// the registry from its configured TTL and refreshed on each successful
    /// use (`22` §7.1 A2), so the deadline bounds **inactivity**, not the
    /// lifetime of an actively-used session.
    expires_at: Option<std::time::Instant>,
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
            expires_at: None,
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

    /// The principal id as a string, for principal-wide revocation (`22` §7.1
    /// A2) where the caller holds an id string rather than a `UserId`.
    fn principal_user_id(&self) -> Option<&str> {
        self.context
            .as_ref()
            .map(|context| context.principal.user_id().as_str())
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
                name: "flowy-agent-store",
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

    /// True when the token's idle deadline has passed (`22` §7.1 A2).
    fn is_expired(&self, now: std::time::Instant) -> bool {
        self.expires_at.is_some_and(|deadline| now >= deadline)
    }

    /// Reset the idle deadline to `ttl` from now (`None` = never expire).
    fn touch(&mut self, ttl: Option<std::time::Duration>) {
        self.expires_at = ttl.map(|ttl| std::time::Instant::now() + ttl);
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
    /// The Skill **file tree** read face (`skill/files` / `skill/file`).
    ///
    /// Separate from `skills` on purpose: a host can wire the catalog without
    /// the file provider, and a client that only sees `skills: true` would
    /// otherwise call `skill/files` and get `unsupported_operation`.
    pub skill_files: bool,
    pub connectors: bool,
    /// `connector/call` readiness — the **call proxy**, wired independently of
    /// the connector catalog (doc 24 §5).
    pub connector_calls: bool,
    pub run_notifications: bool,
    pub approvals: bool,
    pub artifacts: bool,
    pub oauth: bool,
    pub imports: bool,
    pub installs: bool,
    pub marketplaces: bool,
    pub store: bool,
    pub models: bool,
    /// `agent/export` / `team/export` readiness — see
    /// [`CapabilityAvailability::expert_export`]. Reports the **seam**, not the
    /// host's `[expert_export]` policy (that decision is made per request).
    pub expert_export: bool,
}

impl Capabilities {
    fn from_availability(availability: CapabilityAvailability) -> Self {
        Self {
            agents: availability.runtime || availability.agents,
            teams: availability.teams,
            team_runtime: availability.team_runtime,
            skills: availability.skills,
            skill_files: availability.skill_files,
            connectors: availability.connectors,
            connector_calls: availability.connector_calls,
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
            expert_export: availability.expert_export,
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
    #[error("connection token has expired")]
    TokenExpired,
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
            // Another turn already holds this conversation's admission slot. The
            // shared `conflict` code keeps the App Server code set unchanged; the
            // variant's `error_details()` still reaches the client.
            nomifun_common::AppError::ConversationTurnAdmissionConflict => ("conflict", false),
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
                // An unknown, closed or revoked token is an **authentication**
                // failure, not a missing resource: `05` §3.1 answers
                // `unauthenticated`. (Before A2 this answered `not_found`/404,
                // which conflated "no such connection" with "no such object".)
                Self::new(
                    "unauthenticated",
                    error.to_string(),
                    StatusCode::UNAUTHORIZED,
                    false,
                )
            }
            ProtocolError::TokenExpired => {
                // Distinct from `unauthenticated` so a client can tell "expired,
                // re-initialize" from "invalid": `22` §7.1 A2.
                Self::new(
                    "token_expired",
                    "app-server connection token expired; re-initialize",
                    StatusCode::UNAUTHORIZED,
                    true,
                )
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
    /// `team/run` receipts are a distinct shape on purpose (no lead preset), so
    /// they get their own record rather than being coerced into an
    /// `AgentRunReceipt` with fabricated preset metadata.
    TeamRun {
        fingerprint: String,
        receipt: TeamRunReceipt,
    },
    Cancel {
        fingerprint: String,
        view: AgentRunView,
    },
}

/// Default idle lifetime of an App Server connection token (`22` §7.1 A2).
///
/// Active connections renew their deadline on every successful use, so this
/// bounds **inactivity**: a token left unused for this long must be
/// re-established. A host can override it (or disable expiry entirely with
/// `with_connection_ttl(None)`), and tests inject a tiny value to force expiry.
pub const DEFAULT_CONNECTION_TTL: std::time::Duration =
    std::time::Duration::from_secs(12 * 60 * 60);

#[derive(Clone)]
pub struct AppServerRegistry {
    connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
    idempotency: Arc<RwLock<HashMap<String, IdempotencyRecord>>>,
    idempotency_gate: Arc<tokio::sync::Mutex<()>>,
    /// Idle TTL for connection tokens; `None` disables expiry.
    connection_ttl: Option<std::time::Duration>,
}

impl Default for AppServerRegistry {
    fn default() -> Self {
        Self {
            connections: Arc::default(),
            idempotency: Arc::default(),
            idempotency_gate: Arc::default(),
            connection_ttl: Some(DEFAULT_CONNECTION_TTL),
        }
    }
}

impl AppServerRegistry {
    /// Override the connection-token idle TTL (`None` = never expire). Used by
    /// the host to tune it and by tests to force expiry.
    pub fn with_connection_ttl(mut self, ttl: Option<std::time::Duration>) -> Self {
        self.connection_ttl = ttl;
        self
    }

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
        let mut state = ConnectionState::with_capabilities(principal, availability);
        state.touch(self.connection_ttl);
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
        if state.is_expired(std::time::Instant::now()) {
            return Err(ProtocolError::TokenExpired);
        }
        state.initialize(request)
    }

    pub fn close(&self, connection_id: &str) -> bool {
        self.connections
            .write()
            .expect("App Server connection registry lock is not poisoned")
            .remove(connection_id)
            .is_some()
    }

    /// True while the token is present and unexpired. The WebSocket loop uses
    /// this to end a socket whose token was revoked or has expired (`22` §7.1
    /// A2) instead of continuing to serve it.
    pub fn is_live(&self, connection_id: &str) -> bool {
        let connections = self
            .connections
            .read()
            .expect("App Server connection registry lock is not poisoned");
        connections
            .get(connection_id)
            .is_some_and(|state| !state.is_expired(std::time::Instant::now()))
    }

    /// Revoke every connection token issued to `user_id`, returning how many
    /// were dropped. Wired to the host's logout so a revoked session's App
    /// Server tokens die immediately (`22` §7.1 A2).
    pub fn revoke_principal(&self, user_id: &str) -> usize {
        let mut connections = self
            .connections
            .write()
            .expect("App Server connection registry lock is not poisoned");
        let before = connections.len();
        connections.retain(|_, state| state.principal_user_id() != Some(user_id));
        before - connections.len()
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
        if state.is_expired(std::time::Instant::now()) {
            return Err(ProtocolError::TokenExpired);
        }
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
            | Some(IdempotencyRecord::TeamRun { .. })
            | Some(IdempotencyRecord::Cancel { .. }) => Err(ProtocolError::IdempotencyConflict),
            None => Ok(None),
        }
    }

    /// Same contract as [`Self::existing_idempotent_run`] for the `team/run` scope.
    pub fn existing_idempotent_team_run(
        &self,
        scope: &str,
        fingerprint: &str,
    ) -> Result<Option<TeamRunReceipt>, ProtocolError> {
        let records = self
            .idempotency
            .read()
            .expect("App Server idempotency registry lock is not poisoned");
        match records.get(scope) {
            Some(IdempotencyRecord::TeamRun { fingerprint: existing_fingerprint, receipt })
                if existing_fingerprint == fingerprint => Ok(Some(receipt.clone())),
            Some(IdempotencyRecord::AgentRun { .. })
            | Some(IdempotencyRecord::TeamRun { .. })
            | Some(IdempotencyRecord::Cancel { .. }) => Err(ProtocolError::IdempotencyConflict),
            None => Ok(None),
        }
    }

    pub fn remember_idempotent_team_run(
        &self,
        scope: &str,
        fingerprint: String,
        receipt: TeamRunReceipt,
    ) -> Result<TeamRunReceipt, ProtocolError> {
        let mut records = self
            .idempotency
            .write()
            .expect("App Server idempotency registry lock is not poisoned");
        if let Some(existing) = records.get(scope) {
            return match existing {
                IdempotencyRecord::TeamRun { fingerprint: existing_fingerprint, receipt }
                    if existing_fingerprint == &fingerprint => Ok(receipt.clone()),
                IdempotencyRecord::AgentRun { .. }
                | IdempotencyRecord::TeamRun { .. }
                | IdempotencyRecord::Cancel { .. } => Err(ProtocolError::IdempotencyConflict),
            };
        }
        records.insert(
            scope.to_owned(),
            IdempotencyRecord::TeamRun { fingerprint, receipt: receipt.clone() },
        );
        Ok(receipt)
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
                IdempotencyRecord::AgentRun { .. }
                | IdempotencyRecord::TeamRun { .. }
                | IdempotencyRecord::Cancel { .. } => Err(ProtocolError::IdempotencyConflict),
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
            | Some(IdempotencyRecord::TeamRun { .. })
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
                IdempotencyRecord::AgentRun { .. }
                | IdempotencyRecord::TeamRun { .. }
                | IdempotencyRecord::Cancel { .. } => {
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
        // Write lock: a successful call renews the idle deadline (`22` §7.1 A2),
        // so an actively-used connection is never dropped mid-session.
        let mut connections = self
            .connections
            .write()
            .expect("App Server connection registry lock is not poisoned");
        let state = connections
            .get_mut(connection_id)
            .ok_or(ProtocolError::ConnectionNotFound)?;
        if state.is_expired(std::time::Instant::now()) {
            return Err(ProtocolError::TokenExpired);
        }
        if !state.belongs_to(user_id) {
            return Err(ProtocolError::PrincipalMismatch);
        }
        let view = state.require_ready()?.view();
        state.touch(self.connection_ttl);
        Ok(view)
    }
}

/// Adapts [`AppServerRegistry`] to the [`nomifun_common::OnSessionRevoked`] hook
/// so a host can wire `POST /logout` → connection-token revocation (`22` §7.1
/// A2). Holds a clone of the same registry the router state uses, so a
/// revocation here is visible to every in-flight request.
pub struct AppServerSessionRevocation {
    registry: AppServerRegistry,
}

impl AppServerSessionRevocation {
    pub fn new(registry: AppServerRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl nomifun_common::OnSessionRevoked for AppServerSessionRevocation {
    async fn on_session_revoked(&self, user_id: &str) {
        // Best-effort and infallible: a logout must never fail because a token
        // could not be dropped (an undropped token is still bounded by its TTL).
        let _ = self.registry.revoke_principal(user_id);
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
    /// Row-level `provider_models` writer. Used by the agent-store registration
    /// path to persist the per-model fields the provider DTO has no map column
    /// for (`output_limit`, `protocol`) — this is the same row-level face the
    /// settings UI writes those two through, so both paths land on one column
    /// instead of one of them silently dropping the value.
    pub provider_model_service: Option<Arc<ProviderModelService>>,
    /// Optional override for the agent-store config location. `None` resolves
    /// `~/.agent-store/config.toml` on every request.
    pub agent_store_config_path: Option<std::path::PathBuf>,
    /// Whether this host feeds `~/.agent-store/mcp.json` into agent sessions
    /// (`--adopt-store-mcp-declarations`, `20` §7.9), as reported by the launcher.
    /// `None` = the launcher did not say, which `config/get` answers as "unknown"
    /// rather than as "not adopted" ([`AppServerConfigMcpView::adopted`]).
    ///
    /// A host fact, not a file fact: `agent_store_config_path` says where the
    /// file is read from, this says whether it is read at all.
    pub adopt_store_mcp_declarations: Option<bool>,
    /// Agent Store Skill catalog provider. `None` keeps the `skills`
    /// capability off and returns `unsupported_operation` for `skill/*`.
    pub skills: Option<Arc<dyn SkillCatalogProvider>>,
    /// Skill *file tree* read face (`skill/files`, `skill/file`). `None` keeps
    /// those methods answering `unsupported_operation` and the `skill_files`
    /// capability off.
    pub skill_files: Option<Arc<dyn SkillFileProvider>>,
    /// Agent Store Skill write face (`skill/create|update|delete`, `16` R17).
    /// `None` keeps the write methods off (`unsupported_operation`); the read
    /// catalog stays available either way. Host management surface: no HTTP
    /// binding and no counterpart in the published SDK package.
    pub skill_writes: Option<Arc<dyn SkillWriteProvider>>,
    /// Agent Store Connector catalog provider. `None` keeps the `connectors`
    /// capability off and returns `unsupported_operation` for `connector/*`.
    pub connectors: Option<Arc<dyn ConnectorCatalogProvider>>,
    /// The connector **call proxy** (`connector/call`, doc 24 §5). `None` keeps
    /// the `connector_calls` capability off and the method answering
    /// `unsupported_operation`; production wires it only on the agent-store
    /// host, and only when `[connector_proxy]` allows something.
    pub connector_calls: Option<Arc<dyn ConnectorCallProvider>>,
    /// Connector OAuth pass-through. `None` keeps the `oauth` capability off.
    pub connector_auth: Option<Arc<dyn ConnectorAuthProvider>>,
    /// The connector **credential** face (`connector/credential/*`, `34` §6.1).
    /// `None` keeps the capability off and the methods answering
    /// `unsupported_operation`; a read-only host still describes credentials
    /// through the catalog's `credential` block.
    pub connector_credentials: Option<Arc<dyn ConnectorCredentialProvider>>,
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
    /// Expert **definition export** (`agent/export`, `team/export`, doc `32`).
    ///
    /// Wired unconditionally where the agent/team catalogs exist, exactly like
    /// `connector_calls`: the provider's own first gate is the host's
    /// `[expert_export]` table, so "wired" never means "exportable". `None` keeps
    /// the `expert_export` capability off and answers `unsupported_operation`.
    pub expert_packs: Option<Arc<dyn ExpertPackProvider>>,
    /// The single Agent Execution facade (`16` §7 决策 3).
    ///
    /// `team/run` needs more than the [`AgentRuntimeAdapter`] projection: it
    /// materializes a Team's `AgentExecutionTemplate` and resolves the aggregate a
    /// Leader Conversation created. Both are engine operations, and deliberately
    /// not part of the run *projection* the adapter owns. `None` keeps `team/run`
    /// off (`runtime_unavailable`) while `agent/run` keeps working through
    /// `runtime`.
    pub engine: Option<Arc<AgentExecutionEngine>>,
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
            provider_model_service: None,
            agent_store_config_path: None,
            adopt_store_mcp_declarations: None,
            skills: None,
            skill_files: None,
            skill_writes: None,
            connectors: None,
            connector_calls: None,
            connector_auth: None,
            connector_credentials: None,
            imports: None,
            installs: None,
            markets: None,
            agent_catalog: None,
            team_catalog: None,
            expert_packs: None,
            engine: None,
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
        .route("/api/app-server/team/run", post(team_run))
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
        // Skill file tree (doc 24 §4). Distinct path segment from
        // `{skill_id}` so a skill literally named "files" cannot shadow the
        // sub-route.
        .route("/api/app-server/skills/{skill_id}/files", get(list_skill_files_route))
        .route(
            "/api/app-server/skills/{skill_id}/files/{*path}",
            get(read_skill_file_route),
        )
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
        // Connector call proxy (doc 24 §5).
        .route(
            "/api/app-server/connectors/{connector_id}/call",
            post(connector_call_route),
        )
        // Connector credentials (34 §6.1): the user-supplied key/token form.
        // Both writes are POST — this protocol's write verbs are POST
        // throughout, so the client's route table has one verb per method.
        .route(
            "/api/app-server/connectors/{connector_id}/credential",
            get(connector_credential_get_route).post(connector_credential_set_route),
        )
        .route(
            "/api/app-server/connectors/{connector_id}/credential/clear",
            post(connector_credential_clear_route),
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
        // Durable rows hold `fs::canonicalize` output (the repository
        // normalizes through `canonical_directory`), which on Windows is a
        // verbatim extended-length `\\?\C:\...` spelling. That is correct for
        // filesystem syscalls and wrong for every consumer of this projection:
        // the Artifact panel renders `canonical_path` as its workspace
        // breadcrumb and round-trips it back as `POST /api/fs/list {root}`.
        // Project the simplified spelling (`nomifun-common::paths` rule) —
        // doing it here also repairs rows written before the rule, so no
        // migration is needed.
        canonical_path: nomifun_common::paths::marker_string(FsPath::new(&row.root_path)),
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

/// `skill/files`: the readable file inventory of one Skill's directory.
fn skill_file_provider(state: &AppServerRouterState) -> Result<Arc<dyn SkillFileProvider>, AppServerError> {
    state.skill_files.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "the skill file read face is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

/// Map a seam-level skill file failure onto its stable wire code.
fn skill_file_error(error: SkillFileError) -> AppServerError {
    match error {
        SkillFileError::NotFound(message) => {
            AppServerError::new("not_found", message, StatusCode::NOT_FOUND, false)
        }
        SkillFileError::InvalidRequest(message) => {
            AppServerError::new("invalid_request", message, StatusCode::BAD_REQUEST, false)
        }
        SkillFileError::TooLarge { size, limit } => AppServerError::new(
            "response_too_large",
            format!("skill file is {size} bytes; the limit is {limit}"),
            StatusCode::PAYLOAD_TOO_LARGE,
            false,
        ),
        SkillFileError::Internal(message) => AppServerError::new(
            "internal_error",
            message,
            StatusCode::INTERNAL_SERVER_ERROR,
            true,
        ),
    }
}

async fn list_skill_files_impl(
    state: &AppServerRouterState,
    skill_id: &str,
) -> Result<AppServerSkillFileList, AppServerError> {
    skill_file_provider(state)?
        .files(skill_id)
        .await
        .map_err(skill_file_error)
}

/// `skill/file`: one skill-relative file's bytes.
///
/// The provider owns path safety (see the trait docs): the protocol layer
/// deliberately does not second-guess the resolved path, because only the
/// provider knows the skill directory it resolved the id to. The size cap is
/// the provider's too — it refuses an oversized file from its metadata before
/// reading it, so nothing large is ever loaded only to be rejected.
async fn read_skill_file_impl(
    state: &AppServerRouterState,
    skill_id: &str,
    path: &str,
) -> Result<SkillFileBytes, AppServerError> {
    skill_file_provider(state)?
        .read(skill_id, path)
        .await
        .map_err(skill_file_error)
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
    principal: Option<&str>,
) -> Result<Vec<AppServerConnectorSummary>, AppServerError> {
    connector_catalog_provider(state)?
        .list(principal)
        .await
        .map_err(AppServerError::from)
}

async fn list_models_impl(
    state: &AppServerRouterState,
) -> Result<AppServerModelList, AppServerError> {
    model_catalog_provider(state)?.list().await.map_err(AppServerError::from)
}

async fn get_connector_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    principal: Option<&str>,
) -> Result<AppServerConnectorDetail, AppServerError> {
    connector_catalog_provider(state)?
        .get(connector_id, principal)
        .await
        .map_err(AppServerError::from)
}

/// `connector/call`: the call-proxy seam (doc 24 §5).
fn connector_call_provider(state: &AppServerRouterState) -> Result<Arc<dyn ConnectorCallProvider>, AppServerError> {
    state.connector_calls.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "the connector call proxy is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

/// Map a seam-level call failure onto its stable wire code.
///
/// `Failed` is the catch-all on purpose: a transport or protocol failure is not
/// something a caller can act on differently, whereas "not allowed", "switched
/// off", "too slow" and "too big" each have a distinct next move.
fn connector_call_error(error: ConnectorCallError) -> AppServerError {
    match error {
        ConnectorCallError::InvalidRequest(message) => {
            AppServerError::new("invalid_request", message, StatusCode::BAD_REQUEST, false)
        }
        ConnectorCallError::NotFound(message) => {
            AppServerError::new("not_found", message, StatusCode::NOT_FOUND, false)
        }
        ConnectorCallError::Unavailable(message) => {
            AppServerError::new("connector_unavailable", message, StatusCode::BAD_REQUEST, false)
        }
        ConnectorCallError::PolicyDenied(message) => {
            AppServerError::new("policy_denied", message, StatusCode::FORBIDDEN, false)
        }
        ConnectorCallError::Timeout { seconds } => AppServerError::new(
            "connector_call_timeout",
            format!("the connector did not answer within {seconds}s"),
            StatusCode::GATEWAY_TIMEOUT,
            true,
        ),
        ConnectorCallError::TooLarge { size, limit } => AppServerError::new(
            "response_too_large",
            format!("the tool result is {size} bytes; the limit is {limit}"),
            StatusCode::PAYLOAD_TOO_LARGE,
            false,
        ),
        ConnectorCallError::Failed(message) => AppServerError::new(
            "connector_call_failed",
            message,
            StatusCode::BAD_GATEWAY,
            true,
        ),
    }
}

async fn connector_call_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    tool: &str,
    arguments: serde_json::Value,
    principal: Option<&str>,
) -> Result<AppServerConnectorCallResult, AppServerError> {
    connector_call_provider(state)?
        .call_for(connector_id, tool, arguments, principal)
        .await
        .map_err(connector_call_error)
}

async fn connector_status_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    principal: Option<&str>,
) -> Result<AppServerConnectorStatusView, AppServerError> {
    connector_catalog_provider(state)?
        .status(connector_id, principal)
        .await
        .map_err(AppServerError::from)
}

/// `connector/credential/*`: the only face that **writes** a credential (`34` §6.1).
fn connector_credential_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ConnectorCredentialProvider>, AppServerError> {
    state.connector_credentials.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "connector credentials are not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

async fn connector_credential_get_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    principal: Option<&str>,
) -> Result<AppServerConnectorCredential, AppServerError> {
    connector_credential_provider(state)?
        .get(connector_id, principal)
        .await
        .map_err(AppServerError::from)
}

async fn connector_credential_set_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    values: std::collections::HashMap<String, String>,
    principal: Option<&str>,
) -> Result<AppServerConnectorCredential, AppServerError> {
    connector_credential_provider(state)?
        .set(connector_id, values, principal)
        .await
        .map_err(AppServerError::from)
}

async fn connector_credential_clear_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    keys: Option<Vec<String>>,
    principal: Option<&str>,
) -> Result<AppServerConnectorCredential, AppServerError> {
    connector_credential_provider(state)?
        .clear(connector_id, keys, principal)
        .await
        .map_err(AppServerError::from)
}

async fn connector_test_impl(
    state: &AppServerRouterState,
    connector_id: &str,
    principal: Option<&str>,
) -> Result<AppServerConnectorProbeResult, AppServerError> {
    connector_catalog_provider(state)?
        .test_for(connector_id, principal)
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

// --- expert definition export (doc 32) -------------------------------------

fn expert_pack_provider(
    state: &AppServerRouterState,
) -> Result<Arc<dyn ExpertPackProvider>, AppServerError> {
    state.expert_packs.clone().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "expert export is not enabled on this App Server",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })
}

/// Map a seam-level export failure onto its stable wire code.
///
/// `policy_denied` is kept distinct from `not_found`, and `agent_not_installed` /
/// `agent_disabled` from both: "you turned this off" and "you never installed
/// this" and "there is no such Definition" are three different answers, exactly
/// as they are for `preset_disabled` / `agent_not_installed` on the run paths.
fn expert_pack_error(error: ExpertPackError) -> AppServerError {
    match error {
        ExpertPackError::PolicyDenied(message) => {
            AppServerError::new("policy_denied", message, StatusCode::FORBIDDEN, false)
        }
        ExpertPackError::NotInstalled(message) => {
            AppServerError::new("agent_not_installed", message, StatusCode::BAD_REQUEST, false)
        }
        ExpertPackError::Disabled(message) => {
            AppServerError::new("agent_disabled", message, StatusCode::BAD_REQUEST, false)
        }
        ExpertPackError::NotFound(message) => {
            AppServerError::new("not_found", message, StatusCode::NOT_FOUND, false)
        }
        ExpertPackError::TooLarge { size, limit } => AppServerError::new(
            "response_too_large",
            format!("expert pack is {size} bytes; the limit is {limit}"),
            StatusCode::PAYLOAD_TOO_LARGE,
            false,
        ),
        ExpertPackError::Internal(message) => AppServerError::new(
            "internal_error",
            message,
            StatusCode::INTERNAL_SERVER_ERROR,
            true,
        ),
    }
}

async fn export_agent_impl(
    state: &AppServerRouterState,
    agent_id: &str,
) -> Result<AppServerExpertPack, AppServerError> {
    expert_pack_provider(state)?
        .export_agent(agent_id)
        .await
        .map_err(expert_pack_error)
}

/// `team/export`, with the `team_version` guard the other team entry points share.
///
/// The version check runs against the **pack's own** version rather than being
/// pushed into the seam: the pack already carries it, so the protocol layer has
/// everything it needs and the seam stays a two-method read face. A mismatch
/// still fails before the caller sees anything, which is the whole point of the
/// guard (`team/run` answers `version_mismatch` for the same input).
async fn export_team_impl(
    state: &AppServerRouterState,
    team_id: &str,
    team_version: Option<&str>,
) -> Result<AppServerExpertPack, AppServerError> {
    let pack = expert_pack_provider(state)?
        .export_team(team_id)
        .await
        .map_err(expert_pack_error)?;
    if let Some(requested) = team_version.map(str::trim)
        && !requested.is_empty()
        && requested != pack.version
    {
        return Err(AppServerError::new(
            "version_mismatch",
            format!(
                "team {team_id} is installed at version {}, not {requested}",
                pack.version
            ),
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    Ok(pack)
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

/// The default-marketplace plan: which sources to register, and whether
/// registering them includes **fetching** them.
///
/// Split out because that single boolean is the whole download policy
/// (`ensure_default_marketplaces`): a source the operator declared is an
/// explicit request and is fetched; the builtin fallback is registered
/// unfetched. Kept pure — same argument, same answer — so the rule is pinned
/// by a test that needs neither a provider nor a database.
fn default_marketplace_plan(path: &std::path::Path) -> (Vec<(String, String, String)>, bool) {
    match AgentStoreConfig::load(path) {
        Ok(config) if !config.default_marketplaces.is_empty() => (
            config
                .default_marketplaces
                .iter()
                .filter_map(|(id, entry)| {
                    let (kind, source) = entry.resolved()?;
                    Some((id.clone(), kind, source))
                })
                .collect(),
            true,
        ),
        Ok(_) | Err(_) => (AgentStoreConfig::builtin_default_marketplaces(), false),
    }
}

/// Register the default marketplace sources for this host. Idempotent (same
/// source returns the existing row). Failures are non-fatal: a broken default
/// source is reported as a warning and the rest keeps working.
///
/// **Two classes, deliberately different** (doc 30 / D-SDK-1 ④):
/// - a source the operator **declared** under `[default_marketplaces.*]` is an
///   explicit request, so it is registered *and fetched* here (network reach
///   bounded by `tokio::time::timeout`);
/// - the **builtin fallback** — no config file, or one that declares no
///   `default_marketplaces` — is registered **without fetching**. The three
///   official archives are 324 MiB together (289.6 MiB of it `experts` alone),
///   and a fresh install that may never open the store used to pay all of it
///   during boot. Those rows land as "registered, not downloaded" and are
///   fetched by an explicit `market/refresh`.
///
/// Returns `true` when every source is registered (or there is nothing to
/// register) and `false` when at least one source could not be registered; the
/// caller uses that to decide whether a later attempt should retry.
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
    // Load the user config. A file that declares sources drives the list *and*
    // opts into the download; a missing/unreadable file, or one with nothing to
    // declare, falls back to the builtin mirrors, registered unfetched.
    let (sources, fetch) = default_marketplace_plan(&path);
    let mut complete = true;
    for (marketplace_id, source_kind, source) in sources {
        // An unknown kind must not be guessed at: `parse` returns `None` and
        // the source is skipped, so a config naming a kind this build does not
        // have is visibly incomplete rather than silently fetched as `url`.
        let Some(kind) =
            nomifun_api_types::AppServerMarketplaceSourceKind::parse(source_kind.as_str())
        else {
            tracing::warn!(
                marketplace_id = %marketplace_id,
                source_kind = %source_kind,
                "skipping default marketplace with an unknown source kind"
            );
            complete = false;
            continue;
        };
        if !fetch {
            // Registry-only: no network, so no timeout and no staging to
            // reclaim. A failure here is a database failure, and `complete`
            // stays clear so the next store/market call retries.
            if provider
                .register_unfetched(&marketplace_id, &marketplace_id, kind.as_str(), &source)
                .await
                .is_err()
            {
                complete = false;
            }
            continue;
        }
        let request = AppServerMarketplaceAddRequest {
            name: Some(marketplace_id.clone()),
            source_kind: kind,
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

async fn list_skill_files_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(skill_id): Path<String>,
) -> Result<Json<AppServerSkillFileList>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_skill_files_impl(&state, &skill_id).await?))
}

/// Serve one skill file as raw bytes.
///
/// Unlike the public display-asset route, this one **requires** a ready
/// connection: it exposes skill bodies and scripts, not `<img>`-referenceable
/// icons. Path safety lives in the provider.
async fn read_skill_file_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path((skill_id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let file = read_skill_file_impl(&state, &skill_id, &path).await?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, file.content_type)],
        file.bytes,
    ))
}

async fn list_connectors_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<AppServerConnectorSummary>>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(list_connectors_impl(&state, Some(user.id.as_str())).await?))
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
    Ok(Json(
        get_connector_impl(&state, &connector_id, Some(user.id.as_str())).await?,
    ))
}

async fn connector_status_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorStatusView>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(
        connector_status_impl(&state, &connector_id, Some(user.id.as_str())).await?,
    ))
}

/// `POST /api/app-server/connectors/{connector_id}/call`
///
/// The connector id is the only addressable thing: the caller supplies a tool
/// name and an argument object, never a URL, a command or a header. Whether the
/// pair is callable at all is the provider's gate, not this handler's.
async fn connector_call_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
    Json(body): Json<WsConnectorCallBody>,
) -> Result<Json<AppServerConnectorCallResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    // Called *as this caller*: credentials are per-principal, and a pooled stdio
    // session is bound to whoever's env spawned it (34 §7).
    Ok(Json(
        connector_call_impl(
            &state,
            &connector_id,
            &body.tool,
            body.arguments,
            Some(user.id.as_str()),
        )
        .await?,
    ))
}

/// Body of the one-shot HTTP binding: same fields as [`WsConnectorCall`],
/// minus the connector id (it is in the path).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConnectorCallBody {
    tool: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

async fn connector_test_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorProbeResult>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    // The probe is made *as this caller*: a connector's credential references are
    // per-principal, so user A's probe must not authenticate with user B's token
    // (34 §7).
    Ok(Json(
        connector_test_impl(&state, &connector_id, Some(user.id.as_str())).await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorCredentialSetBody {
    /// `KEY -> value`. Only keys the connector's own declaration names.
    values: std::collections::HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorCredentialClearBody {
    /// Absent = every secret field of this connector.
    #[serde(default)]
    keys: Option<Vec<String>>,
}

async fn connector_credential_get_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
) -> Result<Json<AppServerConnectorCredential>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(
        connector_credential_get_impl(&state, &connector_id, Some(user.id.as_str())).await?,
    ))
}

async fn connector_credential_set_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
    Json(body): Json<ConnectorCredentialSetBody>,
) -> Result<Json<AppServerConnectorCredential>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    Ok(Json(
        connector_credential_set_impl(
            &state,
            &connector_id,
            body.values,
            Some(user.id.as_str()),
        )
        .await?,
    ))
}

async fn connector_credential_clear_route(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(connector_id): Path<String>,
    body: Option<Json<ConnectorCredentialClearBody>>,
) -> Result<Json<AppServerConnectorCredential>, AppServerError> {
    state.registry.require_ready(connection_id(&headers)?, &user.id)?;
    let keys = body.and_then(|Json(body)| body.keys);
    Ok(Json(
        connector_credential_clear_impl(&state, &connector_id, keys, Some(user.id.as_str())).await?,
    ))
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

/// `team/run` (`docs/agent-store/05` §5.2, `16` §7 决策 3).
///
/// The HTTP arm mirrors `agent_run` exactly — same connection readiness gate, same
/// idempotency scope shape, one method name difference — because a Team Run is the
/// same kind of side effect (one durable run) with a different trigger.
async fn team_run(
    State(state): State<AppServerRouterState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<TeamRunRequest>, JsonRejection>,
) -> Result<Json<TeamRunReceipt>, AppServerError> {
    let connection_id = connection_id(&headers)?;
    let (principal_id, client_id) =
        state.registry.ready_idempotency_context(connection_id, &user.id)?;
    let Json(request) = body.map_err(|error| nomifun_common::AppError::BadRequest(error.to_string()))?;
    let receipt = execute_team_run(&state, &user, &principal_id, &client_id, request).await?;
    Ok(Json(receipt))
}

/// Idempotent wrapper around [`team_run::execute_team_run`].
///
/// Kept structurally identical to `execute_agent_run`: the key is scoped
/// `principal + client + method + key`, the fingerprint excludes the key itself,
/// and only a *successful* run is remembered — a refusal (uninstalled member,
/// disabled Connector, Leader that never delegated) is re-attempted rather than
/// replayed, so a caller that fixes the cause is not stuck with a cached error.
async fn execute_team_run(
    state: &AppServerRouterState,
    user: &CurrentUser,
    _principal_id: &str,
    client_id: &str,
    request: TeamRunRequest,
) -> Result<TeamRunReceipt, AppServerError> {
    let fingerprint_request = TeamRunRequest {
        idempotency_key: None,
        ..request.clone()
    };
    let fingerprint = request_fingerprint(&fingerprint_request)?;
    let scope = request.idempotency_key.as_ref().map(|key| AppServerIdempotencyScope {
        principal_id: user.id.as_str().to_owned(),
        client_id: client_id.to_owned(),
        method: "team/run".to_owned(),
        idempotency_key: key.clone(),
    });
    let _idempotency_guard = if scope.is_some() {
        Some(state.registry.idempotency_lock().await)
    } else {
        None
    };
    if let Some(scope) = scope.as_ref() {
        if let Some(repository) = state.idempotency.as_ref() {
            if let Some(receipt) =
                load_idempotent_response::<TeamRunReceipt>(repository, scope, &fingerprint).await?
            {
                return Ok(receipt);
            }
        } else if let Some(receipt) =
            state.registry.existing_idempotent_team_run(&scope_key(scope), &fingerprint)?
        {
            return Ok(receipt);
        }
    }

    let receipt = team_run::execute_team_run(state, user, request).await?;
    if let Some(scope) = scope {
        if let Some(repository) = state.idempotency.as_ref() {
            return commit_idempotent_response(repository, scope, fingerprint, &receipt).await;
        }
        return Ok(state
            .registry
            .remember_idempotent_team_run(&scope_key(&scope), fingerprint, receipt)?);
    }
    Ok(receipt)
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
    // doc `29` §6.1：思考等级先校验。这只是词表判定（`low`/`medium`/`high`/`xhigh`，空串＝不指定），
    // 一个坏值不该等到快照解析完、模板物化之后才报。
    let reasoning_effort = normalize_reasoning_effort(request.reasoning_effort)?;
    let preset = preset_service.get(&resolved_preset_id).await?;
    validate_agent_store_preset_source(preset.source, preset.source_key.as_deref(), Some(&preset.name))?;
    // A Preset that `install/disable` switched off must be named as such.
    // `resolve` refuses it too, but with a generic message; the code is what a
    // client branches on, and "you turned this off" is not "this is broken".
    if !preset.enabled {
        return Err(AppServerError::new(
            "preset_disabled",
            format!("preset {resolved_preset_id} is disabled"),
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let mut snapshot = preset_service
        .resolve(
            &resolved_preset_id,
            PresetTarget::ExecutionStep,
            None,
            overrides.clone(),
        )
        .await?;
    // doc `29` §6.2：调用方显式给的模型**无条件**赢过 preset 自带的那个，所以先解析它并重新
    // resolve 一次。手法与下面的宿主默认回退**完全相同**（把模型并进 `overrides` 再解析一遍），
    // 因此走的是同一条权威校验，且每个 mention override 都保住。
    if let Some(model) = request.model {
        let model = resolve_app_server_model(state, Some(model.into_provider_with_model())).await?;
        let preference = team_run::provider_model_preference(&model);
        snapshot = preset_service
            .resolve(
                &resolved_preset_id,
                PresetTarget::ExecutionStep,
                None,
                with_model(overrides, &preference),
            )
            .await?;
    } else if snapshot.resolved_model.is_none() {
        // Installed agent-store presets are created without a model binding
        // (the definition payload has no model mandate). A run without a
        // resolved model is rejected at the runtime boundary, so fall back to
        // the owner's first enabled provider/model when the preset left the
        // model unbound. The fallback must keep every mention override
        // (`include_skills`, `mcp_server_ids`, ...): re-resolving from
        // `PresetOverrides::default()` silently drops them.
        if let Some(model) = default_run_model(state).await? {
            let retry = preset_service
                .resolve(
                    &resolved_preset_id,
                    PresetTarget::ExecutionStep,
                    None,
                    with_model(overrides, &model),
                )
                .await?;
            snapshot = retry;
        }
    }
    // doc `29` §6.3：运行级思考等级挂在**快照**上（免迁移的 JSON 载体），由 attempt runner
    // 投影进尝试会话的 `extra.reasoning_effort`；preset 解析自己永不设置这个字段。
    snapshot.reasoning_effort = reasoning_effort;
    validate_nomi_runtime_type(snapshot.resolved_agent_type.as_deref())?;
    // Preset MCP references must exist and be enabled before the run starts.
    // The attempt runner projects them into the attempt conversation later;
    // silently dropping a missing/disabled connector would hide a pinned
    // dependency, so reject up-front with `connector_unavailable`.
    if !snapshot.mcp_server_ids.is_empty() {
        let connectors = connector_catalog_provider(state)?;
        for mcp_server_id in &snapshot.mcp_server_ids {
            let detail = connectors.get(mcp_server_id, None).await.map_err(AppServerError::from)?;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// doc `27` §5.2：把「这个会话是谁」绑定成一个**专家**（AgentDefinition 的 `agent/list` id）。
    ///
    /// 解析走 `agent/run` 的同一套语义（未安装 → `agent_not_installed`、被停用 → `preset_disabled`、
    /// 来源白名单），并且**只在创建时**生效：会话的 preset 快照此后只读，换专家 = 新建会话。
    /// 专家的技能与连接器随之冻结（见 `app_server_chat_bindings_for_agent`）。
    #[serde(default)]
    pub agent_id: Option<String>,
    /// doc `27` §5.3：把会话建成某个**专家团**的 Leader（`team/list` 的 id）。
    ///
    /// 与 `team/run` 共用同一段编排（成员校验 → 物化/复用模板 → 建 Leader 会话），区别只有一处：
    /// **不发 `goal` 首轮**——第一句话由客户端自己说。与 `agent_id` **互斥**（同时给是
    /// `invalid_request`：会话要么是某个专家，要么是某个团的 Leader）。
    #[serde(default)]
    pub team_id: Option<String>,
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
    /// R15（W10）：本案会话工作区内的**绝对**文件路径（图片附件）。
    ///
    /// 载体＝**路径引用**（用户 2026-09-12 拍板）。准入在
    /// `resolve_conversation_attachments`：必须是会话工作区内的真实文件，相对路径 /
    /// 越界 / 不存在的引用一律拒绝。`#[serde(default)]` 保持纯加法——老客户端不传
    /// 该字段时行为逐字不变（`files` 仍是空）。
    #[serde(default)]
    pub attachments: Vec<String>,
    /// doc `27` 阶段 1：**本轮挂载的技能**，形状与 `agent/run` 的 `mentions` 一致。
    ///
    /// 只接受 `kind: "skill"`。连接器是宿主的工具面开关、专家是会话身份，两者都没有
    /// 随消息走的载体，所以这里**显式拒绝**（`invalid_request`）而不是静默忽略——
    /// 静默忽略会让调用方以为自己挂上了。`#[serde(default)]` 保持纯加法。
    #[serde(default)]
    pub mentions: Vec<MentionRef>,
    /// doc `29` §5.1：**从本轮起**生效的模型（会话级设置，被这条消息顺带切换）。
    ///
    /// 与会话级模型同一形状、同一解析（`ConversationModelRef` + `resolve_app_server_model`）：
    /// `provider_id` 可以是已注册的 provider UUID，也可以是 `~/.agent-store/config.toml` 里
    /// `[providers.<key>]` 的名字（服务端幂等注册后改写为规范 UUID）。
    ///
    /// **没有"只影响这一轮"的模式**：值写进会话行，此后每轮沿用；要还原就再发一次带旧值的
    /// 调用。真·一次性需要引擎级的每轮模型通道，而运行时是会话级、按行构建的（`29` §4.2）。
    #[serde(default)]
    pub model: Option<ConversationModelRef>,
    /// doc `29` §5.1：**从本轮起**生效的 OpenAI 风格思考等级（`low`/`medium`/`high`/`xhigh`）。
    ///
    /// 与 `conversation/create` / `update` 共用 `normalize_reasoning_effort`（空串＝不指定）。
    /// 同为粘性。引擎是否**真的**用上取决于 catalog 是否为该模型声明了等级，没声明时静默
    /// 退回模型默认——与 create/update 同一口径（`29` §9.3）。
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// R15：单次发送允许的附件条数上限。
///
/// 与运行时自己的图片上限（`MAX_IMAGE_ATTACHMENTS = 10`）取同一个数：这里先拦一道
/// 是为了**在碰文件系统之前**拒绝明显过量的请求，而不是替运行时做类型判定。
const MAX_CONVERSATION_ATTACHMENTS: usize = 10;

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
    /// doc `29` §5.5：该会话当前的思考等级（`conversation.extra.reasoning_effort` 的纯投影）。
    ///
    /// 缺席＝未指定（引擎按模型默认走）。**必须能读回**：`create` / `update` / `send` 三条路
    /// 都能写等级，若这里不投影，等级就是一个只写不读的设置，调用方无法确认自己设了什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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
///
/// `last_turn_input_tokens` / `last_turn_output_tokens` (W9 / R14 ③) are the
/// runtime's own report for the most recent completed turn, read back from the
/// durable snapshot so a reloaded WebUI can still render last turn's tokens and
/// catalog-priced cost without replaying the event stream. Both stay off the
/// wire entirely when the row has no value — an unreported turn must not arrive
/// as `0`, and occupancy (`used_tokens`) is never substituted for them.
#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageView {
    pub used_tokens: i64,
    pub window_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_output_tokens: Option<i64>,
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
        last_turn_input_tokens: row.last_turn_input_tokens,
        last_turn_output_tokens: row.last_turn_output_tokens,
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

/// The conversation's persisted reasoning effort (`extra.reasoning_effort`).
///
/// One reader for both consumers (doc `29`): the `ConversationView` projection (§5.5) and the
/// `conversation/send` difference check (§5.2). A blank value reads as "not specified" — the same
/// normalization `normalize_reasoning_effort` applies on the way in, so a stored `""` never
/// differs from an absent key and never triggers a pointless write.
fn conversation_extra_reasoning_effort(extra: &serde_json::Value) -> Option<String> {
    extra
        .get("reasoning_effort")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
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
        reasoning_effort: conversation_extra_reasoning_effort(&conversation.extra),
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
    let agent_id = request
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let team_id = request
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if agent_id.is_some() && team_id.is_some() {
        return Err(AppServerError::new(
            "invalid_request",
            "agent_id and team_id are mutually exclusive: a conversation is opened either as an expert or as a team's leader",
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    // doc `27` §5.3：专家团走 `team/run` 的同一段编排，但**不发 goal 首轮**——Leader 的第一句话
    // 由客户端自己说。这条路径自带工作区与模型解析（与 `team/run` 同源），所以先于下面的共用解析返回。
    if let Some(team_id) = team_id {
        let prepared = team_run::prepare_team_leader_conversation(
            state,
            user,
            team_id,
            None,
            request.workspace.as_ref(),
        )
        .await?;
        return project_conversation_view(state, prepared.conversation).await;
    }
    let workspace = resolved_chat_workspace(state, user, request.workspace.as_ref()).await?;
    let model = resolve_app_server_model(
        state,
        request.model.map(ConversationModelRef::into_provider_with_model),
    )
    .await?;
    let reasoning_effort = normalize_reasoning_effort(request.reasoning_effort)?;
    let workspace_id = workspace.workspace_id().to_owned();
    // doc `27` §5.2：绑定的专家（若有）在这里解析成「定义自带的技能 / 连接器 + 已解析 preset 快照」。
    // 没有 `agent_id` 时是空绑定——与既有行为逐字一致。
    let bindings = app_server_chat_bindings_for_agent(state, agent_id).await?;
    let conversation = conversation_service(state)?
        .create_app_server_nomi_chat(
            user.id.as_str(),
            request.name,
            model,
            workspace.path().to_string_lossy().into_owned(),
            Some(workspace_id),
            reasoning_effort,
            bindings,
        )
        .await
        .map_err(AppServerError::from)?;
    project_conversation_view(state, conversation).await
}

/// The create-time bindings of one App Server chat (doc `27` §5.2).
///
/// `agent_id` is an **AgentDefinition** id: its installed `preset_id` becomes the conversation's
/// identity, and the Definition's own Skills and Connectors become the conversation's fences.
/// Resolution mirrors `agent/run` — not installed, disabled, or a non agent-store preset source all
/// fail here rather than producing a chat that quietly runs as somebody else.
///
/// Everything is frozen at creation: `ConversationService::update` refuses preset / skill / MCP
/// keys afterwards, so **changing the expert means creating another conversation**.
async fn app_server_chat_bindings_for_agent(
    state: &AppServerRouterState,
    agent_id: Option<&str>,
) -> Result<AppServerChatBindings, AppServerError> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        // No Definition: an empty fence is "bind nothing", which is the pre-existing behaviour.
        return Ok(AppServerChatBindings::default());
    };
    let agent = agent_catalog_provider(state)?
        .get(agent_id)
        .await
        .map_err(AppServerError::from)?;
    let Some(preset_id) = agent.summary.preset_id.as_deref() else {
        return Err(AppServerError::new(
            "agent_not_installed",
            format!("agent {agent_id} is not installed; run install/* before creating a conversation with it"),
            StatusCode::BAD_REQUEST,
            false,
        ));
    };
    let preset_service = state.preset_service.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server Preset service is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;
    let preset = preset_service.get(preset_id).await?;
    validate_agent_store_preset_source(preset.source, preset.source_key.as_deref(), Some(&preset.name))?;
    // A Preset that `install/disable` switched off must be named as such, exactly as `agent/run`
    // does — "you turned this off" is not "this is broken".
    if !preset.enabled {
        return Err(AppServerError::new(
            "preset_disabled",
            format!("preset {preset_id} is disabled"),
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let connector_ids = definition_connector_fence(state, agent_id, &agent.summary.connectors).await?;
    // The Definition's Skills and Connectors ride in as **resolve overrides**: the resolved snapshot
    // (not the raw preset) is what `create` freezes into `preset_enabled_skills`. The auto-inject
    // exclusion is added by the conversation seam, which owns that knowledge.
    let overrides = PresetOverrides {
        include_skills: agent.summary.skills.clone(),
        mcp_server_ids: Some(connector_ids.iter().map(|id| id.as_str().to_owned()).collect()),
        ..PresetOverrides::default()
    };
    let snapshot = preset_service
        .resolve(
            preset_id,
            nomifun_api_types::PresetTarget::Conversation,
            None,
            overrides,
        )
        .await?;
    Ok(AppServerChatBindings {
        connector_ids,
        skill_names: agent.summary.skills.clone(),
        preset_snapshot: Some(snapshot),
    })
}

/// Validate a Definition's declared Connectors and return them as canonical ids.
///
/// Same rule as the Team fence (`team_run.rs`): a declared Connector that is disabled must surface
/// as `connector_unavailable` instead of silently binding nothing — a pinned dependency that is
/// missing has to be visible.
async fn definition_connector_fence(
    state: &AppServerRouterState,
    definition_id: &str,
    declared: &[String],
) -> Result<Vec<McpServerId>, AppServerError> {
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let connectors = connector_catalog_provider(state)?;
    let mut fence: Vec<McpServerId> = Vec::with_capacity(declared.len());
    for raw in declared {
        let id = McpServerId::parse(raw).map_err(|error| {
            AppServerError::new(
                "internal_error",
                format!("agent {definition_id} declares an invalid connector id: {error}"),
                StatusCode::INTERNAL_SERVER_ERROR,
                false,
            )
        })?;
        let detail = connectors
            .get(id.as_str(), None)
            .await
            .map_err(AppServerError::from)?;
        if !detail.summary.enabled {
            return Err(AppServerError::new(
                "connector_unavailable",
                format!("connector {raw} bound by the agent is disabled"),
                StatusCode::BAD_REQUEST,
                false,
            ));
        }
        if !fence.contains(&id) {
            fence.push(id);
        }
    }
    Ok(fence)
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
    let provider_id = ensure_agent_store_provider(
        provider_service,
        state.provider_model_service.as_deref(),
        &config,
        &provider_key,
        Some(&model_name),
    )
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

/// The declaration file that sits next to the resolved config file.
///
/// One function for the read face and both write faces: they must never be able
/// to disagree about which file they mean.
fn mcp_declaration_path(config_path: &std::path::Path) -> std::path::PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .join("mcp.json")
}

/// Project the parsed file into its wire view. Sorted, credential-free, and no
/// fact that is not actually in the file.
///
/// `mcp` is the projection of the **sibling** `mcp.json` (`20` §7.9 / `21` D14),
/// resolved by [`mcp_declaration_view`] from the same config path so a host
/// pointed at a custom directory reports the pair it actually reads.
fn config_view(
    config: &AgentStoreConfig,
    exists: bool,
    mcp: Option<AppServerConfigMcpView>,
) -> AppServerConfigView {
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
        // Same absent/present distinction for `[tools]`: a present table is
        // reported with its defaults filled in, so `Some(all-on)` means "the
        // file declares `[tools]` but constrains nothing".
        tools: config.tools.clone(),
        mcp,
    }
}

/// Read the `mcp.json` sibling of `config_path` into its read view.
///
/// `None` means the file is absent **or unreadable**: the settings surface must
/// not fail because a declaration file is broken, and a broken file declares
/// nothing anyway. A present-but-unparseable file projects to
/// `Some { exists: true, .. }` with empty lists rather than an error, so the
/// settings screen stays reachable while the startup log carries the reason.
///
/// `adopted` is the host's own answer ([`AppServerRouterState::adopt_store_mcp_declarations`])
/// and is carried through untouched: this function reports the file, the state
/// reports whether the host uses it, and a client needs both to avoid reading
/// "declared here" as "in force here".
fn mcp_declaration_view(
    config_path: &std::path::Path,
    adopted: Option<bool>,
) -> Option<AppServerConfigMcpView> {
    let path = mcp_declaration_path(config_path);
    let source = std::fs::read_to_string(&path).ok()?;
    let (servers, rejected, error) = match nomifun_api_types::NomiMcpDeclarations::parse(&source) {
        Ok(declarations) => (
            declarations
                .servers
                .iter()
                .map(|server| AppServerConfigMcpServerView {
                    name: server.name.clone(),
                    transport: server.transport_kind().to_owned(),
                    enabled: server.enabled,
                })
                .collect(),
            declarations
                .rejected
                .iter()
                .map(|rejection| AppServerConfigMcpRejectionView {
                    name: rejection.name.clone(),
                    reason: rejection.reason.clone(),
                })
                .collect(),
            None,
        ),
        // A file-level failure is surfaced rather than swallowed: an empty list
        // with no reason is indistinguishable from "the file declares nothing".
        Err(error) => (Vec::new(), Vec::new(), Some(error)),
    };
    Some(AppServerConfigMcpView {
        exists: true,
        adopted,
        servers,
        rejected,
        error,
    })
}

/// `config/get`: the settings file as a wire view.
///
/// A **missing** file is a normal answer (`exists: false`, empty defaults) —
/// never an error, never a fabricated default. An unreadable or unparseable
/// file *is* an error: silently answering with defaults would hide a broken
/// hand-edit behind a settings screen that looks healthy.
fn execute_config_get(state: &AppServerRouterState) -> Result<AppServerConfigView, AppServerError> {
    let path = agent_store_config_file(state)?;
    let mcp = mcp_declaration_view(&path, state.adopt_store_mcp_declarations);
    match std::fs::read_to_string(&path) {
        Ok(source) => {
            let config = AgentStoreConfig::from_source(&source).map_err(|error| {
                config_unavailable(format!("failed to read {}: {error}", path.display()))
            })?;
            Ok(config_view(&config, true, mcp))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(config_view(&AgentStoreConfig::default(), false, mcp))
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
            "config/set needs at least one whitelisted key (default_model, memory.distill_enabled, tools.*)",
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
    if let Some(tools) = patch.tools.as_ref() {
        // The `[tools]` write is likewise unconditional on parsing: every field
        // is a bool or a validated pattern list, so there is no "resolution will
        // fail later" case to refuse here. The policy is read at startup, so a
        // write here changes the next launch's tool surface (and the read view
        // reports it back immediately).
        edited = tools.apply_edits(&edited).map_err(|error| {
            AppServerError::new("invalid_request", error, StatusCode::BAD_REQUEST, false)
        })?;
    }
    write_config_source(&path, &edited)?;
    execute_config_get(state)
}

/// `config/get-mcp`: the declaration file's raw text, for the file editor.
///
/// Missing file = `exists: false` (a normal answer, exactly like
/// `config/get`'s). Unreadable = an error rather than an empty string: an editor
/// that silently starts from "" would overwrite a file it merely could not read
/// (`21` D17).
fn execute_config_get_mcp(
    state: &AppServerRouterState,
) -> Result<AppServerMcpSourceView, AppServerError> {
    let path = mcp_declaration_path(&agent_store_config_file(state)?);
    match std::fs::read_to_string(&path) {
        Ok(source) => Ok(AppServerMcpSourceView { exists: true, source: Some(source) }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AppServerMcpSourceView { exists: false, source: None })
        }
        Err(error) => Err(config_unavailable(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

/// `config/set-mcp`: replace the declaration file with the text the operator
/// supplied, after proving the host can read it back.
///
/// **Fail-closed, unlike the read face.** `config/get` reports an unreadable
/// file as a view (a file-level `error`, empty lists) so a broken hand-edit
/// stays visible rather than being silently ignored; a *write* of something the
/// parser refuses would create exactly that state, so it is refused with the
/// parser's own reason (which carries the line and column) and **nothing is
/// written**. The response is the same re-read `config/get` view the other write
/// faces return, so the caller sees the landing spot.
fn execute_config_set_mcp(
    state: &AppServerRouterState,
    source: String,
) -> Result<AppServerConfigView, AppServerError> {
    if let Err(reason) = nomifun_api_types::NomiMcpDeclarations::parse(&source) {
        return Err(AppServerError::new(
            "mcp_source_invalid",
            format!("mcp.json was not written: {reason}"),
            StatusCode::BAD_REQUEST,
            false,
        ));
    }
    let path = mcp_declaration_path(&agent_store_config_file(state)?);
    write_source_atomically(&path, &source, "json.tmp").map_err(|message| {
        AppServerError::new("mcp_write_failed", message, StatusCode::SERVICE_UNAVAILABLE, false)
    })?;
    execute_config_get(state)
}

/// `config/set-mcp-enabled`: flip one entry's `enabled` member **in place**.
///
/// The edit itself lives in `nomifun_api_types::set_server_enabled_in_source`,
/// which touches only that member's value and refuses (rather than reformats)
/// anything it cannot locate unambiguously — this file is hand-written, and a
/// switch has no business re-indenting it. An edit that changes no bytes is not
/// written at all, so a repeated call leaves the file's mtime alone.
fn execute_config_set_mcp_enabled(
    state: &AppServerRouterState,
    name: String,
    enabled: bool,
) -> Result<AppServerConfigView, AppServerError> {
    let path = mcp_declaration_path(&agent_store_config_file(state)?);
    let source = std::fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AppServerError::new(
                "mcp_server_not_declared",
                format!("{} does not exist, so `{name}` is not declared", path.display()),
                StatusCode::NOT_FOUND,
                false,
            )
        } else {
            AppServerError::new(
                "mcp_write_failed",
                format!("failed to read {}: {error}", path.display()),
                StatusCode::SERVICE_UNAVAILABLE,
                false,
            )
        }
    })?;

    let edited = nomifun_api_types::set_server_enabled_in_source(&source, &name, enabled)
        .map_err(|failure| match failure {
            nomifun_api_types::McpSourceEditError::NotDeclarations(reason) => AppServerError::new(
                "mcp_source_invalid",
                format!("{} is not a valid declaration file: {reason}", path.display()),
                StatusCode::BAD_REQUEST,
                false,
            ),
            nomifun_api_types::McpSourceEditError::ServerMissing => AppServerError::new(
                "mcp_server_not_declared",
                format!("`{name}` is not declared in {}", path.display()),
                StatusCode::NOT_FOUND,
                false,
            ),
            nomifun_api_types::McpSourceEditError::ServerRejected => AppServerError::new(
                "mcp_server_rejected",
                format!("`{name}` is declared but the host refused it — fix it in the file editor"),
                StatusCode::BAD_REQUEST,
                false,
            ),
            nomifun_api_types::McpSourceEditError::NotSurgicallyEditable => AppServerError::new(
                "mcp_source_not_surgically_editable",
                "this file's formatting cannot be edited by a switch safely — use the file editor",
                StatusCode::BAD_REQUEST,
                false,
            ),
        })?;

    if edited != source {
        write_source_atomically(&path, &edited, "json.tmp").map_err(|message| {
            AppServerError::new("mcp_write_failed", message, StatusCode::SERVICE_UNAVAILABLE, false)
        })?;
    }
    execute_config_get(state)
}

/// Minimal, atomic write: a sibling temp file replaces the target (`rename`
/// overwrites on Windows too), so an interrupted save can never leave a
/// half-written file behind. `temp_extension` keeps two files' writers from
/// racing on one temp name. Errors come back as prose; each caller maps them to
/// its own stable code.
fn write_source_atomically(
    path: &std::path::Path,
    source: &str,
    temp_extension: &str,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let temp = path.with_extension(temp_extension);
    std::fs::write(&temp, source)
        .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("failed to write {}: {error}", path.display())
    })
}

/// Minimal, atomic write of `config.toml`.
fn write_config_source(path: &std::path::Path, source: &str) -> Result<(), AppServerError> {
    write_source_atomically(path, source, "toml.tmp").map_err(config_unavailable)
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

/// Persist the agent-store per-model fields that have no provider-level map
/// column: `max_output_size` → `provider_models.output_limit`, and the model's
/// `protocol` → `provider_models.protocol`.
///
/// The settings UI writes these two through the same row-level face
/// (`providerModel.update`), which is why this path uses it too rather than
/// growing `CreateProviderParams`: one column, one write route.
///
/// **Fill-in-only, never overwrite.** A row whose column already holds a value
/// is left exactly as it is; only a NULL column is seeded from the config. Two
/// reasons: `resolve_app_server_model` runs on every model resolution, so this
/// must be idempotent; and the error message that motivated this fix tells the
/// operator to set the ceiling in Settings → Models, so re-asserting the
/// config value afterwards would silently undo their edit.
///
/// Best-effort by design, matching `ProviderService::seed_inferred_profiles_best_effort`:
/// the provider row is already committed, so a failed per-model write must not
/// turn the whole resolution into an error. The anthropic build guard still
/// reports a genuinely missing ceiling with an actionable message.
async fn reconcile_agent_store_model_limits(
    provider_model_service: Option<&ProviderModelService>,
    provider: &nomifun_api_types::ProviderResponse,
    config: &AgentStoreConfig,
    provider_key: &str,
) {
    let Some(service) = provider_model_service else {
        return;
    };
    let output_limits = config.output_limits_for_provider(provider_key);
    let protocols = config.protocols_for_provider(provider_key);
    if output_limits.is_empty() && protocols.is_empty() {
        return;
    }

    // `ProviderResponse.models_detail` is the row-level projection, so the
    // current stored value is read from the response we already have — no
    // extra query, and "already set" is decided against the real column.
    for row in &provider.models_detail {
        let output_limit = output_limits
            .get(&row.model)
            .filter(|_| row.output_limit.is_none())
            .map(|limit| Some(*limit));
        let protocol = protocols
            .get(&row.model)
            .filter(|_| row.protocol.is_none())
            .map(|protocol| Some(protocol.clone()));
        if output_limit.is_none() && protocol.is_none() {
            continue;
        }

        // Only the two fields this function owns are named; everything else is
        // "keep the current value" (absent = keep for every column here).
        let request = UpdateProviderModelRequest {
            provider_id: provider.provider_id.clone(),
            model: row.model.clone(),
            enabled: None,
            sort_order: None,
            tasks: None,
            traits: None,
            protocol,
            connection_role: None,
            params: None,
            context_limit: None,
            output_limit,
            description: None,
        };
        if let Err(error) = service.update(request).await {
            tracing::warn!(
                provider_id = %provider.provider_id,
                model = %row.model,
                %error,
                "failed to persist agent-store model output ceiling/protocol"
            );
        }
    }
}

/// Register (idempotently) the provider named `provider_key` from the
/// agent-store config, returning its canonical provider UUID. Registration
/// reuses `ProviderService::create` so encryption and `provider_models`
/// reconciliation are identical to the normal Allo provider UI path. A
/// previously registered provider with the same `name` is reused as-is — but
/// its per-model `output_limit`/`protocol` columns are still reconciled, so a
/// row registered by an earlier build (which dropped both) is repaired here.
async fn ensure_agent_store_provider(
    provider_service: &ProviderService,
    provider_model_service: Option<&ProviderModelService>,
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
        // Reused row, but not necessarily a complete one: a provider
        // registered before the output ceiling was wired up still carries
        // NULL `output_limit`, which fails an `anthropic` build. Repair it
        // here so every resolution path heals the row it is about to use.
        reconcile_agent_store_model_limits(
            provider_model_service,
            &existing,
            config,
            provider_key,
        )
        .await;
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
            // Seeds fresh membership rows only (`replace = false`), exactly like
            // the context-limit map above; an empty map and `None` are
            // equivalent on create, so a config declaring no protocol is
            // unaffected.
            model_protocols: Some(config.protocols_for_provider(provider_key)),
            model_descriptions: Some(config.display_names_for_provider(provider_key)),
            model_enabled: None,
            model_health: None,
            bedrock_config: None,
            is_full_url: false,
            sort_order: None,
        })
        .await
        .map_err(AppServerError::from)?;
    // The provider DTO has no output-limit map column, so the ceiling is
    // written through the row-level face after the row exists.
    reconcile_agent_store_model_limits(
        provider_model_service,
        &created,
        config,
        provider_key,
    )
    .await;
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
    let conversation = service
        .get_app_server_chat(user.id.as_str(), conversation_id)
        .await
        .map_err(AppServerError::from)?;
    // R15（W10）：附件准入在宿主这一层完成——运行时对 App Server 会话的
    // `image_read_root` 是 `None`（不受限），所以边界不放这里就等于没有。
    let files =
        resolve_conversation_attachments(state, user, &conversation.extra, &request.attachments)
            .await?;
    // doc `27` §4.1：本轮的技能来自结构化 mention（只认 skill）。解析沿用会话层既有
    // 语义——`inject_skills` 会在占用 durable receipt **之前**被解析成不可变快照，
    // 缺失/超限的技能让整次 send 失败，而不是悄悄少挂一个。
    let inject_skills = send_mention_skills(&request.mentions)?;
    // doc `29` §5.2：模型 / 思考等级的**粘性**切换排在所有可预期拒绝之后、真正发送之前。
    // 解析先做（纯校验 + provider 幂等注册，不写库），落库交给下面那个 helper。
    let preferred_model = match request.model {
        Some(model) => {
            Some(resolve_app_server_model(state, Some(model.into_provider_with_model())).await?)
        }
        None => None,
    };
    let preferred_effort = normalize_reasoning_effort(request.reasoning_effort)?;
    apply_send_preferences(state, user, &conversation, preferred_model, preferred_effort).await?;
    let delivery = service
        .send_message_with_idempotency_key(
            user.id.as_str(),
            conversation_id,
            &request.idempotency_key,
            SendMessageRequest {
                content: request.content,
                files,
                inject_skills,
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

/// `conversation/send` 的模型 / 思考等级切换（doc `29` §5.2）。
///
/// 语义是**粘性**的：值落进会话行，**从本轮起**生效，此后每轮沿用——没有"只这一轮"的模式。
/// 三件事的顺序本身就是契约：
///
/// 1. **忙判定在最前**：`ConversationService::update` 换模型会**立即拆运行时**，而拆运行时
///    不看有没有在跑的 turn；send 的准入随后又会以 `Conflict` 拒绝（本地 turn owner，或未证明
///    的 durable running generation）。先落库再被拒 = 用户同时丢掉正在跑的回合和这条消息。
/// 2. **差异判定**：与行上现值全相同就完全不写库、不广播。不带新参数的调用必须**零副作用**——
///    这条不能靠 `#[serde(default)]` 自己成立。
/// 3. **复用 `update`**：它是权威 seam（模型权威校验、preset/skill/MCP 冻结、执行尝试会话拒绝）。
///    另写一条"只改模型/等级"的路等于让模型有两个写入点。
///
/// 残余窗口（如实记录，`29` §5.2）：落库与 turn 准入不在同一个 preparation gate 之下，所以
/// 内部错误仍可能造成"配置已切换、消息未发出"。可预期拒绝已全部前置。
async fn apply_send_preferences(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation: &nomifun_api_types::ConversationResponse,
    model: Option<ProviderWithModel>,
    reasoning_effort: Option<String>,
) -> Result<(), AppServerError> {
    if model.is_none() && reasoning_effort.is_none() {
        return Ok(());
    }
    // 1. 忙判定：两条都要判。`Running` 覆盖 durable running generation（`service.rs:4135`），
    //    `is_processing` 覆盖本地 turn owner（`service.rs:4129`）。宁可 fail-closed 多拒一次，
    //    也不要拆掉一个可能还在跑的运行时。
    if conversation.status == nomifun_common::ConversationStatus::Running
        || conversation
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.is_processing)
    {
        return Err(AppServerError::from(nomifun_common::AppError::Conflict(
            "the conversation is processing a turn; model and reasoning_effort can only be switched while it is idle"
                .to_owned(),
        )));
    }
    // 2. 差异判定：一项都没变就什么都不做（不写库、不广播）。
    let (model_changed, effort_changed) = send_preference_changes(
        model.as_ref(),
        conversation.model.as_ref(),
        reasoning_effort.as_deref(),
        conversation_extra_reasoning_effort(&conversation.extra).as_deref(),
    );
    if !model_changed && !effort_changed {
        return Ok(());
    }
    // 3. 落库：只带真正变化的那一项，避免 `update` 把未变化的值再写一遍。
    let extra = if effort_changed {
        reasoning_effort.map(|effort| serde_json::json!({ "reasoning_effort": effort }))
    } else {
        None
    };
    conversation_service(state)?
        .update(
            user.id.as_str(),
            &conversation.conversation_id,
            nomifun_api_types::UpdateConversationRequest {
                name: None,
                pinned: None,
                model: if model_changed { model } else { None },
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
    Ok(())
}

/// 差异判定的纯函数形式（doc `29` §5.2）：`(model_changed, effort_changed)`。
///
/// 抽出来是为了让「不带新参数的调用**不写库、不广播**」这条不变量能被单测直接钉住——
/// 否则它只能靠一个需要整套 `ConversationService` 的集成测试来保证，而那条路在本 crate 里
/// 没有夹具（见 `29` §10.1 的验收说明）。
///
/// `None` 一律表示"调用方没提这一项"，因此不算变化：只有**明确给出且与现值不同**才算。
fn send_preference_changes(
    requested_model: Option<&ProviderWithModel>,
    current_model: Option<&ProviderWithModel>,
    requested_effort: Option<&str>,
    current_effort: Option<&str>,
) -> (bool, bool) {
    (
        requested_model.is_some_and(|requested| current_model != Some(requested)),
        requested_effort.is_some_and(|requested| current_effort != Some(requested)),
    )
}

/// `conversation/send` 的 mention 准入（doc `27` §4.1）。
///
/// 阶段 1 **只认 `skill`**：技能是每轮载荷（`inject_skills` + 不可变快照），
/// 而连接器是宿主的工具面开关、专家是会话身份——两者都没有随消息走的载体。
/// 因此另外两类是显式 `invalid_request`，不是静默忽略：调用方必须知道
/// "这一轮没挂上"，否则它会以为自己挂上了。
fn send_mention_skills(mentions: &[MentionRef]) -> Result<Vec<String>, AppServerError> {
    let mut skills: Vec<String> = Vec::new();
    for mention in mentions {
        match mention.kind {
            MentionKind::Skill => {
                if mention.id.trim().is_empty() {
                    return Err(send_mention_rejection("a skill mention must name a skill"));
                }
                // 同一轮重复点同一技能只算一次（会话层也会去重；这里让 payload 规范）。
                if !skills.contains(&mention.id) {
                    skills.push(mention.id.clone());
                }
            }
            MentionKind::Agent => {
                return Err(send_mention_rejection(
                    "`agent` mentions are not accepted by conversation/send: an expert is bound when the conversation is created, not per turn",
                ));
            }
            MentionKind::Connector => {
                return Err(send_mention_rejection(
                    "`connector` mentions are not accepted by conversation/send: a connector is enabled on the host, not selected per turn",
                ));
            }
        }
    }
    Ok(skills)
}

fn send_mention_rejection(message: &str) -> AppServerError {
    AppServerError::new(
        "invalid_request",
        message.to_owned(),
        StatusCode::BAD_REQUEST,
        false,
    )
}

/// R15（W10）附件准入：把客户端给的绝对路径收敛成「**本会话工作区内**的真实文件」。
///
/// 为什么边界必须在这里：运行时对 App Server 会话的 `image_read_root` 取的是
/// `NomiBuildExtra.write_root`，而宿主没有设置它（`None` = 不受限）。也就是说
/// 引擎不会替我们收紧——一个远程 WebUI 客户端如果能直接指定任意绝对路径，就能把
/// 宿主上的文件读进模型上下文。所以规则只有一条：**canonicalize 之后必须仍在会话
/// 工作区根之内、且目标是文件**；`..`、指向外部的符号链接、别的盘、相对路径、URL
/// 一律拒绝，不做「尽力而为」的降级。
///
/// 返回的是**客户端原本给的字符串**（不是 canonical 形态）：Windows 的
/// `canonicalize` 会带 `\\?\` 前缀，而运行时要求的是普通绝对路径；canonical 形态
/// 只用来判界与去重。
async fn resolve_conversation_attachments(
    state: &AppServerRouterState,
    user: &CurrentUser,
    conversation_extra: &serde_json::Value,
    attachments: &[String],
) -> Result<Vec<String>, AppServerError> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }
    if attachments.len() > MAX_CONVERSATION_ATTACHMENTS {
        return Err(attachment_rejection(format!(
            "at most {MAX_CONVERSATION_ATTACHMENTS} attachments are allowed"
        )));
    }

    let workspace_id = conversation_workspace_id(conversation_extra).ok_or_else(|| {
        attachment_rejection(
            "this conversation has no workspace, so attachments cannot be referenced".to_owned(),
        )
    })?;
    let workspace =
        resolve_workspace_for_run(state, user, Some(&WorkspaceRef { id: workspace_id }))
            .await?
            .ok_or_else(|| {
                AppServerError::new(
                    "workspace_denied",
                    "workspace is not registered for this owner",
                    StatusCode::FORBIDDEN,
                    false,
                )
            })?;

    let mut files: Vec<String> = Vec::new();
    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    for reference in attachments {
        let (accepted, canonical) = validate_attachment_path(workspace.path(), reference)?;
        if seen.contains(&canonical) {
            // 同一条路径给了两次只算一次（运行时也按 distinct 计数，这里先收敛，
            // 免得把「重复」变成「超上限」）。
            continue;
        }
        seen.push(canonical);
        files.push(accepted);
    }
    Ok(files)
}

fn attachment_rejection(message: String) -> AppServerError {
    AppServerError::new(
        "invalid_request",
        message,
        StatusCode::BAD_REQUEST,
        false,
    )
}

/// 附件路径的准入判定（纯文件系统语义，直接单测）。
///
/// 成功返回 `(客户端给的路径, canonical 形态)`：canonical 用于判界与去重，原字符串
/// 才上 wire。
fn validate_attachment_path(
    root: &std::path::Path,
    reference: &str,
) -> Result<(String, std::path::PathBuf), AppServerError> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return Err(attachment_rejection(
            "attachment paths must not be empty".to_owned(),
        ));
    }
    let candidate = std::path::Path::new(trimmed);
    if !candidate.is_absolute() {
        return Err(attachment_rejection(format!(
            "attachment must be an absolute path inside this conversation's workspace: {trimmed}"
        )));
    }
    let canonical = std::fs::canonicalize(candidate).map_err(|_| {
        attachment_rejection(format!("attachment could not be resolved: {trimmed}"))
    })?;
    if canonical == root || !canonical.starts_with(root) {
        return Err(AppServerError::new(
            "workspace_denied",
            format!("attachment is outside this conversation's workspace: {trimmed}"),
            StatusCode::FORBIDDEN,
            false,
        ));
    }
    let metadata = std::fs::metadata(&canonical).map_err(|_| {
        attachment_rejection(format!("attachment could not be inspected: {trimmed}"))
    })?;
    if !metadata.is_file() {
        return Err(attachment_rejection(format!(
            "attachment is not a file: {trimmed}"
        )));
    }
    Ok((trimmed.to_owned(), canonical))
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
    /// doc `29` §6.1：本次运行显式指定的模型。
    ///
    /// 优先级 = **显式 > preset 自带 > 宿主默认**（§6.2）。形状与解析与
    /// `conversation/create` 完全相同（`ConversationModelRef`，`provider_id` 可以是注册过的 UUID
    /// 或 `config.toml` 的 `[providers.<key>]` 名）。
    ///
    /// `skip_serializing_if`：本结构体会被 `request_fingerprint` 序列化，缺席的新字段**不得**
    /// 进入指纹——否则一次纯升级就会让所有既有 `agent/run` 幂等收据失配并重跑。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ConversationModelRef>,
    /// doc `29` §6.1：本次运行的 OpenAI 风格思考等级（`low`/`medium`/`high`/`xhigh`）。
    ///
    /// 它通过 `ResolvedPresetSnapshot.reasoning_effort` 随参与者落到**尝试会话**的 `extra`
    /// （§6.3/§6.4），因此一次运行的每个 attempt 都用同一个等级。preset 自身不带这个字段。
    /// `skip_serializing_if` 同上：缺席不进指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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

/// Default model for the run fallback: the **host's own** `default_model` first
/// (`~/.agent-store/config.toml` — the same source `conversation/create` and
/// `team/run` resolve through, via `resolve_app_server_model`), then the first
/// enabled provider/model in the provider registry.
///
/// Reading only the registry made `agent/run` fail with `resolved_model is
/// required` on a host whose providers live **solely** in the config file: the
/// config provider is registered into that registry **on demand**, so a fresh
/// Agent Store data dir had none until something else resolved a model first.
/// A run must not depend on another call having happened before it.
///
/// Returns `Ok(None)` when neither source yields a model; the run then fails at
/// the runtime boundary with the standard `InvalidSnapshot` (no silently wrong
/// model).
async fn default_run_model(
    state: &AppServerRouterState,
) -> Result<Option<nomifun_api_types::ModelPreference>, AppServerError> {
    if let Ok(config) = load_agent_store_config(state, None)
        && let Some((provider_key, model_name)) = config.default_selection()
        && let Some(provider_service) = state.provider_service.as_ref()
    {
        // Register (or reuse) the config provider row, exactly as the
        // conversation and team paths do — one resolution, one behaviour.
        let provider_id = ensure_agent_store_provider(
            provider_service,
            state.provider_model_service.as_deref(),
            &config,
            &provider_key,
            Some(&model_name),
        )
        .await?;
        return Ok(Some(nomifun_api_types::ModelPreference {
            provider_id: Some(provider_id),
            model: model_name,
            required: true,
        }));
    }

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

/// Merge an explicitly chosen model into an existing override set.
///
/// Two callers, one behaviour (doc `29` §6.2): the host `default_model` fallback and a caller's
/// explicit `model` on `agent/run`. The provenance is deliberately not part of the name — both
/// routes need exactly this, and the caller decides which model wins.
///
/// Mention overrides (`include_skills`, `mcp_server_ids`, ...) must survive: re-resolving from a
/// fresh `PresetOverrides` silently drops them and the run loses every skill/connector the caller
/// mentioned (WP-2 B5). `resolve` then routes the model through `resolve_model_preference`, so the
/// explicit path gets the same authority checks as the fallback.
fn with_model(
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
        // A revoked or expired token ends the socket at its next frame (`22`
        // §7.1 A2). Method dispatch would answer `unauthenticated` anyway; this
        // also stops event delivery promptly.
        if !state.registry.is_live(connection.connection_id()) {
            break;
        }
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

/// `conversation.listChanged`（第一方会话列表投影）→ App Server 通知。
///
/// 与 [`project_conversation_notification`] 不同，这一条**不属于**某条被订阅的
/// 转写流：它改的是侧栏那一整份列表（自动标题、重命名、删除都由它带出来）。
/// 因此它不设订阅门槛（用户此刻很可能正看着另一个会话），也不占用
/// `conversation_events` 的序号——它不是转写帧，客户端不得据此推进
/// `lastSeenSequence`。
fn project_conversation_list_changed(
    event: &nomifun_api_types::WebSocketMessage<serde_json::Value>,
) -> Option<serde_json::Value> {
    let conversation_id = event
        .data
        .get("conversation_id")
        .and_then(serde_json::Value::as_str)?;
    // 公开契约只承认这三态：将来新增一种 action 属于 wire 变更（要动协议指纹），
    // 所以未知取值不原样透出，按「这一行需要重读」保守处理。
    let action = match event.data.get("action").and_then(serde_json::Value::as_str) {
        Some(action @ ("created" | "updated" | "deleted")) => action,
        _ => "updated",
    };
    Some(ws_notification(
        "conversation/list-changed",
        serde_json::json!({ "conversation_id": conversation_id, "action": action }),
    ))
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
    // 未被投影的事件**不得**消耗序列号，所以序号在 `event.name` 的 match **之后**才取。
    //
    // 原实现把 `conversation_event_sequence` 放在 match 之前，而 match 的 `_ => return None`
    // 会带着已自增的计数直接返回——`confirmation.remove` 这类「带 `conversation_id`、未
    // `hidden`、但没有投影分支」的事件因此吃掉一个序号却不发帧，客户端
    // `lastSeenSequence` 与下一个真实帧之间就出现空洞（`conversations.ts` 据此上报
    // `onResync("gap")`，界面显示「实时内容已重新同步：gap」）。这与上面 `hidden` 早退处
    // 「Do not even advance the public sequence」是同一条不变量。
    // （`conversation.listChanged` 曾是同一类事件，现已由
    // `project_conversation_list_changed` 单独投影，且不经此处、不占序号。）
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
                // 计划与 agent 状态此前只投影了 `kind`，载荷整段丢掉——于是**流式**中的
                // 计划行没有任何步骤（客户端解不出来，掉进兜底行「Agent 活动：plan」，
                // 输入框上方的计划面板只能继续显示上一条旧计划），而 agent 状态药丸也
                // 只能凭空猜一个「已暂停」。同一条会话重新加载时，同一行却带着载荷
                // （`persist_plan` / `persist_agent_status` 落库的形状），两者不一致。
                //
                // 与上面 `tool_call` 同一条不变量：**实时行必须渲染成重新加载后的那一行**，
                // 所以这里照落库形状带上载荷。`plan` 的 `source_call_id` 是内部 id，留在
                // 缝后面（`entries` / `session_id` 与落库内容逐字一致）。
                "plan" => (
                    "message.activity",
                    serde_json::json!({
                        "message_id": message_id,
                        "kind": kind,
                        "content": {
                            "session_id": event.data.pointer("/data/session_id").cloned().unwrap_or(serde_json::Value::Null),
                            "entries": event.data.pointer("/data/entries").cloned().unwrap_or(serde_json::Value::Null),
                        },
                    }),
                ),
                "agent_status" => (
                    "message.activity",
                    serde_json::json!({
                        "message_id": message_id,
                        "kind": kind,
                        "content": event.data.pointer("/data").cloned().unwrap_or(serde_json::Value::Null),
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
    // Only now that the event has a public projection: allocate the sequence.
    let sequence = conversation_event_sequence(subscriptions, conversation_id)?;

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
        // 会话列表投影变更（自动标题 / 重命名 / 删除）：发给该用户的每条连接，
        // 不要求它订阅了这个会话，也不消耗转写序列号。
        if envelope.event.name == "conversation.listChanged" {
            let Some(notification) = project_conversation_list_changed(&envelope.event) else {
                continue;
            };
            if !send_ws_output(&outbound_gate, &outbound_tx, notification).await {
                break;
            }
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
                ConversationSendRequest {
                    content: params.content,
                    idempotency_key: params.idempotency_key,
                    attachments: params.attachments,
                    mentions: params.mentions,
                    model: params.model,
                    reasoning_effort: params.reasoning_effort,
                },
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
        "team/run" => {
            let (principal_id, client_id) = state.registry.ready_idempotency_context(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<TeamRunRequest>(params)?;
            let receipt = execute_team_run(state, user, &principal_id, &client_id, params).await?;
            Ok(ws_response(
                request_id,
                serde_json::to_value(receipt).map_err(|error| {
                    AppServerError::new(
                        "internal_error",
                        format!("failed to encode team run receipt: {error}"),
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
        "skill/files" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillQuery>(params)?;
            let files = list_skill_files_impl(state, &params.skill_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(files).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode skill files: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "skill/file" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsSkillFileQuery>(params)?;
            let file = read_skill_file_impl(state, &params.skill_id, &params.path).await?;
            // JSON has no byte string, so the WS binding base64s the body; the
            // HTTP binding serves the raw bytes instead (see §4.3.1). Both
            // carry the same `content_type`, so a caller can pick either.
            Ok(ws_response(request_id, serde_json::json!({
                "skill_id": params.skill_id,
                "path": params.path,
                "content_type": file.content_type,
                "encoding": "base64",
                "content": BASE64_STANDARD.encode(&file.bytes),
            })))
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
        // ---------------- Agent Store Connector catalog ----------------
        "connector/call" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorCall>(params)?;
            let result = connector_call_impl(
                state,
                &params.connector_id,
                &params.tool,
                params.arguments,
                Some(user.id.as_str()),
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode call result: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/list" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let connectors = list_connectors_impl(state, Some(user.id.as_str())).await?;
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
        // `config/get-mcp` / `config/set-mcp` / `config/set-mcp-enabled`: the
        // host-management read and write faces of `~/.agent-store/mcp.json`
        // (`05` §4.10, `21` D17). Same owner gate, same no-path-parameter rule as
        // `config/set` — the file is resolved from the host's own config
        // location, so no request can name one.
        //
        // `get-mcp` is the only read that returns the file's own text: the
        // editor cannot edit what it cannot see, and the operator is editing
        // their own file on their own machine. It is a separate method so that
        // `config/get` — which every settings dialog calls on open — keeps
        // carrying the file's *verdict* only.
        "config/get-mcp" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            parse_ws_params::<WsConfigQuery>(params)?;
            let view = execute_config_get_mcp(state)?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode mcp source view: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "config/set-mcp" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConfigMcpWrite>(params)?;
            let view = execute_config_set_mcp(state, params.source)?;
            Ok(ws_response(request_id, serde_json::to_value(view).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode config view: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "config/set-mcp-enabled" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConfigMcpToggle>(params)?;
            let view = execute_config_set_mcp_enabled(state, params.name, params.enabled)?;
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
            let connector = get_connector_impl(state, &params.connector_id, Some(user.id.as_str())).await?;
            Ok(ws_response(request_id, serde_json::to_value(connector).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/status" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let status = connector_status_impl(state, &params.connector_id, Some(user.id.as_str())).await?;
            Ok(ws_response(request_id, serde_json::to_value(status).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector status: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/test" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let result = connector_test_impl(state, &params.connector_id, Some(user.id.as_str())).await?;
            Ok(ws_response(request_id, serde_json::to_value(result).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode connector probe: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        // ---------------- Agent Store Connector credentials (34 §6.1) --------
        "connector/credential/get" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorQuery>(params)?;
            let credential =
                connector_credential_get_impl(state, &params.connector_id, Some(user.id.as_str()))
                    .await?;
            Ok(ws_response(request_id, serde_json::to_value(credential).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode credential: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/credential/set" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorCredentialSet>(params)?;
            let credential = connector_credential_set_impl(
                state,
                &params.connector_id,
                params.values,
                Some(user.id.as_str()),
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(credential).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode credential: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "connector/credential/clear" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsConnectorCredentialClear>(params)?;
            let credential = connector_credential_clear_impl(
                state,
                &params.connector_id,
                params.keys,
                Some(user.id.as_str()),
            )
            .await?;
            Ok(ws_response(request_id, serde_json::to_value(credential).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode credential: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
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
        // ---------------- Expert definition export (doc 32) ----------------
        "agent/export" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsAgentExport>(params)?;
            let pack = export_agent_impl(state, &params.agent_id).await?;
            Ok(ws_response(request_id, serde_json::to_value(pack).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode expert pack: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
            })?))
        }
        "team/export" => {
            state.registry.require_ready(connection.connection_id(), &user.id)?;
            let params = parse_ws_params::<WsTeamExport>(params)?;
            let pack = export_team_impl(state, &params.team_id, params.team_version.as_deref()).await?;
            Ok(ws_response(request_id, serde_json::to_value(pack).map_err(|error| {
                AppServerError::new("internal_error", format!("failed to encode expert pack: {error}"), StatusCode::INTERNAL_SERVER_ERROR, true)
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

/// `connector/call` (doc `24` §5.2).
///
/// `arguments` is `serde_json::Value` because an MCP tool's parameters are
/// described by an arbitrary JSON Schema — there is nothing to type them
/// against here, and imposing a shape would only reject valid calls.
///
/// `deny_unknown_fields` is load-bearing: a request carrying `url`, `command`,
/// `headers` or `env` is `invalid_request`, so a caller cannot reach past the
/// registered connector to the transport itself.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConnectorCall {
    connector_id: String,
    tool: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

/// `skill/file`: the skill id plus the file's skill-relative path.
///
/// `deny_unknown_fields` keeps the request honest: the provider — not the
/// caller — decides what `path` may resolve to, and no field can name an
/// absolute location or a snapshot.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsSkillFileQuery {
    skill_id: String,
    path: String,
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

/// `agent/export` params (doc `32` §6.1). Deliberately no `agent_version`: the
/// agent entry points do not pin one either (`agent/get`, `agent/run`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsAgentExport {
    agent_id: String,
}

/// `team/export` params. `team_version` is accepted here for the same reason
/// `team/run` accepts it — a caller that pinned a version wants to know it got
/// that version — but it is checked against the pack, not the catalog.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsTeamExport {
    team_id: String,
    #[serde(default)]
    team_version: Option<String>,
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

/// `connector/credential/set` (`34` §6.1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConnectorCredentialSet {
    connector_id: String,
    values: std::collections::HashMap<String, String>,
}

/// `connector/credential/clear`; `keys` absent = every secret field.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConnectorCredentialClear {
    connector_id: String,
    #[serde(default)]
    keys: Option<Vec<String>>,
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

/// `config/set-mcp` params: the **whole file** the operator wants on disk,
/// verbatim. There is no path parameter and no per-entry shape: the host
/// validates the text with its own parser before writing it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConfigMcpWrite {
    source: String,
}

/// `config/set-mcp-enabled` params. `name` must be an **accepted** server key —
/// an entry the parser refused is reported as its own refusal rather than
/// quietly edited.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WsConfigMcpToggle {
    name: String,
    enabled: bool,
}

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
    /// R15（W10）：会话工作区内的绝对路径附件（纯加法，缺省为空）。
    #[serde(default)]
    attachments: Vec<String>,
    /// doc `27` 阶段 1：本轮挂载的技能（只认 `skill`，见 `send_mention_skills`）。
    #[serde(default)]
    mentions: Vec<MentionRef>,
    /// doc `29` §5.1：从本轮起生效的模型（与 HTTP arm 同形）。
    #[serde(default)]
    model: Option<ConversationModelRef>,
    /// doc `29` §5.1：从本轮起生效的思考等级（与 HTTP arm 同形）。
    #[serde(default)]
    reasoning_effort: Option<String>,
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

    /// doc `29` §6.1：`agent/run` 收可选的 `model` 与 `reasoning_effort`（现有 DTO 加字段）。
    #[test]
    fn agent_run_wire_accepts_an_optional_model_and_reasoning_effort() {
        let request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "agent_id": "x",
            "goal": "g",
            "model": {"provider_id": "opencode", "model": "mimo-v2.5"},
            "reasoning_effort": "high"
        }))
        .unwrap();
        assert_eq!(
            request.model.as_ref().map(|model| model.model.as_str()),
            Some("mimo-v2.5")
        );
        assert_eq!(request.reasoning_effort.as_deref(), Some("high"));

        // Omitted = absent (the precedence chain stays untouched).
        let bare: AgentRunRequest =
            serde_json::from_value(serde_json::json!({"agent_id": "x"})).unwrap();
        assert!(bare.model.is_none());
        assert!(bare.reasoning_effort.is_none());
    }

    /// doc `29` §6.1：`AgentRunRequest` 会被 `request_fingerprint` 序列化，所以**缺席的新字段
    /// 不得进入指纹**——否则一次纯升级就会让所有既有 `agent/run` 幂等收据失配并重跑。
    #[test]
    fn an_absent_run_model_and_effort_stay_out_of_the_idempotency_fingerprint() {
        let bare = AgentRunRequest {
            preset_id: "x".into(),
            agent_version: None,
            goal: "g".into(),
            input: None,
            work_dir: None,
            workspace: None,
            steps: None,
            command_id: None,
            idempotency_key: None,
            mentions: Vec::new(),
            model: None,
            reasoning_effort: None,
        };
        let encoded = serde_json::to_string(&bare).unwrap();
        assert!(!encoded.contains("\"model\""), "{encoded}");
        assert!(!encoded.contains("reasoning_effort"), "{encoded}");

        // A run that does carry them changes the fingerprint — that is the point.
        let switched = AgentRunRequest {
            model: Some(ConversationModelRef {
                provider_id: "opencode".into(),
                model: "mimo-v2.5".into(),
                use_model: None,
            }),
            reasoning_effort: Some("high".into()),
            ..bare
        };
        let switched_encoded = serde_json::to_string(&switched).unwrap();
        assert!(switched_encoded.contains("reasoning_effort"), "{switched_encoded}");
    }

    /// doc `27` §4.1：`conversation/send` 收结构化 mention，但**只认 skill**。
    #[test]
    fn conversation_send_wire_accepts_skill_mentions_only() {
        let request: ConversationSendRequest = serde_json::from_value(serde_json::json!({
            "content": "write the release notes",
            "idempotency_key": "client-op-01",
            "mentions": [
                {"kind": "skill", "id": "release-notes"},
                {"kind": "skill", "id": "release-notes"}
            ]
        }))
        .unwrap();
        // Duplicates collapse: the same skill selected twice is one skill.
        assert_eq!(
            send_mention_skills(&request.mentions).unwrap(),
            vec!["release-notes".to_owned()]
        );

        // Omitted mentions default to the empty vec (byte-identical old behaviour).
        let bare: ConversationSendRequest = serde_json::from_value(serde_json::json!({
            "content": "hi",
            "idempotency_key": "k"
        }))
        .unwrap();
        assert!(bare.mentions.is_empty());
        assert!(send_mention_skills(&bare.mentions).unwrap().is_empty());

        // The WS arm shares the shape.
        let ws: WsConversationSend = serde_json::from_value(serde_json::json!({
            "conversation_id": "0190f5fe-7c00-7a00-8000-000000000001",
            "content": "hi",
            "idempotency_key": "k",
            "mentions": [{"kind": "skill", "id": "a"}]
        }))
        .unwrap();
        assert_eq!(send_mention_skills(&ws.mentions).unwrap(), vec!["a".to_owned()]);
    }

    /// 另外两类 mention 在 send 上没有载体：必须显式拒绝，而不是静默不挂。
    #[test]
    fn conversation_send_rejects_mentions_it_cannot_honour() {
        for kind in [MentionKind::Agent, MentionKind::Connector] {
            let error = send_mention_skills(&[MentionRef { kind, id: "x".into() }]).unwrap_err();
            assert_eq!(error.code, "invalid_request");
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert!(!error.retryable);
        }
        // An unnamed skill is not a skill.
        let unnamed = send_mention_skills(&[MentionRef {
            kind: MentionKind::Skill,
            id: "  ".into(),
        }])
        .unwrap_err();
        assert_eq!(unnamed.code, "invalid_request");
    }

    /// doc `29` §5.1：`conversation/send` 收可选的 `model` 与 `reasoning_effort`（HTTP 与 WS 同形）。
    #[test]
    fn conversation_send_wire_accepts_an_optional_model_and_reasoning_effort() {
        let request: ConversationSendRequest = serde_json::from_value(serde_json::json!({
            "content": "switch models",
            "idempotency_key": "k",
            "model": {"provider_id": "opencode", "model": "mimo-v2.5"},
            "reasoning_effort": "xhigh"
        }))
        .unwrap();
        assert_eq!(
            request.model.as_ref().map(|model| model.provider_id.as_str()),
            Some("opencode")
        );
        assert_eq!(request.reasoning_effort.as_deref(), Some("xhigh"));

        // Absent = absent: the pre-`fp-6` wire shape has to keep working verbatim.
        let bare: ConversationSendRequest = serde_json::from_value(serde_json::json!({
            "content": "hi",
            "idempotency_key": "k"
        }))
        .unwrap();
        assert!(bare.model.is_none());
        assert!(bare.reasoning_effort.is_none());

        // The WS arm shares the shape.
        let ws: WsConversationSend = serde_json::from_value(serde_json::json!({
            "conversation_id": "0190f5fe-7c00-7a00-8000-000000000001",
            "content": "hi",
            "idempotency_key": "k",
            "model": {"provider_id": "opencode", "model": "mimo-v2.5"},
            "reasoning_effort": "low"
        }))
        .unwrap();
        assert!(ws.model.is_some());
        assert_eq!(ws.reasoning_effort.as_deref(), Some("low"));
    }

    /// doc `29` §5.2：差异判定——`None` 是"调用方没提这一项"，**不算变化**；
    /// 只有明确给出且与现值不同才算。这条不变量保证不带新参数的调用零副作用（不写库、不广播）。
    #[test]
    fn send_preferences_only_change_when_the_value_actually_differs() {
        let current = ProviderWithModel {
            provider_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            model: "mimo-v2.5".into(),
            use_model: None,
        };
        let other = ProviderWithModel {
            model: "mimo-v2.5-pro".into(),
            ..current.clone()
        };

        // Nothing asked for: no change at all.
        assert_eq!(
            send_preference_changes(None, Some(&current), None, Some("high")),
            (false, false)
        );
        // Asked for, but identical to what the row already holds: still no write.
        assert_eq!(
            send_preference_changes(Some(&current), Some(&current), Some("high"), Some("high")),
            (false, false)
        );
        // A real difference in either field is a write (and only that field).
        assert_eq!(
            send_preference_changes(Some(&other), Some(&current), None, Some("high")),
            (true, false)
        );
        assert_eq!(
            send_preference_changes(None, Some(&current), Some("low"), Some("high")),
            (false, true)
        );
        // A row with no effort yet: asking for one is a change.
        assert_eq!(
            send_preference_changes(None, Some(&current), Some("low"), None),
            (false, true)
        );
    }

    /// doc `29` §5.5：等级的唯一读取口径。空串/空白与缺席**等价**——否则一个存成 `""` 的行
    /// 会与"没指定"永远判为不同，每次 send 都白写一遍。
    #[test]
    fn conversation_extra_reasoning_effort_reads_one_normalized_value() {
        assert_eq!(
            conversation_extra_reasoning_effort(&serde_json::json!({"reasoning_effort": " high "})),
            Some("high".to_owned())
        );
        assert_eq!(
            conversation_extra_reasoning_effort(&serde_json::json!({"reasoning_effort": "  "})),
            None
        );
        assert_eq!(
            conversation_extra_reasoning_effort(&serde_json::json!({"reasoning_effort": ""})),
            None
        );
        assert_eq!(conversation_extra_reasoning_effort(&serde_json::json!({})), None);
        // A non-string value is "unreadable", not "some effort".
        assert_eq!(
            conversation_extra_reasoning_effort(&serde_json::json!({"reasoning_effort": 3})),
            None
        );
    }

    /// doc `27` §5.2：`conversation/create` 收可选的 `agent_id`（现有 DTO 加字段）。
    #[test]
    fn conversation_create_wire_accepts_an_optional_agent_id() {
        let bound: ConversationCreateRequest =
            serde_json::from_value(serde_json::json!({"agent_id": "wb-demo-software-architect"}))
                .unwrap();
        assert_eq!(bound.agent_id.as_deref(), Some("wb-demo-software-architect"));
        // Omitting it is the plain conversation it always was.
        let bare: ConversationCreateRequest = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(bare.agent_id.is_none());
    }

    /// 没有 `agent_id`（或只有空白）时是**空绑定**——与既有行为逐字一致。
    #[tokio::test]
    async fn conversation_create_without_an_agent_binds_nothing() {
        let state = AppServerRouterState::default();
        for agent_id in [None, Some("   ")] {
            let bindings = app_server_chat_bindings_for_agent(&state, agent_id)
                .await
                .unwrap();
            assert!(bindings.connector_ids.is_empty());
            assert!(bindings.skill_names.is_empty());
            assert!(bindings.preset_snapshot.is_none());
        }
    }

    /// 未安装的专家、以及宿主没接预设服务，都必须**点名原因**，而不是开出一个悄悄以别人身份跑的会话。
    #[tokio::test]
    async fn conversation_create_names_why_an_expert_cannot_be_bound() {
        let summary = |id: &str, preset_id: Option<&str>| nomifun_api_types::AppServerAgentSummary {
            id: id.into(),
            version: "1.0.0".into(),
            name: id.into(),
            preset_id: preset_id.map(str::to_owned),
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
        };
        let mut state = AppServerRouterState::default();
        state.agent_catalog = Some(Arc::new(crate::catalog::FakeAgentCatalog {
            agents: vec![
                summary("wb-not-installed", None),
                summary("wb-installed", Some("0190f5fe-7c00-7a00-8000-000000000030")),
            ],
        }));

        let error = app_server_chat_bindings_for_agent(&state, Some("wb-not-installed"))
            .await
            .unwrap_err();
        assert_eq!(error.code, "agent_not_installed");

        // Installed, but this host wired no preset service: a named refusal, not a panic.
        let error = app_server_chat_bindings_for_agent(&state, Some("wb-installed"))
            .await
            .unwrap_err();
        assert_eq!(error.code, "runtime_unavailable");
    }

    /// 定义声明的连接器栅栏：停用 ⇒ `connector_unavailable`（不是悄悄不绑），非法 id ⇒ 内部错误。
    #[tokio::test]
    async fn definition_connector_fence_refuses_disabled_and_malformed_ids() {
        let bindable = "0190f5fe-7c00-7a00-8000-000000000321";
        let disabled = "0190f5fe-7c00-7a00-8000-000000000322";
        let summary = |id: &str, enabled: bool| AppServerConnectorSummary {
            id: id.to_owned(),
            name: format!("server-{id}"),
            description: None,
            kind: "mcp".into(),
            transport_summary: "stdio".into(),
            auth_mode: "none".into(),
            enabled,
            status: if enabled {
                nomifun_api_types::AppServerConnectorStatus::Configured
            } else {
                nomifun_api_types::AppServerConnectorStatus::Installed
            },
            avatar_url: None,
            credential: None,
        };
        let mut state = AppServerRouterState::default();
        state.connectors = Some(Arc::new(crate::catalog::FakeConnectorCatalog {
            connectors: vec![summary(bindable, true), summary(disabled, false)],
            auth_required_ids: vec![],
            probe_fail_ids: vec![],
        }));

        let fence = definition_connector_fence(
            &state,
            "wb-demo",
            &[bindable.to_owned(), bindable.to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(fence.len(), 1, "the same Connector declared twice is one fence entry");

        let error = definition_connector_fence(&state, "wb-demo", &[disabled.to_owned()])
            .await
            .unwrap_err();
        assert_eq!(error.code, "connector_unavailable");

        let error = definition_connector_fence(&state, "wb-demo", &["not-a-uuid".to_owned()])
            .await
            .unwrap_err();
        assert_eq!(error.code, "internal_error");
    }

    /// doc `27` §5.3：`conversation/create` 也收 `team_id`（与 `agent_id` 互斥）。
    #[test]
    fn conversation_create_wire_accepts_an_optional_team_id() {
        let bound: ConversationCreateRequest = serde_json::from_value(
            serde_json::json!({"team_id": "wb-demo-software-company"}),
        )
        .unwrap();
        assert_eq!(bound.team_id.as_deref(), Some("wb-demo-software-company"));
        assert!(bound.agent_id.is_none());
    }

    /// 一个会话要么是某个专家，要么是某个团的 Leader——同时给两个是 `invalid_request`，
    /// 且这个判定发生在读任何工作区 / 目录之前。
    #[tokio::test]
    async fn conversation_create_refuses_an_agent_and_a_team_together() {
        let state = AppServerRouterState::default();
        let user = CurrentUser {
            id: UserId::new(),
            username: "test-user".into(),
        };
        let request = ConversationCreateRequest {
            name: None,
            model: None,
            workspace: None,
            reasoning_effort: None,
            agent_id: Some("wb-demo-software-architect".into()),
            team_id: Some("wb-demo-software-company".into()),
        };
        let error = create_conversation_for_user(&state, &user, request)
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_request");
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
        let merged = with_model(base, &model);
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

    // ---- A2 (`22` §7.1): connection-token TTL + principal revocation -------

    /// An idle token past its TTL is refused with `token_expired` — a distinct
    /// code from the `unauthenticated` an unknown/revoked token gets.
    #[tokio::test]
    async fn an_idle_connection_token_expires_with_a_distinct_code() {
        let registry = AppServerRegistry::default()
            .with_connection_ttl(Some(std::time::Duration::from_millis(100)));
        let owner = UserId::new();
        let connection = registry.open(
            LocalPrincipal::from_authenticated_user(owner.clone(), LocalTransport::WebSocket),
            true,
        );
        registry.initialize(connection.connection_id(), request()).unwrap();
        registry.mark_initialized(connection.connection_id(), &owner).unwrap();
        assert!(registry.require_ready(connection.connection_id(), &owner).is_ok());

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        assert!(matches!(
            registry.require_ready(connection.connection_id(), &owner),
            Err(ProtocolError::TokenExpired)
        ));
        // The two wire codes stay distinct (`22` §7.1 A2 / `05` §10).
        assert_eq!(
            AppServerError::from(ProtocolError::TokenExpired).code,
            "token_expired"
        );
        assert_eq!(
            AppServerError::from(ProtocolError::ConnectionNotFound).code,
            "unauthenticated"
        );
    }

    /// Use renews the idle deadline, so an actively-used connection is not cut
    /// off by its TTL.
    #[tokio::test]
    async fn a_used_connection_token_renews_its_deadline() {
        let registry = AppServerRegistry::default()
            .with_connection_ttl(Some(std::time::Duration::from_millis(400)));
        let owner = UserId::new();
        let connection = registry.open(
            LocalPrincipal::from_authenticated_user(owner.clone(), LocalTransport::WebSocket),
            true,
        );
        registry.initialize(connection.connection_id(), request()).unwrap();
        registry.mark_initialized(connection.connection_id(), &owner).unwrap();

        // Two uses spaced under the TTL: without renewal the second would be
        // past the original (t0 + 400ms) deadline.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert!(registry.require_ready(connection.connection_id(), &owner).is_ok());
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert!(registry.require_ready(connection.connection_id(), &owner).is_ok());
    }

    /// Revoking a principal drops only that principal's tokens, immediately.
    #[test]
    fn revoking_a_principal_invalidates_only_its_tokens() {
        let registry = AppServerRegistry::default();
        let owner = UserId::new();
        let other = UserId::new();
        let mut open_ready = |user: &UserId| {
            let connection = registry.open(
                LocalPrincipal::from_authenticated_user(user.to_owned(), LocalTransport::Http),
                true,
            );
            registry.initialize(connection.connection_id(), request()).unwrap();
            registry.mark_initialized(connection.connection_id(), user).unwrap();
            assert!(registry.require_ready(connection.connection_id(), user).is_ok());
            connection.connection_id().to_owned()
        };
        let owner_a = open_ready(&owner);
        let owner_b = open_ready(&owner);
        let other_c = open_ready(&other);
        drop(open_ready);

        assert_eq!(registry.revoke_principal(owner.as_str()), 2);
        // Immediately dead for the revoked principal...
        assert!(matches!(
            registry.require_ready(&owner_a, &owner),
            Err(ProtocolError::ConnectionNotFound)
        ));
        assert!(matches!(
            registry.require_ready(&owner_b, &owner),
            Err(ProtocolError::ConnectionNotFound)
        ));
        // ...and untouched for everyone else.
        assert!(registry.require_ready(&other_c, &other).is_ok());
    }

    /// A host can turn expiry off entirely (`with_connection_ttl(None)`), which
    /// is the pre-A2 behaviour.
    #[test]
    fn disabling_the_ttl_keeps_tokens_live() {
        let registry = AppServerRegistry::default().with_connection_ttl(None);
        let owner = UserId::new();
        let connection = registry.open(
            LocalPrincipal::from_authenticated_user(owner.clone(), LocalTransport::Http),
            true,
        );
        registry.initialize(connection.connection_id(), request()).unwrap();
        registry.mark_initialized(connection.connection_id(), &owner).unwrap();
        assert!(registry.require_ready(connection.connection_id(), &owner).is_ok());
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
        assert_eq!(payload["server"]["name"], "flowy-agent-store");
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

    /// 会话列表投影变更（自动标题 / 重命名 / 删除）走一条**不设订阅门槛**的通知：
    /// 侧栏展示的是整份列表，用户此刻往往正看着另一个会话。但它绝不能占用转写
    /// 序列号——它不是转写帧。
    #[tokio::test]
    async fn conversation_list_changed_is_pushed_without_a_subscription() {
        let bus = BroadcastEventBus::new(16);
        let owner = UserId::new();
        // 刻意不订阅任何会话。
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

        // 别人的列表变更不得越过这条连接。
        bus.send_to_user(
            "other-owner",
            WebSocketMessage::new(
                "conversation.listChanged",
                serde_json::json!({
                    "conversation_id": "0190f5fe-7c00-7a00-8000-00000000ffff",
                    "action": "updated",
                }),
            ),
        );
        // 自动标题落库后服务端发的就是这一条（`service.rs` 的 `broadcast_list_changed`）。
        bus.send_to_user(
            owner.as_str(),
            WebSocketMessage::new(
                "conversation.listChanged",
                serde_json::json!({
                    "conversation_id": "0190f5fe-7c00-7a00-8000-00000000000a",
                    "action": "updated",
                    "source": "nomifun",
                }),
            ),
        );

        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("list change must be delivered")
            .expect("channel stays open");
        let value: serde_json::Value = serde_json::from_str(&frame).expect("frame is JSON");
        assert_eq!(value["method"], "conversation/list-changed");
        assert_eq!(value["params"]["conversation_id"], "0190f5fe-7c00-7a00-8000-00000000000a");
        assert_eq!(value["params"]["action"], "updated");
        // 转写序列号属于 `conversation/event`，这一条不带、也不推进它。
        assert!(value["params"].get("sequence").is_none());
        assert!(rx.try_recv().is_err(), "the foreign owner's change stays on its own connection");
        task.abort();
    }

    #[test]
    fn conversation_list_changed_projection_keeps_the_three_documented_actions() {
        let event = |action: serde_json::Value| {
            WebSocketMessage::new(
                "conversation.listChanged",
                serde_json::json!({
                    "conversation_id": "0190f5fe-7c00-7a00-8000-00000000000a",
                    "action": action,
                }),
            )
        };

        for action in ["created", "updated", "deleted"] {
            let notification = project_conversation_list_changed(&event(serde_json::json!(action)))
                .expect("the three documented actions are projected");
            assert_eq!(notification["method"], "conversation/list-changed");
            assert_eq!(notification["params"]["action"], action);
        }

        // 未知取值不原样透出：契约只承认那三态，按「这一行需要重读」保守处理。
        let unknown = project_conversation_list_changed(&event(serde_json::json!("renamed")))
            .expect("an unknown action still means the row is stale");
        assert_eq!(unknown["params"]["action"], "updated");
        let missing = project_conversation_list_changed(&event(serde_json::Value::Null))
            .expect("a missing action is still a list change");
        assert_eq!(missing["params"]["action"], "updated");

        // 没有 conversation_id 的事件不成通知。
        assert!(
            project_conversation_list_changed(&WebSocketMessage::new(
                "conversation.listChanged",
                serde_json::json!({ "action": "updated" }),
            ))
            .is_none()
        );
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

    /// 未被投影的事件**不得**消耗序列号。
    ///
    /// `conversation_event_sequence` 在 `event.name` 的 match **之前**自增，而该 match 的
    /// `_ => return None` 会带着已经自增的计数直接返回。于是 `confirmation.remove`
    /// 这类「带 `conversation_id`、未 `hidden`、但投影器没有对应分支」的事件会吃掉一个
    /// 序号却不发出任何帧——客户端 `lastSeenSequence` 与下一个真实帧之间就出现空洞，
    /// `conversations.ts` 据此上报 `onResync("gap")`，用户看到
    /// 「实时内容已重新同步：gap」。计数器必须只在真正发出通知时才前进。
    #[test]
    fn unprojected_conversation_event_must_not_burn_a_sequence_number() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-000000000009";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));

        // 有 `conversation_id`、没 `hidden`，但投影器没有 `confirmation.remove` 分支。
        let unprojected = WebSocketMessage::new("confirmation.remove", serde_json::json!({
            "conversation_id": conversation_id,
            "id": "0190f5fe-7c00-7a00-0000-0000000000conf",
        }));
        assert!(
            project_conversation_notification(&unprojected, &subscriptions).is_none(),
            "no public projection exists for this event"
        );

        let public = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-000000000011",
            "type": "content",
            "data": {"content": "hi"},
        }));
        let notification =
            project_conversation_notification(&public, &subscriptions).expect("content is public");
        assert_eq!(
            notification["params"]["sequence"], 1,
            "an unprojected event must not consume a sequence number"
        );
    }

    /// 实时计划行必须带上步骤，和重新加载后的那一行一致。
    ///
    /// 投影器此前只发 `kind: "plan"`，客户端 `planData()` 解不出步骤，于是流式中的计划
    /// 掉进兜底行「Agent 活动：plan」，输入框上方的计划面板只能继续显示上一条旧计划；
    /// 而 `conversation/messages` 回来的同一行却带着 `entries`。`update_plan` 的
    /// `source_call_id` 是内部 id，不能借这条投影泄漏出去。
    #[test]
    fn live_plan_and_agent_status_frames_carry_their_payload() {
        let conversation_id = "0190f5fe-7c00-7a00-8000-00000000000a";
        let subscriptions = Arc::new(RwLock::new(WsSubscriptions {
            conversations: HashSet::from([conversation_id.to_owned()]),
            ..Default::default()
        }));

        let plan = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-0000000000plan",
            "type": "plan",
            "data": {
                "session_id": "update_plan",
                "source_call_id": "internal-call-id",
                "entries": [
                    { "content": "创建示例计划", "status": "in_progress" },
                    { "content": "演示计划更新", "status": "pending" },
                ],
            },
        }));
        let projected = project_conversation_notification(&plan, &subscriptions).expect("plan is public");
        assert_eq!(projected["params"]["event_type"], "message.activity");
        assert_eq!(projected["params"]["payload"]["kind"], "plan");
        assert_eq!(projected["params"]["payload"]["message_id"], "0190f5fe-7c00-7a00-0000-0000000000plan");
        let entries = projected["params"]["payload"]["content"]["entries"]
            .as_array()
            .expect("a live plan must carry its steps");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["status"], "in_progress");
        assert_eq!(
            projected["params"]["payload"]["content"]["session_id"], "update_plan",
            "the live row must carry what the reloaded row carries"
        );
        assert!(
            projected["params"]["payload"]["content"].get("source_call_id").is_none(),
            "the plan's internal source call id must stay behind the seam"
        );

        let status = WebSocketMessage::new("message.stream", serde_json::json!({
            "conversation_id": conversation_id,
            "msg_id": "0190f5fe-7c00-7a00-0000-0000000000stat",
            "type": "agent_status",
            "data": { "backend": "nomi", "status": "error", "agent_name": "Nomi", "session_id": null },
        }));
        let projected = project_conversation_notification(&status, &subscriptions).expect("status is public");
        assert_eq!(
            projected["params"]["payload"]["content"]["status"], "error",
            "without the payload the status pill can only guess a state"
        );
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
max_output_size = 32000
protocol = "anthropic"
display_name = "MiMo V2.5 Free"
"#;

    /// The download policy of the default marketplace fallback: only a source
    /// the operator **declared** is fetched during boot. The three builtin
    /// mirrors are 324 MiB together (`experts` alone is 289.6 MiB), so a host
    /// that declares nothing must register them and download nothing.
    #[test]
    fn only_declared_default_marketplaces_are_fetched_at_boot() {
        let dir = std::env::temp_dir().join(format!("allo-defaults-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");

        // No config file at all: the fresh-install case.
        let (sources, fetch) = default_marketplace_plan(&dir.join("absent.toml"));
        assert_eq!(sources, AgentStoreConfig::builtin_default_marketplaces());
        assert!(!fetch, "a fresh install must not download the official archives");

        // A config file that exists but declares no marketplace: same answer.
        let bare = dir.join("bare.toml");
        std::fs::write(&bare, "[memory]\ndistill_enabled = false\n").expect("bare config");
        let (sources, fetch) = default_marketplace_plan(&bare);
        assert_eq!(sources, AgentStoreConfig::builtin_default_marketplaces());
        assert!(!fetch, "declaring no source must not mean 'download the builtin ones'");

        // A declared source is an explicit request: registered *and* fetched,
        // and the builtin mirrors do not ride along.
        let declared = dir.join("declared.toml");
        std::fs::write(
            &declared,
            "[default_marketplaces.company]\nsource_kind = \"directory\"\n\
             source = \"/tmp/company-tools\"\n",
        )
        .expect("declared config");
        let (sources, fetch) = default_marketplace_plan(&declared);
        assert!(fetch, "declaring a source is the opt-in to downloading it");
        assert_eq!(
            sources,
            vec![(
                "company".to_owned(),
                "directory".to_owned(),
                "/tmp/company-tools".to_owned()
            )]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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
            // The row-level writer the registration path uses for the
            // per-model fields the provider DTO has no map column for.
            provider_model_service: Some(Arc::new(
                nomifun_system::ProviderModelService::new(
                    Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
                    Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
                ),
            )),
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

        // 3b. The output ceiling and per-model protocol land on the row. These
        //     two have no provider-level map column, so they are written
        //     through the row-level face; a NULL `output_limit` here is what
        //     makes an `anthropic` provider fail to build at all.
        let mimo_row = provider
            .models_detail
            .iter()
            .find(|row| row.model == "mimo-v2.5-free")
            .expect("mimo row must exist");
        assert_eq!(
            mimo_row.output_limit,
            Some(32_000),
            "max_output_size must reach provider_models.output_limit"
        );
        assert_eq!(mimo_row.protocol.as_deref(), Some("anthropic"));
        // A sibling that declares neither key stays NULL — no invented values.
        let laguna_row = provider
            .models_detail
            .iter()
            .find(|row| row.model == "laguna-s-2.1-free")
            .expect("laguna row must exist");
        assert_eq!(laguna_row.output_limit, None);
        assert_eq!(laguna_row.protocol, None);

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

    /// Config A declares no per-model ceiling; config B adds one. A provider
    /// first registered under A carries a NULL `output_limit`, which is exactly
    /// the state that fails an `anthropic` runtime build. Resolving again under
    /// B must repair that existing row — not just seed brand-new ones.
    #[tokio::test]
    async fn agent_store_registration_repairs_a_previously_null_output_ceiling() {
        let db = nomifun_db::init_database_memory_with_owner(UserId::new())
            .await
            .expect("in-memory db");
        let pool = db.pool().clone();

        let dir = std::env::temp_dir().join(format!("allo-repair-test-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp config dir");
        let path = dir.join("config.toml");

        let without_ceiling = r#"
default_model = "mify/mimo-v2.5-pro"

[providers.mify]
type = "anthropic"
api_key = "sk-test-not-a-real-key"
base_url = "https://example.invalid/v1"

[models."mify/mimo-v2.5-pro"]
provider = "mify"
model = "mimo-v2.5-pro"
max_context_size = 1024000
"#;
        let with_ceiling = r#"
default_model = "mify/mimo-v2.5-pro"

[providers.mify]
type = "anthropic"
api_key = "sk-test-not-a-real-key"
base_url = "https://example.invalid/v1"

[models."mify/mimo-v2.5-pro"]
provider = "mify"
model = "mimo-v2.5-pro"
max_context_size = 1024000
max_output_size = 8000
"#;

        let state = |path: &std::path::Path| AppServerRouterState {
            provider_service: Some(Arc::new(ProviderService::new(
                Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
                Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
                [9u8; 32],
            ))),
            provider_model_service: Some(Arc::new(
                nomifun_system::ProviderModelService::new(
                    Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
                    Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
                ),
            )),
            agent_store_config_path: Some(path.to_path_buf()),
            ..Default::default()
        };

        let read_output_limit = |pool: nomifun_db::SqlitePool, provider_name: &str| {
            let provider_name = provider_name.to_owned();
            async move {
                let providers = ProviderService::new(
                    Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
                    Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
                    [9u8; 32],
                )
                .list()
                .await
                .expect("provider list");
                providers
                    .into_iter()
                    .find(|provider| provider.name == provider_name)
                    .expect("provider row")
                    .models_detail
                    .into_iter()
                    .find(|row| row.model == "mimo-v2.5-pro")
                    .expect("model row")
                    .output_limit
            }
        };

        // Register under the ceiling-less config: the row exists but is NULL.
        std::fs::write(&path, without_ceiling).expect("config without ceiling");
        let first = state(&path);
        resolve_app_server_model(&first, None).await.expect("first resolution");
        assert_eq!(
            read_output_limit(pool.clone(), "mify").await,
            None,
            "no declared ceiling must stay NULL"
        );

        // The same provider key, now resolved under a config that declares the
        // ceiling: the existing row must be repaired in place.
        std::fs::write(&path, with_ceiling).expect("config with ceiling");
        let second = state(&path);
        resolve_app_server_model(&second, None).await.expect("second resolution");
        assert_eq!(
            read_output_limit(pool.clone(), "mify").await,
            Some(8_000),
            "an existing NULL ceiling must be filled in from the config"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The repair pass runs on every model resolution, so it must never
    /// overwrite a value that is already set: the runtime tells the operator to
    /// fix a missing ceiling in Settings → Models, and re-asserting the config
    /// afterwards would silently undo that edit.
    #[tokio::test]
    async fn agent_store_registration_never_overwrites_an_explicit_output_ceiling() {
        let db = nomifun_db::init_database_memory_with_owner(UserId::new())
            .await
            .expect("in-memory db");
        let pool = db.pool().clone();

        let dir = std::env::temp_dir().join(format!("allo-nooverwrite-test-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp config dir");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
default_model = "mify/mimo-v2.5-pro"

[providers.mify]
type = "anthropic"
api_key = "sk-test-not-a-real-key"
base_url = "https://example.invalid/v1"

[models."mify/mimo-v2.5-pro"]
provider = "mify"
model = "mimo-v2.5-pro"
max_context_size = 1024000
max_output_size = 8000
"#,
        )
        .expect("temp config file");

        let provider_service = Arc::new(ProviderService::new(
            Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
            Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
            [11u8; 32],
        ));
        let provider_model_service = Arc::new(nomifun_system::ProviderModelService::new(
            Arc::new(nomifun_db::SqliteProviderModelRepository::new(pool.clone())),
            Arc::new(nomifun_db::SqliteProviderRepository::new(pool.clone())),
        ));
        let state = AppServerRouterState {
            provider_service: Some(provider_service.clone()),
            provider_model_service: Some(provider_model_service.clone()),
            agent_store_config_path: Some(path.clone()),
            ..Default::default()
        };

        let resolved = resolve_app_server_model(&state, None).await.expect("resolution");
        assert_eq!(
            read_ceiling(&provider_service, &resolved.provider_id, "mimo-v2.5-pro").await,
            Some(8_000),
            "the config value seeds a NULL row"
        );

        // Simulate the operator setting a different ceiling in Settings → Models.
        provider_model_service
            .update(UpdateProviderModelRequest {
                provider_id: resolved.provider_id.clone(),
                model: "mimo-v2.5-pro".to_owned(),
                enabled: None,
                sort_order: None,
                tasks: None,
                traits: None,
                protocol: None,
                connection_role: None,
                params: None,
                context_limit: None,
                output_limit: Some(Some(4096)),
                description: None,
            })
            .await
            .expect("operator edit");

        // Another resolution must leave that explicit value alone.
        resolve_app_server_model(&state, None).await.expect("re-resolution");
        assert_eq!(
            read_ceiling(&provider_service, &resolved.provider_id, "mimo-v2.5-pro").await,
            Some(4_096),
            "an already-set ceiling must never be overwritten by the config"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Read one model row's `output_limit` through the provider projection.
    async fn read_ceiling(
        provider_service: &ProviderService,
        provider_id: &str,
        model: &str,
    ) -> Option<i64> {
        provider_service
            .list()
            .await
            .expect("provider list")
            .into_iter()
            .find(|provider| provider.provider_id == provider_id)
            .expect("provider row")
            .models_detail
            .into_iter()
            .find(|row| row.model == model)
            .expect("model row")
            .output_limit
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
            last_turn_input_tokens: None,
            last_turn_output_tokens: None,
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

    /// W9 / R14 ③：`conversation/get` 的 `context_usage` 必须带上持久化的「上一轮」
    /// token，重载后的 WebUI 才能显示上一轮用量与金额（不靠重放事件流）。
    #[test]
    fn context_usage_view_carries_the_persisted_last_turn_tokens() {
        let projected = context_usage_view(nomifun_db::models::AppServerContextUsageRow {
            id: 0,
            conversation_id: "0190f5fe-7c00-7a00-8000-000000000099".to_owned(),
            context_tokens: 100_000,
            window_tokens: 200_000,
            last_turn_input_tokens: Some(1_200),
            last_turn_output_tokens: Some(340),
            updated_at: 123,
        });
        assert_eq!(projected.last_turn_input_tokens, Some(1_200));
        assert_eq!(projected.last_turn_output_tokens, Some(340));
        let json = serde_json::to_value(&projected).expect("view serializes");
        assert_eq!(json["last_turn_input_tokens"], 1_200);
        assert_eq!(json["last_turn_output_tokens"], 340);

        // A runtime-reported `0` is a measurement, not absence: it stays on the
        // wire so the client can tell "reported zero" from "never reported".
        let zero_input = context_usage_view(nomifun_db::models::AppServerContextUsageRow {
            id: 0,
            conversation_id: "0190f5fe-7c00-7a00-8000-000000000099".to_owned(),
            context_tokens: 500,
            window_tokens: 200_000,
            last_turn_input_tokens: Some(0),
            last_turn_output_tokens: Some(42),
            updated_at: 123,
        });
        let zero_json = serde_json::to_value(&zero_input).expect("view serializes");
        assert_eq!(zero_json["last_turn_input_tokens"], 0);
        assert_eq!(zero_json["last_turn_output_tokens"], 42);
    }

    /// 未上报（库里是 NULL）时整对字段**不上 wire**：不显示 0，也绝不拿上下文占用
    /// 顶替本轮 token（批 6 已定的金额口径）。
    #[test]
    fn context_usage_view_omits_unreported_last_turn_tokens_entirely() {
        let projected = context_usage_view(nomifun_db::models::AppServerContextUsageRow {
            id: 0,
            conversation_id: "0190f5fe-7c00-7a00-8000-000000000099".to_owned(),
            context_tokens: 100_000,
            window_tokens: 200_000,
            last_turn_input_tokens: None,
            last_turn_output_tokens: None,
            updated_at: 123,
        });
        assert_eq!(projected.last_turn_input_tokens, None);
        let json = serde_json::to_value(&projected).expect("view serializes");
        assert!(
            json.get("last_turn_input_tokens").is_none(),
            "absent key, not a zero: {json}"
        );
        assert!(json.get("last_turn_output_tokens").is_none(), "{json}");
        // Occupancy stays occupancy — it never becomes the missing turn tokens.
        assert_eq!(json["used_tokens"], 100_000);
        assert_eq!(json["window_tokens"], 200_000);
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

    /// `~/.agent-store/mcp.json` is projected read-only next to the settings
    /// file: accepted entries, per-entry refusals with reasons, and **no
    /// credential values** (`20` §7.9 / `21` D14).
    #[tokio::test]
    async fn config_get_projects_mcp_declarations_and_refusals() {
        let dir = config_temp_dir("mcp-view");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        std::fs::write(
            dir.join("mcp.json"),
            r#"{ "mcpServers": {
                 "linear": { "url": "https://mcp.linear.app/mcp", "headers": { "Authorization": "Bearer sk-mcp-must-never-reach-the-wire" }, "bearerTokenEnvVar": "GITHUB_TOKEN" },
                 "filesystem": { "command": "npx", "args": ["-y", "srv"], "env": { "TOKEN": "sk-mcp-env-must-never-reach-the-wire" }, "cwd": "/srv/fs-declared", "enabledTools": ["read_file"], "disabledTools": ["write_file"] },
                 "off": { "command": "never", "enabled": false },
                 "bad": { "command": "npx", "headers": { "X-Tenant": "acme" } }
               } }"#,
        )
        .expect("temp mcp file");
        let (mut state, user, connection, subscriptions) = config_dispatch_state(&path);

        let view = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");

        assert_eq!(view["mcp"]["exists"], serde_json::json!(true));
        // Sorted by key, transports labelled, `enabled` reported as declared.
        assert_eq!(view["mcp"]["servers"][0]["name"], serde_json::json!("filesystem"));
        assert_eq!(view["mcp"]["servers"][0]["transport"], serde_json::json!("stdio"));
        assert_eq!(view["mcp"]["servers"][0]["enabled"], serde_json::json!(true));
        assert_eq!(view["mcp"]["servers"][1]["name"], serde_json::json!("linear"));
        assert_eq!(view["mcp"]["servers"][1]["transport"], serde_json::json!("http"));
        assert_eq!(view["mcp"]["servers"][2]["name"], serde_json::json!("off"));
        assert_eq!(view["mcp"]["servers"][2]["enabled"], serde_json::json!(false));

        // A refused entry names itself and says why. `headers` on a stdio entry
        // is structurally wrong for any version of the file, which is why it is
        // the counterexample here rather than a field we might later support.
        assert_eq!(view["mcp"]["rejected"][0]["name"], serde_json::json!("bad"));
        let reason = view["mcp"]["rejected"][0]["reason"].as_str().unwrap_or_default();
        assert!(reason.contains("`headers`"), "{reason}");
        assert!(view["mcp"].get("error").is_none(), "{view}");

        // Credential values have no field on the wire, in env or in headers.
        let encoded = view.to_string();
        assert!(!encoded.contains("sk-mcp-must-never-reach-the-wire"), "{encoded}");
        assert!(!encoded.contains("sk-mcp-env-must-never-reach-the-wire"), "{encoded}");
        assert!(!encoded.contains("Authorization"), "{encoded}");

        // Nor do the fields that only steer the engine: the view is
        // `name` / `transport` / `enabled` plus the refusal reasons (`05` §4.10).
        // Asserted on the *values*, so a future projection has to be a deliberate
        // change rather than an accident of adding a field to the view struct.
        for absent in [
            "/srv/fs-declared",
            "read_file",
            "write_file",
            "GITHUB_TOKEN",
        ] {
            assert!(!encoded.contains(absent), "{absent} reached the wire: {encoded}");
        }

        // The host's adoption answer rides along, and its **absence** stays absent
        // on the wire: "the launcher did not say" is a state of its own, and
        // collapsing it to `false` would misreport an adopting host built before
        // this field existed.
        assert!(view["mcp"].get("adopted").is_none(), "{view}");

        // `adopted: false` with a perfectly good file is the pair the field
        // exists for: `servers` describes the **file**, `adopted` the **host**,
        // and without it this host's screen looks like one that injects them.
        state.adopt_store_mcp_declarations = Some(false);
        let inert =
            dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
                .await
                .expect("config/get");
        assert_eq!(inert["mcp"]["adopted"], serde_json::json!(false));
        assert_eq!(inert["mcp"]["servers"][0]["name"], serde_json::json!("filesystem"));

        state.adopt_store_mcp_declarations = Some(true);
        let used =
            dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
                .await
                .expect("config/get");
        assert_eq!(used["mcp"]["adopted"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// No declaration file, a broken one, and no config file at all each answer
    /// without an error — and a broken file reports why instead of looking empty.
    #[tokio::test]
    async fn config_get_reports_mcp_absence_and_breakage() {
        let dir = config_temp_dir("mcp-absent");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        let view = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert_eq!(view["mcp"], serde_json::Value::Null, "{view}");

        std::fs::write(dir.join("mcp.json"), "{ not json").expect("temp broken mcp file");
        let view = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert_eq!(view["mcp"]["exists"], serde_json::json!(true));
        assert_eq!(view["mcp"]["servers"], serde_json::json!([]));
        let error = view["mcp"]["error"].as_str().unwrap_or_default();
        assert!(error.contains("JSON"), "{view}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `config/set` answers with the same read-back, so a declaration file is
    /// reported even on a write that only touched `config.toml`.
    #[tokio::test]
    async fn config_set_reads_the_mcp_projection_back() {
        let dir = config_temp_dir("mcp-set");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        std::fs::write(
            dir.join("mcp.json"),
            r#"{ "mcpServers": { "filesystem": { "command": "npx" } } }"#,
        )
        .expect("temp mcp file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

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

        assert_eq!(view["mcp"]["servers"][0]["name"], serde_json::json!("filesystem"));

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
    async fn config_set_writes_the_tool_policy_and_reads_it_back() {
        let dir = config_temp_dir("tools");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        // A file without `[tools]` reports "not configured" — never a fabricated
        // all-on policy, so a client can tell "absent" from "declared".
        let before = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert!(before["tools"].is_null(), "absent table must be null: {before}");

        let view = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({
                "tools": {
                    "disabled": ["update_plan", "remember"],
                    "computer": false,
                    "domains": { "cron": false }
                }
            }),
        )
        .await
        .expect("config/set");
        // Canonical order in the read-back, and untouched switches stay on.
        assert_eq!(view["tools"]["disabled"], serde_json::json!(["remember", "update_plan"]));
        assert_eq!(view["tools"]["computer"], serde_json::json!(false));
        assert_eq!(view["tools"]["browser"], serde_json::json!(true));
        assert_eq!(view["tools"]["domains"]["cron"], serde_json::json!(false));
        assert_eq!(view["tools"]["domains"]["media"], serde_json::json!(true));

        // The value really landed in the host file the launcher reads, and
        // everything else survived byte-for-byte.
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert!(on_disk.contains("[tools]"), "{on_disk}");
        assert!(on_disk.contains("[tools.domains]"), "{on_disk}");
        assert!(on_disk.contains("disabled = [\"remember\", \"update_plan\"]"), "{on_disk}");
        assert!(on_disk.contains("computer = false"), "{on_disk}");
        assert!(on_disk.contains("cron = false"), "{on_disk}");
        assert!(on_disk.contains("# allo host config — do not reformat"), "{on_disk}");
        assert!(on_disk.contains("[models.\"opencode/mimo-v2.5-free\"]"), "{on_disk}");
        let credential_line = COMMENTED_CONFIG
            .lines()
            .find(|line| line.starts_with("api_key"))
            .expect("fixture carries a credential line");
        assert!(on_disk.contains(credential_line), "{on_disk}");
        assert_eq!(on_disk.matches("[tools]").count(), 1, "{on_disk}");
        assert_eq!(on_disk.matches("disabled").count(), 1, "{on_disk}");
        assert!(!dir.join("config.toml.tmp").exists(), "temp file must not survive");

        // Re-read agrees without a second write.
        let again = dispatch_config(&state, &connection, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect("config/get");
        assert_eq!(again, view);

        // A second, narrower patch rewrites only what it names.
        let narrowed = dispatch_config(
            &state,
            &connection,
            &user,
            &subscriptions,
            "config/set",
            serde_json::json!({ "tools": { "disabled": ["remember"] } }),
        )
        .await
        .expect("config/set");
        assert_eq!(narrowed["tools"]["disabled"], serde_json::json!(["remember"]));
        assert_eq!(narrowed["tools"]["computer"], serde_json::json!(false));
        assert_eq!(narrowed["tools"]["domains"]["cron"], serde_json::json!(false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_refuses_tool_patches_outside_the_whitelist() {
        let dir = config_temp_dir("tools-refused");
        let path = dir.join("config.toml");
        std::fs::write(&path, COMMENTED_CONFIG).expect("temp config file");
        let (state, user, connection, subscriptions) = config_dispatch_state(&path);

        for body in [
            // An empty `[tools]` table names nothing: "sent" must never be
            // mistakable for "stored".
            serde_json::json!({ "tools": {} }),
            serde_json::json!({ "tools": null }),
            // The write whitelist is the boundary: unknown keys are hard
            // refusals, not silently dropped fields.
            serde_json::json!({ "tools": { "force_enable_everything": true } }),
            serde_json::json!({ "tools": { "domains": { "bogus": false } } }),
            serde_json::json!({ "tools": { "credentials": { "TOKEN": "x" } } }),
            // Wrong value shapes.
            serde_json::json!({ "tools": { "disabled": "remember" } }),
            serde_json::json!({ "tools": { "computer": "false" } }),
            serde_json::json!({ "tools": { "disabled": ["bad\u{7}name"] } }),
        ] {
            let error = dispatch_config(&state, &connection, &user, &subscriptions, "config/set", body.clone())
                .await
                .expect_err("must be refused");
            assert_eq!(error.code, "invalid_request", "{body}");
        }

        // Nothing was written: a refused patch never creates `[tools]`.
        let on_disk = std::fs::read_to_string(&path).expect("config on disk");
        assert!(!on_disk.contains("[tools]"), "{on_disk}");
        assert!(!on_disk.contains("bad"), "{on_disk}");

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

        // A connection this registry never issued is an authentication failure
        // (`22` §7.1 A2: `05` §3.1 answers `unauthenticated`), not a file read:
        // the config face adds no new reachable surface.
        let unknown = ConnectionState::new(
            LocalPrincipal::from_authenticated_user(user.id.clone(), LocalTransport::WebSocket),
            true,
        );
        let error = dispatch_config(&state, &unknown, &user, &subscriptions, "config/get", serde_json::json!({}))
            .await
            .expect_err("unknown connection must be refused");
        assert_eq!(error.code, "unauthenticated");

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
            avatar_url: None,
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
            avatar_url: None,
            credential: None,
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
                connectors: vec![],
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

    // ---- Skill file tree: `skill/files` · `skill/file` (doc 24 §4) ----------
    //
    // --- expert definition export (doc `32`) -------------------------------
    //
    // These pin the *protocol* layer's half of the contract, mirroring the
    // skill-file group below: the capability bit tracks the seam, an uninjected
    // seam stays closed, and every seam error keeps its own stable wire code —
    // notably that a policy refusal is `policy_denied` and NOT `not_found`, and
    // that `response_too_large` is not folded into `invalid_request`.

    /// A persona body distinctive enough that finding it anywhere else is proof
    /// of a leak, not a coincidence.
    const PERSONA_SENTINEL: &str = "PERSONA-ONLY-IN-THE-PACK-e7f1";

    fn sample_expert_pack() -> AppServerExpertPack {
        AppServerExpertPack {
            pack_format: nomifun_api_types::APP_SERVER_EXPERT_PACK_FORMAT,
            kind: nomifun_api_types::AppServerExpertPackKind::Agent,
            id: "wb-demo-software-team-lead".into(),
            version: "1.0.0".into(),
            name: "software-team-lead".into(),
            display_name: None,
            description: Some("lead".into()),
            persona: nomifun_api_types::AppServerExpertPersona {
                instructions: PERSONA_SENTINEL.into(),
                memory: None,
                background: None,
            },
            model: nomifun_api_types::AppServerExpertModel {
                declared: Some("gpt-5".into()),
                resolved: None,
                effort: None,
                max_turns: None,
            },
            skills: vec![nomifun_api_types::AppServerExpertSkillRef {
                name: "release-notes".into(),
                id: "release-notes".into(),
            }],
            connectors: vec![],
            tool_policy: nomifun_api_types::AppServerExpertToolPolicy {
                tools: vec!["read_file".into()],
                disallowed_tools: vec![],
            },
            team: None,
            provenance: nomifun_api_types::AppServerExpertProvenance {
                source: "codebuddy-plugin".into(),
                snapshot_id: "snap-demo".into(),
                content_digest: "digest-demo".into(),
                preset_id: Some("preset-demo".into()),
                preset_revision: Some(3),
            },
            runtime_binding: nomifun_api_types::AppServerExpertRuntimeBinding {
                runtime: "nomi".into(),
                portable: false,
            },
        }
    }

    /// Expert pack seam stub. `fail` is interior-mutable for the same reason
    /// `FakeSkillFiles::fail` is: the trait takes `&self` while a test needs to
    /// hand back one specific error.
    struct FakeExpertPacks {
        pack: AppServerExpertPack,
        fail: std::sync::Mutex<Option<ExpertPackError>>,
    }

    impl FakeExpertPacks {
        fn ok(pack: AppServerExpertPack) -> Self {
            Self {
                pack,
                fail: std::sync::Mutex::new(None),
            }
        }

        fn failing(error: ExpertPackError) -> Self {
            Self {
                pack: sample_expert_pack(),
                fail: std::sync::Mutex::new(Some(error)),
            }
        }

        fn take_failure(&self) -> Option<ExpertPackError> {
            self.fail.lock().expect("fake expert pack lock").take()
        }
    }

    #[async_trait::async_trait]
    impl ExpertPackProvider for FakeExpertPacks {
        async fn export_agent(
            &self,
            _agent_id: &str,
        ) -> Result<AppServerExpertPack, ExpertPackError> {
            if let Some(error) = self.take_failure() {
                return Err(error);
            }
            Ok(self.pack.clone())
        }

        async fn export_team(&self, _team_id: &str) -> Result<AppServerExpertPack, ExpertPackError> {
            if let Some(error) = self.take_failure() {
                return Err(error);
            }
            Ok(self.pack.clone())
        }
    }

    #[tokio::test]
    async fn expert_export_requires_its_own_capability() {
        // The agent/team catalogs alone must NOT advertise export: a client that
        // saw `agents: true` and called `agent/export` would get
        // `unsupported_operation` after the fact.
        let catalog_only = AppServerRouterState {
            agent_catalog: Some(Arc::new(crate::catalog::FakeAgentCatalog { agents: vec![] })),
            team_catalog: Some(Arc::new(crate::catalog::FakeTeamCatalog {
                teams: vec![],
                connectors: vec![],
            })),
            ..Default::default()
        };
        let availability = CapabilityAvailability::from_state(&catalog_only);
        assert!(availability.agents && availability.teams);
        assert!(
            !availability.expert_export,
            "the catalogs alone must not advertise the export face"
        );
        assert!(!Capabilities::from_availability(availability).expert_export);

        let wired = AppServerRouterState {
            expert_packs: Some(Arc::new(FakeExpertPacks::ok(sample_expert_pack()))),
            ..Default::default()
        };
        assert!(CapabilityAvailability::from_state(&wired).expert_export);
    }

    #[tokio::test]
    async fn expert_export_is_closed_without_a_provider() {
        let bare = AppServerRouterState::default();
        assert!(expert_pack_provider(&bare).is_err());
        assert_eq!(
            code_of(export_agent_impl(&bare, "demo").await),
            "unsupported_operation"
        );
        assert_eq!(
            code_of(export_team_impl(&bare, "demo", None).await),
            "unsupported_operation"
        );
    }

    #[tokio::test]
    async fn expert_export_impls_round_trip_through_the_provider() {
        let state = AppServerRouterState {
            expert_packs: Some(Arc::new(FakeExpertPacks::ok(sample_expert_pack()))),
            ..Default::default()
        };
        let pack = export_agent_impl(&state, "wb-demo-software-team-lead")
            .await
            .unwrap();
        assert_eq!(pack.pack_format, nomifun_api_types::APP_SERVER_EXPERT_PACK_FORMAT);
        assert_eq!(pack.persona.instructions, PERSONA_SENTINEL);
        assert_eq!(pack.skills[0].name, "release-notes");
        // A pinned version that matches passes; see the mismatch test below.
        export_team_impl(&state, "wb-demo-team", Some("1.0.0")).await.unwrap();
    }

    #[tokio::test]
    async fn team_export_version_guard_refuses_a_mismatch() {
        let state = AppServerRouterState {
            expert_packs: Some(Arc::new(FakeExpertPacks::ok(sample_expert_pack()))),
            ..Default::default()
        };
        // An empty/absent pin is not a mismatch — the same reading `team/run`
        // takes (`team_version` is optional, and blank means "unset").
        export_team_impl(&state, "wb-demo-team", None).await.unwrap();
        export_team_impl(&state, "wb-demo-team", Some("   ")).await.unwrap();
        assert_eq!(
            code_of(export_team_impl(&state, "wb-demo-team", Some("2.0.0")).await),
            "version_mismatch"
        );
    }

    #[tokio::test]
    async fn expert_export_errors_keep_their_stable_codes() {
        // `policy_denied` must stay distinguishable from `not_found`: "you turned
        // this off" and "there is no such expert" call for completely different
        // client behaviour. `agent_not_installed` / `agent_disabled` likewise.
        let cases = [
            (ExpertPackError::PolicyDenied("denied".into()), "policy_denied"),
            (
                ExpertPackError::NotInstalled("member x is not installed".into()),
                "agent_not_installed",
            ),
            (ExpertPackError::Disabled("off".into()), "agent_disabled"),
            (ExpertPackError::NotFound("gone".into()), "not_found"),
            (
                ExpertPackError::TooLarge { size: 10, limit: 5 },
                "response_too_large",
            ),
            (ExpertPackError::Internal("boom".into()), "internal_error"),
        ];
        for (error, expected) in cases {
            let state = AppServerRouterState {
                expert_packs: Some(Arc::new(FakeExpertPacks::failing(error))),
                ..Default::default()
            };
            assert_eq!(code_of(export_agent_impl(&state, "demo").await), expected);
        }
    }

    #[tokio::test]
    async fn the_persona_lives_on_the_pack_and_never_on_the_catalog_faces() {
        // This is the guard that keeps the two faces apart. `agent/get` is what
        // every store UI calls while browsing; if the persona ever became a field
        // there, the export gate would be decorative — and the next person to
        // "just add a field" would have no way to tell.
        let mut summary = sample_agent_summary();
        summary.preset_id = Some("preset-demo".into());
        let detail = nomifun_api_types::AppServerAgentDetail {
            summary,
            effort: Some("high".into()),
            max_turns: Some(40),
            disallowed_tools: vec![],
            // Deliberately set to the sentinel: these two frontmatter fields ARE
            // on the catalog face, so the guard has to be about `instructions`
            // specifically, not about "any long string".
            memory: Some(PERSONA_SENTINEL.into()),
            background: None,
            isolation: None,
            permission_mode_ignored: false,
            display_description: None,
            quick_prompts: vec![],
            tags: vec![],
            default_init_prompt: None,
            expert_type: None,
            category_id: None,
        };
        let catalog_json = serde_json::to_value(&detail).unwrap();
        assert!(
            catalog_json.get("instructions").is_none(),
            "the catalog face must have no field that could carry the persona body"
        );
        assert!(
            catalog_json.get("persona").is_none(),
            "the catalog face must not grow a persona object"
        );

        let pack_json = serde_json::to_value(sample_expert_pack()).unwrap();
        assert_eq!(pack_json["persona"]["instructions"], PERSONA_SENTINEL);
        assert_eq!(
            pack_json["pack_format"],
            nomifun_api_types::APP_SERVER_EXPERT_PACK_FORMAT
        );
        // And the pack really is the only place it appears.
        assert!(!serde_json::to_string(&detail).unwrap().contains("persona"));
    }

    // The real path-safety logic lives in the host adapter
    // (`nomifun-app/src/app_server_skill_files.rs`), which owns the filesystem.
    // These tests pin the *protocol* layer's half of the contract: the
    // capability bit is honest, an uninjected seam stays closed, and each seam
    // error keeps its stable wire code — notably that `TooLarge` is
    // `response_too_large` and not folded into `invalid_request`.

    /// Skill file seam stub. `fail` is interior-mutable because the provider
    /// trait takes `&self` (production implementations are stateless) while a
    /// test needs to hand back a specific error once.
    struct FakeSkillFiles {
        fail: std::sync::Mutex<Option<SkillFileError>>,
    }

    impl FakeSkillFiles {
        fn ok() -> Self {
            Self { fail: std::sync::Mutex::new(None) }
        }

        fn failing(error: SkillFileError) -> Self {
            Self { fail: std::sync::Mutex::new(Some(error)) }
        }

        fn take_failure(&self) -> Option<SkillFileError> {
            self.fail.lock().expect("fake skill file lock").take()
        }
    }

    #[async_trait::async_trait]
    impl SkillFileProvider for FakeSkillFiles {
        async fn files(&self, skill_id: &str) -> Result<AppServerSkillFileList, SkillFileError> {
            if let Some(error) = self.take_failure() {
                return Err(error);
            }
            Ok(AppServerSkillFileList {
                skill_id: skill_id.to_owned(),
                files: vec![nomifun_api_types::AppServerSkillFile {
                    path: "SKILL.md".into(),
                    size: 12,
                    digest: "abc".into(),
                }],
                content_digest: "tree".into(),
                truncated: false,
            })
        }

        async fn read(&self, _skill_id: &str, path: &str) -> Result<SkillFileBytes, SkillFileError> {
            if let Some(error) = self.take_failure() {
                return Err(error);
            }
            Ok(SkillFileBytes {
                bytes: path.as_bytes().to_vec(),
                content_type: "text/markdown; charset=utf-8".into(),
            })
        }
    }

    /// Assert an error's wire code without requiring `Debug` on the success
    /// type: `SkillFileBytes` carries file contents and deliberately has none,
    /// so a body can never reach a log or a panic message.
    fn code_of<T>(result: Result<T, AppServerError>) -> &'static str {
        match result {
            Ok(_) => panic!("expected an error, got a success"),
            Err(error) => error.code,
        }
    }

    #[tokio::test]
    async fn skill_file_face_requires_its_own_capability() {
        // The catalog alone must NOT advertise the file face: a client that saw
        // `skills: true` and called `skill/files` would get
        // `unsupported_operation` after the fact.
        let catalog_only = AppServerRouterState {
            skills: Some(Arc::new(crate::catalog::FakeSkillCatalog { skills: vec![] })),
            ..Default::default()
        };
        let availability = CapabilityAvailability::from_state(&catalog_only);
        assert!(availability.skills);
        assert!(!availability.skill_files, "catalog alone must not advertise the file face");
        assert!(!Capabilities::from_availability(availability).skill_files);

        let wired = AppServerRouterState {
            skill_files: Some(Arc::new(FakeSkillFiles::ok())),
            ..Default::default()
        };
        let availability = CapabilityAvailability::from_state(&wired);
        assert!(availability.skill_files);
    }

    #[tokio::test]
    async fn skill_file_face_is_closed_without_a_provider() {
        let bare = AppServerRouterState::default();
        assert!(skill_file_provider(&bare).is_err());
        assert_eq!(
            code_of(list_skill_files_impl(&bare, "demo").await),
            "unsupported_operation"
        );
        assert_eq!(
            code_of(read_skill_file_impl(&bare, "demo", "SKILL.md").await),
            "unsupported_operation"
        );
    }

    #[tokio::test]
    async fn skill_file_impls_round_trip_through_the_provider() {
        let state = AppServerRouterState {
            skill_files: Some(Arc::new(FakeSkillFiles::ok())),
            ..Default::default()
        };
        let listing = list_skill_files_impl(&state, "demo").await.unwrap();
        assert_eq!(listing.skill_id, "demo");
        assert_eq!(listing.files[0].path, "SKILL.md");
        assert!(!listing.truncated);

        let file = read_skill_file_impl(&state, "demo", "hello").await.unwrap();
        assert_eq!(file.bytes, b"hello");
    }

    #[tokio::test]
    async fn skill_file_errors_keep_their_stable_codes() {
        // `TooLarge` must stay distinguishable from the other two: a caller
        // that saw `invalid_request` would treat a size refusal as a bad
        // request and retry the same way forever.
        let cases = [
            (SkillFileError::NotFound("gone".into()), "not_found"),
            (SkillFileError::InvalidRequest("bad path".into()), "invalid_request"),
            (
                SkillFileError::TooLarge { size: 10, limit: 5 },
                "response_too_large",
            ),
        ];
        for (error, expected) in cases {
            let state = AppServerRouterState {
                skill_files: Some(Arc::new(FakeSkillFiles::failing(error))),
                ..Default::default()
            };
            assert_eq!(code_of(list_skill_files_impl(&state, "demo").await), expected);
        }
    }

    // ---- Connector call proxy: `connector/call` (doc 24 §5) ---------------
    //
    // The gates and the execution live in the host adapter
    // (`nomifun-app/src/app_server_connector_call.rs`). These pin the protocol
    // layer's half: an honest capability bit, a closed seam when unwired, and —
    // the part that matters most — that each seam error keeps its own wire code,
    // so "not allowed", "switched off", "too slow" and "too big" never collapse
    // into one indistinguishable failure.

    struct FakeConnectorCalls {
        fail: std::sync::Mutex<Option<ConnectorCallError>>,
    }

    impl FakeConnectorCalls {
        fn ok() -> Self {
            Self { fail: std::sync::Mutex::new(None) }
        }

        fn failing(error: ConnectorCallError) -> Self {
            Self { fail: std::sync::Mutex::new(Some(error)) }
        }
    }

    #[async_trait::async_trait]
    impl ConnectorCallProvider for FakeConnectorCalls {
        async fn call(
            &self,
            connector_id: &str,
            tool: &str,
            arguments: serde_json::Value,
        ) -> Result<AppServerConnectorCallResult, ConnectorCallError> {
            if let Some(error) = self.fail.lock().expect("fake call lock").take() {
                return Err(error);
            }
            Ok(AppServerConnectorCallResult {
                is_error: false,
                result: serde_json::json!({
                    "connector_id": connector_id,
                    "tool": tool,
                    "arguments": arguments,
                }),
            })
        }
    }

    #[tokio::test]
    async fn connector_call_face_requires_its_own_capability() {
        // The catalog alone must not advertise the call proxy: a client that
        // saw `connectors: true` and called `connector/call` would get
        // `unsupported_operation` after the fact.
        let catalog_only = AppServerRouterState {
            connectors: Some(Arc::new(crate::catalog::FakeConnectorCatalog {
                connectors: vec![],
                auth_required_ids: vec![],
                probe_fail_ids: vec![],
            })),
            ..Default::default()
        };
        let availability = CapabilityAvailability::from_state(&catalog_only);
        assert!(availability.connectors);
        assert!(!availability.connector_calls);
        assert!(!Capabilities::from_availability(availability).connector_calls);

        let wired = AppServerRouterState {
            connector_calls: Some(Arc::new(FakeConnectorCalls::ok())),
            ..Default::default()
        };
        assert!(CapabilityAvailability::from_state(&wired).connector_calls);
    }

    #[tokio::test]
    async fn connector_call_face_is_closed_without_a_provider() {
        let bare = AppServerRouterState::default();
        assert!(connector_call_provider(&bare).is_err());
        let error = connector_call_impl(&bare, "conn-1", "echo", serde_json::json!({}), None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "unsupported_operation");
    }

    #[tokio::test]
    async fn connector_call_errors_keep_their_stable_codes() {
        let cases = [
            (ConnectorCallError::InvalidRequest("".into()), "invalid_request"),
            (ConnectorCallError::NotFound("nope".into()), "not_found"),
            (ConnectorCallError::Unavailable("off".into()), "connector_unavailable"),
            (ConnectorCallError::PolicyDenied("not listed".into()), "policy_denied"),
            (ConnectorCallError::Timeout { seconds: 3 }, "connector_call_timeout"),
            (
                ConnectorCallError::TooLarge { size: 9, limit: 1 },
                "response_too_large",
            ),
            (ConnectorCallError::Failed("boom".into()), "connector_call_failed"),
        ];
        for (error, expected) in cases {
            let state = AppServerRouterState {
                connector_calls: Some(Arc::new(FakeConnectorCalls::failing(error))),
                ..Default::default()
            };
            let wire = connector_call_impl(&state, "conn-1", "echo", serde_json::json!({}), None)
                .await
                .unwrap_err();
            assert_eq!(wire.code, expected, "wrong wire code for {expected}");
        }
    }

    #[tokio::test]
    async fn connector_call_passes_the_tool_and_arguments_through() {
        let state = AppServerRouterState {
            connector_calls: Some(Arc::new(FakeConnectorCalls::ok())),
            ..Default::default()
        };
        let result = connector_call_impl(
            &state,
            "conn-1",
            "create_issue",
            serde_json::json!({ "title": "t" }),
            None,
        )
        .await
        .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.result["tool"], "create_issue");
        assert_eq!(result.result["arguments"]["title"], "t");
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
                        // 这个测试替身只枚举磁盘上的技能；市场图标的解析在
                        // `nomifun-app`（`app_server_entry_assets`），不在这里。
                        avatar_url: None,
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
        assert_eq!(error.code, "unauthenticated");

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

    // ---- R15：附件准入（只许会话工作区内的绝对路径） -------------------------

    /// 独立的临时目录：仓库没有 `tempfile` dev-dep，这里用系统临时目录 + 唯一名，
    /// 测试自己清理。
    fn attachment_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("allo-attachments-{}", generate_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::canonicalize(&dir).expect("canonical temp dir")
    }

    #[test]
    fn attachment_paths_must_be_absolute_files_inside_the_workspace() {
        let root = attachment_temp_dir();
        let inside = root.join("shot.png");
        std::fs::write(&inside, b"png-bytes").expect("write inside file");
        std::fs::create_dir_all(root.join("nested")).expect("nested dir");

        let outside = attachment_temp_dir();
        let escaped = outside.join("secret.png");
        std::fs::write(&escaped, b"secret").expect("write outside file");

        // 工作区内的真实文件：接受，且返回的是**客户端给的字符串**（不是 canonical
        // 形态——Windows 的 canonicalize 会带 `\\?\` 前缀，运行时不吃那个）。
        let (accepted, canonical) = validate_attachment_path(&root, &inside.to_string_lossy())
            .expect("an absolute file inside the workspace is accepted");
        assert_eq!(accepted, inside.to_string_lossy());
        assert_eq!(canonical, std::fs::canonicalize(&inside).unwrap());

        let escape_via_dotdot = root
            .join("..")
            .join(outside.file_name().expect("outside dir name"))
            .join("secret.png");
        for rejected in [
            "shot.png".to_owned(),                                 // 相对路径
            "  ".to_owned(),                                       // 空
            root.join("missing.png").to_string_lossy().into_owned(), // 不存在
            root.join("nested").to_string_lossy().into_owned(),    // 目录不是文件
            escaped.to_string_lossy().into_owned(),                // 工作区之外
            escape_via_dotdot.to_string_lossy().into_owned(),      // `..` 逃逸
        ] {
            let error =
                validate_attachment_path(&root, &rejected).expect_err("must be refused");
            assert!(
                matches!(error.code, "invalid_request" | "workspace_denied"),
                "{rejected:?} must be refused, got {}",
                error.code
            );
        }

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn ws_conversation_send_takes_attachments_additively() {
        // 老客户端不传 `attachments`：行为逐字不变（空）。
        let bare: WsConversationSend = serde_json::from_value(serde_json::json!({
            "conversation_id": "c1",
            "content": "hi",
            "idempotency_key": "k1",
        }))
        .expect("clients that predate attachments keep working");
        assert!(bare.attachments.is_empty());

        let with: WsConversationSend = serde_json::from_value(serde_json::json!({
            "conversation_id": "c1",
            "content": "hi",
            "idempotency_key": "k1",
            "attachments": ["C:/ws/a.png"],
        }))
        .expect("attachments are additive");
        assert_eq!(with.attachments, vec!["C:/ws/a.png".to_owned()]);

        // `deny_unknown_fields` 仍然成立：写错字段名（例如历史上的 `files`）要被拒，
        // 而不是静默丢掉附件。
        assert!(
            serde_json::from_value::<WsConversationSend>(serde_json::json!({
                "conversation_id": "c1",
                "content": "hi",
                "idempotency_key": "k1",
                "files": [],
            }))
            .is_err()
        );
    }
}
