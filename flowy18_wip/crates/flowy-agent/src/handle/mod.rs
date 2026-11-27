//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    AcceptanceQueue, AcceptedMessage, BranchHeartbeat, DeliveryCommit, Engine,
    EngineConfig, LoopOutcome, ModelRoute, PendingCard, PendingRouterFollowup,
    PromptTurn, ReceiptState, RequiredDeliverable, SessionOrigin,
    SessionRecord, SessionRole, SessionWorkspace, StopHandoffManifest,
    SubmissionOutcome, ToolProvider, TurnObserver, TurnState, WorkAccessMode,
    complete_text,
    tools::{
        ToolGuard, ToolRegistry, pi_shell_definition_for_host,
        register_builtin_tools,
    },
    types::{
        CancelToken, ContentBlock, Message, MessageSource, Role, ToolCallArgs,
    },
};
use flowy_skills::{
    InstalledSkill, SkillId, format_available_skills, format_skill_invocation,
};

use crate::artifact_store::{
    ArtifactRecord, ArtifactStore, SessionDeletionState,
};
use crate::config::LlmConfig;
pub use crate::index::AgentWorkspace;
use crate::index::{SessionIndex, SessionRecord as IndexRecord};
use crate::projection::{
    ChatHistory, ChatLine, ChatProjection, FileChangeKind, TurnStatus,
    VerificationIssue,
};
use crate::title::{self, TITLE_MAX_CHARS};

/// The lease handed to a consumer claim; long enough for a normal turn.
const ACCEPTED_LEASE_MS: u64 = 30_000;

/// First retry delay after a failed turn; doubled per attempt, capped.
const ACCEPTED_RELEASE_BASE_MS: u64 = 5_000;
const ACCEPTED_RELEASE_MAX_MS: u64 = 5 * 60_000;

/// Attempts before a receipt is buried instead of retried. The third failure
/// is surfaced to the user; the tenth is terminal.
const ACCEPTED_MAX_ATTEMPTS: u32 = 10;

/// Maximum number of durable acceptance receipts that may execute concurrently
/// across independent workspaces.
const DEFAULT_MAX_CONCURRENT_WORKS: usize = 3;

mod artifacts;
mod automation;
mod boot;
mod guards;
mod observer;
#[cfg(test)]
mod provider_tests;
mod queue;
mod router;
#[cfg(test)]
mod router_tests;
mod sessions;
mod submit;
#[cfg(test)]
mod tests;
mod work;
mod work_events;
mod workspace_lock;

pub use self::guards::{confined_to, has_reserved_device_component};
use self::guards::{ensure_tool_cwd, tool_registry, tool_target};
use self::observer::{
    apply_delivery_commit_to_live, clear_live_heartbeat, confirm_commit_audit,
    extract_session_preview, mark_delivery_failed, persist_index,
};
pub use self::router::{
    ChildWorkSummary, DispatchWorkResult, RoutedPendingCard, StopWorkResult,
};
use self::work::{
    latest_session_submission_id, reconcile_partial_work_after_override,
    submission_committed_in_session,
};
use self::workspace_lock::{WorkspaceAccessMode, WorkspaceLockTable};

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub project_id: Option<String>,
    pub pinned: bool,
    pub archived: bool,
}

/// One scored match returned by [`AgentHandle::search_sessions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSearchHit {
    pub session_id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub updated_at: u64,
    pub pinned: bool,
    pub archived: bool,
    pub snippet: Option<String>,
    pub score: u32,
}

#[derive(Clone, Debug)]
pub enum AgentUpdate {
    Initialized,
    Sessions,
    Skills,
    Artifacts,
    Session {
        id: String,
    },
    /// Lifecycle transition for one durable acceptance receipt.
    Receipt {
        id: String,
        state: ReceiptState,
    },
    /// The session index could not be loaded or saved.
    StorageError,
}

/// Explicit target for host-level task cancellation, decoupled from UI focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskTarget {
    Session(String),
    Receipt(String),
}

/// Outcome of requesting cancellation on a [`TaskTarget`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    Cancelled,
    AlreadySettled,
    FinalizingNotCancellable,
}

/// User selection submitted to resolve a persisted [`PendingCard`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardSelection {
    Disambiguate { object_ids: Vec<String> },
    ApproveTool { args_hash: String },
    DenyTool { reason: Option<String> },
}

/// Structured card-level user actions executed exclusively by the host kernel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAction {
    OverrideDeliver {
        session_id: String,
        report_id: String,
        artifact_path: String,
    },
    RetryDelivery {
        session_id: String,
        submission_id: Option<String>,
    },
    RetryDeadReceipt {
        receipt_id: String,
    },
    ResolveCard {
        session_id: String,
        card_id: String,
        selection: CardSelection,
    },
}

/// Result of applying one [`CardAction`] inside the host kernel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardActionOutcome {
    ArtifactDelivered { artifact_id: String },
    DeliveryRetried,
    ReceiptRequeued { receipt_id: String },
    CardResolved { card_id: String, resumed: bool },
}

/// One accepted receipt claimed by `run_accepted_once`.
#[derive(Clone, Debug)]
pub struct AcceptedRun {
    pub receipt_id: String,
    pub attempt_id: String,
    pub session_id: String,
    pub submission: u128,
}

/// Unified host status for one work item (queried by `receipt_id` or
/// `submission_id`), reconciling requirements, outcome, and visible artifacts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkStatus {
    pub work_id: String,
    pub session_id: Option<String>,
    pub submission_id: Option<String>,
    pub contract_revision: Option<u32>,
    pub receipt_state: Option<ReceiptState>,
    pub outcome: Option<SubmissionOutcome>,
    pub required_items: Vec<RequiredDeliverable>,
    pub delivered_artifact_ids: Vec<String>,
    pub missing_items: Vec<String>,
}

/// Maximum automatic LLM title attempts per session before falling back
/// permanently to the graceful fallback title (prevents offline retry loops).
const MAX_AUTO_TITLE_ATTEMPTS: u8 = 2;

struct Live {
    projection: ChatProjection,
    title: String,
    titled: bool,
    pinned: bool,
    archived: bool,
    /// A sidecar title request is still needed for this session.
    auto_title_pending: bool,
    /// Guards against spawning duplicate concurrent title requests.
    auto_title_in_flight: bool,
    /// Total sidecar title attempts already dispatched for this session.
    auto_title_attempts: u8,
    /// Cancels the in-flight title request on stop or manual rename.
    title_signal: CancelToken,
    status: TurnStatus,
    updated_at: u64,
    turn_started_at: Option<u64>,
    workspace: Option<AgentWorkspace>,
    origin: SessionOrigin,
    session_workspace: Option<SessionWorkspace>,
    heartbeat: Option<BranchHeartbeat>,
    role: SessionRole,
    root_session_id: Option<String>,
    access_mode: WorkAccessMode,
    /// True while the current turn is a background host event (`[WorkEventBatch]` / `SystemEvent`).
    active_turn_is_internal: bool,
    /// Non-busy status captured before an internal turn so transient failures restore cleanly.
    active_turn_prev_status: Option<TurnStatus>,
    /// Canonical card key (`verify:...` / `artifact:...`) -> originating dispatch `receipt_id`.
    card_receipts: BTreeMap<String, String>,
    /// Dispatch `receipt_id` for the currently active (or latest) Worker dispatch cycle.
    active_dispatch_receipt_id: Option<String>,
}

struct ResumedSubmission {
    pub(super) contract: Option<crate::verify::BriefContract>,
    pub(super) skip_contract_evolve: bool,
    pub(super) model_prompt_override: Option<String>,
    pub(super) model_prompt_suffix: Option<String>,
    pub(super) display_text_override: Option<String>,
    pub(super) skip_push_user_line: bool,
    pub(super) preserve_turn_denial: bool,
    pub(super) preserve_tool_approval: bool,
    pub(super) prompt_source: MessageSource,
    pub(super) is_router_announcement: bool,
    pub(super) bypass_busy_queue: bool,
    pub(super) dispatch_receipt_id: Option<String>,
    pub(super) force_reconfirm_tools: BTreeSet<String>,
    pub(super) pre_staged: Option<guards::PreStagedWorkerProviders>,
}

impl Default for ResumedSubmission {
    fn default() -> Self {
        Self {
            contract: None,
            skip_contract_evolve: false,
            model_prompt_override: None,
            model_prompt_suffix: None,
            display_text_override: None,
            skip_push_user_line: false,
            preserve_turn_denial: false,
            preserve_tool_approval: false,
            prompt_source: MessageSource::User,
            is_router_announcement: false,
            bypass_busy_queue: false,
            dispatch_receipt_id: None,
            force_reconfirm_tools: BTreeSet::new(),
            pre_staged: None,
        }
    }
}

