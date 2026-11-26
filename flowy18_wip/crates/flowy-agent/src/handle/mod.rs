//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    AcceptanceQueue, AcceptedMessage, BranchHeartbeat, DeliveryCommit, Engine,
    EngineConfig, EnqueueOutcome, LoopOutcome, ModelRoute, PendingCard,
    PendingRouterFollowup, PromptTurn, ReceiptState, RequiredDeliverable,
    RunError, SessionOrigin, SessionRecord, SessionRole, SessionWorkspace,
    StopHandoffManifest, SubmissionOutcome, TurnObserver, TurnState,
    WorkAccessMode, complete_text, echo_stream_fn, log_preview,
    tools::{
        ToolGuard, ToolRegistry, pi_shell_definition, register_builtin_tools,
    },
    types::{
        CancelToken, ContentBlock, Message, MessageSource, Role, ToolCallArgs,
    },
};
use flowy_skills::{
    InstalledSkill, SkillId, cleanup_staging, format_available_skills,
    format_skill_invocation, load_installed, seed_official,
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
mod guards;
mod observer;
mod queue;
mod router;
#[cfg(test)]
mod router_tests;
#[cfg(test)]
mod tests;
mod work;
mod work_events;
mod workspace_lock;

use self::guards::{
    build_pre_tool_hook, ensure_tool_cwd, read_only_tool_registry,
    session_working_directory, tool_registry, tool_target,
};
pub use self::guards::{confined_to, has_reserved_device_component};
use self::observer::{
    UiObserver, apply_delivery_commit_to_live, backfill_conversation_artifacts,
    capture_turn_artifacts, clear_live_heartbeat, confirm_commit_audit,
    ensure_engine_session, extract_session_preview, finish_interrupted_lines,
    hydrate_engine, import_engine_sessions, mark_delivery_failed,
    migrate_legacy_source_deletions, persist_index, recover_session_deletions,
    sync_engine_titles,
};
use self::queue::{
    push_reconciled_receipt_updates, reconcile_committed_acceptances,
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
    /// One-shot pre-tool approval grants pinned to exact `(tool_name, args_hash)`
    /// per session, consumed upon execution.
    approved_tool_calls: Mutex<HashMap<String, Vec<(String, String)>>>,
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
/// `notified_quarantined`).
struct Harness {
    runtime: tokio::runtime::Handle,
    state: Arc<UiState>,
    engine: Arc<Mutex<Option<Arc<Engine>>>>,
    config: Mutex<LlmConfig>,
    skills: Mutex<Vec<InstalledSkill>>,
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
        Self {
            inner: Arc::new(Harness {
                runtime: rt_handle,
                state,
                engine,
                config: Mutex::new(config),
                skills: Mutex::new(Vec::new()),
            }),
            _runtime: owner,
        }
    }

    fn initialize(&self) {
        self.reload_skills();
        let artifact_root = self.inner.state.root.join("artifacts");
        match ArtifactStore::open(artifact_root.clone()) {
            Ok(store) => {
                let _ = self.inner.state.artifact_store.set(store);
            }
            Err(error) => {
                log_storage_io_error(
                    "open artifact store",
                    &artifact_root,
                    &error,
                );
            }
        }
        let config = lock(&self.inner.config).clone();
        let sandbox = flowy_agent_core::tools::detect_sandbox_availability();
        tracing::info!(
            target: "flowy.sandbox",
            status = sandbox.telemetry_label(),
            enforced = sandbox.is_enforced(),
            "host sandbox posture probed at boot"
        );
        let boot_receipts = self.boot(config);
        // History is fully loaded and recovered before this point. Keep the
        // library in its existing loading state until the backfill finishes so
        // the UI never flashes an empty catalog first.
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            backfill_conversation_artifacts(&self.inner.state, engine);
        }
        // A catalog refresh may have arrived while history was being read.
        // Apply its latest route before making initialization observable.
        let cwd = ensure_tool_cwd(&self.inner.state.root);
        {
            let mut initializing = lock(&self.inner.state.initializing);
            let config = lock(&self.inner.config).clone();
            if let Some(engine) = engine {
                engine.set_config(engine_config(&config, &cwd));
            }
            *initializing = false;
        }
        self.inner.state.push_update(AgentUpdate::Initialized);
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner.state.push_update(AgentUpdate::Skills);
        self.inner.state.push_update(AgentUpdate::Artifacts);
        push_reconciled_receipt_updates(&self.inner.state, boot_receipts);
        if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
            self.inner.kick_autonomous_drain();
        }
        self.inner.schedule_pending_router_followups();
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::with_config",
            "done"
        );
    }

    fn reload_skills(&self) {
        let root = &self.inner.state.root;
        cleanup_staging(root);
        if let Err(error) = seed_official(root) {
            tracing::warn!(
                target: "flowy.skills",
                error = %error,
                "official skill seed failed"
            );
        }
        match load_installed(root) {
            Ok(catalog) => {
                for failure in &catalog.failures {
                    tracing::warn!(
                        target: "flowy.skills",
                        error = %failure,
                        "installed skill could not be loaded"
                    );
                }
                *lock(&self.inner.skills) = catalog.skills;
            }
            Err(error) => {
                tracing::warn!(
                    target: "flowy.skills",
                    error = %error,
                    "installed skill catalog load failed"
                );
                lock(&self.inner.skills).clear();
            }
        }
    }

    pub fn replace_skill_catalog(&self, skills: Vec<InstalledSkill>) {
        *lock(&self.inner.skills) = skills;
        self.inner.state.push_update(AgentUpdate::Skills);
    }

    #[must_use]
    pub fn skill_catalog(&self) -> Vec<InstalledSkill> {
        self.inner.skill_catalog()
    }

    fn boot(&self, config: LlmConfig) -> Vec<(String, ReceiptState)> {
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::boot",
            "enter"
        );
        let state = Arc::clone(&self.inner.state);
        let index = match SessionIndex::load(&state.root) {
            Ok(index) => index,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                // Keep the damaged file, but do not let it lock the whole app.
                if let Err(recovery_error) = recover_corrupt_index(&state.root)
                {
                    log_storage_io_error(
                        "quarantine corrupt session index",
                        &SessionIndex::path(&state.root),
                        &recovery_error,
                    );
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return Vec::new();
                }
                state.report_storage_error();
                SessionIndex::default()
            }
            Err(error) => {
                log_storage_io_error(
                    "open session index",
                    &SessionIndex::path(&state.root),
                    &error,
                );
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return Vec::new();
            }
        };
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let tool_cwd = ensure_tool_cwd(&state.root);
        let engine = match Engine::open(
            state.root.clone(),
            engine_config(&config, &tool_cwd),
            Arc::clone(&observer),
        ) {
            Ok(engine) => Arc::new(engine),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                // A damaged history file must not hide a usable index.
                *lock(&state.history_unavailable) = true;
                if let Err(recovery_error) =
                    recover_corrupt_history(&state.root)
                {
                    log_storage_io_error(
                        "quarantine corrupt structured history",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        &recovery_error,
                    );
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return Vec::new();
                }
                state.report_storage_error();
                match Engine::open(
                    state.root.clone(),
                    engine_config(&config, &tool_cwd),
                    observer,
                ) {
                    Ok(engine) => Arc::new(engine),
                    Err(error) => {
                        log_storage_io_error(
                            "reopen structured history after recovery",
                            &flowy_agent_core::SessionIndex::path(&state.root),
                            &error,
                        );
                        *lock(&state.load_failed) = true;
                        state.report_storage_error();
                        return Vec::new();
                    }
                }
            }
            Err(error) => {
                log_storage_io_error(
                    "open structured history",
                    &flowy_agent_core::SessionIndex::path(&state.root),
                    &error,
                );
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return Vec::new();
            }
        };
        let deleted_sessions = if let Some(store) = state.artifact_store.get() {
            let mut deleted = recover_session_deletions(store, &engine, &state);
            deleted.extend(migrate_legacy_source_deletions(
                store, &engine, &state,
            ));
            deleted
        } else {
            BTreeSet::new()
        };
        let SessionIndex { active, sessions } = index;
        {
            let mut live = lock(&state.live);
            for record in sessions {
                if deleted_sessions.contains(&record.id) {
                    continue;
                }
                let source_deleted =
                    state.artifact_store.get().is_some_and(|store| match store
                        .source_deleted(&record.id)
                    {
                        Ok(deleted) => deleted,
                        Err(error) => {
                            log_storage_io_error(
                                "read source deletion marker",
                                &state.root.join("artifacts"),
                                &error,
                            );
                            false
                        }
                    });
                if engine.session(&record.id).is_none() && source_deleted {
                    continue;
                }
                let mut status = record.status;
                let mut lines = record.lines;
                if status.is_busy() {
                    status = TurnStatus::Stopped;
                    finish_interrupted_lines(&mut lines);
                }
                live.insert(
                    record.id,
                    Live {
                        projection: ChatProjection::from_lines(lines),
                        title: record.title,
                        titled: record.titled,
                        pinned: record.pinned,
                        archived: record.archived,
                        auto_title_pending: false,
                        auto_title_in_flight: false,
                        auto_title_attempts: 0,
                        title_signal: CancelToken::new(),
                        status,
                        updated_at: record.updated_at,
                        turn_started_at: None,
                        workspace: record.workspace,
                        origin: record.origin,
                        session_workspace: record.session_workspace,
                        heartbeat: None,
                        role: record.role,
                        root_session_id: record.root_session_id,
                        access_mode: record.access_mode,
                    },
                );
            }
            drop(live);
            let active_workspace = active.as_ref().and_then(|id| {
                lock(&state.live)
                    .get(id)
                    .and_then(|session| session.workspace.clone())
            });
            *lock(&state.active) = active;
            *lock(&state.workspace_for_new_sessions) = active_workspace;
        }
        // If the UI index was lost or quarantined, rebuild the visible sessions
        // from the structured history instead of hiding them.
        if let Some(store) = state.artifact_store.get() {
            let mut commits = Vec::new();
            let mut recovery_ok = !*lock(&state.history_unavailable);
            for session in engine.sessions() {
                match store.recover_commits(&session.delivery_commits) {
                    Ok(()) => {
                        for commit in &session.delivery_commits {
                            confirm_commit_audit(&state.root, store, commit);
                        }
                        if let Some(live) =
                            lock(&state.live).get_mut(&session.id)
                            && matches!(live.status, TurnStatus::DeliveryFailed)
                        {
                            live.status = TurnStatus::Completed;
                            live.projection.lines.retain(|line| {
                                !matches!(
                                    line,
                                    ChatLine::Status { text }
                                        if text == "chat.status_delivery_failed"
                                )
                            });
                        }
                    }
                    Err(error) => {
                        log_storage_io_error(
                            "recover delivered artifacts",
                            &state.root.join("artifacts"),
                            &error,
                        );
                        recovery_ok = false;
                        mark_delivery_failed(&state, &session.id);
                        state.report_storage_error();
                    }
                }
                commits.extend(session.delivery_commits);
            }
            // This runs before the engine is published to callers, so there
            // are no active runs whose staging files could be removed.
            if let Err(error) = store.cleanup_uncommitted(&commits, recovery_ok)
            {
                log_storage_io_error(
                    "clean up uncommitted artifacts",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
            }
        } else {
            tracing::error!(
                target: "flowy.storage",
                operation = "open artifact store",
                root = %state.root.join("artifacts").display(),
                "artifact store unavailable"
            );
            state.report_storage_error();
        }
        import_engine_sessions(&engine, &state, &deleted_sessions);
        // A crash leaves the old status in the file; persist the repaired view.
        let index_persisted = if persist_index(&state) == Ok(()) {
            true
        } else {
            state.report_storage_error();
            false
        };
        if index_persisted && let Some(store) = state.artifact_store.get() {
            for session_id in &deleted_sessions {
                if let Err(error) = store.complete_session_deletion(session_id)
                {
                    log_storage_io_error(
                        "complete recovered session deletion",
                        &state.root.join("artifacts"),
                        &error,
                    );
                    state.report_storage_error();
                }
            }
        }
        hydrate_engine(&engine, &state, &config);
        sync_engine_titles(&engine, &state);
        let mut receipt_updates = reconcile_committed_acceptances(
            &state.root,
            &engine,
            state.artifact_store.get(),
        );
        // Desktop is single-instance: any remaining unexpired lease belongs to
        // a crashed predecessor process and must transition immediately to
        // Retrying rather than falsely displaying Running until lease expiry,
        // EXCEPT sessions intentionally parked in WaitingForUser / pending_cards.
        let waiting_sessions: BTreeSet<String> = engine
            .sessions()
            .into_iter()
            .filter(|rec| {
                rec.status == TurnState::WaitingForUser
                    || !rec.pending_cards.is_empty()
            })
            .map(|rec| rec.id)
            .collect();
        if let Ok(released_ids) = AcceptanceQueue::new(state.root.clone())
            .release_uncommitted_leases_except(now_ms(), &waiting_sessions)
        {
            for id in released_ids {
                receipt_updates.push((id, ReceiptState::Retrying));
            }
        }
        // Deterministically clean up any empty Worker left behind by a crash
        // between worker creation and `accept_strict` receipt enqueue, and
        // ensure durable `pending_router_followups` are projected cleanly.
        {
            let all_receipts = AcceptanceQueue::new(state.root.clone())
                .all()
                .unwrap_or_default();
            let mut removed_any = false;
            for rec in engine.sessions() {
                if rec.role == SessionRole::Worker
                    && rec.messages.is_empty()
                    && rec.delivery_commits.is_empty()
                    && rec.pending_cards.is_empty()
                    && rec.work_contract.is_none()
                {
                    let has_live_receipt = all_receipts.iter().any(|r| {
                        r.completed_at_ms.is_none()
                            && !r.cancelled
                            && !r.dead
                            && r.target_session_id
                                .as_deref()
                                .or(r.session_hint.as_deref())
                                == Some(rec.id.as_str())
                    });
                    if !has_live_receipt {
                        let _ = engine.remove_session(&rec.id);
                        if let Some(root_id) = rec.root_session_id.as_deref() {
                            let _ = engine.remove_dispatch_requests_for_worker(
                                root_id, &rec.id,
                            );
                        }
                        lock(&state.live).remove(&rec.id);
                        removed_any = true;
                    }
                } else if rec.role == SessionRole::Router
                    && !rec.pending_router_followups.is_empty()
                    && let Some(live) = lock(&state.live).get_mut(&rec.id)
                {
                    for item in &rec.pending_router_followups {
                        let already_tail = live
                            .projection
                            .lines
                            .iter()
                            .rev()
                            .take(rec.pending_router_followups.len() + 2)
                            .any(|line| matches!(line, ChatLine::User { text } if text == &item.text));
                        if !already_tail {
                            live.projection.push_user(&item.text);
                        }
                    }
                }
            }
            if removed_any {
                let _ = persist_index(&state);
            }
        }
        if let Ok(unresolved) =
            crate::delivery::DeliveryStore::new(state.root.join("delivery"))
                .in_doubt_deliveries()
        {
            for item in unresolved {
                tracing::warn!(
                    target: "flowy.delivery",
                    delivery_id = %item.delivery_id,
                    artifact_hash = %item.artifact_hash,
                    artifact_path = %item.artifact_path,
                    "unresolved deliver_intent detected at startup"
                );
            }
        }
        *lock(&self.inner.engine) = Some(engine);
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::boot",
            sessions = lock(&state.live).len(),
            "done"
        );
        receipt_updates
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
        engine.set_config(engine_config(&config, &cwd));
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

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let live = lock(&self.inner.state.live);
        let mut rows: Vec<SessionSummary> = live
            .iter()
            .filter(|(_, session)| session.role != SessionRole::Worker)
            .map(|(id, session)| SessionSummary {
                id: id.clone(),
                title: session.title.clone(),
                updated_at: session.updated_at,
                project_id: session
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.project_id.clone()),
                pinned: session.pinned,
                archived: session.archived,
            })
            .collect();
        drop(live);
        rows.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        rows
    }

    /// Manually rename a session, persisting atomically to both `history.json`
    /// and `index.json` (rolling back on error) and locking out any subsequent
    /// automatic title overwrite. Direct metadata mutation on hidden internal
    /// `SessionRole::Worker` sessions is rejected.
    pub fn rename_session(
        &self,
        session_id: &str,
        new_title: &str,
    ) -> Result<(), &'static str> {
        let cleaned = title::clamp_manual_title(new_title, TITLE_MAX_CHARS);
        if cleaned.is_empty() {
            return Err("chat.rename_empty");
        }
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let previous = lock(&self.inner.state.live)
            .get(session_id)
            .map(|session| {
                if session.role == SessionRole::Worker {
                    return Err("notice.agent_failed");
                }
                session.title_signal.cancel();
                Ok((
                    session.title.clone(),
                    session.titled,
                    session.auto_title_pending,
                    session.auto_title_in_flight,
                ))
            })
            .ok_or("notice.session_storage_failed")??;
        ensure_engine_session(&engine, session_id)
            .and_then(|()| {
                engine.set_session_title(session_id, cleaned.clone(), true)
            })
            .map_err(|error| {
                log_storage_io_error(
                    "rename session",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.title = cleaned;
            session.titled = true;
            session.auto_title_pending = false;
            session.auto_title_in_flight = false;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            let (old_title, old_titled, old_pending, old_in_flight) = previous;
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.title.clone_from(&old_title);
                session.titled = old_titled;
                session.auto_title_pending = old_pending;
                session.auto_title_in_flight = old_in_flight;
            }
            let _ = engine.set_session_title(session_id, old_title, old_titled);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        observer::sync_artifact_title(&self.inner.state, &engine, session_id);
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Pin or unpin a session durably across both `history.json` and
    /// `index.json` (rolling back on error) without changing its activity
    /// timestamp. Direct metadata mutation on hidden internal
    /// `SessionRole::Worker` sessions is rejected.
    pub fn set_session_pinned(
        &self,
        session_id: &str,
        pinned: bool,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let old_pinned = lock(&self.inner.state.live)
            .get(session_id)
            .map(|session| {
                if session.role == SessionRole::Worker {
                    return Err("notice.agent_failed");
                }
                Ok(session.pinned)
            })
            .ok_or("notice.session_storage_failed")??;
        ensure_engine_session(&engine, session_id)
            .and_then(|()| engine.set_session_pinned(session_id, pinned))
            .map_err(|error| {
                log_storage_io_error(
                    "set session pinned",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.pinned = pinned;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.pinned = old_pinned;
            }
            let _ = engine.set_session_pinned(session_id, old_pinned);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Archive or unarchive a session durably across both `history.json` and
    /// `index.json` (rolling back on error) without deleting history or
    /// artifacts. Active/running sessions — including any Router whose child
    /// Workers are active, hold `active_claims`, or have non-terminal queued
    /// receipts — cannot be archived until settled so background workers and
    /// directory leases are never hidden from view. Direct metadata mutation on
    /// hidden internal `SessionRole::Worker` sessions is rejected.
    pub fn set_session_archived(
        &self,
        session_id: &str,
        archived: bool,
    ) -> Result<(), &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let mut family_ids: BTreeSet<String> =
            std::iter::once(session_id.to_owned())
                .chain(
                    engine
                        .child_sessions(session_id)
                        .into_iter()
                        .map(|child| child.id),
                )
                .collect();
        let has_pending_followups = archived
            && engine
                .with_session(session_id, |rec| {
                    !rec.pending_router_followups.is_empty()
                })
                .unwrap_or(false);
        let old_archived = {
            let live = lock(&self.inner.state.live);
            let Some(session) = live.get(session_id) else {
                return Err("notice.session_storage_failed");
            };
            if session.role == SessionRole::Worker {
                return Err("notice.agent_failed");
            }
            let prev = session.archived;
            if archived {
                for (cid, child) in live.iter() {
                    if child.root_session_id.as_deref() == Some(session_id) {
                        family_ids.insert(cid.clone());
                    }
                }
            }
            let any_live_busy = archived
                && family_ids.iter().any(|member| {
                    live.get(member).is_some_and(|s| s.status.is_busy())
                });
            let any_claimed = archived
                && lock(&self.inner.state.active_claims)
                    .values()
                    .any(|claim| family_ids.contains(&claim.session_id));
            drop(live);
            if archived {
                for member in &family_ids {
                    self.inner.settle_finishing_fence(&engine, member);
                }
            }
            let any_engine_active = archived
                && family_ids.iter().any(|member| engine.is_active(member));
            let any_queued_receipt = archived
                && AcceptanceQueue::new(self.inner.state.root.clone())
                    .all()
                    .unwrap_or_default()
                    .into_iter()
                    .any(|receipt| {
                        receipt.completed_at_ms.is_none()
                            && !receipt.cancelled
                            && !receipt.dead
                            && receipt
                                .target_session_id
                                .as_deref()
                                .or(receipt.session_hint.as_deref())
                                .is_some_and(|sid| family_ids.contains(sid))
                    });
            if archived
                && (has_pending_followups
                    || any_live_busy
                    || any_engine_active
                    || any_claimed
                    || any_queued_receipt)
            {
                return Err("notice.agent_busy");
            }
            prev
        };
        ensure_engine_session(&engine, session_id)
            .and_then(|()| engine.set_session_archived(session_id, archived))
            .map_err(|error| {
                log_storage_io_error(
                    "set session archived",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;
        let cleared_active = if archived {
            let mut active = lock(&self.inner.state.active);
            if active.as_deref() == Some(session_id) {
                active.take()
            } else {
                None
            }
        } else {
            None
        };
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.archived = archived;
        }
        if let Err(err) = persist_index(&self.inner.state) {
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.archived = old_archived;
            }
            if let Some(prev_active) = cleared_active {
                *lock(&self.inner.state.active) = Some(prev_active);
            }
            let _ = engine.set_session_archived(session_id, old_archived);
            self.inner.state.report_storage_error();
            return Err(err);
        }
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(())
    }

    /// Zero-dependency in-memory search across redacted session titles and
    /// projected message lines. Empty queries return the most recent active
    /// top-level sessions (`Standard` and `Router`). Hidden internal
    /// `SessionRole::Worker` sessions are never returned directly: non-empty
    /// matches inside a Worker fold their redacted snippet and score into the
    /// owning top-level Router (deduplicated by `root_session_id`), while
    /// internal `SystemEvent` / `CardResume` control rows are skipped.
    #[must_use]
    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<SessionSearchHit> {
        if limit == 0 {
            return Vec::new();
        }
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|t| !t.is_empty())
            .collect();
        let full_query = tokens.join(" ");
        let live = lock(&self.inner.state.live);
        if tokens.is_empty() {
            let mut hits: Vec<SessionSearchHit> = live
                .iter()
                .filter(|(_, session)| {
                    session.role != SessionRole::Worker && !session.archived
                })
                .map(|(id, session)| {
                    let project_id = session
                        .workspace
                        .as_ref()
                        .map(|ws| ws.project_id.clone());
                    let safe_title = title::redact_search_text(&session.title);
                    let score = if session.pinned { 20 } else { 10 };
                    let safe_preview =
                        extract_session_preview(&session.projection.lines)
                            .map(|text| title::redact_search_text(&text));
                    SessionSearchHit {
                        session_id: id.clone(),
                        title: safe_title,
                        project_id,
                        updated_at: session.updated_at,
                        pinned: session.pinned,
                        archived: session.archived,
                        snippet: safe_preview,
                        score,
                    }
                })
                .collect();
            drop(live);
            hits.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| b.updated_at.cmp(&a.updated_at))
            });
            hits.truncate(limit);
            return hits;
        }

        // `(hit, has_matched_snippet)` keyed by top-level session ID.
        let mut by_root: HashMap<String, (SessionSearchHit, bool)> =
            HashMap::new();

        // Pass 1: score visible top-level sessions (`Standard` | `Router`).
        for (id, session) in live.iter() {
            if session.role == SessionRole::Worker {
                continue;
            }
            let project_id =
                session.workspace.as_ref().map(|ws| ws.project_id.clone());
            let safe_title = title::redact_search_text(&session.title);
            let title_lower = safe_title.to_lowercase();
            let mut best_score = score_text_match(
                &title_lower,
                &tokens,
                &full_query,
                120,
                95,
                75,
                45,
            );
            let (body_score, body_snippet) = score_projected_lines(
                &session.projection.lines,
                &title_lower,
                &tokens,
                best_score,
            );
            best_score = best_score.max(body_score);
            if best_score == 0 {
                continue;
            }
            if !session.archived {
                best_score += 10;
            }
            if session.pinned {
                best_score += 5;
            }
            let has_match_snippet = body_snippet.is_some();
            let snippet = body_snippet.or_else(|| {
                extract_session_preview(&session.projection.lines)
                    .map(|text| title::redact_search_text(&text))
            });
            by_root.insert(
                id.clone(),
                (
                    SessionSearchHit {
                        session_id: id.clone(),
                        title: safe_title,
                        project_id,
                        updated_at: session.updated_at,
                        pinned: session.pinned,
                        archived: session.archived,
                        snippet,
                        score: best_score,
                    },
                    has_match_snippet,
                ),
            );
        }

        // Pass 2: fold hidden `Worker` body matches into their owning `Router`.
        for session in live.values() {
            if session.role != SessionRole::Worker {
                continue;
            }
            let Some(root_id) = session.root_session_id.as_deref() else {
                continue;
            };
            let Some(parent) = live.get(root_id) else {
                continue;
            };
            if parent.role == SessionRole::Worker {
                continue;
            }
            let parent_safe_title = title::redact_search_text(&parent.title);
            let parent_title_lower = parent_safe_title.to_lowercase();
            let (body_score, body_snippet) = score_projected_lines(
                &session.projection.lines,
                &parent_title_lower,
                &tokens,
                0,
            );
            if body_score == 0 {
                continue;
            }
            let mut folded_score = body_score;
            if !parent.archived {
                folded_score += 10;
            }
            if parent.pinned {
                folded_score += 5;
            }
            if let Some((existing, has_match)) = by_root.get_mut(root_id) {
                if folded_score > existing.score {
                    existing.score = folded_score;
                    if body_snippet.is_some() {
                        existing.snippet = body_snippet;
                        *has_match = true;
                    }
                } else if !*has_match && body_snippet.is_some() {
                    existing.snippet = body_snippet;
                    *has_match = true;
                }
                existing.updated_at =
                    existing.updated_at.max(session.updated_at);
            } else {
                let project_id =
                    parent.workspace.as_ref().map(|ws| ws.project_id.clone());
                let snippet = body_snippet.or_else(|| {
                    extract_session_preview(&parent.projection.lines)
                        .map(|text| title::redact_search_text(&text))
                });
                by_root.insert(
                    root_id.to_owned(),
                    (
                        SessionSearchHit {
                            session_id: root_id.to_owned(),
                            title: parent_safe_title,
                            project_id,
                            updated_at: parent
                                .updated_at
                                .max(session.updated_at),
                            pinned: parent.pinned,
                            archived: parent.archived,
                            snippet,
                            score: folded_score,
                        },
                        true,
                    ),
                );
            }
        }
        drop(live);
        let mut hits: Vec<SessionSearchHit> =
            by_root.into_values().map(|(hit, _)| hit).collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        hits.truncate(limit);
        hits
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

    /// Returns `true` when `session_id` has an unsettled failed verification
    /// gate in its latest turn, a `Partial` latest submission, or a
    /// turn/delivery error awaiting user attention.
    #[must_use]
    pub fn session_needs_attention(&self, session_id: &str) -> bool {
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
        *lock(&self.inner.state.workspace_for_new_sessions) = None;
        self.set_origin_for_new_sessions(SessionOrigin::Home);
        self.inner.create_session_with_role_inner(
            SessionOrigin::Home,
            None,
            true,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )
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
        *lock(&self.inner.state.workspace_for_new_sessions) =
            Some(workspace.clone());
        self.set_origin_for_new_sessions(origin.clone());
        self.inner.create_session_with_role_inner(
            origin,
            Some(workspace),
            true,
            SessionRole::Router,
            None,
            WorkAccessMode::ReadWrite,
        )
    }

    /// Submit one durable user message directly to an explicit `router_id`
    /// (decoupled from UI `active` focus; queues FIFO durably while Router is busy).
    pub fn submit_router_message(
        &self,
        router_id: &str,
        submission: u128,
        text: &str,
    ) -> Result<String, &'static str> {
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
        let active = lock(&self.inner.state.active).clone();
        active
            .and_then(|id| {
                lock(&self.inner.state.live).get(&id).map(|session| {
                    ChatHistory {
                        rows: session.projection.visible_lines(),
                        streaming: session.projection.streaming_row(),
                        status: session.status.clone(),
                    }
                })
            })
            .unwrap_or_default()
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

    fn root_session_id_for(&self, session_id: &str) -> String {
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
    pub(super) fn create_background_home_session(
        &self,
    ) -> Result<String, &'static str> {
        self.create_session_inner(SessionOrigin::Home, None, false)
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn submit_message_targeted(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
        reply_to_object_id: Option<&str>,
        resumed: Option<ResumedSubmission>,
    ) -> Result<String, &'static str> {
        let _dispatch = lock(&self.state.dispatch);
        self.submit_message_targeted_locked(
            explicit_session_id,
            text,
            submission,
            selection,
            pending_claim,
            reply_to_object_id,
            resumed,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal dispatch entrypoint shares one lock-validated path across direct chat, receipts, and card resume"
    )]
    pub(super) fn submit_message_targeted_locked(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
        reply_to_object_id: Option<&str>,
        resumed: Option<ResumedSubmission>,
    ) -> Result<String, &'static str> {
        let text = text.trim();
        tracing::debug!(
            target: "flowy.prompt",
            stage = "AgentHandle::submit_message",
            chars = text.chars().count(),
            retry = submission.is_some(),
            "enter"
        );
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        if self.is_initializing() {
            return Err("library.loading");
        }
        let state = &self.state;
        if *lock(&state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        // Re-verify receipt liveness inside `dispatch` so a concurrent
        // `cancel_task(TaskTarget::Receipt)` between `bind` and submit never
        // dispatches a cancelled prompt (closes Window A TOCTOU).
        if let Some(claim) = &pending_claim {
            let queue = AcceptanceQueue::new(state.root.clone());
            let still_owned = queue
                .get(&claim.receipt_id)
                .map_err(|_| "notice.session_storage_failed")?
                .is_some_and(|receipt| {
                    receipt.completed_at_ms.is_none()
                        && !receipt.dead
                        && !receipt.cancelled
                        && !receipt.quarantined
                        && receipt.lease.as_ref().is_some_and(|lease| {
                            lease.attempt_id == claim.attempt_id
                        })
                });
            if !still_owned {
                return Err(RECEIPT_SUPERSEDED_ERR);
            }
        }
        let engine = lock(&self.engine).clone().ok_or("notice.agent_failed")?;
        let bypass_busy_queue =
            resumed.as_ref().is_some_and(|item| item.bypass_busy_queue);
        let is_router_announcement = resumed
            .as_ref()
            .is_some_and(|item| item.is_router_announcement);
        if let Some(sub) = submission {
            if bypass_busy_queue {
                let message_id = format!("submission-{sub:032x}");
                let committed = engine.with_sessions(|records| {
                    records.iter().find_map(|rec| {
                        rec.messages
                            .iter()
                            .any(|m| m.role == Role::User && m.id == message_id)
                            .then(|| rec.id.clone())
                    })
                });
                if let Some(session) = committed {
                    return Ok(session);
                }
            } else if let Some(session) = engine.submitted_session(sub) {
                return Ok(session);
            }
        }
        if let Some(sub) = submission {
            let expected_card_id = format!("disambig-submission-{sub:032x}");
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
        }
        if selection.is_some_and(|selection| {
            self.state.context_generation.load(Ordering::Acquire)
                != selection.generation
                || lock(&state.active).as_deref() != selection.active
        }) {
            return Err("notice.agent_busy");
        }
        let id = match explicit_session_id {
            Some(target_id) => {
                if !lock(&state.live).contains_key(target_id) {
                    return Err("notice.agent_failed");
                }
                target_id.to_owned()
            }
            None => self.ensure_active_session_locked()?,
        };
        if let Err(error) = ensure_engine_session(&engine, &id) {
            log_storage_io_error(
                "ensure structured session",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            state.report_storage_error();
        }
        let core_session = engine.session(&id).ok_or("notice.agent_failed")?;
        let is_resumed = resumed.is_some();
        let is_card_resume = resumed.as_ref().is_some_and(|item| {
            matches!(item.prompt_source, MessageSource::CardResume)
                || item.contract.is_some()
        });
        if !is_card_resume
            && (!core_session.pending_cards.is_empty()
                || core_session.status == TurnState::WaitingForUser
                || lock(&state.live).get(&id).is_some_and(|session| {
                    session.status == TurnStatus::WaitingForUser
                }))
        {
            return Err("notice.card_pending");
        }
        self.settle_finishing_fence(&engine, &id);
        let target_busy = lock(&state.live)
            .get(&id)
            .is_some_and(|session| session.status.is_busy())
            || engine.is_active(&id);
        if core_session.role == SessionRole::Router
            && target_busy
            && !bypass_busy_queue
            && pending_claim.is_none()
        {
            let sub_u128 = if let Some(sub) = submission {
                sub
            } else {
                let mut bytes = [0_u8; 16];
                getrandom::fill(&mut bytes)
                    .map_err(|_| "notice.agent_failed")?;
                u128::from_le_bytes(bytes)
            };
            let sub_id = format!("submission-{sub_u128:032x}");
            let outcome = engine
                .enqueue_router_followup(
                    &id,
                    PendingRouterFollowup {
                        submission_id: sub_id,
                        text: text.to_owned(),
                        reply_to_object_id: reply_to_object_id
                            .map(str::to_owned),
                        created_at_ms: now_ms(),
                    },
                    work_events::MAX_ROUTER_FOLLOWUPS,
                )
                .map_err(|error| match error {
                    RunError::Storage(io_err) => {
                        log_storage_io_error(
                            "persist router follow-up",
                            &flowy_agent_core::SessionIndex::path(&state.root),
                            &io_err,
                        );
                        state.report_storage_error();
                        "notice.session_storage_failed"
                    }
                    _ => "notice.agent_failed",
                })?;
            match outcome {
                EnqueueOutcome::Duplicate => return Ok(id),
                EnqueueOutcome::QueueFull => {
                    return Err("notice.router_queue_full");
                }
                EnqueueOutcome::Enqueued => {
                    {
                        let mut live = lock(&state.live);
                        if let Some(session) = live.get_mut(&id) {
                            session.projection.push_user(text);
                            session.updated_at = now_ms();
                        }
                    }
                    if persist_index(state).is_err() {
                        state.report_storage_error();
                    }
                    state.push_update(AgentUpdate::Session { id: id.clone() });
                    return Ok(id);
                }
            }
        }
        if target_busy
            || (pending_claim.is_some()
                && core_session.role != SessionRole::Router
                && self.in_flight_work_count() >= self.max_concurrent_works())
        {
            return Err("notice.agent_busy");
        }
        let config = lock(&self.config).clone();
        let skills = self.skill_catalog();
        let (mut prompt, active_skills) = prepare_skill_prompt(text, &skills)?;
        let prompt_source = resumed
            .as_ref()
            .map_or(MessageSource::User, |item| item.prompt_source.clone());
        let skip_push_user_line = resumed
            .as_ref()
            .is_some_and(|item| item.skip_push_user_line);
        let skip_contract_evolve = resumed
            .as_ref()
            .is_some_and(|item| item.skip_contract_evolve);
        let preserve_turn_denial = resumed
            .as_ref()
            .is_some_and(|item| item.preserve_turn_denial);
        let preserve_tool_approval = resumed
            .as_ref()
            .is_some_and(|item| item.preserve_tool_approval);
        if let Some(override_model) = resumed
            .as_ref()
            .and_then(|item| item.model_prompt_override.clone())
        {
            let display = resumed
                .as_ref()
                .and_then(|item| item.display_text_override.clone())
                .or_else(|| Some(text.to_owned()));
            prompt = match &prompt_source {
                MessageSource::SystemEvent { event_kind } => {
                    PromptTurn::system_event(event_kind.clone(), override_model)
                }
                MessageSource::CardResume => {
                    PromptTurn::card_resume(override_model, display)
                }
                _ => PromptTurn {
                    model: override_model,
                    display,
                    source: prompt_source.clone(),
                },
            };
        } else {
            if let Some(suffix) = resumed
                .as_ref()
                .and_then(|item| item.model_prompt_suffix.as_deref())
            {
                prompt.model.push_str(suffix);
            }
            prompt.source = prompt_source;
        }
        let is_internal_event_turn = is_router_announcement
            || matches!(
                prompt.source,
                MessageSource::SystemEvent { .. } | MessageSource::CardResume
            );
        let skip_auto_title =
            core_session.role == SessionRole::Worker || is_internal_event_turn;
        let pre_resolved_contract = resumed.and_then(|item| item.contract);
        let title_prompt =
            if is_resumed || observer::is_internal_control_line_text(text) {
                lock(&state.live)
                    .get(&id)
                    .and_then(|session| {
                        session.projection.lines.iter().find_map(|line| {
                            match line {
                            ChatLine::User { text }
                                if !text.trim().is_empty()
                                    && !observer::is_internal_control_line_text(
                                        text,
                                    ) =>
                            {
                                Some(text.clone())
                            }
                            _ => None,
                        }
                        })
                    })
                    .unwrap_or_else(|| text.to_owned())
            } else {
                text.to_owned()
            };
        let stream = config
            .api_key
            .is_none()
            .then(|| echo_stream_fn(text.to_owned()));
        let workspace = lock(&state.live)
            .get(&id)
            .and_then(|session| session.workspace.clone());
        let _ = is_resumed;
        let cwd = core_session.workspace.as_ref().map_or_else(
            || session_working_directory(&state.root, workspace.as_ref()),
            |workspace| workspace.root.clone(),
        );
        if !preserve_turn_denial {
            lock(&state.denied_tool_turns).remove(&id);
        }
        if !preserve_tool_approval {
            lock(&state.approved_tool_calls).remove(&id);
        }
        let mut run_config = engine_config(&config, &cwd);
        let mut staged_intents_for_turn: Option<
            crate::publish_tool::StagedIntentLog,
        > = None;
        if core_session.role == SessionRole::Router {
            run_config.registry = router::router_tool_registry(
                Arc::downgrade(self),
                id.clone(),
                is_router_announcement,
            );
            run_config.pre_tool_hook = (!is_router_announcement).then(|| {
                router::build_router_pre_tool_hook(
                    Arc::downgrade(self),
                    id.clone(),
                )
            });
            run_config.system = router::build_router_system_prompt(
                self,
                &id,
                is_router_announcement,
            );
        } else if core_session.access_mode == WorkAccessMode::ReadOnly {
            run_config.registry =
                read_only_tool_registry(&cwd, &skills, active_skills);
            run_config.pre_tool_hook = None;
            run_config.system = append_skill_listing(
                "You are running in read-only inspection mode. Inspect files using only read_file, glob, and grep; do not attempt to modify files or execute shell commands.",
                &skills,
            );
        } else {
            run_config.registry = tool_registry(&cwd, &skills, active_skills);
            run_config.pre_tool_hook = Some(build_pre_tool_hook(
                id.clone(),
                cwd.clone(),
                Arc::clone(state),
            ));
            if core_session.origin == SessionOrigin::Home
                && let Some(submission) = submission
                && let Some(session_workspace) = core_session.workspace.as_ref()
                && let Some(store) = state.artifact_store.get().cloned()
            {
                let contract = if let Some(resolved) = pre_resolved_contract {
                    resolved
                } else if skip_contract_evolve {
                    core_session.work_contract.as_ref().map_or_default(
                        crate::verify::BriefContract::from_work_contract,
                    )
                } else {
                    match crate::verify::resolve_contract_sync(
                        core_session.work_contract.as_ref(),
                        text,
                        reply_to_object_id,
                        now_ms(),
                    ) {
                        crate::verify::ResolveOutcome::Ambiguous {
                            mut card,
                            ..
                        } => {
                            if let PendingCard::Disambiguation {
                                card_id, ..
                            } = &mut card
                            {
                                *card_id = format!(
                                    "disambig-submission-{submission:032x}"
                                );
                            }
                            return self.park_disambiguation_locked(
                                &engine,
                                &id,
                                card,
                                text,
                                skip_push_user_line,
                                pending_claim,
                            );
                        }
                        crate::verify::ResolveOutcome::Resolved {
                            contract,
                            ..
                        } => contract,
                    }
                };
                let submission_id = format!("submission-{submission:032x}");
                let output_root =
                    session_workspace.root.join("outputs").join(&submission_id);
                std::fs::create_dir_all(&output_root).map_err(|error| {
                    log_storage_io_error(
                        "create turn output directory",
                        &output_root,
                        &error,
                    );
                    "notice.session_storage_failed"
                })?;
                let delivery = crate::delivery::DeliveryStore::new(
                    state.root.join("delivery"),
                );
                if core_session.work_contract.is_some() || !contract.is_empty()
                {
                    run_config.work_contract =
                        Some(contract.to_work_contract());
                }
                let expected_revision = run_config
                    .work_contract
                    .as_ref()
                    .map(|wc| wc.revision.max(1));
                let semantic_brief = compose_semantic_brief(
                    &core_session.messages,
                    if skip_contract_evolve { "" } else { text },
                );
                let semantic = state
                    .semantic_verify_enabled
                    .load(Ordering::Acquire)
                    .then(|| {
                        config.api_key.as_ref().map(|api_key| {
                            crate::semantic_verify::SemanticContext {
                                brief: semantic_brief.clone(),
                                route: model_route(&config),
                                api_key: api_key.clone(),
                            }
                        })
                    })
                    .flatten();
                let failures: crate::verify::VerifyFailureTracker =
                    Arc::new(Mutex::new(HashMap::new()));
                run_config.registry.register(
                    crate::verify::definition_with_options(
                        Some(delivery.clone()),
                        semantic.clone(),
                        Some(contract.clone()),
                        Some(Arc::clone(&failures)),
                    ),
                );
                run_config.registry.register(
                    crate::verify::artifact_definition_with_options(
                        Some(delivery.clone()),
                        semantic,
                        Some(contract),
                        Some(failures),
                    ),
                );
                let staged_log: crate::publish_tool::StagedIntentLog =
                    Arc::new(Mutex::new(Vec::new()));
                staged_intents_for_turn = Some(Arc::clone(&staged_log));
                run_config.registry.register(
                    crate::publish_tool::definition_with_revision(
                        store.clone(),
                        Some(delivery),
                        expected_revision,
                        Some(staged_log),
                    ),
                );
                run_config.delivery_committer = Some(Arc::new(store));
                run_config.system = format!(
                    "Files to deliver to this user must be created in the current turn output directory: {}. Use write_file (or create_deck for .pptx presentations) with a path inside that directory. Before calling publish_file, verify every .pptx file with verify_deck and every .docx/.xlsx file with verify_artifact. If verification returns next_action=\"rerender_once\", fix and re-verify at most once; if verification still fails or returns next_action=\"clarify_or_rebuild\", stop and explain the issues instead of calling publish_file. publish_file only stages files; delivery occurs after the whole turn completes successfully. Do not treat ordinary text links or intermediate files as delivered artifacts. Maximum 8 files, 128 MiB each, 256 MiB per turn.",
                    output_root.display()
                );
            }
            run_config.system =
                append_skill_listing(&run_config.system, &skills);
        }
        let mut workspace_lease = if let Some(access_mode) =
            self.session_access_mode(&id)
        {
            self.settle_finishing_workspace_lease(&engine, &cwd, access_mode);
            Some(
                state
                    .workspace_locks
                    .try_acquire(&cwd, access_mode, &id)
                    .ok_or("notice.agent_busy")?,
            )
        } else {
            None
        };
        tracing::debug!(
            target: "flowy.prompt",
            stage = "AgentHandle::submit_message",
            session = %id,
            live = config.api_key.is_some(),
            model = %config.model,
            base_url = %config.base_url,
            "→ Engine::start_with_config"
        );
        let turn = engine
            .start_with_config(
                id.clone(),
                prompt.clone(),
                run_config,
                stream,
                submission,
            )
            .map_err(|error| {
                if let RunError::Storage(storage_error) = &error {
                    log_storage_io_error(
                        "start agent turn",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        storage_error,
                    );
                }
                match error {
                    RunError::Storage(_) => "notice.session_storage_failed",
                    RunError::Busy => "notice.agent_busy",
                    _ => "notice.agent_failed",
                }
            })?;
        let (fallback_title, title_signal) = {
            let mut live = lock(&state.live);
            let session = live.get_mut(&id).expect("active session exists");
            let now = now_ms();
            session.status = TurnStatus::Running;
            session.turn_started_at = Some(now);
            session.updated_at = now;
            session.heartbeat = Some(BranchHeartbeat {
                last_completed: None,
                last_completed_at_ms: None,
                current_action: "thinking".to_owned(),
                next_check_hint: Some(
                    "awaiting first tool or model step".to_owned(),
                ),
                awaiting_evidence: true,
            });
            if !skip_push_user_line {
                session.projection.push_user(text);
            }
            let title = if skip_auto_title {
                (None, None)
            } else if session.titled {
                let signal = if session.auto_title_pending
                    && !session.auto_title_in_flight
                    && session.auto_title_attempts < MAX_AUTO_TITLE_ATTEMPTS
                    && config.api_key.is_some()
                {
                    session.title_signal.cancel();
                    let signal = CancelToken::new();
                    session.title_signal = signal.clone();
                    Some(signal)
                } else {
                    None
                };
                (None, signal)
            } else {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
                session.auto_title_pending = true;
                let signal = config.api_key.is_some().then(CancelToken::new);
                if let Some(signal) = &signal {
                    session.title_signal.cancel();
                    session.title_signal = signal.clone();
                }
                (Some(session.title.clone()), signal)
            };
            drop(live);
            title
        };
        if let Some(title) = fallback_title
            && engine.set_session_title(&id, title, true).is_err()
        {
            state.report_storage_error();
        }
        if persist_index(state).is_err() {
            state.report_storage_error();
        }
        let turn_done_signal = if let Some(claim) = pending_claim {
            let hb = claim.heartbeat_cancel.clone();
            let receipt_id = claim.receipt_id.clone();
            lock(&state.active_claims).insert(receipt_id.clone(), claim);
            state.push_update(AgentUpdate::Receipt {
                id: receipt_id,
                state: ReceiptState::Running,
            });
            Some(hb)
        } else {
            None
        };
        state.push_update(AgentUpdate::Session { id: id.clone() });
        state.push_update(AgentUpdate::Sessions);
        let state = Arc::clone(state);
        let weak_harness = Arc::downgrade(self);
        let receipt = id.clone();
        let completed_submission = submission;
        let session_role = core_session.role;
        let session_root_id = core_session.root_session_id;
        self.runtime.spawn(async move {
            let outcome = turn.await;
            if let Some(signal) = turn_done_signal {
                signal.cancel();
            }
            match &outcome {
                Ok(LoopOutcome::Completed) => {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = true,
                        "turn finished"
                    );
                }
                Ok(loop_outcome) => {
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = false,
                        ?loop_outcome,
                        prompt_preview = %log_preview(&prompt.model, 200),
                        "turn finished"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = false,
                        error = %error,
                        prompt_preview = %log_preview(&prompt.model, 200),
                        "turn finished"
                    );
                }
            }
            if matches!(outcome, Ok(LoopOutcome::Completed)) {
                capture_turn_artifacts(
                    &engine,
                    &state,
                    &id,
                    completed_submission,
                );
            } else if matches!(
                outcome,
                Ok(LoopOutcome::Aborted | LoopOutcome::WaitingForUser)
            ) && let Some(staged_log) = staged_intents_for_turn
            {
                let pending = std::mem::take(
                    &mut *staged_log
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                if !pending.is_empty() {
                    let expected_sub = completed_submission
                        .map(|s| format!("submission-{s:032x}"));
                    let live_delivery_failed =
                        lock(&state.live).get(&id).is_some_and(|session| {
                            matches!(session.status, TurnStatus::DeliveryFailed)
                        });
                    let has_durable_commit =
                        expected_sub.as_deref().is_some_and(|sub| {
                            engine.session(&id).is_some_and(|rec| {
                                rec.delivery_commits
                                    .iter()
                                    .any(|commit| commit.submission_id == sub)
                            })
                        });
                    if !live_delivery_failed && !has_durable_commit {
                        let gate = crate::delivery::DeliveryStore::new(
                            state.root.join("delivery"),
                        );
                        let reason = match &outcome {
                            Ok(LoopOutcome::WaitingForUser) => {
                                "turn_waiting_for_user"
                            }
                            _ => "turn_aborted",
                        };
                        let now = now_ms();
                        for (delivery_id, artifact_path, sha256) in pending {
                            let _ = gate.abort_intent(
                                &delivery_id,
                                &artifact_path,
                                &sha256,
                                reason,
                                now,
                            );
                        }
                    }
                }
            }
            // Release the directory lease strictly AFTER `turn.await` and
            // artifact capture / staged-intent cleanup, and strictly BEFORE
            // acquiring `accept_drain -> dispatch` (`finish_accepted_if_owned`),
            // `queue.complete`, `kick_autonomous_drain`, or `generate_title`.
            if let Some(lease) = workspace_lease.as_mut() {
                lease.release();
            }
            let expected_sub =
                completed_submission.map(|s| format!("submission-{s:032x}"));
            if matches!(outcome, Ok(LoopOutcome::WaitingForUser)) {
                // PendingCard::ToolApproval is durably persisted in
                // `history.json`. Complete the AcceptanceQueue receipt (matching
                // the Disambiguation handoff) so 30s lease expiry or a later
                // `DenyTool` never causes autonomous_drain to re-run this prompt.
                let matching_claims =
                    take_session_claims(&state.active_claims, &id);
                for claim in matching_claims {
                    claim.heartbeat_cancel.cancel();
                    let queue = AcceptanceQueue::new(state.root.clone());
                    if queue
                        .complete(
                            &claim.receipt_id,
                            &claim.attempt_id,
                            now_ms(),
                        )
                        .unwrap_or(false)
                    {
                        state.push_update(AgentUpdate::Receipt {
                            id: claim.receipt_id,
                            state: ReceiptState::Completed,
                        });
                    }
                }
                if state.autonomous_drain.load(Ordering::Acquire)
                    && let Some(harness) = weak_harness.upgrade()
                {
                    harness.kick_autonomous_drain();
                }
            } else if state.autonomous_drain.load(Ordering::Acquire)
                && let Some(harness) = weak_harness.upgrade()
            {
                let live_delivery_failed =
                    lock(&state.live).get(&id).is_some_and(|session| {
                        matches!(session.status, TurnStatus::DeliveryFailed)
                    });
                let has_durable_commit =
                    expected_sub.as_deref().is_some_and(|sub| {
                        engine.session(&id).is_some_and(|rec| {
                            rec.delivery_commits
                                .iter()
                                .any(|commit| commit.submission_id == sub)
                        })
                    });
                let completed = matches!(outcome, Ok(LoopOutcome::Completed))
                    || live_delivery_failed
                    || (matches!(outcome, Err(RunError::Storage(_)))
                        && has_durable_commit);
                let _ = harness.finish_accepted_if_owned(
                    &id,
                    expected_sub.as_deref(),
                    completed,
                );
                harness.kick_autonomous_drain();
                drop(harness);
            }
            if session_role == SessionRole::Worker
                && let Some(root_id) = session_root_id
                && root_id != id
            {
                let kind = match &outcome {
                    Ok(LoopOutcome::Completed) => {
                        work_events::WorkEventKind::Completed
                    }
                    Ok(LoopOutcome::WaitingForUser) => {
                        work_events::WorkEventKind::WaitingForUser
                    }
                    Ok(LoopOutcome::Aborted) => {
                        work_events::WorkEventKind::Cancelled
                    }
                    Ok(LoopOutcome::Failed) | Err(_) => {
                        work_events::WorkEventKind::Failed
                    }
                };
                let contract_revision = engine
                    .with_session(&id, |rec| {
                        expected_sub
                            .as_deref()
                            .and_then(|sub| {
                                rec.submission_revisions.get(sub).copied()
                            })
                            .or_else(|| {
                                rec.work_contract.as_ref().map(|c| c.revision)
                            })
                    })
                    .flatten();
                work_events::push_work_event(
                    &state,
                    &root_id,
                    work_events::WorkEvent {
                        worker_id: id.clone(),
                        submission_id: expected_sub,
                        contract_revision,
                        kind,
                        observed_at_ms: now_ms(),
                    },
                );
                if let Some(harness) = weak_harness.upgrade() {
                    work_events::schedule_router_inbox_flush(
                        &harness, &root_id,
                    );
                }
            } else if session_role == SessionRole::Router {
                if is_router_announcement {
                    work_events::clear_announcement_in_flight(&state, &id);
                }
                if let Some(harness) = weak_harness.upgrade() {
                    work_events::schedule_router_inbox_flush(&harness, &id);
                }
            }
            if matches!(outcome, Ok(LoopOutcome::Completed))
                && let Some(signal) = title_signal
            {
                tracing::debug!(
                    target: "flowy.prompt",
                    stage = "AgentHandle::generate_title",
                    session = %id,
                    "sidecar LLM"
                );
                observer::generate_title(
                    engine,
                    state,
                    config,
                    signal,
                    id,
                    title_prompt,
                )
                .await;
            }
        });
        Ok(receipt)
    }

    fn park_disambiguation_locked(
        &self,
        engine: &Engine,
        id: &str,
        card: PendingCard,
        text: &str,
        skip_push_user_line: bool,
        pending_claim: Option<AcceptedClaim>,
    ) -> Result<String, &'static str> {
        let state = &self.state;
        let now = now_ms();
        engine.park_pending_card(id, card, None).map_err(|error| {
            log_storage_io_error(
                "park disambiguation card",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            state.report_storage_error();
            "notice.session_storage_failed"
        })?;
        let fallback_title = {
            let mut live = lock(&state.live);
            let session = live.get_mut(id).expect("active session exists");
            session.status = TurnStatus::WaitingForUser;
            session.turn_started_at = None;
            session.updated_at = now;
            session.heartbeat = Some(BranchHeartbeat {
                last_completed: session
                    .heartbeat
                    .as_ref()
                    .and_then(|hb| hb.last_completed.clone()),
                last_completed_at_ms: session
                    .heartbeat
                    .as_ref()
                    .and_then(|hb| hb.last_completed_at_ms),
                current_action: "waiting_for_user".to_owned(),
                next_check_hint: Some(
                    "awaiting disambiguation card selection".to_owned(),
                ),
                awaiting_evidence: true,
            });
            if !skip_push_user_line {
                session.projection.push_user(text);
            }
            session.projection.lines.push(ChatLine::Status {
                text: TurnStatus::WaitingForUser.key().into(),
            });
            let title = if session.titled
                || session.role == SessionRole::Worker
                || observer::is_internal_control_line_text(text)
            {
                None
            } else {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
                session.auto_title_pending = true;
                Some(session.title.clone())
            };
            drop(live);
            title
        };
        if let Some(title) = fallback_title
            && engine.set_session_title(id, title, true).is_err()
        {
            state.report_storage_error();
        }
        if let Some(claim) = pending_claim {
            claim.heartbeat_cancel.cancel();
            let queue = AcceptanceQueue::new(state.root.clone());
            if queue
                .complete(&claim.receipt_id, &claim.attempt_id, now)
                .unwrap_or(false)
            {
                state.push_update(AgentUpdate::Receipt {
                    id: claim.receipt_id,
                    state: ReceiptState::Completed,
                });
            }
        }
        if persist_index(state).is_err() {
            state.report_storage_error();
        }
        state.push_update(AgentUpdate::Session { id: id.to_owned() });
        state.push_update(AgentUpdate::Sessions);
        Ok(id.to_owned())
    }

    fn ensure_active_session_locked(&self) -> Result<String, &'static str> {
        let active = lock(&self.state.active).clone();
        if let Some(id) = active
            && lock(&self.state.live).contains_key(&id)
        {
            return Ok(id);
        }
        let workspace = lock(&self.state.workspace_for_new_sessions).clone();
        let origin = lock(&self.state.origin_for_new_sessions).clone();
        self.create_session_inner(origin, workspace, true)
    }
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

fn append_skill_listing(base: &str, skills: &[InstalledSkill]) -> String {
    let listing = format_available_skills(skills);
    match (base.trim().is_empty(), listing.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_owned(),
        (true, false) => listing,
        (false, false) => format!("{base}\n\n{listing}"),
    }
}

fn engine_config(config: &LlmConfig, cwd: &Path) -> EngineConfig {
    EngineConfig {
        route: model_route(config),
        api_key: config.api_key.clone(),
        system: String::new(),
        registry: tool_registry(cwd, &[], Vec::new()),
        cwd: Some(cwd.display().to_string()),
        todos: None,
        delivery_committer: None,
        work_contract: None,
        pre_tool_hook: None,
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

fn recover_corrupt_history(root: &Path) -> std::io::Result<()> {
    let path = flowy_agent_core::SessionIndex::path(root);
    if !path.exists() {
        return Ok(());
    }
    let backup =
        path.with_file_name(format!("history.corrupt-{}.json", now_ms()));
    std::fs::rename(path, backup)
}

fn recover_corrupt_index(root: &Path) -> std::io::Result<()> {
    let path = SessionIndex::path(root);
    if !path.exists() {
        return Ok(());
    }
    let backup =
        path.with_file_name(format!("index.corrupt-{}.json", now_ms()));
    std::fs::rename(path, backup)
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

/// Scans projected conversation rows (skipping internal host control/event
/// prompts) against tokenized lowercased search terms and returns the best
/// body match score plus its redacted snippet.
fn score_projected_lines(
    lines: &[ChatLine],
    title_lower: &str,
    tokens: &[String],
    existing_title_score: u32,
) -> (u32, Option<String>) {
    let mut best_score = existing_title_score;
    let mut best_snippet = None;
    for line in lines.iter().rev() {
        let raw_text = match line {
            ChatLine::User { text }
                if !observer::is_internal_control_line_text(text) =>
            {
                text.as_str()
            }
            ChatLine::Assistant { text } => text.as_str(),
            _ => continue,
        };
        let cleaned =
            title::redact_search_text(&title::strip_markdown_inline(raw_text));
        if cleaned.is_empty() {
            continue;
        }
        let lower = cleaned.to_lowercase();
        if tokens.iter().all(|tok| lower.contains(tok.as_str())) {
            let snippet = extract_match_snippet(&cleaned, &tokens[0]);
            return (best_score.max(55), Some(snippet));
        }
        if best_score == 0
            && best_snippet.is_none()
            && tokens.iter().all(|tok| {
                title_lower.contains(tok.as_str())
                    || lower.contains(tok.as_str())
            })
        {
            let first_body_tok = tokens
                .iter()
                .find(|tok| lower.contains(tok.as_str()))
                .map_or(tokens[0].as_str(), String::as_str);
            best_snippet =
                Some(extract_match_snippet(&cleaned, first_body_tok));
            best_score = 40;
        }
    }
    (best_score, best_snippet)
}

/// Scores a lowercased candidate string against tokenized lowercased search terms.
fn score_text_match(
    haystack_lower: &str,
    tokens: &[String],
    full_query: &str,
    exact_score: u32,
    prefix_score: u32,
    substring_score: u32,
    fuzzy_score: u32,
) -> u32 {
    if haystack_lower.is_empty() || tokens.is_empty() {
        return 0;
    }
    if haystack_lower == full_query {
        return exact_score;
    }
    if haystack_lower.starts_with(full_query) {
        return prefix_score;
    }
    if tokens
        .iter()
        .all(|tok| haystack_lower.contains(tok.as_str()))
    {
        return substring_score;
    }
    if is_subsequence(haystack_lower, full_query) {
        return fuzzy_score;
    }
    0
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars().filter(|ch| !ch.is_whitespace());
    let Some(mut target) = needle_chars.next() else {
        return false;
    };
    for ch in haystack.chars() {
        if ch == target {
            match needle_chars.next() {
                Some(next) => target = next,
                None => return true,
            }
        }
    }
    false
}

/// Extracts a char-safe ~64-char snippet centered around the first match of
/// `needle_lower` inside `text` (plain text with Markdown already stripped).
fn extract_match_snippet(text: &str, needle_lower: &str) -> String {
    const WINDOW: usize = 64;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= WINDOW {
        return text.to_owned();
    }
    let lower_chars: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = needle_lower.chars().collect();
    let match_pos = if needle.is_empty() || needle.len() > lower_chars.len() {
        0
    } else {
        lower_chars
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .unwrap_or(0)
    };
    let start = match_pos.saturating_sub(18).min(chars.len());
    let end = (start + WINDOW).min(chars.len());
    let mut snippet: String = chars[start..end].iter().collect();
    if start > 0 {
        snippet = format!("\u{2026}{}", snippet.trim_start());
    }
    if end < chars.len() {
        snippet = format!("{}\u{2026}", snippet.trim_end());
    }
    snippet
}
