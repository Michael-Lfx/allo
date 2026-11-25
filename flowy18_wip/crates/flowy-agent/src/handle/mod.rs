//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    AcceptanceQueue, AcceptedMessage, DeliveryCommit, Engine, EngineConfig,
    LoopOutcome, ModelRoute, ReceiptState, RequiredDeliverable, RunError,
    SessionOrigin, SessionRecord, SessionWorkspace, SubmissionOutcome,
    TurnObserver, TurnState, complete_text, echo_stream_fn, log_preview,
    tools::{
        ToolGuard, ToolRegistry, pi_shell_definition, register_builtin_tools,
    },
    types::{
        CancelToken, ContentBlock, Message, MessageSource, Role, ToolCallArgs,
    },
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

mod artifacts;
mod guards;
mod observer;
mod queue;
#[cfg(test)]
mod tests;
mod work;

pub use self::guards::{confined_to, has_reserved_device_component};
use self::guards::{
    ensure_tool_cwd, session_working_directory, tool_registry, tool_target,
};
use self::observer::{
    UiObserver, apply_delivery_commit_to_live, backfill_conversation_artifacts,
    capture_turn_artifacts, confirm_commit_audit, ensure_engine_session,
    extract_session_preview, finish_interrupted_lines, hydrate_engine,
    import_engine_sessions, mark_delivery_failed,
    migrate_legacy_source_deletions, persist_index, recover_session_deletions,
    sync_engine_titles,
};
use self::queue::{
    push_reconciled_receipt_updates, reconcile_committed_acceptances,
};
use self::work::{
    latest_session_submission_id, reconcile_partial_work_after_override,
    submission_committed_in_session,
};

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AgentUpdate {
    Initialized,
    Sessions,
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
}

/// Result of applying one [`CardAction`] inside the host kernel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardActionOutcome {
    ArtifactDelivered { artifact_id: String },
    DeliveryRetried,
    ReceiptRequeued { receipt_id: String },
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

struct Live {
    projection: ChatProjection,
    title: String,
    titled: bool,
    /// A sidecar title request is queued for this session.
    auto_title_pending: bool,
    /// Cancels the in-flight title request on stop.
    title_signal: CancelToken,
    status: TurnStatus,
    updated_at: u64,
    turn_started_at: Option<u64>,
    workspace: Option<AgentWorkspace>,
    origin: SessionOrigin,
    session_workspace: Option<SessionWorkspace>,
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

struct UiState {
    root: PathBuf,
    live: Mutex<HashMap<String, Live>>,
    /// File-tool arguments held between `tool_start` and `tool_end`, keyed by
    /// session and call id, so the change card never re-reads the file.
    tool_args: Mutex<HashMap<String, serde_json::Value>>,
    active: Mutex<Option<String>>,
    accepted_claim: Mutex<Option<AcceptedClaim>>,
    accepted_lease_ms: AtomicU64,
    autonomous_drain: AtomicBool,
    drain_wakeup_scheduled: AtomicBool,
    /// Epoch ms deadline of the currently scheduled drain backoff sleep (`0` = none).
    drain_sleep_until_ms: AtomicU64,
    drain_cancel: CancelToken,
    #[cfg(test)]
    before_accepted_bind_hook: Mutex<Option<AcceptedSubmitHook>>,
    #[cfg(test)]
    before_accepted_submit_hook: Mutex<Option<AcceptedSubmitHook>>,
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
            active: Mutex::new(None),
            accepted_claim: Mutex::new(None),
            accepted_lease_ms: AtomicU64::new(ACCEPTED_LEASE_MS),
            autonomous_drain: AtomicBool::new(false),
            drain_wakeup_scheduled: AtomicBool::new(false),
            drain_sleep_until_ms: AtomicU64::new(0),
            drain_cancel: CancelToken::new(),
            #[cfg(test)]
            before_accepted_bind_hook: Mutex::new(None),
            #[cfg(test)]
            before_accepted_submit_hook: Mutex::new(None),
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
    _runtime: tokio::runtime::Runtime,
}

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        self.state.autonomous_drain.store(false, Ordering::Release);
        self.state.drain_cancel.cancel();
    }
}