#[derive(Clone)]
pub struct RegisteredProvider {
    pub(super) registration_id: u64,
    pub(super) provider: Arc<dyn ToolProvider>,
    pub(super) cached_router_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderApprovalBinding {
    pub(super) provider_id: String,
    pub(super) registration_id: u64,
    pub(super) tool_name: String,
    pub(super) tool_fingerprint: String,
    pub(super) cwd_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedToolGrant {
    pub(super) tool_name: String,
    pub(super) args_hash: String,
    pub(super) provider_binding: Option<ProviderApprovalBinding>,
}

impl ApprovedToolGrant {
    pub(super) const fn host(tool_name: String, args_hash: String) -> Self {
        Self {
            tool_name,
            args_hash,
            provider_binding: None,
        }
    }

    pub(super) const fn provider(
        tool_name: String,
        args_hash: String,
        binding: ProviderApprovalBinding,
    ) -> Self {
        Self {
            tool_name,
            args_hash,
            provider_binding: Some(binding),
        }
    }
}

#[derive(Clone)]
struct AcceptedClaim {
    receipt_id: String,
    attempt_id: String,
    session_id: String,
    submission_id: String,
    attempts: u32,
    heartbeat_cancel: CancelToken,
}

#[cfg(test)]
type AcceptedSubmitHook = Arc<dyn Fn(&str) + Send + Sync>;

#[cfg(test)]
type WorkerFaultHook =
    Arc<dyn Fn(&str, &str) -> Result<(), &'static str> + Send + Sync>;

struct UiState {
    root: PathBuf,
    live: Mutex<HashMap<String, Live>>,
    /// File-tool arguments held between `tool_start` and `tool_end`, keyed by
    /// session and call id, so the change card never re-reads the file.
    tool_args: Mutex<HashMap<String, serde_json::Value>>,
    /// One-shot pre-tool approval grants pinned to exact invocation + live
    /// provider identity per session, consumed upon execution.
    approved_tool_calls: Mutex<HashMap<String, Vec<ApprovedToolGrant>>>,
    /// In-memory live binding recorded when a provider `PreToolDecision::Ask`
    /// parks `PendingCard::ToolApproval`, keyed by `(session_id, args_hash)`.
    /// Cleared on provider swap/unregister, turn settle, and process restart.
    pending_provider_approvals:
        Mutex<HashMap<(String, String), ProviderApprovalBinding>>,
    /// Turn-scoped `UserDenied` anti-bypass guard per session; cleared once the
    /// resumed denial turn completes or a brand-new user turn starts.
    denied_tool_turns: Mutex<HashMap<String, String>>,
    active: Mutex<Option<String>>,
    /// Active acceptance receipt claims keyed by `receipt_id`.
    active_claims: Mutex<HashMap<String, AcceptedClaim>>,
    /// Normalized directory-level read/write leases across all running turns.
    workspace_locks: WorkspaceLockTable,
    /// Per-Router bounded memory inbox for coalescing Worker terminal events.
    /// Strict leaf lock.
    router_inboxes: Mutex<HashMap<String, work_events::RouterInbox>>,
    max_concurrent_works: AtomicUsize,
    accepted_lease_ms: AtomicU64,
    autonomous_drain: AtomicBool,
    shutdown_requested: AtomicBool,
    drain_wakeup_scheduled: AtomicBool,
    /// Epoch ms deadline of the currently scheduled drain backoff sleep (`0` = none).
    drain_sleep_until_ms: AtomicU64,
    drain_cancel: CancelToken,
    #[cfg(test)]
    before_accepted_bind_hook: Mutex<Option<AcceptedSubmitHook>>,
    #[cfg(test)]
    before_accepted_submit_hook: Mutex<Option<AcceptedSubmitHook>>,
    #[cfg(test)]
    before_worker_strict_enqueue_hook: Mutex<Option<WorkerFaultHook>>,
    #[cfg(test)]
    after_first_child_deleted_hook: Mutex<Option<WorkerFaultHook>>,
    workspace_for_new_sessions: Mutex<Option<AgentWorkspace>>,
    origin_for_new_sessions: Mutex<SessionOrigin>,
    artifact_store: OnceLock<ArtifactStore>,
    /// Backs the Worker automation tools; `None` hides them.
    automation: Mutex<Option<Arc<flowy_automation::AutomationService>>>,
    initializing: Mutex<bool>,
    context_generation: AtomicU64,
    semantic_verify_enabled: AtomicBool,
    updates: Mutex<VecDeque<AgentUpdate>>,
    notified_quarantined: Mutex<BTreeSet<String>>,
    load_failed: Mutex<bool>,
    history_unavailable: Mutex<bool>,
    accept_drain: Mutex<()>,
    dispatch: Mutex<()>,
    index_write: Mutex<()>,
}

fn env_semantic_verify_enabled() -> bool {
    !matches!(
        std::env::var("FLOWY_L3_VERIFY")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("0" | "false" | "off")
    )
}

impl UiState {
    fn new(root: PathBuf, artifact_store: Option<ArtifactStore>) -> Self {
        let store_slot = OnceLock::new();
        if let Some(store) = artifact_store {
            let _ = store_slot.set(store);
        }
        Self {
            root,
            live: Mutex::new(HashMap::new()),
            tool_args: Mutex::new(HashMap::new()),
            approved_tool_calls: Mutex::new(HashMap::new()),
            pending_provider_approvals: Mutex::new(HashMap::new()),
            denied_tool_turns: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            active_claims: Mutex::new(HashMap::new()),
            workspace_locks: WorkspaceLockTable::new(),
            router_inboxes: Mutex::new(HashMap::new()),
            max_concurrent_works: AtomicUsize::new(
                DEFAULT_MAX_CONCURRENT_WORKS,
            ),
            accepted_lease_ms: AtomicU64::new(ACCEPTED_LEASE_MS),
            autonomous_drain: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            drain_wakeup_scheduled: AtomicBool::new(false),
            drain_sleep_until_ms: AtomicU64::new(0),
            drain_cancel: CancelToken::new(),
            #[cfg(test)]
            before_accepted_bind_hook: Mutex::new(None),
            #[cfg(test)]
            before_accepted_submit_hook: Mutex::new(None),
            #[cfg(test)]
            before_worker_strict_enqueue_hook: Mutex::new(None),
            #[cfg(test)]
            after_first_child_deleted_hook: Mutex::new(None),
            workspace_for_new_sessions: Mutex::new(None),
            origin_for_new_sessions: Mutex::new(SessionOrigin::Unknown),
            artifact_store: store_slot,
            automation: Mutex::new(None),
            initializing: Mutex::new(true),
            context_generation: AtomicU64::new(0),
            semantic_verify_enabled: AtomicBool::new(
                env_semantic_verify_enabled(),
            ),
            updates: Mutex::new(VecDeque::new()),
            notified_quarantined: Mutex::new(BTreeSet::new()),
            load_failed: Mutex::new(false),
            history_unavailable: Mutex::new(false),
            accept_drain: Mutex::new(()),
            dispatch: Mutex::new(()),
            index_write: Mutex::new(()),
        }
    }

    fn push_update(&self, update: AgentUpdate) {
        lock(&self.updates).push_back(update);
    }

    fn stream_changed(&self, session: &str) {
        let mut updates = lock(&self.updates);
        if !updates.iter().any(|update| matches!(update, AgentUpdate::Session { id } if id == session)) {
            updates.push_back(AgentUpdate::Session { id: session.to_owned() });
        }
    }

    fn report_storage_error(&self) {
        tracing::error!(
            target: "flowy.storage",
            operation = "report session storage failure",
            root = %self.root.display(),
            "session storage failure reached the UI"
        );
        self.push_update(AgentUpdate::StorageError);
    }
}

/// External-only owner of the Tokio [`tokio::runtime::Runtime`].
///
/// Internal spawned tasks hold only `Weak<Harness>` (which owns a cheap
/// [`tokio::runtime::Handle`]), guaranteeing `Runtime::drop` never executes on
/// a Tokio worker thread when external [`AgentHandle`]s are dropped.
struct RuntimeOwner {
    state: Arc<UiState>,
    engine: Arc<Mutex<Option<Arc<Engine>>>>,
    _runtime: tokio::runtime::Runtime,
}

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        self.state.shutdown_requested.store(true, Ordering::Release);
        self.state.autonomous_drain.store(false, Ordering::Release);
        self.state.drain_cancel.cancel();
        let engine = lock(&self.engine).clone();
        if let Some(engine) = engine {
            engine.cancel_all_active();
        }
    }
}

/// Lock ordering invariant (outer -> inner; never acquire in reverse):
/// `accept_drain` -> `dispatch` -> `engine` (clone & release) ->
/// `index_write` -> `live` -> leaf locks (`active`, `active_claims`,
/// `workspace_locks`, `router_inboxes`, `updates`, `tool_args`,
/// `approved_tool_calls`, `pending_provider_approvals`, `denied_tool_turns`,
/// `notified_quarantined`, `providers`).
///
/// `providers` is strictly a leaf lock: callers must clone the `Vec` snapshot
/// and release it immediately before invoking any provider callback.
struct Harness {
    runtime: tokio::runtime::Handle,
    state: Arc<UiState>,
    engine: Arc<Mutex<Option<Arc<Engine>>>>,
    config: Mutex<LlmConfig>,
    skills: Mutex<Vec<InstalledSkill>>,
    process_nonce: String,
    next_provider_registration_id: AtomicU64,
    providers: Mutex<Vec<RegisteredProvider>>,
}

struct SubmissionSelection<'a> {
    active: Option<&'a str>,
    generation: u64,
}

#[derive(Clone)]
pub struct AgentHandle {
    inner: Arc<Harness>,
    _runtime: Arc<RuntimeOwner>,
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHandle").finish_non_exhaustive()
    }
}

impl Default for AgentHandle {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("flowy-agent"))
    }
}

impl AgentHandle {
    #[must_use]
    pub fn new(store_root: PathBuf) -> Self {
        Self::with_config(store_root, LlmConfig::from_env())
    }

    #[must_use]
    pub fn with_config(store_root: PathBuf, config: LlmConfig) -> Self {
        let handle = Self::uninitialized(store_root, config);
        handle.initialize();
        handle
    }

    /// Load history, recover artifact groups and clean pending deletions off
    /// the caller's UI thread. Queries remain empty/loading until Initialized.
    #[must_use]
    pub fn with_config_background(
        store_root: PathBuf,
        config: LlmConfig,
    ) -> Self {
        let handle = Self::uninitialized(store_root, config);
        let worker = handle.clone();
        if std::thread::Builder::new()
            .name("flowy-storage-init".into())
            .spawn(move || worker.initialize())
            .is_err()
        {
            *lock(&handle.inner.state.load_failed) = true;
            *lock(&handle.inner.state.initializing) = false;
            handle.inner.state.report_storage_error();
            handle.inner.state.push_update(AgentUpdate::Initialized);
        }
        handle
    }

    pub fn is_initializing(&self) -> bool {
        *lock(&self.inner.state.initializing)
    }

    fn uninitialized(store_root: PathBuf, config: LlmConfig) -> Self {
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::with_config",
            root = %store_root.display(),
            live = config.api_key.is_some(),
            model = %config.model,
            "enter"
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("flowy-agent")
            .build()
            .expect("tokio runtime for agent harness");
        let rt_handle = runtime.handle().clone();
        let state = Arc::new(UiState::new(store_root, None));
        let engine = Arc::new(Mutex::new(None));
        let owner = Arc::new(RuntimeOwner {
            state: Arc::clone(&state),
            engine: Arc::clone(&engine),
            _runtime: runtime,
        });
        let mut nonce_bytes = [0_u8; 16];
        let process_nonce = if getrandom::fill(&mut nonce_bytes).is_ok() {
            nonce_bytes
                .iter()
                .fold(String::with_capacity(32), |mut out, b| {
                    use std::fmt::Write as _;
                    let _ = write!(out, "{b:02x}");
                    out
                })
        } else {
            format!("fallback-{}-{}", std::process::id(), next_stamp())
        };
        Self {
            inner: Arc::new(Harness {
                runtime: rt_handle,
                state,
                engine,
                config: Mutex::new(config),
                skills: Mutex::new(Vec::new()),
                process_nonce,
                next_provider_registration_id: AtomicU64::new(1),
                providers: Mutex::new(Vec::new()),
            }),
            _runtime: owner,
        }
    }

    pub fn replace_skill_catalog(&self, skills: Vec<InstalledSkill>) {
        *lock(&self.inner.skills) = skills;
        self.inner.state.push_update(AgentUpdate::Skills);
    }

    /// Register or idempotently replace (by [`ToolProvider::id`]) a stateful
    /// Worker capability provider.
    ///
    /// Allocates a fresh process-local registration ID, pre-caches
    /// `router_capability_hint()` outside any lock (so Router prompt builds
    /// perform zero provider callbacks), and invalidates any pending approvals
    /// belonging to a replaced provider instance.
    pub fn register_tool_provider(&self, provider: Arc<dyn ToolProvider>) {
        let id = provider.id().trim().to_owned();
        if id.is_empty() {
            return;
        }
        let cached_router_hint = router::normalize_router_provider_hint(
            provider.router_capability_hint(),
        );
        let registration_id = self
            .inner
            .next_provider_registration_id
            .fetch_add(1, Ordering::Relaxed);
        let entry = RegisteredProvider {
            registration_id,
            provider,
            cached_router_hint,
        };
        let replaced_reg_id = {
            let mut guard = lock(&self.inner.providers);
            if let Some(existing) = guard
                .iter_mut()
                .find(|item| item.provider.id().trim() == id)
            {
                let old_id = existing.registration_id;
                *existing = entry;
                Some(old_id)
            } else {
                guard.push(entry);
                None
            }
        };
        if let Some(old_id) = replaced_reg_id {
            lock(&self.inner.state.pending_provider_approvals)
                .retain(|_, binding| binding.registration_id != old_id);
        }
    }

    /// Unregister a previously registered [`ToolProvider`] by ID. Returns
    /// `true` if a matching provider was removed.
    #[must_use]
    pub fn unregister_tool_provider(&self, provider_id: &str) -> bool {
        let trimmed = provider_id.trim();
        if trimmed.is_empty() {
            return false;
        }
        let mut removed_ids = Vec::new();
        {
            let mut guard = lock(&self.inner.providers);
            guard.retain(|item| {
                if item.provider.id().trim() == trimmed {
                    removed_ids.push(item.registration_id);
                    false
                } else {
                    true
                }
            });
        }
        if removed_ids.is_empty() {
            return false;
        }
        lock(&self.inner.state.pending_provider_approvals).retain(
            |_, binding| !removed_ids.contains(&binding.registration_id),
        );
        true
    }

    /// Snapshot IDs of currently registered [`ToolProvider`]s in order.
    #[must_use]
    pub fn registered_tool_provider_ids(&self) -> Vec<String> {
        lock(&self.inner.providers)
            .iter()
            .map(|item| item.provider.id().to_owned())
            .collect()
    }

    #[must_use]
    pub fn skill_catalog(&self) -> Vec<InstalledSkill> {
        self.inner.skill_catalog()
    }

    pub fn apply_config(&self, config: LlmConfig) -> Result<(), &'static str> {
        {
            let initializing = lock(&self.inner.state.initializing);
            if *initializing {
                *lock(&self.inner.config) = config;
                return Ok(());
            }
        }
        if *lock(&self.inner.state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let Some(engine) = lock(&self.inner.engine).clone() else {
            return Err("notice.agent_failed");
        };
        let cwd = ensure_tool_cwd(&self.inner.state.root);
        engine.set_config(engine_config(&config, &cwd, &self.inner.state.root));
        let runnable = config.can_run_turns();
        *lock(&self.inner.config) = config;
        if runnable && self.inner.state.autonomous_drain.load(Ordering::Acquire)
        {
            self.inner.kick_autonomous_drain();
        }
        if runnable {
            self.inner.schedule_pending_router_followups();
        }
        Ok(())
    }

    /// Honest OS sandbox availability status for the host shell layer.
    #[must_use]
    pub fn sandbox_availability(
        &self,
    ) -> flowy_agent_core::tools::SandboxAvailability {
        flowy_agent_core::tools::detect_sandbox_availability()
    }

    pub fn drain_updates(&self) -> Vec<AgentUpdate> {
        lock(&self.inner.state.updates).drain(..).collect()
    }

    /// Durable session role (`Standard`, `Router`, or `Worker`).
    #[must_use]
    pub fn session_role(&self, session_id: &str) -> SessionRole {
        self.inner.session_role(session_id)
    }

    /// Durable FIFO user follow-ups currently queued on a Router session.
    #[must_use]
    pub fn pending_router_followups(
        &self,
        router_id: &str,
    ) -> Vec<PendingRouterFollowup> {
        let engine = lock(&self.inner.engine).clone();
        engine.map_or_default(|eng| eng.pending_router_followups(router_id))
    }

    pub fn active_session_id(&self) -> Option<String> {
        lock(&self.inner.state.active).clone()
    }

    /// Returns the IDs of sessions whose turn is currently running.
    #[must_use]
    pub fn busy_session_ids(&self) -> Vec<String> {
        let live = lock(&self.inner.state.live);
        let mut ids: Vec<(u64, String)> = live
            .iter()
            .filter(|(_, session)| session.status.is_busy())
            .map(|(id, session)| (session.updated_at, id.clone()))
            .collect();
        drop(live);
        ids.sort_by_key(|(updated_at, _)| std::cmp::Reverse(*updated_at));
        ids.into_iter().map(|(_, id)| id).collect()
    }

    /// Number of sessions currently executing a turn.
    #[must_use]
    pub fn running_session_count(&self) -> usize {
        lock(&self.inner.state.live)
            .values()
            .filter(|session| session.status.is_busy())
            .count()
    }

    /// Returns a compact single-line preview of the latest user or assistant
    /// message in `session_id`, if any.
    #[must_use]
    pub fn session_preview(&self, session_id: &str) -> Option<String> {
        lock(&self.inner.state.live)
            .get(session_id)
            .and_then(|session| {
                extract_session_preview(&session.projection.lines)
            })
    }

    /// Returns `true` when `session_id` (or any owned child Worker when
    /// `session_id` is a [`SessionRole::Router`]) has an unsettled failed
    /// verification gate in its latest turn, a pending card, a `Partial` latest
    /// submission, or a turn/delivery error awaiting user attention.
    #[must_use]
    pub fn session_needs_attention(&self, session_id: &str) -> bool {
        if self.single_session_needs_attention(session_id) {
            return true;
        }
        if self.inner.session_role(session_id) == SessionRole::Router {
            let engine = lock(&self.inner.engine).clone();
            if let Some(eng) = engine {
                return eng.child_session_ids(session_id).iter().any(
                    |child_id| self.single_session_needs_attention(child_id),
                );
            }
        }
        false
    }