/// Lock ordering invariant (outer -> inner; never acquire in reverse):
/// `accept_drain` -> `dispatch` -> `engine` (clone & release) ->
/// `index_write` -> `live` -> leaf locks (`active`, `accepted_claim`,
/// `updates`, `tool_args`, `notified_quarantined`).
struct Harness {
    runtime: tokio::runtime::Handle,
    state: Arc<UiState>,
    engine: Mutex<Option<Arc<Engine>>>,
    config: Mutex<LlmConfig>,
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
        let owner = Arc::new(RuntimeOwner {
            state: Arc::clone(&state),
            _runtime: runtime,
        });
        Self {
            inner: Arc::new(Harness {
                runtime: rt_handle,
                state,
                engine: Mutex::new(None),
                config: Mutex::new(config),
            }),
            _runtime: owner,
        }
    }

    fn initialize(&self) {
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
        self.inner.state.push_update(AgentUpdate::Artifacts);
        push_reconciled_receipt_updates(&self.inner.state, boot_receipts);
        if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
            self.inner.kick_autonomous_drain();
        }
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::with_config",
            "done"
        );
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
                        auto_title_pending: false,
                        title_signal: CancelToken::new(),
                        status,
                        updated_at: record.updated_at,
                        turn_started_at: None,
                        workspace: record.workspace,
                        origin: record.origin,
                        session_workspace: record.session_workspace,
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
        // Retrying rather than falsely displaying Running until lease expiry.
        if let Ok(released_ids) = AcceptanceQueue::new(state.root.clone())
            .release_uncommitted_leases(now_ms())
        {
            for id in released_ids {
                receipt_updates.push((id, ReceiptState::Retrying));
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
            .map(|(id, session)| SessionSummary {
                id: id.clone(),
                title: session.title.clone(),
                updated_at: session.updated_at,
                project_id: session
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.project_id.clone()),
            })
            .collect();
        drop(live);
        rows.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
        rows
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
                    TurnStatus::Error { .. } | TurnStatus::DeliveryFailed
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
            .and_then(|engine| engine.session(session_id))
            .is_some_and(|record| {
                latest_session_submission_id(&record).is_some_and(|sub_id| {
                    matches!(
                        record.submission_outcomes.get(&sub_id),
                        Some(SubmissionOutcome::Partial { .. })
                    )
                })
            })
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
        if self.is_initializing() {
            return Err("library.loading");
        }
        if *lock(&self.inner.state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let id = format!("s-{}", next_stamp());
        let now = now_ms();
        let workspace =
            lock(&self.inner.state.workspace_for_new_sessions).clone();
        let origin = lock(&self.inner.state.origin_for_new_sessions).clone();
        let session_workspace = match &origin {
            SessionOrigin::Home => {
                let root = self
                    .inner
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
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        // The durable history write may fail on the same broken volume the
        // index uses; the live session still works in memory and the failure is
        // reported through `AgentUpdate::StorageError`.
        engine
            .create_session_with_metadata(
                id.clone(),
                now,
                origin.clone(),
                session_workspace.clone(),
            )
            .map_err(|_| "notice.session_storage_failed")?;
        lock(&self.inner.state.live).insert(
            id.clone(),
            Live {
                projection: ChatProjection::new(),
                title: String::new(),
                titled: false,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: TurnStatus::Idle,
                updated_at: now,
                turn_started_at: None,
                workspace,
                origin,
                session_workspace,
            },
        );
        *lock(&self.inner.state.active) = Some(id.clone());
        self.inner
            .state
            .context_generation
            .fetch_add(1, Ordering::AcqRel);
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(id)
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
        self.inner
            .submit_message_targeted(None, text, submission, selection, None)
    }
}

impl Harness {
    pub(super) fn is_initializing(&self) -> bool {
        *lock(&self.state.initializing)
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

    pub(super) fn has_busy_session(&self) -> bool {
        let live_busy = lock(&self.state.live)
            .values()
            .any(|session| session.status.is_busy());
        if live_busy {
            return true;
        }
        let engine = lock(&self.engine).clone();
        engine
            .as_ref()
            .is_some_and(|engine| !engine.active_session_ids().is_empty())
    }

    pub(super) fn is_session_busy_or_active(&self, id: &str) -> bool {
        if lock(&self.state.live)
            .get(id)
            .is_some_and(|session| session.status.is_busy())
        {
            return true;
        }
        let engine = lock(&self.engine).clone();
        engine.as_ref().is_some_and(|engine| engine.is_active(id))
    }

    /// Create a new durable Home session in the background without ever
    /// mutating the UI's active selection or `context_generation`.
    pub(super) fn create_background_home_session(
        &self,
    ) -> Result<String, &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        if *lock(&self.state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let id = format!("s-{}", next_stamp());
        let now = now_ms();
        let root = self
            .state
            .root
            .join("sessions")
            .join("workspaces")
            .join(&id);
        std::fs::create_dir_all(&root).map_err(|error| {
            log_storage_io_error("create session workspace", &root, &error);
            "notice.session_storage_failed"
        })?;
        let session_workspace = Some(SessionWorkspace { root });
        let engine = lock(&self.engine).clone().ok_or("notice.agent_failed")?;
        engine
            .create_session_with_metadata(
                id.clone(),
                now,
                SessionOrigin::Home,
                session_workspace.clone(),
            )
            .map_err(|_| "notice.session_storage_failed")?;
        lock(&self.state.live).insert(
            id.clone(),
            Live {
                projection: ChatProjection::new(),
                title: String::new(),
                titled: false,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: TurnStatus::Idle,
                updated_at: now,
                turn_started_at: None,
                workspace: None,
                origin: SessionOrigin::Home,
                session_workspace,
            },
        );
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.push_update(AgentUpdate::Sessions);
        Ok(id)
    }

    pub(super) fn submit_message_targeted(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
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
        let _dispatch = lock(&state.dispatch);
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
        if let Some(session) =
            submission.and_then(|id| engine.submitted_session(id))
        {
            return Ok(session);
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
        if lock(&state.live)
            .get(&id)
            .is_some_and(|session| session.status.is_busy())
            || engine.is_active(&id)
            || (pending_claim.is_some() && self.has_busy_session())
        {
            return Err("notice.agent_busy");
        }
        if let Err(error) = ensure_engine_session(&engine, &id) {
            log_storage_io_error(
                "ensure structured session",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            state.report_storage_error();
        }
        let config = lock(&self.config).clone();
        let prompt = text.to_owned();
        let stream = config
            .api_key
            .is_none()
            .then(|| echo_stream_fn(prompt.clone()));
        let workspace = lock(&state.live)
            .get(&id)
            .and_then(|session| session.workspace.clone());
        let core_session = engine.session(&id).ok_or("notice.agent_failed")?;
        let cwd = core_session.workspace.as_ref().map_or_else(
            || session_working_directory(&state.root, workspace.as_ref()),
            |workspace| workspace.root.clone(),
        );
        let mut run_config = engine_config(&config, &cwd);
        if core_session.origin == SessionOrigin::Home
            && let Some(submission) = submission
            && let Some(session_workspace) = core_session.workspace.as_ref()
            && let Some(store) = state.artifact_store.get().cloned()
        {
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
            let contract = crate::verify::BriefContract::evolve(
                core_session.work_contract.as_ref(),
                &prompt,
            );
            if core_session.work_contract.is_some() || !contract.is_empty() {
                run_config.work_contract = Some(contract.to_work_contract());
            }
            let semantic_brief =
                compose_semantic_brief(&core_session.messages, &prompt);
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
            run_config
                .registry
                .register(crate::publish_tool::definition(
                    store.clone(),
                    Some(delivery),
                ));
            run_config.delivery_committer = Some(Arc::new(store));
            run_config.system = format!(
                "Files to deliver to this user must be created in the current turn output directory: {}. Use write_file (or create_deck for .pptx presentations) with a path inside that directory. Before calling publish_file, verify every .pptx file with verify_deck and every .docx/.xlsx file with verify_artifact. If verification returns next_action=\"rerender_once\", fix and re-verify at most once; if verification still fails or returns next_action=\"clarify_or_rebuild\", stop and explain the issues instead of calling publish_file. publish_file only stages files; delivery occurs after the whole turn completes successfully. Do not treat ordinary text links or intermediate files as delivered artifacts. Maximum 8 files, 128 MiB each, 256 MiB per turn.",
                output_root.display()
            );
        }
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
            session.projection.push_user(text);
            let title = if session.titled {
                (None, None)
            } else {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
                let signal = config.api_key.is_some().then(CancelToken::new);
                if let Some(signal) = &signal {
                    session.title_signal = signal.clone();
                    session.auto_title_pending = true;
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
            *lock(&state.accepted_claim) = Some(claim);
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
                        prompt_preview = %log_preview(&prompt, 200),
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
                        prompt_preview = %log_preview(&prompt, 200),
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
            }
            if state.autonomous_drain.load(Ordering::Acquire)
                && let Some(harness) = weak_harness.upgrade()
            {
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
                // Partial deliverables and post-history DeliveryFailed turns
                // already completed their LLM execution and left durable cards
                // for user OverrideDeliver / RetryDelivery — settle the queue
                // receipt instead of re-running the entire prompt 10x. Never
                // mark `LoopOutcome::Aborted` as Completed: host cancellations
                // already cleared `accepted_claim` as Cancelled, while provider
                // stream aborts must release for retry instead of completing.
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
                    engine, state, config, signal, id, prompt,
                )
                .await;
            }
        });
        Ok(receipt)
    }

    fn ensure_active_session_locked(&self) -> Result<String, &'static str> {
        let active = lock(&self.state.active).clone();
        if let Some(id) = active
            && lock(&self.state.live).contains_key(&id)
        {
            return Ok(id);
        }
        if self.is_initializing() {
            return Err("library.loading");
        }
        if *lock(&self.state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        let id = format!("s-{}", next_stamp());
        let now = now_ms();
        let workspace = lock(&self.state.workspace_for_new_sessions).clone();
        let origin = lock(&self.state.origin_for_new_sessions).clone();
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
            .create_session_with_metadata(
                id.clone(),
                now,
                origin.clone(),
                session_workspace.clone(),
            )
            .map_err(|_| "notice.session_storage_failed")?;
        lock(&self.state.live).insert(
            id.clone(),
            Live {
                projection: ChatProjection::new(),
                title: String::new(),
                titled: false,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: TurnStatus::Idle,
                updated_at: now,
                turn_started_at: None,
                workspace,
                origin,
                session_workspace,
            },
        );
        *lock(&self.state.active) = Some(id.clone());
        self.state.context_generation.fetch_add(1, Ordering::AcqRel);
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.push_update(AgentUpdate::Sessions);
        Ok(id)
    }
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
            msg.role == Role::User && matches!(msg.source, MessageSource::User)
        })
        .map(|msg| msg.text().trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect();
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

fn engine_config(config: &LlmConfig, cwd: &Path) -> EngineConfig {
    EngineConfig {
        route: model_route(config),
        api_key: config.api_key.clone(),
        system: String::new(),
        registry: tool_registry(cwd),
        cwd: Some(cwd.display().to_string()),
        todos: None,
        delivery_committer: None,
        work_contract: None,
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