    fn single_session_needs_attention(&self, session_id: &str) -> bool {
        let live_attention = lock(&self.inner.state.live)
            .get(session_id)
            .is_some_and(|session| {
                if matches!(
                    session.status,
                    TurnStatus::Error { .. }
                        | TurnStatus::DeliveryFailed
                        | TurnStatus::WaitingForUser
                ) {
                    return true;
                }
                let latest_turn_start = session
                    .projection
                    .lines
                    .iter()
                    .rposition(|line| matches!(line, ChatLine::User { .. }))
                    .map_or(0, |index| index + 1);
                session.projection.lines[latest_turn_start..].iter().any(
                    |line| {
                        matches!(
                            line,
                            ChatLine::Verification {
                                ok: false,
                                override_used: false,
                                ..
                            }
                        )
                    },
                )
            });
        if live_attention {
            return true;
        }
        let engine = lock(&self.inner.engine).clone();
        engine
            .as_ref()
            .and_then(|engine| {
                engine.with_session(session_id, |record| {
                    if !record.pending_cards.is_empty()
                        || record.status == TurnState::WaitingForUser
                    {
                        return true;
                    }
                    latest_session_submission_id(record).is_some_and(|sub_id| {
                        matches!(
                            record.submission_outcomes.get(&sub_id),
                            Some(SubmissionOutcome::Partial { .. })
                        )
                    })
                })
            })
            .unwrap_or(false)
    }

    /// Returns the live [`TurnStatus`] of `session_id` in O(1) without disk I/O
    /// or cloning session history.
    #[must_use]
    pub fn session_status(&self, session_id: &str) -> TurnStatus {
        lock(&self.inner.state.live)
            .get(session_id)
            .map_or(TurnStatus::Idle, |session| session.status.clone())
    }

    /// Returns the projected lines of `session_id` so UI inspectors can
    /// render localized progress without mixing presentation copy into the
    /// agent layer.
    #[must_use]
    pub fn session_lines(&self, session_id: &str) -> Vec<ChatLine> {
        lock(&self.inner.state.live)
            .get(session_id)
            .map_or_default(|session| session.projection.lines.clone())
    }

    /// Returns persisted interactive cards awaiting user resolution on
    /// `session_id` (disambiguation or tool-approval gates).
    #[must_use]
    pub fn session_pending_cards(&self, session_id: &str) -> Vec<PendingCard> {
        let engine = lock(&self.inner.engine).clone();
        engine
            .as_ref()
            .and_then(|engine| {
                engine.with_session(session_id, |rec| rec.pending_cards.clone())
            })
            .unwrap_or_default()
    }

    /// Returns the most recent persisted [`StopHandoffManifest`] recorded for
    /// `session_id` after cancellation cleanup settled, if any.
    #[must_use]
    pub fn session_stop_handoff(
        &self,
        session_id: &str,
    ) -> Option<StopHandoffManifest> {
        let engine = lock(&self.inner.engine).clone();
        engine.as_ref().and_then(|engine| {
            engine.with_session(session_id, |rec| rec.last_stop_handoff.clone())
        })?
    }

    /// Returns the latest honest 3-part execution heartbeat for `session_id`.
    #[must_use]
    pub fn session_heartbeat(
        &self,
        session_id: &str,
    ) -> Option<BranchHeartbeat> {
        lock(&self.inner.state.live)
            .get(session_id)
            .and_then(|session| session.heartbeat.clone())
    }

    /// Memory-only selection revision, including empty Home/Project contexts.
    pub fn context_generation(&self) -> u64 {
        self.inner.state.context_generation.load(Ordering::Acquire)
    }

    /// Set the workspace inherited by sessions created from this context.
    /// Existing sessions retain the workspace saved with their own history.
    pub fn set_workspace_for_new_sessions(
        &self,
        workspace: Option<AgentWorkspace>,
    ) {
        *lock(&self.inner.state.workspace_for_new_sessions) = workspace;
        self.inner
            .state
            .context_generation
            .fetch_add(1, Ordering::AcqRel);
    }

    /// Set the explicit origin used by sessions created through the host.
    /// Legacy and restored sessions keep their persisted origin.
    pub fn set_origin_for_new_sessions(&self, origin: SessionOrigin) {
        *lock(&self.inner.state.origin_for_new_sessions) = origin;
        self.inner
            .state
            .context_generation
            .fetch_add(1, Ordering::AcqRel);
    }

    /// Create a new global-home session with an isolated, durable workspace.
    pub fn create_home_session(&self) -> Result<String, &'static str> {
        *lock(&self.inner.state.workspace_for_new_sessions) = None;
        self.set_origin_for_new_sessions(SessionOrigin::Home);
        self.create_session()
    }

    /// Create an explicit Home-origin [`SessionRole::Router`] root conversation.
    pub fn create_router_session(&self) -> Result<String, &'static str> {
        let id = self.inner.create_session_with_role_inner(
            SessionOrigin::Home,
            None,
            true,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )?;
        *lock(&self.inner.state.workspace_for_new_sessions) = None;
        *lock(&self.inner.state.origin_for_new_sessions) = SessionOrigin::Home;
        Ok(id)
    }

    /// Create an explicit Project-origin [`SessionRole::Router`] root
    /// conversation bound to `workspace` (Router itself never locks `workspace`;
    /// spawned Workers inherit `workspace`).
    pub fn create_router_project_session(
        &self,
        workspace: AgentWorkspace,
    ) -> Result<String, &'static str> {
        let origin = SessionOrigin::Project {
            project_id: Some(workspace.project_id.clone()),
        };
        let id = self.inner.create_session_with_role_inner(
            origin.clone(),
            Some(workspace.clone()),
            true,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )?;
        *lock(&self.inner.state.workspace_for_new_sessions) = Some(workspace);
        *lock(&self.inner.state.origin_for_new_sessions) = origin;
        Ok(id)
    }

    /// Submit one durable user message directly to an explicit `router_id`
    /// (decoupled from UI `active` focus; queues FIFO durably while Router is busy).
    pub fn submit_router_message(
        &self,
        router_id: &str,
        submission: u128,
        text: &str,
    ) -> Result<String, &'static str> {
        if self.inner.is_initializing() {
            return Err("library.loading");
        }
        if self.inner.session_role(router_id) != SessionRole::Router {
            return Err("notice.agent_failed");
        }
        self.inner.submit_message_targeted(
            Some(router_id),
            text,
            Some(submission),
            None,
            None,
            None,
            None,
        )
    }

    /// Atomically validate the UI's captured draft context (`expected_active`,
    /// `expected_generation`, `expected_workspace`), lazily create a
    /// [`SessionRole::Router`] when no session is active, and submit `text`
    /// with idempotent `submission`.
    ///
    /// Rolls back a freshly created empty Router if submission fails before
    /// committing so failed first sends never leave ghost conversations behind.
    pub fn submit_router_chat_if_active(
        &self,
        submission: u128,
        text: &str,
        expected_active: Option<&str>,
        expected_generation: u64,
        expected_workspace: Option<AgentWorkspace>,
    ) -> Result<String, &'static str> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("notice.prompt_empty");
        }
        if self.inner.is_initializing() {
            return Err("library.loading");
        }
        let state = &self.inner.state;
        if *lock(&state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let _dispatch = lock(&state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        if let Some(existing_sid) = engine.submitted_session(submission) {
            return Ok(existing_sid);
        }
        let expected_card_id = format!("disambig-submission-{submission:032x}");
        if let Some(existing_sid) =
            engine.sessions().into_iter().find_map(|rec| {
                rec.pending_cards
                    .iter()
                    .any(|card| card.card_id() == expected_card_id)
                    .then_some(rec.id)
            })
        {
            return Ok(existing_sid);
        }
        let still_current = state.context_generation.load(Ordering::Acquire)
            == expected_generation
            && lock(&state.active).as_deref() == expected_active;
        if let Some(router_id) = expected_active {
            if self.inner.session_role(router_id) != SessionRole::Router {
                return Err("notice.agent_failed");
            }
            return self.inner.submit_message_targeted_locked(
                Some(router_id),
                text,
                Some(submission),
                None,
                None,
                None,
                None,
            );
        }
        let origin = match &expected_workspace {
            Some(ws) => SessionOrigin::Project {
                project_id: Some(ws.project_id.clone()),
            },
            None => SessionOrigin::Home,
        };
        let router_id = self.inner.create_session_with_role_inner(
            origin.clone(),
            expected_workspace.clone(),
            still_current,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )?;
        let created_home_dir =
            matches!(origin, SessionOrigin::Home).then(|| {
                state
                    .root
                    .join("sessions")
                    .join("workspaces")
                    .join(&router_id)
            });
        match self.inner.submit_message_targeted_locked(
            Some(&router_id),
            text,
            Some(submission),
            None,
            None,
            None,
            None,
        ) {
            Ok(sid) => {
                if still_current {
                    *lock(&state.workspace_for_new_sessions) =
                        expected_workspace;
                    *lock(&state.origin_for_new_sessions) = origin;
                }
                Ok(sid)
            }
            Err(err) => {
                let _ = engine.remove_session(&router_id);
                lock(&state.live).remove(&router_id);
                if lock(&state.active).as_deref() == Some(router_id.as_str()) {
                    *lock(&state.active) = None;
                }
                if let Some(dir) = created_home_dir.as_deref() {
                    let _ = std::fs::remove_dir_all(dir);
                }
                let _ = persist_index(state);
                state.push_update(AgentUpdate::Sessions);
                Err(err)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn set_before_worker_strict_enqueue_hook(
        &self,
        hook: Option<WorkerFaultHook>,
    ) {
        *lock(&self.inner.state.before_worker_strict_enqueue_hook) = hook;
    }

    #[cfg(test)]
    pub(crate) fn set_after_first_child_deleted_hook(
        &self,
        hook: Option<WorkerFaultHook>,
    ) {
        *lock(&self.inner.state.after_first_child_deleted_hook) = hook;
    }

    #[cfg(test)]
    pub(crate) fn inject_work_event_for_test(
        &self,
        router_id: &str,
        worker_id: &str,
        submission_id: Option<String>,
        contract_revision: Option<u32>,
    ) {
        work_events::push_work_event(
            &self.inner.state,
            router_id,
            work_events::WorkEvent {
                worker_id: worker_id.to_owned(),
                submission_id,
                contract_revision,
                kind: work_events::WorkEventKind::Completed,
                observed_at_ms: now_ms(),
            },
        );
        work_events::schedule_router_inbox_flush(&self.inner, router_id);
    }

    /// Return the active session's workspace, or the context for the next
    /// session when no session is active.
    #[must_use]
    pub fn active_workspace(&self) -> Option<AgentWorkspace> {
        let active = lock(&self.inner.state.active).clone();
        if let Some(id) = active {
            return lock(&self.inner.state.live)
                .get(&id)
                .and_then(|session| session.workspace.clone());
        }
        lock(&self.inner.state.workspace_for_new_sessions).clone()
    }

    pub fn has_messages(&self) -> bool {
        let active = lock(&self.inner.state.active).clone();
        active.is_some_and(|id| {
            lock(&self.inner.state.live)
                .get(&id)
                .is_some_and(|session| !session.projection.lines.is_empty())
        })
    }

    pub fn lines(&self) -> Vec<ChatLine> {
        self.chat_history().rows
    }

    pub fn chat_history(&self) -> ChatHistory {
        let Some(active_id) = lock(&self.inner.state.active).clone() else {
            return ChatHistory::default();
        };
        let Some((mut rows, streaming, status, role)) =
            lock(&self.inner.state.live).get(&active_id).map(|session| {
                (
                    session.projection.visible_lines(),
                    session.projection.streaming_row(),
                    session.status.clone(),
                    session.role,
                )
            })
        else {
            return ChatHistory::default();
        };
        if role == SessionRole::Router {
            self.append_projected_child_cards(&active_id, &mut rows);
        }
        ChatHistory {
            rows,
            streaming,
            status,
        }
    }

    /// Deterministically project owned child Workers' `Verification` (tagged
    /// with `source_session_id`) and `Artifact` cards onto a parent Router
    /// timeline, deduplicated by report/artifact identity so refresh and
    /// restart render each card strictly once.
    fn append_projected_child_cards(
        &self,
        router_id: &str,
        rows: &mut Vec<ChatLine>,
    ) {
        let Some(engine) = lock(&self.inner.engine).clone() else {
            return;
        };
        let child_ids = engine.child_session_ids(router_id);
        if child_ids.is_empty() {
            return;
        }

        #[derive(Clone, Copy)]
        enum CardSlot {
            Row(usize),
            Turn(usize, usize),
            Tail(usize),
        }

        let mut seen_artifacts: BTreeSet<String> = rows
            .iter()
            .filter_map(|line| match line {
                ChatLine::Artifact { artifact_id } => Some(artifact_id.clone()),
                _ => None,
            })
            .collect();
        let mut seen_reports: HashMap<String, CardSlot> = HashMap::new();
        for (idx, line) in rows.iter().enumerate() {
            if let ChatLine::Verification {
                report_id,
                artifact_path,
                ..
            } = line
            {
                let key = verification_dedup_key(report_id, artifact_path);
                seen_reports.insert(key, CardSlot::Row(idx));
            }
        }

        let live = lock(&self.inner.state.live);
        let user_positions: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| {
                matches!(line, ChatLine::User { .. }).then_some(idx)
            })
            .collect();
        let work_turns =
            Self::collect_work_dispatch_turns(rows, &user_positions);
        let mut turn_inserts: Vec<Vec<ChatLine>> =
            vec![Vec::new(); user_positions.len()];
        let mut tail_inserts: Vec<ChatLine> = Vec::new();
        let push_to_slot = |target_turn: Option<usize>,
                            card: ChatLine,
                            turn_inserts: &mut [Vec<ChatLine>],
                            tail_inserts: &mut Vec<ChatLine>|
         -> CardSlot {
            if let Some(turn) = target_turn {
                let pos = turn_inserts[turn].len();
                turn_inserts[turn].push(card);
                CardSlot::Turn(turn, pos)
            } else {
                let pos = tail_inserts.len();
                tail_inserts.push(card);
                CardSlot::Tail(pos)
            }
        };
        for child_id in &child_ids {
            let Some(child_live) = live.get(child_id) else {
                continue;
            };
            for line in &child_live.projection.lines {
                match line {
                    ChatLine::Verification {
                        report_id,
                        artifact_path,
                        artifact_hash,
                        ok,
                        issues,
                        semantic_hint,
                        override_used,
                        provenance_warnings,
                        ..
                    } => {
                        let key =
                            verification_dedup_key(report_id, artifact_path);
                        let receipt_id = child_live
                            .card_receipts
                            .get(&verification_receipt_key(
                                report_id,
                                artifact_path,
                            ))
                            .map(String::as_str);
                        let target_turn =
                            work_turns.resolve(child_id, receipt_id);
                        let card = ChatLine::Verification {
                            report_id: report_id.clone(),
                            artifact_path: artifact_path.clone(),
                            artifact_hash: artifact_hash.clone(),
                            ok: *ok,
                            issues: issues.clone(),
                            semantic_hint: semantic_hint.clone(),
                            override_used: *override_used,
                            provenance_warnings: provenance_warnings.clone(),
                            source_session_id: Some(child_id.clone()),
                        };
                        match seen_reports.get(&key).copied() {
                            Some(CardSlot::Row(idx)) => rows[idx] = card,
                            Some(CardSlot::Turn(t, p)) => {
                                turn_inserts[t][p] = card;
                            }
                            Some(CardSlot::Tail(p)) => {
                                tail_inserts[p] = card;
                            }
                            None => {
                                let slot = push_to_slot(
                                    target_turn,
                                    card,
                                    &mut turn_inserts,
                                    &mut tail_inserts,
                                );
                                seen_reports.insert(key, slot);
                            }
                        }
                    }
                    ChatLine::Artifact { artifact_id }
                        if seen_artifacts.insert(artifact_id.clone()) =>
                    {
                        let receipt_id = child_live
                            .card_receipts
                            .get(&artifact_receipt_key(artifact_id))
                            .map(String::as_str);
                        let target_turn =
                            work_turns.resolve(child_id, receipt_id);
                        push_to_slot(
                            target_turn,
                            ChatLine::Artifact {
                                artifact_id: artifact_id.clone(),
                            },
                            &mut turn_inserts,
                            &mut tail_inserts,
                        );
                    }
                    _ => {}
                }
            }
        }
        drop(live);
        if turn_inserts.iter().any(|bucket| !bucket.is_empty()) {
            let extra: usize = turn_inserts.iter().map(Vec::len).sum::<usize>()
                + tail_inserts.len();
            let existing = std::mem::take(rows);
            let mut out = Vec::with_capacity(existing.len() + extra);
            let mut iter = existing.into_iter();
            let mut emitted = 0_usize;
            for (turn_idx, _) in user_positions.iter().enumerate() {
                let end = user_positions
                    .get(turn_idx + 1)
                    .copied()
                    .unwrap_or(usize::MAX);
                while emitted < end {
                    let Some(row) = iter.next() else {
                        break;
                    };
                    out.push(row);
                    emitted += 1;
                }
                out.append(&mut turn_inserts[turn_idx]);
            }
            out.extend(iter);
            out.extend(tail_inserts);
            *rows = out;
        } else {
            rows.extend(tail_inserts);
        }
    }

    pub fn turn_status(&self) -> TurnStatus {
        let active = lock(&self.inner.state.active).clone();
        active
            .and_then(|id| {
                lock(&self.inner.state.live)
                    .get(&id)
                    .map(|session| session.status.clone())
            })
            .unwrap_or_default()
    }

    /// Map each dispatched/revised `(work_id, receipt_id)` (and latest
    /// `work_id` fallback for legacy untagged cards) to its Router user turn
    /// index in `user_positions`.
    fn collect_work_dispatch_turns(
        rows: &[ChatLine],
        user_positions: &[usize],
    ) -> WorkDispatchTurns {
        let mut work_turns = WorkDispatchTurns::default();
        if user_positions.is_empty() {
            return work_turns;
        }
        let mut current_turn: Option<usize> = None;
        let mut user_cursor = 0_usize;
        let mut pending_tools: VecDeque<&str> = VecDeque::new();
        for (idx, line) in rows.iter().enumerate() {
            while user_cursor < user_positions.len()
                && user_positions[user_cursor] <= idx
            {
                current_turn = Some(user_cursor);
                pending_tools.clear();
                user_cursor += 1;
            }
            match line {
                ChatLine::ToolCall { name, .. } => {
                    pending_tools.push_back(name.as_str());
                }
                ChatLine::ToolResult {
                    text,
                    is_error: false,
                } => {
                    let prev_tool = pending_tools.pop_front();
                    if let Some(turn) = current_turn
                        && let Ok(val) =
                            serde_json::from_str::<serde_json::Value>(text)
                        && let Some(work_id) = val
                            .get("work_id")
                            .and_then(serde_json::Value::as_str)
                        && !work_id.is_empty()
                    {
                        let receipt_id = val
                            .get("receipt_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|rid| !rid.is_empty());
                        if val.get("receipt_id").is_some()
                            || matches!(prev_tool, Some("dispatch_work"))
                        {
                            work_turns
                                .latest_by_work
                                .insert(work_id.to_owned(), turn);
                            if let Some(rid) = receipt_id {
                                work_turns
                                    .by_receipt
                                    .entry(work_id.to_owned())
                                    .or_default()
                                    .entry(rid.to_owned())
                                    .or_insert(turn);
                            }
                        }
                    }
                }
                ChatLine::ToolResult { is_error: true, .. } => {
                    let _ = pending_tools.pop_front();
                }
                _ => {}
            }
        }
        work_turns
    }

    pub fn working_elapsed_secs(&self) -> Option<u64> {
        let active = lock(&self.inner.state.active).clone();
        let started = active.and_then(|id| {
            lock(&self.inner.state.live)
                .get(&id)
                .and_then(|session| session.turn_started_at)
        })?;
        Some(now_ms().saturating_sub(started) / 1000)
    }

    pub fn session_title(&self) -> String {
        let active = lock(&self.inner.state.active).clone();
        active
            .and_then(|id| {
                lock(&self.inner.state.live)
                    .get(&id)
                    .map(|session| session.title.clone())
            })
            .unwrap_or_default()
    }

    pub fn create_session(&self) -> Result<String, &'static str> {
        let workspace =
            lock(&self.inner.state.workspace_for_new_sessions).clone();
        let origin = lock(&self.inner.state.origin_for_new_sessions).clone();
        self.inner.create_session_inner(origin, workspace, true)
    }

    pub fn select_session(&self, id: &str) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        self.select_session_locked(id)
    }

    pub fn select_session_if_active(
        &self,
        id: &str,
        expected: Option<&str>,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        if lock(&self.inner.state.active).as_deref() != expected {
            return Err("notice.agent_busy");
        }
        self.select_session_locked(id)
    }

    fn select_session_locked(&self, id: &str) -> Result<(), &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        let workspace = lock(&self.inner.state.live)
            .get(id)
            .map(|session| session.workspace.clone())
            .ok_or("notice.agent_failed")?;
        let previous_active = lock(&self.inner.state.active).clone();
        let previous_workspace =
            lock(&self.inner.state.workspace_for_new_sessions).clone();
        *lock(&self.inner.state.active) = Some(id.to_owned());
        *lock(&self.inner.state.workspace_for_new_sessions) = workspace;
        if persist_index(&self.inner.state).is_err() {
            *lock(&self.inner.state.active) = previous_active;
            *lock(&self.inner.state.workspace_for_new_sessions) =
                previous_workspace;
            self.inner.state.report_storage_error();
            return Err("notice.session_storage_failed");
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner
            .state
            .push_update(AgentUpdate::Session { id: id.to_owned() });
        self.inner
            .state
            .context_generation
            .fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn clear_active(&self) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        self.clear_active_locked()
    }

    fn clear_active_locked(&self) -> Result<(), &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        let previous = lock(&self.inner.state.active).clone();
        *lock(&self.inner.state.active) = None;
        if let Err(error) = persist_index(&self.inner.state) {
            *lock(&self.inner.state.active) = previous;
            self.inner.state.report_storage_error();
            return Err(error);
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner
            .state
            .context_generation
            .fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn begin_home_conversation_if_active(
        &self,
        expected: Option<&str>,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        if lock(&self.inner.state.active).as_deref() != expected {
            return Err("notice.agent_busy");
        }
        self.clear_active_locked()?;
        *lock(&self.inner.state.workspace_for_new_sessions) = None;
        *lock(&self.inner.state.origin_for_new_sessions) = SessionOrigin::Home;
        Ok(())
    }

    pub fn send(&self, text: &str) -> Result<(), &'static str> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "notice.agent_failed")?;
        self.submit_message(text, Some(u128::from_le_bytes(bytes)), None)
            .map(|_| ())
    }

    /// Return this submission's existing session when its receipt is retried.
    pub fn submit_chat(
        &self,
        submission: u128,
        text: &str,
    ) -> Result<String, &'static str> {
        self.submit_message(text, Some(submission), None)
    }

    /// Submit the captured UI request only while its original selection is
    /// still current. Retried durable submission IDs keep their original receipt.
    pub fn submit_chat_if_active(
        &self,
        submission: u128,
        text: &str,
        expected: Option<&str>,
        expected_generation: u64,
    ) -> Result<String, &'static str> {
        self.submit_message(
            text,
            Some(submission),
            Some(SubmissionSelection {
                active: expected,
                generation: expected_generation,
            }),
        )
    }

    /// Host disclosure switch controlling whether `verify_deck` may send
    /// extracted brief + deck text to the configured LLM for L3 semantic hints.
    pub fn set_semantic_verify_enabled(&self, enabled: bool) {
        self.inner
            .state
            .semantic_verify_enabled
            .store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn semantic_verify_enabled(&self) -> bool {
        self.inner
            .state
            .semantic_verify_enabled
            .load(Ordering::Acquire)
    }

    /// Whether the configured provider requires the cloud catalog before running.
    #[must_use]
    pub fn requires_cloud_ready(&self) -> bool {
        lock(&self.inner.config).requires_cloud_ready()
    }

    /// Whether the handle has an actionable model route (or local test echo).
    #[must_use]
    pub fn can_run_turns(&self) -> bool {
        self.inner.can_run_turns()
    }

    /// Resolve the top-level root conversation ID for `session_id` (returns
    /// parent Router ID for a child `Worker`, or `session_id` itself otherwise).
    #[must_use]
    pub fn root_session_id(&self, session_id: &str) -> String {
        self.inner.root_session_id_for(session_id)
    }

    /// Whether any live session is currently running a turn or reaping a
    /// cancelled turn's child processes.
    ///
    /// Draining the acceptance queue is a level condition over the whole
    /// handle: a receipt must not start while another session is still
    /// running or reaping, even if the user is looking at an idle one.
    #[must_use]
    pub fn has_busy_session(&self) -> bool {
        self.inner.has_busy_session()
    }

    fn submit_message(
        &self,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
    ) -> Result<String, &'static str> {
        self.inner.submit_message_targeted(
            None, text, submission, selection, None, None, None,
        )
    }
}

impl Harness {
    pub(super) fn session_role(&self, id: &str) -> SessionRole {
        let live_role =
            lock(&self.state.live).get(id).map(|session| session.role);
        if let Some(role) = live_role {
            return role;
        }
        let engine = lock(&self.engine).clone();
        engine
            .and_then(|eng| eng.with_session(id, |rec| rec.role))
            .unwrap_or(SessionRole::Standard)
    }

    /// Unified workspace lock access mode for `id`:
    /// - `Router` -> `None` (never acquires a directory lock)
    /// - `Worker(ReadOnly)` -> `Some(WorkspaceAccessMode::Read)`
    /// - `Worker(ReadWrite)` / `Standard` -> `Some(WorkspaceAccessMode::Write)`
    pub(super) fn session_access_mode(
        &self,
        id: &str,
    ) -> Option<WorkspaceAccessMode> {
        let role_and_mode = lock(&self.state.live)
            .get(id)
            .map(|session| (session.role, session.access_mode));
        let (role, mode) = if let Some(pair) = role_and_mode {
            pair
        } else {
            let engine = lock(&self.engine).clone();
            engine
                .and_then(|eng| {
                    eng.with_session(id, |rec| (rec.role, rec.access_mode))
                })
                .unwrap_or((SessionRole::Standard, WorkAccessMode::ReadWrite))
        };
        match role {
            SessionRole::Router => None,
            SessionRole::Worker => Some(match mode {
                WorkAccessMode::ReadOnly => WorkspaceAccessMode::Read,
                WorkAccessMode::ReadWrite => WorkspaceAccessMode::Write,
            }),
            SessionRole::Standard => Some(WorkspaceAccessMode::Write),
        }
    }

    pub(super) fn schedule_pending_router_followups(self: &Arc<Self>) {
        if self.state.shutdown_requested.load(Ordering::Acquire)
            || !self.can_run_turns()
        {
            return;
        }
        let engine = lock(&self.engine).clone();
        let Some(engine) = engine else {
            return;
        };
        let routers: Vec<String> = engine
            .sessions()
            .into_iter()
            .filter(|rec| {
                rec.role == SessionRole::Router
                    && !rec.pending_router_followups.is_empty()
            })
            .map(|rec| rec.id)
            .collect();
        for router_id in routers {
            work_events::schedule_router_inbox_flush(self, &router_id);
        }
    }

    pub(super) fn is_initializing(&self) -> bool {
        *lock(&self.state.initializing)
    }

    pub(super) fn skill_catalog(&self) -> Vec<InstalledSkill> {
        lock(&self.skills).clone()
    }

    pub(super) fn tool_providers(&self) -> Vec<RegisteredProvider> {
        lock(&self.providers).clone()
    }

    pub(super) fn can_run_turns(&self) -> bool {
        lock(&self.config).can_run_turns()
    }

    pub(super) fn root_session_id_for(&self, session_id: &str) -> String {
        lock(&self.engine)
            .as_ref()
            .and_then(|engine| engine.session(session_id))
            .map_or_else(
                || session_id.to_owned(),
                |record| record.effective_root_session_id().to_owned(),
            )
    }

    /// If `UiObserver::finished`/`waiting_for_user`/`stop_settled` has already
    /// flipped `live[id].status` out of a busy state, `Engine::finish_after_observer`
    /// is microseconds away from calling `ticket.finish()`. Wait briefly for the
    /// `RunFence` drop so callers polling `session_status` never observe a
    /// spurious `notice.agent_busy` or stale `has_busy_session() == true`.
    pub(super) fn settle_finishing_fence(&self, engine: &Engine, id: &str) {
        if engine.is_active(id)
            && !engine.is_cancelling(id)
            && lock(&self.state.live)
                .get(id)
                .is_some_and(|session| !session.status.is_busy())
        {
            let _ = engine
                .wait_until_idle(id, std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn has_busy_session(&self) -> bool {
        let live_busy = lock(&self.state.live)
            .values()
            .any(|session| session.status.is_busy());
        if live_busy {
            return true;
        }
        let Some(engine) = lock(&self.engine).clone() else {
            return false;
        };
        let active = engine.active_session_ids();
        if active.is_empty() {
            return false;
        }
        for sid in &active {
            self.settle_finishing_fence(&engine, sid);
        }
        !engine.active_session_ids().is_empty()
    }

    pub(super) fn is_session_busy_or_active(&self, id: &str) -> bool {
        if lock(&self.state.live)
            .get(id)
            .is_some_and(|session| session.status.is_busy())
        {
            return true;
        }
        let Some(engine) = lock(&self.engine).clone() else {
            return false;
        };
        self.settle_finishing_fence(&engine, id);
        engine.is_active(id)
    }

    pub(super) fn is_session_waiting_for_user(&self, id: &str) -> bool {
        if lock(&self.state.live).get(id).is_some_and(|session| {
            matches!(session.status, TurnStatus::WaitingForUser)
        }) {
            return true;
        }
        let engine = lock(&self.engine).clone();
        engine.as_ref().is_some_and(|engine| {
            engine
                .with_session(id, |rec| {
                    rec.status == TurnState::WaitingForUser
                        || !rec.pending_cards.is_empty()
                })
                .unwrap_or(false)
        })
    }

    /// Count of distinct non-Router work sessions currently executing or holding
    /// an unsettled acceptance claim (combining foreground direct Standard
    /// turns, active queue claims, and still-reaping engine turns, while
    /// excluding lightweight `SessionRole::Router` turns across all 3 sources).
    pub(super) fn in_flight_work_count(&self) -> usize {
        let sessions: BTreeSet<String> = {
            let live = lock(&self.state.live);
            let mut ids: BTreeSet<String> = live
                .iter()
                .filter(|(_, session)| {
                    session.status.is_busy()
                        && session.role != SessionRole::Router
                })
                .map(|(id, _)| id.clone())
                .collect();
            for claim in lock(&self.state.active_claims).values() {
                if live
                    .get(&claim.session_id)
                    .is_none_or(|s| s.role != SessionRole::Router)
                {
                    ids.insert(claim.session_id.clone());
                }
            }
            drop(live);
            ids
        };
        let mut total = sessions.len();
        let engine = lock(&self.engine).clone();
        if let Some(engine) = engine {
            for sid in engine.active_session_ids() {
                if !sessions.contains(&sid)
                    && self.session_role(&sid) != SessionRole::Router
                {
                    self.settle_finishing_fence(&engine, &sid);
                    if engine.is_active(&sid) {
                        total += 1;
                    }
                }
            }
        }
        total
    }

    /// Mirror [`Self::settle_finishing_fence`] for `workspace_locks`: if every
    /// conflicting holder on `cwd` has already left busy/active state (its turn
    /// ended and the monitor task is microseconds away from releasing its
    /// `WorkspaceLease`), wait briefly so immediate back-to-back submits or
    /// `ResolveCard` calls never observe a spurious `notice.agent_busy`.
    fn settle_finishing_workspace_lease(
        &self,
        engine: &Engine,
        cwd: &Path,
        mode: WorkspaceAccessMode,
    ) {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(150);
        loop {
            let holders =
                self.state.workspace_locks.conflicting_holders(cwd, mode);
            if holders.is_empty() {
                return;
            }
            let all_finishing = {
                let live = lock(&self.state.live);
                let ok = holders.iter().all(|sid| {
                    !live.get(sid).is_some_and(|s| s.status.is_busy())
                        && !engine.is_cancelling(sid)
                });
                drop(live);
                ok
            };
            if !all_finishing {
                return;
            }
            for sid in &holders {
                self.settle_finishing_fence(engine, sid);
                if engine.is_active(sid) {
                    return;
                }
            }
            let remaining =
                deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return;
            }
            if self.state.workspace_locks.wait_for_release_step(
                cwd,
                mode,
                remaining.min(std::time::Duration::from_millis(25)),
            ) {
                return;
            }
        }
    }

    /// Resolve the working directory for `session_id` without mutating the
    /// filesystem, matching the exact resolution in `submit_message_targeted_locked`.
    pub(super) fn session_workspace_path(&self, id: &str) -> Option<PathBuf> {
        let engine = lock(&self.engine).clone();
        if let Some(root) = engine
            .as_ref()
            .and_then(|eng| {
                eng.with_session(id, |rec| {
                    rec.workspace.as_ref().map(|ws| ws.root.clone())
                })
            })
            .flatten()
        {
            return Some(root);
        }
        lock(&self.state.live).get(id).map(|session| {
            session
                .session_workspace
                .as_ref()
                .map(|ws| ws.root.clone())
                .or_else(|| {
                    session.workspace.as_ref().map(|ws| ws.root.clone())
                })
                .unwrap_or_else(|| self.state.root.join("sessions").join("cwd"))
        })
    }

    /// Create a new durable Home session in the background without ever
    /// mutating the UI's active selection or `context_generation`.
    pub(super) fn create_background_session_with_role(
        &self,
        origin: SessionOrigin,
        workspace: Option<AgentWorkspace>,
        role: SessionRole,
    ) -> Result<String, &'static str> {
        self.create_session_with_role_inner(
            origin,
            workspace,
            false,
            role,
            None,
            WorkAccessMode::ReadWrite,
        )
    }

    fn create_session_inner(
        &self,
        origin: SessionOrigin,
        workspace: Option<AgentWorkspace>,
        make_active: bool,
    ) -> Result<String, &'static str> {
        self.create_session_with_role_inner(
            origin,
            workspace,
            make_active,
            SessionRole::Standard,
            None,
            WorkAccessMode::ReadWrite,
        )
    }

    fn create_session_with_role_inner(
        &self,
        origin: SessionOrigin,
        workspace: Option<AgentWorkspace>,
        make_active: bool,
        role: SessionRole,
        root_session_id: Option<String>,
        access_mode: WorkAccessMode,
    ) -> Result<String, &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        if *lock(&self.state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let id = format!("s-{}", next_stamp());
        let now = now_ms();
        let session_workspace = match &origin {
            SessionOrigin::Home => {
                let root = self
                    .state
                    .root
                    .join("sessions")
                    .join("workspaces")
                    .join(&id);
                std::fs::create_dir_all(&root).map_err(|error| {
                    log_storage_io_error(
                        "create session workspace",
                        &root,
                        &error,
                    );
                    "notice.session_storage_failed"
                })?;
                Some(SessionWorkspace { root })
            }
            SessionOrigin::Project { .. } => {
                workspace.as_ref().map(|workspace| SessionWorkspace {
                    root: workspace.root.clone(),
                })
            }
            SessionOrigin::Unknown => None,
        };
        let engine = lock(&self.engine).clone().ok_or("notice.agent_failed")?;
        engine
            .create_session_with_role(
                id.clone(),
                now,
                origin.clone(),
                session_workspace.clone(),
                role,
                root_session_id.clone(),
                access_mode,
            )
            .map_err(|_| "notice.session_storage_failed")?;
        lock(&self.state.live).insert(
            id.clone(),
            Live {
                projection: ChatProjection::new(),
                title: String::new(),
                titled: false,
                pinned: false,
                archived: false,
                auto_title_pending: false,
                auto_title_in_flight: false,
                auto_title_attempts: 0,
                title_signal: CancelToken::new(),
                status: TurnStatus::Idle,
                updated_at: now,
                turn_started_at: None,
                workspace,
                origin,
                session_workspace,
                heartbeat: None,
                role,
                root_session_id,
                access_mode,
                active_turn_is_internal: false,
                active_turn_prev_status: None,
                card_receipts: BTreeMap::new(),
                active_dispatch_receipt_id: None,
            },
        );
        if make_active {
            *lock(&self.state.active) = Some(id.clone());
            self.state.context_generation.fetch_add(1, Ordering::AcqRel);
        }
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.push_update(AgentUpdate::Sessions);
        Ok(id)
    }
}

#[derive(Debug, Default)]
struct WorkDispatchTurns {
    /// `work_id -> (receipt_id -> router_user_turn_index)` (first turn owning a receipt wins).
    by_receipt: HashMap<String, HashMap<String, usize>>,
    /// `work_id -> latest_router_user_turn_index` fallback for legacy untagged cards.
    latest_by_work: HashMap<String, usize>,
}

impl WorkDispatchTurns {
    fn resolve(
        &self,
        work_id: &str,
        receipt_id: Option<&str>,
    ) -> Option<usize> {
        receipt_id
            .map(str::trim)
            .filter(|rid| !rid.is_empty())
            .and_then(|rid| self.by_receipt.get(work_id)?.get(rid).copied())
            .or_else(|| self.latest_by_work.get(work_id).copied())
    }
}

/// Canonical deduplication key for projected verification cards across refresh
/// and restart (prefers non-empty `report_id`, falls back to normalized path).
fn verification_dedup_key(report_id: &str, artifact_path: &str) -> String {
    let rid = report_id.trim();
    if rid.is_empty() {
        format!(
            "path:{}",
            crate::delivery::artifact_key(std::path::Path::new(artifact_path))
        )
    } else {
        format!("report:{rid}")
    }
}

fn verification_receipt_key(report_id: &str, artifact_path: &str) -> String {
    format!(
        "verify:{}",
        verification_dedup_key(report_id, artifact_path)
    )
}

fn artifact_receipt_key(artifact_id: &str) -> String {
    format!("artifact:{artifact_id}")
}

/// Remove and return all active receipt claims targeting `session_id`.
fn take_session_claims(
    claims: &Mutex<HashMap<String, AcceptedClaim>>,
    session_id: &str,
) -> Vec<AcceptedClaim> {
    let mut map = lock(claims);
    let mut removed = Vec::new();
    map.retain(|_, claim| {
        if claim.session_id == session_id {
            removed.push(claim.clone());
            false
        } else {
            true
        }
    });
    drop(map);
    removed
}

/// Internal sentinel returned when a receipt was cancelled/superseded between
/// `queue.bind` and `dispatch` entry. Never surfaced to callers.
const RECEIPT_SUPERSEDED_ERR: &str = "__flowy_receipt_superseded__";

/// Compose the full literal brief for L3 semantic verification across turns so
/// narrow follow-ups (e.g. "只改日期为 2026-12-01") preserve earlier turns'
/// unrevoked requirements while explicitly prioritizing the latest revision.
fn compose_semantic_brief(
    messages: &[Message],
    current_prompt: &str,
) -> String {
    const MAX_BRIEF_CHARS: usize = 2_000;
    let prior: Vec<String> = messages
        .iter()
        .filter(|msg| {
            msg.role == Role::User
                && matches!(msg.source, MessageSource::User)
                && msg.display_text.as_deref() != Some("")
        })
        .map(|msg| msg.text().trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect();
    if current_prompt.trim().is_empty() {
        let start = prior.len().saturating_sub(4);
        return prior[start..]
            .join("\n\n")
            .chars()
            .take(MAX_BRIEF_CHARS)
            .collect();
    }
    if prior.is_empty() {
        return current_prompt.to_owned();
    }
    // Always place the latest user instruction FIRST so L3's 2,000-char prompt
    // cap never chops off the newest override/modification request; fill the
    // remaining budget with prior turns (preserving original turn order).
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = write!(
        out,
        "当前最新修改要求（与前序要求冲突时以本轮为准）：\n{current_prompt}"
    );
    let header = "\n\n原始与前序用户要求（未在本轮修改的部分仍须满足）：\n";
    let used = out.chars().count();
    let remaining_for_prior =
        MAX_BRIEF_CHARS.saturating_sub(used + header.chars().count());
    if remaining_for_prior == 0 {
        return out;
    }
    let mut selected: Vec<(usize, String)> = Vec::new();
    let mut budget = remaining_for_prior;
    for (idx, text) in prior.iter().enumerate().rev() {
        if budget < 16 {
            break;
        }
        let prefix = format!("[Turn {}] ", idx + 1);
        let overhead = prefix.chars().count() + 1;
        if budget <= overhead {
            break;
        }
        let take_chars = budget - overhead;
        let snippet: String = text.chars().take(take_chars).collect();
        budget = budget.saturating_sub(overhead + snippet.chars().count());
        selected.push((idx + 1, snippet));
    }
    if !selected.is_empty() {
        selected.reverse();
        out.push_str(header);
        for (turn_no, snippet) in selected {
            let _ = writeln!(out, "[Turn {turn_no}] {snippet}");
        }
    }
    out
}

fn model_route(config: &LlmConfig) -> ModelRoute {
    ModelRoute {
        provider: if config.provider.is_empty() {
            "flowy".into()
        } else {
            config.provider.clone()
        },
        model: if config.model.is_empty() {
            "mock".into()
        } else {
            config.model.clone()
        },
        base_url: config.base_url.clone(),
        context_window: config.context_window,
        max_tokens: config.max_tokens,
    }
}

fn prepare_skill_prompt(
    text: &str,
    skills: &[InstalledSkill],
) -> Result<(PromptTurn, Vec<SkillId>), &'static str> {
    let Some(command) = text.strip_prefix("/skill:") else {
        return Ok((PromptTurn::plain(text), Vec::new()));
    };
    let (name, args) = command
        .split_once(' ')
        .map_or((command, ""), |(name, args)| (name, args.trim()));
    let name = name.trim();
    if name.is_empty() {
        return Err("notice.skill_not_found");
    }
    let mut matches: Vec<&InstalledSkill> = skills
        .iter()
        .filter(|skill| {
            skill.name == name
                || format!("{}/{}", skill.id.owner_handle, skill.id.slug)
                    == name
        })
        .collect();
    let skill = match matches.len() {
        0 => return Err("notice.skill_not_found"),
        1 => matches.remove(0),
        _ => return Err("notice.skill_ambiguous"),
    };
    let model =
        format_skill_invocation(skill, (!args.is_empty()).then_some(args));
    Ok((
        PromptTurn::with_display(model, text),
        vec![skill.id.clone()],
    ))
}

const BASE_BEHAVIOR_PROMPT: &str = "Always respond in the same language as the user's latest message unless explicitly asked otherwise.";

/// Returns `true` when `dir` contains at least one user-visible file or folder
/// outside the current turn's `outputs/<current_submission>` directory and
/// dot-prefixed metadata (including deliverables from earlier turns).
fn workspace_has_user_files(dir: &Path, current_submission: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return true;
        };
        if name.starts_with('.') {
            continue;
        }
        if name == "outputs" {
            if let Ok(sub_entries) = std::fs::read_dir(entry.path())
                && sub_entries.flatten().any(|sub| {
                    sub.file_name().to_str() != Some(current_submission)
                })
            {
                return true;
            }
            continue;
        }
        return true;
    }
    false
}

fn append_skill_listing(base: &str, skills: &[InstalledSkill]) -> String {
    let listing = format_available_skills(skills);
    match (base.trim().is_empty(), listing.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_owned(),
        (true, false) => listing,
        (false, false) => format!("{base}\n\n{listing}"),
    }
}

fn engine_config(
    config: &LlmConfig,
    cwd: &Path,
    data_root: &Path,
) -> EngineConfig {
    EngineConfig {
        route: model_route(config),
        api_key: config.api_key.clone(),
        system: String::new(),
        registry: tool_registry(cwd, data_root, &[], Vec::new()),
        cwd: Some(cwd.display().to_string()),
        todos: None,
        delivery_committer: None,
        work_contract: None,
        pre_tool_hook: None,
        pre_activated_tools: Vec::new(),
    }
}

fn log_storage_io_error(
    operation: &'static str,
    path: &Path,
    error: &std::io::Error,
) {
    tracing::error!(
        target: "flowy.storage",
        operation,
        path = %path.display(),
        kind = ?error.kind(),
        error = %error,
        "storage operation failed"
    );
}

fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let stamp = now_ms().saturating_mul(1000);
    stamp + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod workspace_probe_tests {
    use super::workspace_has_user_files;

    #[test]
    fn ignores_internal_outputs_and_dotfiles_but_detects_user_files() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-ws-probe-{}", super::next_stamp()));
        std::fs::create_dir_all(dir.join("outputs/submission-1")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        assert!(
            !workspace_has_user_files(&dir, "submission-1"),
            "workspace with only current submission dir and dotfiles must be treated as empty"
        );
        std::fs::create_dir_all(dir.join("outputs/submission-0")).unwrap();
        std::fs::write(
            dir.join("outputs/submission-0/index.html"),
            "<h1>prev</h1>",
        )
        .unwrap();
        assert!(
            workspace_has_user_files(&dir, "submission-1"),
            "prior turn deliverables under outputs/ must be detected as non-empty"
        );
        let _ = std::fs::remove_dir_all(dir.join("outputs/submission-0"));
        std::fs::write(dir.join("index.html"), "<h1>hi</h1>").unwrap();
        assert!(
            workspace_has_user_files(&dir, "submission-1"),
            "workspace with user-visible files must be detected as non-empty"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
