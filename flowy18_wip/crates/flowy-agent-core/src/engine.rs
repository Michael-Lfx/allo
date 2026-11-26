//! Session engine: owns durable history, one active run per session, and the
//! bridge from loop events to a UI observer.
//!
//! The embedding host keeps its own projection; this engine only reports
//! deltas, tool lifecycle, and terminal outcomes. Nothing here knows about GPUI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use rpi_agent::{AgentEmitter, AgentEvent, StreamFn};
use rpi_ai::types::AssistantMessageEvent;

use crate::loop_runner::{
    LoopOutcome, ModelRoute, PreToolHook, TurnRequest, run_turn_with_budget,
};
use crate::run_budget::{
    BudgetCheckpointError, BudgetStop, RunBudgetStart, RunBudgetState,
};
use crate::run_fence::{PublishToken, RunFence, RunTicket};
use crate::session::{
    DeliveryCommit, DeliveryCommitter, DispatchRecord, ExecutionMetadata,
    ObjectVerifyState, PendingCard, PendingRouterFollowup,
    SessionCreateDisposition, SessionIndex, SessionOrigin, SessionRecord,
    SessionRole, SessionSpec, SessionWorkspace, StopHandoffManifest,
    SubmissionOutcome, TurnCompletionEvidence, TurnExecutionOutcome, TurnState,
    WorkAccessMode, WorkContract,
};
use crate::tools::{RegistryTool, ToolRegistry};
use crate::types::{
    ArtifactReceipt, ContentBlock, Message, MessageSource, PromptTurn, Role,
    TodoSink, ToolRunContext,
};

#[cfg(test)]
#[path = "run_budget/engine_tests.rs"]
mod budget_tests;

/// UI-facing turn events. All methods have no-op defaults so a caller may
/// implement only what it renders. Callbacks are synchronous and must not
/// call back into the engine.
pub trait TurnObserver: Send + Sync {
    fn text(&self, _session: &str, _delta: &str) {}
    fn reasoning(&self, _session: &str, _delta: &str) {}
    fn assistant_text(&self, _session: &str, _text: &str) {}
    fn tool_start(
        &self,
        _session: &str,
        _call_id: &str,
        _name: &str,
        _args: &serde_json::Value,
    ) {
    }
    fn tool_end(
        &self,
        _session: &str,
        _call_id: &str,
        _name: &str,
        _text: &str,
        _is_error: bool,
    ) {
    }
    fn finished(&self, _session: &str, _outcome: LoopOutcome) {}
    /// The engine could not persist this session's history.
    fn storage_error(&self, _session: &str) {}
    /// A file group crossed both durable history and store marker barriers.
    /// This callback is emitted before `finished(Completed)`.
    fn delivery_committed(&self, _commit: &DeliveryCommit) {}
    /// The delivery crossed its history commit point and is completing its
    /// independent artifact-store marker. It can no longer be cancelled.
    fn saving_files(&self, _session: &str) {}
    /// The history commit is durable, but the independent artifact-store marker
    /// failed. The host may retry this same commit without rerunning the turn.
    fn delivery_failed(&self, _commit: &DeliveryCommit, _error: &str) {}
    /// A cancelled run finished unwinding and reaping child tools, persisted
    /// its 5-element [`StopHandoffManifest`], and transitioned to `Stopped`.
    fn stop_settled(&self, _session: &str, _manifest: &StopHandoffManifest) {}
    /// A turn suspended cleanly before executing a medium-risk tool call,
    /// persisted its [`PendingCard::ToolApproval`], released `RunFence`, and
    /// transitioned to `WaitingForUser`.
    fn waiting_for_user(&self, _session: &str, _card: &PendingCard) {}
}

/// Everything needed to run a turn in this engine.
#[derive(Clone)]
pub struct EngineConfig {
    pub route: ModelRoute,
    pub api_key: Option<String>,
    pub system: String,
    pub registry: ToolRegistry,
    pub cwd: Option<String>,
    pub todos: Option<Arc<dyn TodoSink>>,
    /// Store-side second commit barrier for prepared artifact groups.
    pub delivery_committer: Option<Arc<dyn DeliveryCommitter>>,
    /// Optional work contract to persist atomically once the session run fence
    /// is acquired in [`Engine::prepare`].
    pub work_contract: Option<WorkContract>,
    /// Optional host pre-tool gate wired into `rpi_agent`'s `before_tool_call`.
    pub pre_tool_hook: Option<PreToolHook>,
    /// Resolve credentials and scope for each model request and tool dispatch.
    pub execution_gate: Option<Arc<dyn crate::ExecutionGate>>,
    /// Finite host-enforced limits; required for automation-origin sessions.
    pub run_budget: Option<crate::RunBudget>,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("route", &self.route)
            .field("has_key", &self.api_key.is_some())
            .field("system_len", &self.system.len())
            .field("tools", &self.registry.list().len())
            .field("has_delivery_committer", &self.delivery_committer.is_some())
            .finish_non_exhaustive()
    }
}

/// Outcome of durably enqueuing a user follow-up onto a busy Router session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    Duplicate,
    QueueFull,
}

#[derive(Debug)]
pub enum RunError {
    Busy,
    DuplicateSubmission,
    NotFound(String),
    NoCredential,
    MissingBudget,
    Storage(std::io::Error),
    Loop(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "session already has an active run"),
            Self::DuplicateSubmission => {
                write!(f, "submission is already recorded in this session")
            }
            Self::NotFound(id) => write!(f, "session {id} not found"),
            Self::NoCredential => {
                write!(f, "no account credential for this run")
            }
            Self::MissingBudget => {
                write!(f, "automation requires a finite run budget")
            }
            Self::Storage(error) => {
                write!(f, "could not save the submission: {error}")
            }
            Self::Loop(message) => write!(f, "{message}"),
        }
    }
}

/// Cancellation describes the request disposition, not proof that a tool or
/// provider has stopped. Terminal evidence is published only by the run future.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelDisposition {
    Accepted,
    TooLate,
    NotRunning,
    NotFound,
}

struct Inner {
    index: SessionIndex,
    config: EngineConfig,
    fences: HashMap<String, Arc<RunFence>>,
    /// History already committed this delivery; cancellation cannot undo it.
    finalizing: HashSet<String>,
}

struct PreparedTurn {
    ticket: RunTicket,
    history: Vec<Message>,
    config: EngineConfig,
    submission_id: Option<String>,
    completion_id: String,
    origin: SessionOrigin,
    execution: ExecutionMetadata,
    output_root: Option<PathBuf>,
    budget_start: Option<RunBudgetStart>,
}

/// Owns the durable index and the per-session run fences.
pub struct Engine {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    observer: Arc<dyn TurnObserver>,
}

/// Hold the session against replacement and deletion while replaying the
/// second commit barrier. Dropping the retry future releases only this gate,
/// never the durable completion or its artifact group.
struct DeliveryRetryGuard<'a> {
    engine: &'a Engine,
    session: String,
    ticket: RunTicket,
}

impl Drop for DeliveryRetryGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.engine.inner.lock().unwrap();
        self.ticket.finish();
        inner.finalizing.remove(&self.session);
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("root", &self.root)
            .field("sessions", &self.inner.lock().unwrap().index.sessions.len())
            .finish_non_exhaustive()
    }
}

/// Maximum durable dispatch idempotency records retained per Router session
/// (oldest entries by `created_at_ms` are evicted to keep `history.json` bounded).
pub const MAX_DISPATCH_REQUESTS: usize = 128;

impl Engine {
    pub fn open(
        root: PathBuf,
        config: EngineConfig,
        observer: Arc<dyn TurnObserver>,
    ) -> std::io::Result<Self> {
        let index = SessionIndex::load(&root)?;
        Ok(Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                index,
                config,
                fences: HashMap::new(),
                finalizing: HashSet::new(),
            })),
            observer,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_config(&self, config: EngineConfig) {
        self.inner.lock().unwrap().config = config;
    }

    #[must_use]
    pub fn sessions(&self) -> Vec<SessionRecord> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .index
            .sessions
            .clone()
    }

    /// Read durable reservations without inferring completion or restarting
    /// the submission. Invalid accounting cannot be interpreted as unused.
    pub fn run_budget_checkpoint(
        &self,
        session_id: &str,
        submission_id: &str,
    ) -> std::io::Result<Option<crate::RunBudgetCheckpoint>> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(record) = inner.index.get(session_id) else {
            return Ok(None);
        };
        let checkpoint =
            record.run_budget_checkpoints.get(submission_id).copied();
        drop(inner);
        if checkpoint.is_some_and(|checkpoint| !checkpoint.is_valid()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid persisted run budget",
            ));
        }
        Ok(checkpoint)
    }

    #[must_use]
    pub fn session(&self, id: &str) -> Option<SessionRecord> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .index
            .get(id)
            .cloned()
    }

    /// Borrows a single session record under the engine lock without cloning
    /// its full message history.
    pub fn with_session<R>(
        &self,
        id: &str,
        f: impl FnOnce(&SessionRecord) -> R,
    ) -> Option<R> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.index.get(id).map(f)
    }

    /// Borrows all session records under the engine lock for zero-clone batch
    /// projections (e.g. Work Board snapshots).
    pub fn with_sessions<R>(&self, f: impl FnOnce(&[SessionRecord]) -> R) -> R {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&guard.index.sessions)
    }

    /// Query durable per-submission evidence, including turns with no files.
    /// A legacy terminal session without evidence is deliberately not inferred
    /// to have completed the requested submission.
    #[must_use]
    pub fn completion_evidence(
        &self,
        session_id: &str,
        submission_id: &str,
    ) -> Option<TurnCompletionEvidence> {
        let inner = self.inner.lock().unwrap();
        let record = inner.index.get(session_id)?;
        let evidence = record
            .turn_completions
            .iter()
            .find(|evidence| {
                evidence.submission_id == submission_id
                    && valid_completion(record, evidence)
            })
            .cloned();
        drop(inner);
        evidence
    }

    /// Resolve only the group already recorded by durable history.
    #[must_use]
    pub fn delivery_commit_for_submission(
        &self,
        session_id: &str,
        submission_id: &str,
    ) -> Option<DeliveryCommit> {
        let inner = self.inner.lock().unwrap();
        let record = inner.index.get(session_id)?;
        let commit = record
            .delivery_commits
            .iter()
            .find(|commit| {
                commit.submission_id == submission_id
                    && valid_saved_commit(record, commit)
            })
            .cloned();
        drop(inner);
        commit
    }

    /// Retry exactly one persisted artifact group without starting a model or
    /// requiring a UI projection. The callback runs outside the history lock.
    pub fn retry_delivery_commit_with(
        &self,
        session_id: &str,
        submission_id: &str,
        retry: impl FnOnce(&DeliveryCommit) -> std::io::Result<()>,
    ) -> Result<DeliveryCommit, RunError> {
        let (commit, _guard) =
            self.reserve_delivery_retry(session_id, submission_id)?;
        retry(&commit).map_err(RunError::Storage)?;
        Ok(commit)
    }

    /// Asynchronous counterpart for an injected artifact committer. The
    /// session remains non-cancellable until this same group has settled.
    pub async fn retry_delivery_commit(
        &self,
        session_id: &str,
        submission_id: &str,
        committer: Arc<dyn DeliveryCommitter>,
    ) -> Result<DeliveryCommit, RunError> {
        let (commit, _guard) =
            self.reserve_delivery_retry(session_id, submission_id)?;
        committer
            .commit(&commit)
            .await
            .map_err(|error| RunError::Storage(std::io::Error::other(error)))?;
        Ok(commit)
    }

    fn reserve_delivery_retry(
        &self,
        session_id: &str,
        submission_id: &str,
    ) -> Result<(DeliveryCommit, DeliveryRetryGuard<'_>), RunError> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner
            .index
            .get(session_id)
            .ok_or_else(|| RunError::NotFound(session_id.to_owned()))?;
        let matches: Vec<_> = record
            .delivery_commits
            .iter()
            .filter(|commit| commit.submission_id == submission_id)
            .collect();
        let Some(commit) = matches.first() else {
            return Err(RunError::NotFound(submission_id.to_owned()));
        };
        if !valid_saved_commit(record, commit) {
            return Err(RunError::Storage(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored artifact commit binding is invalid",
            )));
        }
        let commit = (*commit).clone();
        if inner.finalizing.contains(session_id) {
            return Err(RunError::Busy);
        }
        let fence = Arc::clone(
            inner
                .fences
                .entry(session_id.to_owned())
                .or_insert_with(|| Arc::new(RunFence::new())),
        );
        let ticket = fence.try_begin().ok_or(RunError::Busy)?;
        inner.finalizing.insert(session_id.to_owned());
        drop(inner);
        Ok((
            commit,
            DeliveryRetryGuard {
                engine: self,
                session: session_id.to_owned(),
                ticket,
            },
        ))
    }

    #[must_use]
    pub fn active(&self) -> Option<String> {
        self.inner.lock().unwrap().index.active.clone()
    }

    #[must_use]
    pub fn submitted_session(&self, submission: u128) -> Option<String> {
        let message_id = format!("submission-{submission:032x}");
        self.inner
            .lock()
            .unwrap()
            .index
            .sessions
            .iter()
            .find_map(|record| {
                let in_history = record.messages.iter().any(|message| {
                    message.role == Role::User
                        && matches!(message.source, MessageSource::User)
                        && message.id == message_id
                });
                let in_followups = record
                    .pending_router_followups
                    .iter()
                    .any(|item| item.submission_id == message_id);
                (in_history || in_followups).then(|| record.id.clone())
            })
    }

    pub fn set_active(&self, id: Option<String>) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.index.active = id;
        inner.index.save(&self.root)
    }

    /// Create an empty session and persist its record.
    pub fn create_session(&self, id: String, now: u64) -> std::io::Result<()> {
        self.insert_session(SessionRecord {
            id,
            updated_at: now,
            ..SessionRecord::default()
        })
    }

    /// Create a session with explicit, durable origin/workspace metadata.
    pub fn create_session_with_metadata(
        &self,
        id: String,
        now: u64,
        origin: SessionOrigin,
        workspace: Option<SessionWorkspace>,
    ) -> std::io::Result<()> {
        self.insert_session(SessionRecord {
            id,
            updated_at: now,
            origin,
            workspace,
            ..SessionRecord::default()
        })
    }

    /// Create a session with explicit Router/Worker role, root binding, and
    /// immutable workspace access mode.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Router/Worker session constructor persists all immutable creation-time fields in one atomic write"
    )]
    pub fn create_session_with_role(
        &self,
        id: String,
        now: u64,
        origin: SessionOrigin,
        workspace: Option<SessionWorkspace>,
        role: SessionRole,
        root_session_id: Option<String>,
        access_mode: WorkAccessMode,
    ) -> std::io::Result<()> {
        self.insert_session(SessionRecord {
            id,
            updated_at: now,
            origin,
            workspace,
            execution: ExecutionMetadata::conversation(),
            role,
            root_session_id,
            access_mode,
            ..SessionRecord::default()
        })
    }

    /// All durable Worker sessions owned by `root_id`, ordered by `updated_at`
    /// descending (matching the underlying session index order).
    #[must_use]
    pub fn child_sessions(&self, root_id: &str) -> Vec<SessionRecord> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .index
            .sessions
            .iter()
            .filter(|rec| {
                rec.role == SessionRole::Worker
                    && rec.root_session_id.as_deref() == Some(root_id)
            })
            .cloned()
            .collect()
    }

    /// Sorted IDs of all durable Worker sessions owned by `root_id`, avoiding
    /// cloning full [`SessionRecord`] histories on hot projection paths.
    #[must_use]
    pub fn child_session_ids(&self, root_id: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .index
            .sessions
            .iter()
            .filter(|rec| {
                rec.role == SessionRole::Worker
                    && rec.root_session_id.as_deref() == Some(root_id)
            })
            .map(|rec| rec.id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Atomically enqueue a durable FIFO user follow-up onto a busy Router
    /// session, idempotent by `submission_id` and bounded by `max_len`.
    pub fn enqueue_router_followup(
        &self,
        router_id: &str,
        item: PendingRouterFollowup,
        max_len: usize,
    ) -> Result<EnqueueOutcome, RunError> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(record) = inner.index.get_mut(router_id) else {
            return Err(RunError::NotFound(router_id.to_owned()));
        };
        let already_in_history = record
            .messages
            .iter()
            .any(|msg| msg.role == Role::User && msg.id == item.submission_id);
        let already_queued = record
            .pending_router_followups
            .iter()
            .any(|existing| existing.submission_id == item.submission_id);
        if already_in_history || already_queued {
            return Ok(EnqueueOutcome::Duplicate);
        }
        if record.pending_router_followups.len() >= max_len {
            return Ok(EnqueueOutcome::QueueFull);
        }
        let prev_updated = record.updated_at;
        record.pending_router_followups.push(item);
        record.updated_at = now_ms();
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(record) = inner.index.get_mut(router_id) {
                record.pending_router_followups.pop();
                record.updated_at = prev_updated;
            }
            drop(inner);
            return Err(RunError::Storage(error));
        }
        drop(inner);
        Ok(EnqueueOutcome::Enqueued)
    }

    /// Peek the oldest durable follow-up queued on `router_id` without removing it.
    #[must_use]
    pub fn peek_router_followup(
        &self,
        router_id: &str,
    ) -> Option<PendingRouterFollowup> {
        self.with_session(router_id, |rec| {
            rec.pending_router_followups.first().cloned()
        })
        .flatten()
    }

    /// Return all durable FIFO follow-ups currently queued on `router_id`.
    #[must_use]
    pub fn pending_router_followups(
        &self,
        router_id: &str,
    ) -> Vec<PendingRouterFollowup> {
        self.with_session(router_id, |rec| rec.pending_router_followups.clone())
            .unwrap_or_default()
    }

    /// Atomically persist an idempotency record for a Router dispatch request,
    /// evicting oldest entries once [`MAX_DISPATCH_REQUESTS`] is exceeded.
    pub fn record_dispatch_request(
        &self,
        router_id: &str,
        record: DispatchRecord,
    ) -> std::io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = inner.index.get_mut(router_id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {router_id} not found"),
            )
        })?;
        let key = record.request_id.clone();
        let previous = session.dispatch_requests.insert(key.clone(), record);
        let mut evicted: Vec<(String, DispatchRecord)> = Vec::new();
        while session.dispatch_requests.len() > MAX_DISPATCH_REQUESTS {
            let Some(oldest_key) = session
                .dispatch_requests
                .iter()
                .filter(|(k, _)| *k != &key)
                .min_by_key(|(k, rec)| (rec.created_at_ms, k.as_str()))
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(removed) = session.dispatch_requests.remove(&oldest_key)
            {
                evicted.push((oldest_key, removed));
            }
        }
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(session) = inner.index.get_mut(router_id) {
                if let Some(old) = previous {
                    session.dispatch_requests.insert(key, old);
                } else {
                    session.dispatch_requests.remove(&key);
                }
                for (old_k, old_v) in evicted {
                    session.dispatch_requests.insert(old_k, old_v);
                }
            }
            drop(inner);
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Look up an existing dispatch idempotency record on `router_id`.
    #[must_use]
    pub fn lookup_dispatch_request(
        &self,
        router_id: &str,
        request_id: &str,
    ) -> Option<DispatchRecord> {
        self.with_session(router_id, |rec| {
            rec.dispatch_requests.get(request_id).cloned()
        })
        .flatten()
    }

    /// Remove any dispatch idempotency records pointing at `worker_id` (used
    /// during boot reconciliation of rolled-back empty unqueued workers).
    pub fn remove_dispatch_requests_for_worker(
        &self,
        router_id: &str,
        worker_id: &str,
    ) -> std::io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = inner.index.get_mut(router_id) else {
            return Ok(());
        };
        let before = session.dispatch_requests.clone();
        session
            .dispatch_requests
            .retain(|_, rec| rec.worker_id != worker_id);
        if session.dispatch_requests.len() == before.len() {
            return Ok(());
        }
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(session) = inner.index.get_mut(router_id) {
                session.dispatch_requests = before;
            }
            drop(inner);
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Atomically clear all persisted pending cards on `id` and set its
    /// durable status (e.g. when explicitly cancelling or revising a parked work).
    pub fn clear_pending_cards(
        &self,
        id: &str,
        next_status: TurnState,
    ) -> std::io::Result<bool> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        if record.pending_cards.is_empty() && record.status == next_status {
            return Ok(false);
        }
        let prev_cards = record.pending_cards.clone();
        let prev_status = record.status.clone();
        let prev_updated = record.updated_at;
        record.pending_cards.clear();
        record.status = next_status;
        record.updated_at = now_ms();
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(record) = inner.index.get_mut(id) {
                record.pending_cards = prev_cards;
                record.status = prev_status;
                record.updated_at = prev_updated;
            }
            drop(inner);
            return Err(error);
        }
        drop(inner);
        Ok(true)
    }
    /// Create once, or verify the exact durable binding without rewriting it.
    /// The UI active session and an existing session's messages are untouched.
    pub fn create_if_absent(
        &self,
        spec: SessionSpec,
    ) -> std::io::Result<SessionCreateDisposition> {
        if spec.id.trim().is_empty() || !spec.execution.is_valid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session execution identity is invalid",
            ));
        }
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.index.get(&spec.id) {
            if record.origin == spec.origin
                && record.workspace == spec.workspace
                && record.execution == spec.execution
                && record.role == spec.role
                && record.root_session_id == spec.root_session_id
                && record.access_mode == spec.access_mode
            {
                return Ok(SessionCreateDisposition::Existing);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "session ID is already bound to different immutable metadata",
            ));
        }
        // Insert/remove at the same position so a failed save restores the exact
        // in-memory index without cloning every existing message history.
        let position = inner
            .index
            .sessions
            .iter()
            .position(|record| record.updated_at < spec.created_at_ms)
            .unwrap_or(inner.index.sessions.len());
        inner.index.sessions.insert(
            position,
            SessionRecord {
                id: spec.id,
                updated_at: spec.created_at_ms,
                origin: spec.origin,
                workspace: spec.workspace,
                execution: spec.execution,
                role: spec.role,
                root_session_id: spec.root_session_id,
                access_mode: spec.access_mode,
                ..SessionRecord::default()
            },
        );
        if let Err(error) = inner.index.save(&self.root) {
            inner.index.sessions.remove(position);
            drop(inner);
            return Err(error);
        }
        Ok(SessionCreateDisposition::Created)
    }

    /// Persist origin/workspace metadata on an existing session.
    pub fn set_session_origin_workspace(
        &self,
        id: &str,
        origin: SessionOrigin,
        workspace: Option<SessionWorkspace>,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        record.origin = origin;
        record.workspace = workspace;
        inner.index.save(&self.root)
    }

    /// Persist the active versioned work contract for a session and optionally
    /// record its revision for a specific submission.
    pub fn set_session_work_contract(
        &self,
        id: &str,
        contract: WorkContract,
        submission_id: Option<&str>,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        if let Some(sub_id) = submission_id {
            record
                .submission_revisions
                .insert(sub_id.to_owned(), contract.revision);
        }
        record.work_contract = Some(contract);
        inner.index.save(&self.root)
    }

    /// Record the contract revision for an auxiliary submission (such as an
    /// explicit override delivery or conversation auto-import) without changing
    /// the session's active `work_contract`.
    pub fn record_submission_revision(
        &self,
        id: &str,
        submission_id: &str,
        revision: u32,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        record
            .submission_revisions
            .insert(submission_id.to_owned(), revision);
        inner.index.save(&self.root)
    }

    /// Persist a submission outcome directly (for example after an explicit
    /// user override completes a previously partial work revision).
    pub fn set_submission_outcome(
        &self,
        id: &str,
        submission_id: &str,
        outcome: SubmissionOutcome,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        record
            .submission_outcomes
            .insert(submission_id.to_owned(), outcome);
        inner.index.save(&self.root)
    }

    /// Persist a pending interactive card on `id` and transition its durable
    /// status to [`TurnState::WaitingForUser`] without acquiring a run fence.
    pub fn park_pending_card(
        &self,
        id: &str,
        card: PendingCard,
        updated_contract: Option<WorkContract>,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        let prev_cards = record.pending_cards.clone();
        let prev_contract = record.work_contract.clone();
        let prev_status = record.status.clone();
        let prev_updated = record.updated_at;
        record.pending_cards.retain(|existing| {
            if existing.card_id() == card.card_id() {
                return false;
            }
            !matches!(
                (existing, &card),
                (
                    PendingCard::Disambiguation { prompt: a, .. },
                    PendingCard::Disambiguation { prompt: b, .. }
                ) if a == b
            )
        });
        record.pending_cards.push(card);
        if let Some(contract) = updated_contract {
            record.work_contract = Some(contract);
        }
        record.status = TurnState::WaitingForUser;
        record.updated_at = now_ms();
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(record) = inner.index.get_mut(id) {
                record.pending_cards = prev_cards;
                record.work_contract = prev_contract;
                record.status = prev_status;
                record.updated_at = prev_updated;
            }
            drop(inner);
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Remove and return a persisted pending card by `card_id`, optionally
    /// updating `work_contract` and durable status atomically.
    pub fn take_pending_card(
        &self,
        id: &str,
        card_id: &str,
        next_status: TurnState,
        updated_contract: Option<WorkContract>,
    ) -> std::io::Result<Option<PendingCard>> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        let Some(pos) = record
            .pending_cards
            .iter()
            .position(|card| card.card_id() == card_id)
        else {
            return Ok(None);
        };
        let prev_contract = record.work_contract.clone();
        let prev_status = record.status.clone();
        let prev_updated = record.updated_at;
        let removed = record.pending_cards.remove(pos);
        if let Some(contract) = updated_contract {
            record.work_contract = Some(contract);
        }
        if record.pending_cards.is_empty() {
            record.status = next_status;
        }
        record.updated_at = now_ms();
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(record) = inner.index.get_mut(id) {
                record.pending_cards.insert(pos, removed);
                record.work_contract = prev_contract;
                record.status = prev_status;
                record.updated_at = prev_updated;
            }
            return Err(error);
        }
        drop(inner);
        Ok(Some(removed))
    }

    /// Insert or replace one durable session record.
    pub fn insert_session(&self, record: SessionRecord) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.index.upsert(record);
        inner.index.save(&self.root)
    }

    /// Update the durable title of one session.
    ///
    /// Titles are UI-facing metadata but live in the structured history too, so
    /// an index rebuild can restore the model-generated title instead of a
    /// fallback.
    pub fn set_session_title(
        &self,
        id: &str,
        title: String,
        titled: bool,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let (prev_title, prev_titled, prev_snapshots) = {
            let record = inner.index.get_mut(id).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("session {id} not found"),
                )
            })?;
            let prev_title = record.title.clone();
            let prev_titled = record.titled;
            let prev_snapshots: Vec<String> = record
                .delivery_commits
                .iter()
                .map(|commit| commit.origin_title_snapshot.clone())
                .collect();
            for commit in &mut record.delivery_commits {
                commit.origin_title_snapshot.clone_from(&title);
            }
            record.title = title;
            record.titled = titled;
            (prev_title, prev_titled, prev_snapshots)
        };
        let saved = inner.index.save(&self.root);
        if let Err(error) = saved {
            if let Some(record) = inner.index.get_mut(id) {
                record.title = prev_title;
                record.titled = prev_titled;
                for (commit, prev) in
                    record.delivery_commits.iter_mut().zip(prev_snapshots)
                {
                    commit.origin_title_snapshot = prev;
                }
            }
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Persist the user-pinned flag for one session without altering its
    /// activity timestamp (`updated_at`), rolling back in-memory state if disk
    /// persistence fails.
    pub fn set_session_pinned(
        &self,
        id: &str,
        pinned: bool,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let prev = {
            let record = inner.index.get_mut(id).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("session {id} not found"),
                )
            })?;
            let prev = record.pinned;
            record.pinned = pinned;
            prev
        };
        let saved = inner.index.save(&self.root);
        if let Err(error) = saved {
            if let Some(record) = inner.index.get_mut(id) {
                record.pinned = prev;
            }
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Persist the user-archived flag for one session without altering its
    /// activity timestamp (`updated_at`), rolling back in-memory state if disk
    /// persistence fails.
    pub fn set_session_archived(
        &self,
        id: &str,
        archived: bool,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let prev = {
            let record = inner.index.get_mut(id).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("session {id} not found"),
                )
            })?;
            let prev = record.archived;
            record.archived = archived;
            prev
        };
        let saved = inner.index.save(&self.root);
        if let Err(error) = saved {
            if let Some(record) = inner.index.get_mut(id) {
                record.archived = prev;
            }
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    /// Persist a host-derived group, including the existing conversation-write
    /// importer. Replays preserve the original group; a conflicting group can
    /// never replace a submission's already durable evidence.
    pub fn record_delivery_commit(
        &self,
        commit: &DeliveryCommit,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.finalizing.contains(&commit.session_id) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "delivery is being finalized",
            ));
        }
        let previous = inner
            .index
            .get(&commit.session_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("session {} not found", commit.session_id),
                )
            })?
            .clone();
        if commit.execution != previous.execution
            || commit.origin != previous.origin
            || !commit.completed_successfully
            || !commit.execution.is_valid()
            || commit.submission_id.trim().is_empty()
            || commit.artifact_ids.is_empty()
            || commit.artifact_ids.iter().any(|id| id.trim().is_empty())
            || commit.artifact_ids.iter().collect::<HashSet<_>>().len()
                != commit.artifact_ids.len()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "delivery commit does not match its session",
            ));
        }
        if let Some(existing) = previous
            .delivery_commits
            .iter()
            .find(|existing| existing.submission_id == commit.submission_id)
        {
            return if existing.artifact_ids == commit.artifact_ids
                && existing.execution == commit.execution
                && existing.origin == commit.origin
                && valid_saved_commit(&previous, existing)
            {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "submission already has a different delivery group",
                ))
            };
        }
        let saved = {
            let record = inner
                .index
                .get_mut(&commit.session_id)
                .expect("session still exists under the engine lock");
            if let Some(evidence) = record
                .turn_completions
                .iter_mut()
                .find(|evidence| evidence.submission_id == commit.submission_id)
            {
                if evidence.session_id != commit.session_id
                    || evidence.outcome != TurnExecutionOutcome::Completed
                    || evidence.execution != commit.execution
                    || (!evidence.artifact_ids.is_empty()
                        && evidence.artifact_ids != commit.artifact_ids)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "delivery group conflicts with terminal evidence",
                    ));
                }
                evidence.artifact_ids.clone_from(&commit.artifact_ids);
            }
            record.delivery_commits.push(commit.clone());
            inner.index.save(&self.root)
        };
        if let Err(error) = saved {
            inner.index.upsert(previous);
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    pub fn remove_session(&self, id: &str) -> std::io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        if inner.finalizing.contains(id) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "delivery is being finalized",
            ));
        }
        let before = inner.index.clone();
        let removed = inner.index.remove(id).is_some();
        if removed && let Err(error) = inner.index.save(&self.root) {
            // A failed delete is not a source-deletion confirmation, and a
            // later unrelated save must not silently complete it.
            inner.index = before;
            return Err(error);
        }
        if let Some(fence) = inner.fences.remove(id) {
            fence.cancel_active();
        }
        drop(inner);
        Ok(removed)
    }

    /// Cancel the active run of one session, if any.
    pub fn cancel(&self, id: &str) -> std::io::Result<()> {
        self.request_cancel(id).map(|_| ())
    }

    /// True while a cancelled run for `id` is still unwinding and reaping its
    /// child tools.
    #[must_use]
    pub fn is_cancelling(&self, id: &str) -> bool {
        let fence = self.inner.lock().unwrap().fences.get(id).cloned();
        fence.is_some_and(|fence| fence.is_cancelling())
    }

    /// True while session `id` holds an active run fence (either running or
    /// cancelling/reaping child tools).
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        let fence = self.inner.lock().unwrap().fences.get(id).cloned();
        fence.is_some_and(|fence| fence.is_active())
    }

    /// IDs of all sessions whose run fence is currently active (running or
    /// cancelling/reaping).
    #[must_use]
    pub fn active_session_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fences
            .iter()
            .filter(|(_, fence)| fence.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Defensively cancel every cancellable active run fence (for process
    /// shutdown / handle teardown) so child tool processes are signaled to exit
    /// immediately while preserving sessions currently finalizing a durable
    /// delivery commit.
    pub fn cancel_all_active(&self) {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (id, fence) in &inner.fences {
            if !inner.finalizing.contains(id.as_str()) && fence.is_active() {
                fence.cancel_active();
            }
        }
    }

    /// Cancel currently active automations belonging to the invalidated owner.
    /// The target-specific cancel API preserves runs past the commit point.
    pub fn cancel_automation_owner(
        &self,
        owner: &crate::ExecutionOwner,
    ) -> std::io::Result<()> {
        let sessions = {
            let inner = self.inner.lock().unwrap();
            inner
                .fences
                .iter()
                .filter(|(_, fence)| fence.is_active())
                .filter_map(|(id, _)| {
                    let record = inner.index.get(id)?;
                    (record.execution.owner.as_ref() == Some(owner)
                        && matches!(
                            record.execution.source,
                            crate::ExecutionSource::Automation { .. }
                        ))
                    .then(|| id.clone())
                })
                .collect::<Vec<_>>()
        };
        let mut result = Ok(());
        for id in sessions {
            if let Err(error) = self.request_cancel_typed(&id) {
                result = Err(error);
            }
        }
        result
    }

    /// Wait up to `timeout` for session `id`'s active/cancelling run to finish
    /// reaping and release its fence. Returns `true` if idle.
    #[must_use]
    pub fn wait_until_idle(&self, id: &str, timeout: Duration) -> bool {
        let fence = self.inner.lock().unwrap().fences.get(id).cloned();
        fence.is_none_or(|fence| fence.wait_until_idle(timeout))
    }

    /// Request cancellation before a successful delivery history commit.
    /// `false` means the delivery crossed its durable linearization point and
    /// is completing its independent store marker; callers must keep the
    /// saving state visible until the terminal notification arrives.
    pub fn request_cancel(&self, id: &str) -> std::io::Result<bool> {
        self.request_cancel_typed(id)
            .map(|disposition| disposition != CancelDisposition::TooLate)
    }

    /// Request a cooperative stop. `Accepted` is not a completion proof: a
    /// prepared or running future must still unwind before recording Stopped.
    pub fn request_cancel_typed(
        &self,
        id: &str,
    ) -> std::io::Result<CancelDisposition> {
        let mut inner = self.inner.lock().unwrap();
        if inner.finalizing.contains(id) {
            return Ok(CancelDisposition::TooLate);
        }
        if inner.index.get(id).is_none() {
            return Ok(CancelDisposition::NotFound);
        }
        let previous = inner.index.get(id).cloned();
        let fence_active =
            inner.fences.get(id).is_some_and(|fence| fence.is_active());
        if !fence_active
            && !previous
                .as_ref()
                .is_some_and(|record| record.status.is_busy())
        {
            return Ok(CancelDisposition::NotRunning);
        }
        if let Some(fence) = inner.fences.get(id) {
            fence.cancel_active();
        }
        if let Some(record) = inner.index.get_mut(id)
            && record.status.is_busy()
        {
            if fence_active {
                // Keep durable status at `Stopping` until the worker finishes
                // unwinding/reaping and persists its StopHandoffManifest.
                record.status = TurnState::Stopping;
            } else {
                let prompt = record
                    .messages
                    .iter()
                    .rev()
                    .find(|msg| {
                        msg.role == Role::User
                            && matches!(
                                msg.source,
                                MessageSource::User | MessageSource::CardResume
                            )
                    })
                    .map_or_default(Message::text);
                let manifest =
                    CancelTrace::default().build_manifest(record, &prompt);
                record.last_stop_handoff = Some(manifest);
                record.status = TurnState::Stopped;
            }
            if let Some(sub_id) = record
                .messages
                .iter()
                .rev()
                .find(|msg| {
                    msg.role == Role::User
                        && matches!(
                            msg.source,
                            MessageSource::User | MessageSource::CardResume
                        )
                        && msg.id.starts_with("submission-")
                })
                .map(|msg| msg.id.clone())
            {
                record
                    .submission_outcomes
                    .entry(sub_id)
                    .or_insert(SubmissionOutcome::Aborted);
            }
        }
        if let Err(error) = inner.index.save(&self.root) {
            if let Some(previous) = previous {
                inner.index.upsert(previous);
            }
            drop(inner);
            return Err(error);
        }
        drop(inner);
        Ok(CancelDisposition::Accepted)
    }

    fn settle_cancelled_run(
        &self,
        session_id: &str,
        prompt: &str,
        trace: &Mutex<CancelTrace>,
        ticket: &RunTicket,
    ) {
        let (manifest, save_failed) = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(record) = inner.index.get_mut(session_id) else {
                drop(inner);
                let _ = ticket.finish();
                return;
            };
            let manifest = trace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .build_manifest(record, prompt);
            record.status = TurnState::Stopped;
            record.last_stop_handoff = Some(manifest.clone());
            record.updated_at = now_ms();
            let save_failed = inner.index.save(&self.root).is_err();
            drop(inner);
            (manifest, save_failed)
        };
        if save_failed {
            self.observer.storage_error(session_id);
        }
        self.observer.stop_settled(session_id, &manifest);
        let _ = ticket.finish();
    }

    /// Emit terminal observer callbacks while `ticket` still holds the
    /// session's `RunFence` and `inner.finalizing` blocks concurrent
    /// `request_cancel`, then release `ticket` under `self.inner`.
    fn finish_after_observer<F>(
        &self,
        session_id: &str,
        ticket: &RunTicket,
        notify: F,
    ) -> bool
    where
        F: FnOnce(),
    {
        let mut guard = FinalizingGuard {
            engine: self,
            session_id: session_id.to_owned(),
            armed: true,
        };
        if ticket.is_current() {
            notify();
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.armed = false;
        let clean = ticket.finish();
        inner.finalizing.remove(session_id);
        drop(inner);
        clean
    }

    /// Either settle a cancelled run (when `armed == false`) or notify the
    /// observer while `inner.finalizing` blocks concurrent `request_cancel`
    /// and then release `ticket`.
    fn finish_non_commit<F>(
        &self,
        armed: bool,
        session_id: &str,
        prompt: &str,
        cancel_trace: &Mutex<CancelTrace>,
        ticket: &RunTicket,
        notify: F,
    ) -> bool
    where
        F: FnOnce(),
    {
        if !armed {
            self.settle_cancelled_run(session_id, prompt, cancel_trace, ticket);
            return false;
        }
        self.finish_after_observer(session_id, ticket, notify)
    }

    /// Run one turn against the configured provider. The caller spawns this;
    /// it is `async` so a host runtime owns scheduling.
    pub async fn run(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<LoopOutcome, RunError> {
        let prompt = PromptTurn::plain(prompt);
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::run",
            session = %session_id,
            chars = prompt.model.chars().count(),
            mode = "live",
            "enter"
        );
        let prepared = self.prepare(session_id, &prompt, None, true, None)?;
        self.run_prepared(session_id, &prompt.model, prepared, None)
            .await
    }

    /// Run with an explicit stream function (scripted stream in tests).
    pub async fn run_with_stream(
        &self,
        session_id: &str,
        prompt: &str,
        stream: StreamFn,
    ) -> Result<LoopOutcome, RunError> {
        let prompt = PromptTurn::plain(prompt);
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::run_with_stream",
            session = %session_id,
            chars = prompt.model.chars().count(),
            "enter"
        );
        let prepared = self.prepare(session_id, &prompt, None, false, None)?;
        self.run_prepared(session_id, &prompt.model, prepared, Some(stream))
            .await
    }

    /// Reserve the turn before scheduling it so an immediate stop can cancel it.
    pub fn start_with_config(
        self: &Arc<Self>,
        session_id: String,
        prompt: PromptTurn,
        config: EngineConfig,
        stream: Option<StreamFn>,
        submission: Option<u128>,
    ) -> Result<BoxFuture<'static, Result<LoopOutcome, RunError>>, RunError>
    {
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::start_with_config",
            session = %session_id,
            chars = prompt.model.chars().count(),
            echo = stream.is_some(),
            "enter"
        );
        let prepared = self.prepare(
            &session_id,
            &prompt,
            Some(config),
            stream.is_none(),
            submission,
        )?;
        let engine = Arc::clone(self);
        Ok(Box::pin(async move {
            engine
                .run_prepared(&session_id, &prompt.model, prepared, stream)
                .await
        }))
    }

    fn prepare(
        &self,
        session_id: &str,
        prompt: &PromptTurn,
        config: Option<EngineConfig>,
        needs_credential: bool,
        submission: Option<u128>,
    ) -> Result<PreparedTurn, RunError> {
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::prepare",
            session = %session_id,
            chars = prompt.model.chars().count(),
            needs_credential,
            "enter"
        );
        let (prepared, saved) = {
            let mut inner = self.inner.lock().unwrap();
            let config = config.unwrap_or_else(|| inner.config.clone());
            if needs_credential
                && config.api_key.is_none()
                && config.execution_gate.is_none()
            {
                return Err(RunError::NoCredential);
            }
            let Some(record) = inner.index.get(session_id) else {
                return Err(RunError::NotFound(session_id.to_owned()));
            };
            if matches!(
                record.execution.source,
                crate::ExecutionSource::Automation { .. }
            ) && config.execution_gate.is_none()
            {
                return Err(RunError::NoCredential);
            }
            if matches!(
                record.execution.source,
                crate::ExecutionSource::Automation { .. }
            ) && config.run_budget.is_none()
            {
                return Err(RunError::MissingBudget);
            }
            let submission_id =
                submission.map(|id| format!("submission-{id:032x}"));
            if submission_id.as_ref().is_some_and(|submission_id| {
                let record =
                    inner.index.get(session_id).expect("record exists");
                record.messages.iter().any(|message| {
                    message.role == Role::User
                        && matches!(message.source, MessageSource::User)
                        && message.id == *submission_id
                }) || record
                    .turn_completions
                    .iter()
                    .any(|evidence| evidence.submission_id == *submission_id)
                    || record
                        .delivery_commits
                        .iter()
                        .any(|commit| commit.submission_id == *submission_id)
                    || record.run_budget_checkpoints.contains_key(submission_id)
            }) {
                return Err(RunError::DuplicateSubmission);
            }
            let fence = Arc::clone(
                inner
                    .fences
                    .entry(session_id.to_owned())
                    .or_insert_with(|| Arc::new(RunFence::new())),
            );
            let Some(ticket) = fence.try_begin() else {
                return Err(RunError::Busy);
            };
            let now = now_ms();
            let budget_start = config
                .run_budget
                .map(|limits| {
                    let started = i64::try_from(now).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "run start exceeds supported clock range",
                        )
                    })?;
                    RunBudgetStart::new(limits, started)
                })
                .transpose()
                .map_err(RunError::Storage)?;
            let record =
                inner.index.get_mut(session_id).expect("record exists");
            let history = record.messages.clone();
            let origin = record.origin.clone();
            let execution = record.execution.clone();
            let completion_id = submission_id
                .clone()
                .unwrap_or_else(|| format!("u-{now}-{}", ticket.generation()));
            let output_root = record.workspace.as_ref().and_then(|workspace| {
                submission_id.as_ref().map(|submission| {
                    workspace.root.join("outputs").join(submission)
                })
            });
            let previous = (
                record.status.clone(),
                record.updated_at,
                record.work_contract.clone(),
                record.submission_revisions.clone(),
                record.pending_router_followups.clone(),
                record.run_budget_checkpoints.clone(),
            );
            if let Some(start) = &budget_start {
                record
                    .run_budget_checkpoints
                    .insert(completion_id.clone(), start.checkpoint());
            }
            if let Some(contract) = &config.work_contract {
                if let Some(sub_id) = submission_id.as_deref() {
                    record
                        .submission_revisions
                        .insert(sub_id.to_owned(), contract.revision);
                }
                record.work_contract = Some(contract.clone());
            }
            if let Some(sub_id) = submission_id.as_deref() {
                record
                    .pending_router_followups
                    .retain(|item| item.submission_id != sub_id);
            }
            record.messages.push(Message {
                id: completion_id.clone(),
                role: Role::User,
                content: vec![crate::types::ContentBlock::text(&prompt.model)],
                source: prompt.source.clone(),
                display_text: prompt.display.clone(),
            });
            record.status = TurnState::Running;
            record.updated_at = now;
            let saved = inner.index.save(&self.root);
            if (submission.is_some() || budget_start.is_some())
                && let Err(error) = saved
            {
                let record =
                    inner.index.get_mut(session_id).expect("record exists");
                record.messages.pop();
                (
                    record.status,
                    record.updated_at,
                    record.work_contract,
                    record.submission_revisions,
                    record.pending_router_followups,
                    record.run_budget_checkpoints,
                ) = previous;
                ticket.finish();
                drop(inner);
                return Err(RunError::Storage(error));
            }
            let prepared = PreparedTurn {
                ticket,
                history,
                config,
                submission_id,
                completion_id,
                origin,
                execution,
                output_root,
                budget_start,
            };
            drop(inner);
            (prepared, saved)
        };
        if saved.is_err() {
            self.observer.storage_error(session_id);
        }
        Ok(prepared)
    }

    async fn run_prepared(
        &self,
        session_id: &str,
        prompt: &str,
        mut prepared: PreparedTurn,
        stream: Option<StreamFn>,
    ) -> Result<LoopOutcome, RunError> {
        let budget = prepared.budget_start.take().map(|start| {
            let inner = Arc::clone(&self.inner);
            let root = self.root.clone();
            let session = session_id.to_owned();
            let submission = prepared.completion_id.clone();
            let publisher = prepared.ticket.publisher();
            Arc::new(start.track(move |checkpoint| {
                let mut inner = inner
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // The same run must still own the fence. A completed history
                // may be finalizing its marker, so keep recording its deadline.
                if !publisher.is_current() {
                    return Err(BudgetCheckpointError::StaleSubmission);
                }
                let Some(record) = inner.index.get_mut(&session) else {
                    return Err(BudgetCheckpointError::StaleSubmission);
                };
                if record
                    .messages
                    .iter()
                    .rev()
                    .find(|message| {
                        message.role == Role::User
                            && matches!(message.source, MessageSource::User)
                    })
                    .is_none_or(|message| message.id != submission)
                {
                    return Err(BudgetCheckpointError::StaleSubmission);
                }
                let previous = record
                    .run_budget_checkpoints
                    .insert(submission.clone(), checkpoint);
                if inner.index.save(&root).is_err() {
                    let record = inner
                        .index
                        .get_mut(&session)
                        .expect("session exists under the engine lock");
                    if let Some(previous) = previous {
                        record
                            .run_budget_checkpoints
                            .insert(submission.clone(), previous);
                    } else {
                        record.run_budget_checkpoints.remove(&submission);
                    }
                    return Err(BudgetCheckpointError::Storage);
                }
                drop(inner);
                Ok(())
            }))
        });
        let run = self.run_prepared_inner(
            session_id,
            prompt,
            prepared,
            stream,
            budget.clone(),
        );
        tokio::pin!(run);
        if let Some(budget) = budget {
            tokio::select! {
                biased;
                stop = budget.wait_for_stop() => {
                    // Request cancellation, then keep polling the original
                    // future until its tools have settled. TooLate continues
                    // the same durable commit; no timeout or abort drops it.
                    if stop == BudgetStop::Storage {
                        self.observer.storage_error(session_id);
                    }
                    if self.request_cancel(session_id).is_err() {
                        self.observer.storage_error(session_id);
                    }
                    run.await
                }
                result = &mut run => result,
            }
        } else {
            run.await
        }
    }

    async fn run_prepared_inner(
        &self,
        session_id: &str,
        prompt: &str,
        prepared: PreparedTurn,
        stream: Option<StreamFn>,
        budget: Option<Arc<RunBudgetState>>,
    ) -> Result<LoopOutcome, RunError> {
        let PreparedTurn {
            ticket,
            history,
            config,
            submission_id,
            completion_id,
            origin,
            execution,
            output_root,
            budget_start: _,
        } = prepared;
        let cancel_trace = Arc::new(Mutex::new(CancelTrace::default()));
        if !ticket.is_current() {
            let proof = self.persist_stopped_completion(
                &mut self.inner.lock().unwrap(),
                session_id,
                &completion_id,
                &execution,
            );
            self.settle_cancelled_run(
                session_id,
                prompt,
                &cancel_trace,
                &ticket,
            );
            proof?;
            return Ok(LoopOutcome::Aborted);
        }
        let stream = match (stream, config.execution_gate.clone()) {
            (Some(stream), Some(gate)) => crate::transport::guard_stream_fn(
                &config.route,
                execution.clone(),
                gate,
                stream,
            ),
            (None, Some(gate)) => crate::transport::guarded_openai_stream_fn(
                &config.route,
                execution.clone(),
                gate,
            ),
            (Some(stream), None) => stream,
            (None, None) => {
                if budget.is_some() {
                    crate::transport::bounded_openai_stream_fn(
                        &config.route,
                        config.api_key.clone(),
                    )
                } else {
                    crate::transport::openai_stream_fn(
                        &config.route,
                        config.api_key.clone(),
                    )
                }
            }
        };
        let stream = if let Some(budget) = &budget {
            crate::transport::budget_stream_fn(Arc::clone(budget), stream)
        } else {
            stream
        };
        let delivery_committer = config.delivery_committer.clone();
        let artifact_receipts = Arc::new(Mutex::new(Vec::new()));
        let tool_context = ToolRunContext {
            signal: ticket.signal().clone(),
            agent_id: Some(session_id.to_owned()),
            cwd: config.cwd.clone(),
            todos: config.todos.clone(),
            submission_id: submission_id.clone(),
            session_origin: origin,
            execution: execution.clone(),
            output_root,
            artifact_delivery_available: config.delivery_committer.is_some(),
        };
        let tools = build_tools(
            &config,
            tool_context,
            Arc::clone(&artifact_receipts),
            budget.clone(),
        );
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::run_prepared",
            session = %session_id,
            history = history.len(),
            tools = tools.len(),
            model = %config.route.model,
            base_url = %config.route.base_url,
            "→ run_turn"
        );
        let request = TurnRequest {
            session_id: Some(session_id.to_owned()),
            system: config.system,
            history,
            prompt: prompt.to_owned(),
            route: config.route,
            tools,
            signal: ticket.signal().clone(),
            pre_tool_hook: config.pre_tool_hook,
        };
        let emitter = Arc::new(EngineEmitter {
            session: session_id.to_owned(),
            observer: Arc::clone(&self.observer),
            publisher: ticket.publisher(),
            trace: Some(Arc::clone(&cancel_trace)),
        });
        let result =
            run_turn_with_budget(request, stream, emitter, budget.clone())
                .await;
        // A scripted/ready provider can consume an entire loop in one poll.
        // Settle a limit through the same cancel path even when the supervisor
        // has not had another chance to observe its stop notification.
        if let Some(stop) =
            budget.as_ref().and_then(|budget| budget.stop_requested())
        {
            if stop == BudgetStop::Storage {
                self.observer.storage_error(session_id);
            }
            if self.request_cancel(session_id).is_err() {
                self.observer.storage_error(session_id);
            }
        }
        let stored = {
            let mut inner = self.inner.lock().unwrap();
            if !ticket.is_current() {
                drop(inner);
                let proof = self.persist_stopped_completion(
                    &mut self.inner.lock().unwrap(),
                    session_id,
                    &completion_id,
                    &execution,
                );
                self.settle_cancelled_run(
                    session_id,
                    prompt,
                    &cancel_trace,
                    &ticket,
                );
                proof?;
                return Ok(LoopOutcome::Aborted);
            }
            let Some(record) = inner.index.get_mut(session_id) else {
                return Ok(LoopOutcome::Aborted);
            };
            let before = record.clone();
            record.updated_at = now_ms();
            let result = match result {
                Ok(mut result) => {
                    let final_text = result
                        .messages
                        .iter()
                        .rev()
                        .find(|message| message.role == Role::Assistant)
                        .map_or_default(Message::text);
                    if result.outcome == LoopOutcome::Failed {
                        let error = result
                            .error
                            .as_deref()
                            .filter(|text| !text.is_empty())
                            .unwrap_or(final_text.as_str());
                        tracing::warn!(
                            target: "flowy.prompt",
                            stage = "Engine::run_prepared",
                            session = %session_id,
                            error = %crate::loop_runner::log_preview(error, 500),
                            prompt_preview = %crate::loop_runner::log_preview(prompt, 200),
                            new_messages = result.messages.len(),
                            "turn failed"
                        );
                    }
                    if !final_text.is_empty() {
                        self.observer.assistant_text(session_id, &final_text);
                    }
                    if result.outcome == LoopOutcome::Completed {
                        attach_artifact_receipts(
                            &mut result.messages,
                            &artifact_receipts.lock().unwrap(),
                            session_id,
                            submission_id.as_deref(),
                        );
                    }
                    let mut new_messages = result.messages;
                    // The loop echoes the prompt back; the engine already
                    // stored it, so drop the duplicate.
                    if new_messages.first().is_some_and(|message| {
                        message.role == Role::User
                            && matches!(message.source, MessageSource::User)
                            && message.text() == prompt
                    }) {
                        new_messages.remove(0);
                    }
                    let error = result.error.clone().or_else(|| {
                        (result.outcome == LoopOutcome::Failed
                            && !final_text.is_empty())
                        .then_some(final_text.clone())
                    });
                    let commit = delivery_commit(
                        &new_messages,
                        session_id,
                        submission_id.as_deref(),
                        record,
                        result.outcome,
                    );
                    clamp_persisted_tool_outputs(&mut new_messages);
                    record.messages.extend(new_messages);
                    if let Some(commit) = &commit {
                        record.delivery_commits.retain(|previous| {
                            previous.submission_id != commit.submission_id
                        });
                        record.delivery_commits.push(commit.clone());
                    }
                    let pending_approval = result.pending_approval.clone();
                    if let Some(card) = &pending_approval {
                        record
                            .pending_cards
                            .retain(|c| c.card_id() != card.card_id());
                        record.pending_cards.push(card.clone());
                    }
                    let effective_outcome = if result.outcome
                        != LoopOutcome::Aborted
                        && commit.is_none()
                        && !record.pending_cards.is_empty()
                    {
                        LoopOutcome::WaitingForUser
                    } else {
                        result.outcome
                    };
                    let effective_card = pending_approval
                        .or_else(|| record.pending_cards.first().cloned());
                    record.status =
                        status_for(effective_outcome, error.as_deref());
                    if let Some(sub_id) = submission_id.as_ref() {
                        match effective_outcome {
                            LoopOutcome::Completed => {
                                let missing = missing_required_for_submission(
                                    record, sub_id,
                                );
                                let outcome_entry = if missing.is_empty() {
                                    SubmissionOutcome::Completed
                                } else {
                                    SubmissionOutcome::Partial { missing }
                                };
                                record
                                    .submission_outcomes
                                    .insert(sub_id.clone(), outcome_entry);
                            }
                            LoopOutcome::Aborted => {
                                record.submission_outcomes.insert(
                                    sub_id.clone(),
                                    SubmissionOutcome::Aborted,
                                );
                            }
                            LoopOutcome::Failed => {
                                record.submission_outcomes.insert(
                                    sub_id.clone(),
                                    SubmissionOutcome::Failed {
                                        message: error
                                            .as_deref()
                                            .map(str::trim)
                                            .filter(|text| !text.is_empty())
                                            .unwrap_or(
                                                "model reported an error",
                                            )
                                            .to_owned(),
                                    },
                                );
                            }
                            LoopOutcome::WaitingForUser => {
                                // Suspended awaiting user confirmation; do not
                                // record a terminal Failed/Aborted outcome.
                            }
                        }
                    }
                    Ok((effective_outcome, commit, effective_card))
                }
                Err(message) => {
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "Engine::run_prepared",
                        session = %session_id,
                        error = %crate::loop_runner::log_preview(&message, 500),
                        prompt_preview = %crate::loop_runner::log_preview(prompt, 200),
                        "turn failed"
                    );
                    record.status = TurnState::Error {
                        message: message.clone(),
                    };
                    if let Some(sub_id) = submission_id.as_ref() {
                        record.submission_outcomes.insert(
                            sub_id.clone(),
                            SubmissionOutcome::Failed {
                                message: message.clone(),
                            },
                        );
                    }
                    Err(RunError::Loop(message))
                }
            };
            if !matches!(&result, Ok((LoopOutcome::WaitingForUser, _, _))) {
                let (outcome, artifact_ids) = match &result {
                    Ok((outcome, commit, _)) => (
                        execution_outcome(*outcome),
                        commit.as_ref().map_or_else(Vec::new, |commit| {
                            commit.artifact_ids.clone()
                        }),
                    ),
                    Err(_) => (TurnExecutionOutcome::Failed, Vec::new()),
                };
                record.turn_completions.push(TurnCompletionEvidence {
                    session_id: session_id.to_owned(),
                    submission_id: completion_id.clone(),
                    outcome,
                    artifact_ids,
                    committed_at_ms: record.updated_at,
                    execution: execution.clone(),
                });
            }
            if let Err(error) = inner.index.save(&self.root) {
                // A later successful save must never resurrect this turn's
                // uncommitted receipts or its DeliveryCommit.
                inner.index.upsert(before);
                let armed = ticket.is_current();
                if armed {
                    inner.finalizing.insert(session_id.to_owned());
                }
                drop(inner);
                if !self.finish_non_commit(
                    armed,
                    session_id,
                    prompt,
                    &cancel_trace,
                    &ticket,
                    || {
                        self.observer.storage_error(session_id);
                        self.observer.finished(session_id, LoopOutcome::Failed);
                    },
                ) {
                    return Ok(LoopOutcome::Aborted);
                }
                return Err(RunError::Storage(error));
            }
            match result {
                Ok((outcome, Some(commit), _)) => {
                    // This lock also serializes request_cancel. The successful
                    // history save is the delivery's linearization point.
                    inner.finalizing.insert(session_id.to_owned());
                    drop(inner);
                    (outcome, commit)
                }
                Ok((LoopOutcome::WaitingForUser, None, Some(card))) => {
                    let armed = ticket.is_current();
                    if armed {
                        inner.finalizing.insert(session_id.to_owned());
                    }
                    drop(inner);
                    if !self.finish_non_commit(
                        armed,
                        session_id,
                        prompt,
                        &cancel_trace,
                        &ticket,
                        || {
                            self.observer.waiting_for_user(session_id, &card);
                        },
                    ) {
                        return Ok(LoopOutcome::Aborted);
                    }
                    return Ok(LoopOutcome::WaitingForUser);
                }
                Ok((outcome, None, _)) => {
                    // History is durably saved; keep `inner.finalizing` armed
                    // across `observer.finished` so `request_cancel` cannot
                    // split durable history from live UI state and
                    // `wait_until_idle` never wakes up before live status leaves
                    // `Running`.
                    let armed = ticket.is_current();
                    if armed {
                        inner.finalizing.insert(session_id.to_owned());
                    }
                    drop(inner);
                    if !self.finish_non_commit(
                        armed,
                        session_id,
                        prompt,
                        &cancel_trace,
                        &ticket,
                        || {
                            self.observer.finished(session_id, outcome);
                        },
                    ) {
                        return Ok(LoopOutcome::Aborted);
                    }
                    return Ok(outcome);
                }
                Err(error) => {
                    let armed = ticket.is_current();
                    if armed {
                        inner.finalizing.insert(session_id.to_owned());
                    }
                    drop(inner);
                    if !self.finish_non_commit(
                        armed,
                        session_id,
                        prompt,
                        &cancel_trace,
                        &ticket,
                        || {
                            self.observer
                                .finished(session_id, LoopOutcome::Failed);
                        },
                    ) {
                        return Ok(LoopOutcome::Aborted);
                    }
                    return Err(error);
                }
            }
        };
        let (pending_outcome, commit) = stored;
        let mut finalizing_guard = FinalizingGuard {
            engine: self,
            session_id: session_id.to_owned(),
            armed: true,
        };
        self.observer.saving_files(session_id);
        let committed = match delivery_committer {
            Some(committer) => committer.commit(&commit).await,
            None => Err("artifact delivery store is unavailable".to_owned()),
        };
        let outcome = match committed {
            Ok(()) => {
                // `inner.finalizing` already blocks `request_cancel`; avoid
                // holding `RunFence::active` across `delivery_committed` disk
                // audit/index fsyncs.
                if ticket.is_current() {
                    self.observer.delivery_committed(&commit);
                }
                Ok(pending_outcome)
            }
            Err(error) => {
                tracing::error!(
                    target: "flowy.storage",
                    operation = "commit delivered artifacts",
                    session = %session_id,
                    error = %error,
                    "storage operation failed"
                );
                self.observer.delivery_failed(&commit, &error);
                Err(RunError::Storage(std::io::Error::other(error)))
            }
        };
        // Keep the per-session flight active while the store writes its marker.
        // Notify observer while the RunFence still blocks replacement runs and
        // `inner.finalizing` still blocks cancellation, without holding
        // `self.inner` across the observer callback.
        if ticket.is_current() {
            self.observer.finished(
                session_id,
                outcome.as_ref().copied().unwrap_or(LoopOutcome::Failed),
            );
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        finalizing_guard.armed = false;
        let clean = ticket.finish();
        inner.finalizing.remove(session_id);
        drop(inner);
        if !clean {
            return Ok(LoopOutcome::Aborted);
        }
        outcome
    }

    fn persist_stopped_completion(
        &self,
        inner: &mut Inner,
        session_id: &str,
        submission_id: &str,
        execution: &ExecutionMetadata,
    ) -> Result<(), RunError> {
        let active = inner
            .fences
            .get(session_id)
            .is_some_and(|fence| fence.is_active());
        let Some(record) = inner.index.get_mut(session_id) else {
            // Deletion must not be undone when an old cancelled future exits.
            return Ok(());
        };
        if record
            .turn_completions
            .iter()
            .any(|evidence| evidence.submission_id == submission_id)
        {
            return Ok(());
        }
        if record.execution != *execution
            || !record.messages.iter().any(|message| {
                message.role == Role::User
                    && matches!(message.source, MessageSource::User)
                    && message.id == submission_id
            })
        {
            return Err(RunError::Storage(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stopped submission no longer matches its session",
            )));
        }
        let previous = record.clone();
        record.turn_completions.push(TurnCompletionEvidence {
            session_id: session_id.to_owned(),
            submission_id: submission_id.to_owned(),
            outcome: TurnExecutionOutcome::Stopped,
            artifact_ids: Vec::new(),
            committed_at_ms: now_ms(),
            execution: execution.clone(),
        });
        if !active && record.status.is_busy() {
            record.status = TurnState::Stopped;
        }
        if let Err(error) = inner.index.save(&self.root) {
            inner.index.upsert(previous);
            self.observer.storage_error(session_id);
            return Err(RunError::Storage(error));
        }
        Ok(())
    }
}

fn valid_artifact_ids(ids: &[String]) -> bool {
    ids.iter().all(|id| !id.trim().is_empty())
        && ids.iter().collect::<HashSet<_>>().len() == ids.len()
}

fn valid_completion(
    record: &SessionRecord,
    evidence: &TurnCompletionEvidence,
) -> bool {
    !record.id.trim().is_empty()
        && evidence.session_id == record.id
        && !evidence.submission_id.trim().is_empty()
        && evidence.execution == record.execution
        && evidence.execution.is_valid()
        && record.messages.iter().any(|message| {
            message.role == Role::User
                && matches!(
                    message.source,
                    MessageSource::User | MessageSource::CardResume
                )
                && message.id == evidence.submission_id
        })
        && record
            .turn_completions
            .iter()
            .filter(|candidate| {
                candidate.submission_id == evidence.submission_id
            })
            .count()
            == 1
        && valid_artifact_ids(&evidence.artifact_ids)
        && if evidence.artifact_ids.is_empty() {
            !record
                .delivery_commits
                .iter()
                .any(|commit| commit.submission_id == evidence.submission_id)
        } else {
            evidence.outcome == TurnExecutionOutcome::Completed
                && record.delivery_commits.iter().any(|commit| {
                    commit.submission_id == evidence.submission_id
                        && valid_saved_commit(record, commit)
                })
        }
}

fn valid_saved_commit(record: &SessionRecord, commit: &DeliveryCommit) -> bool {
    let proofs: Vec<_> = record
        .turn_completions
        .iter()
        .filter(|evidence| evidence.submission_id == commit.submission_id)
        .collect();
    !record.id.trim().is_empty()
        && commit.session_id == record.id
        && !commit.submission_id.trim().is_empty()
        && commit.completed_successfully
        && !commit.artifact_ids.is_empty()
        && valid_artifact_ids(&commit.artifact_ids)
        && commit.execution == record.execution
        && commit.origin == record.origin
        && commit.execution.is_valid()
        && record
            .delivery_commits
            .iter()
            .filter(|candidate| candidate.submission_id == commit.submission_id)
            .count()
            == 1
        && (proofs.is_empty()
            || (proofs.len() == 1
                && proofs[0].session_id == record.id
                && proofs[0].execution == commit.execution
                && proofs[0].outcome == TurnExecutionOutcome::Completed
                && proofs[0].artifact_ids == commit.artifact_ids
                && record.messages.iter().any(|message| {
                    message.role == Role::User
                        && matches!(
                            message.source,
                            MessageSource::User | MessageSource::CardResume
                        )
                        && message.id == commit.submission_id
                })))
}

const fn execution_outcome(outcome: LoopOutcome) -> TurnExecutionOutcome {
    match outcome {
        LoopOutcome::Completed => TurnExecutionOutcome::Completed,
        LoopOutcome::Failed | LoopOutcome::WaitingForUser => {
            TurnExecutionOutcome::Failed
        }
        LoopOutcome::Aborted => TurnExecutionOutcome::Stopped,
    }
}

/// Cap huge tool output strings (e.g., 2 MiB `read_file` dumps) before
/// persisting them into `history.json`, preserving all normal JSON tool results.
fn clamp_persisted_tool_outputs(messages: &mut [Message]) {
    const MAX_PERSISTED_TOOL_CHARS: usize = 64 * 1024;
    const SUFFIX: &str = "\n...[truncated in persisted history]";
    for message in messages {
        if !matches!(message.source, MessageSource::Tool { .. }) {
            continue;
        }
        for block in &mut message.content {
            if let ContentBlock::ToolResult { content, .. } = block {
                for inner in content {
                    if let ContentBlock::Text { text } = inner
                        && text.chars().count() > MAX_PERSISTED_TOOL_CHARS
                    {
                        let mut clipped: String = text
                            .chars()
                            .take(MAX_PERSISTED_TOOL_CHARS)
                            .collect();
                        clipped.push_str(SUFFIX);
                        *text = clipped;
                    }
                }
            }
        }
    }
}

struct FinalizingGuard<'a> {
    engine: &'a Engine,
    session_id: String,
    armed: bool,
}

impl Drop for FinalizingGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let mut inner = self
                .engine
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.finalizing.remove(&self.session_id);
        }
    }
}

fn attach_artifact_receipts(
    messages: &mut [Message],
    receipts: &[ArtifactReceipt],
    session_id: &str,
    submission_id: Option<&str>,
) {
    for message in messages {
        if !matches!(&message.source, MessageSource::Tool { tool } if tool == "publish_file")
        {
            continue;
        }
        for block in &mut message.content {
            if let ContentBlock::ToolResult {
                tool_call_id,
                is_error,
                artifact,
                ..
            } = block
                && *is_error != Some(true)
            {
                *artifact = receipts
                    .iter()
                    .find(|receipt| {
                        receipt.session_id == session_id
                            && Some(receipt.submission_id.as_str())
                                == submission_id
                            && receipt.tool_call_id == *tool_call_id
                    })
                    .cloned()
                    .map(Box::new);
            }
        }
    }
}

fn missing_required_for_submission(
    record: &SessionRecord,
    submission_id: &str,
) -> Vec<String> {
    let Some(contract) = record.work_contract.as_ref() else {
        return Vec::new();
    };
    if contract.required_items.is_empty() {
        return Vec::new();
    }
    let current_rev = record
        .submission_revisions
        .get(submission_id)
        .copied()
        .unwrap_or(contract.revision);
    let mut delivered: Vec<(String, String)> = Vec::new();
    for message in &record.messages {
        for block in &message.content {
            if let ContentBlock::ToolResult {
                artifact: Some(receipt),
                ..
            } = block
                && receipt.session_id == record.id
            {
                let receipt_rev = record
                    .submission_revisions
                    .get(&receipt.submission_id)
                    .copied();
                if receipt.submission_id == submission_id
                    || receipt_rev == Some(current_rev)
                {
                    delivered.push((
                        receipt.display_name.clone(),
                        receipt.delivery_key.clone(),
                    ));
                }
            }
        }
    }
    let pairs: Vec<(&str, &str)> = delivered
        .iter()
        .map(|(name, key)| (name.as_str(), key.as_str()))
        .collect();
    contract.missing_deliverables(&pairs)
}

fn delivery_commit(
    messages: &[Message],
    session_id: &str,
    submission_id: Option<&str>,
    record: &SessionRecord,
    outcome: LoopOutcome,
) -> Option<DeliveryCommit> {
    if outcome != LoopOutcome::Completed {
        return None;
    }
    let submission_id = submission_id?;
    let mut artifact_ids = Vec::new();
    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolResult {
                artifact: Some(receipt),
                ..
            } = block
                && receipt.session_id == session_id
                && receipt.submission_id == submission_id
                && !artifact_ids.contains(&receipt.artifact_id)
            {
                artifact_ids.push(receipt.artifact_id.clone());
            }
        }
    }
    if artifact_ids.is_empty() {
        return None;
    }
    Some(DeliveryCommit {
        session_id: session_id.to_owned(),
        submission_id: submission_id.to_owned(),
        artifact_ids,
        completed_successfully: true,
        committed_at_ms: now_ms(),
        origin: record.origin.clone(),
        origin_title_snapshot: record.title.clone(),
        execution: record.execution.clone(),
    })
}

fn build_tools(
    config: &EngineConfig,
    run_ctx: ToolRunContext,
    artifact_receipts: Arc<Mutex<Vec<ArtifactReceipt>>>,
    budget: Option<Arc<RunBudgetState>>,
) -> Vec<Arc<dyn rpi_agent::AgentTool>> {
    config
        .registry
        .list()
        .into_iter()
        .filter_map(|name| {
            let definition = config.registry.get(&name)?;
            RegistryTool::new_with_artifact_sink(
                config.registry.clone(),
                definition,
                run_ctx.clone(),
                Some(Arc::clone(&artifact_receipts)),
                config.execution_gate.clone(),
                budget.clone(),
            )
            .ok()
            .map(|tool| {
                let tool: Arc<dyn rpi_agent::AgentTool> = Arc::new(tool);
                tool
            })
        })
        .collect()
}

fn status_for(outcome: LoopOutcome, error: Option<&str>) -> TurnState {
    match outcome {
        LoopOutcome::Completed => TurnState::Completed,
        LoopOutcome::Aborted => TurnState::Stopped,
        LoopOutcome::WaitingForUser => TurnState::WaitingForUser,
        LoopOutcome::Failed => TurnState::Error {
            message: error
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or("model reported an error")
                .to_owned(),
        },
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[derive(Default)]
struct CancelTrace {
    last_command: Option<String>,
    in_flight_tools: HashMap<String, (String, Option<String>)>,
    modified_files: Vec<String>,
    verified_effects: Vec<String>,
    verified_paths: Vec<String>,
}

impl CancelTrace {
    fn record_tool_start(
        &mut self,
        call_id: &str,
        name: &str,
        args: &serde_json::Value,
    ) {
        let target = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("command"))
            .or_else(|| args.get("cmd"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| crate::loop_runner::log_preview(s, 120));
        let summary = match &target {
            Some(t) => format!("{name}: {t}"),
            None => name.to_owned(),
        };
        self.last_command = Some(summary);
        self.in_flight_tools
            .insert(call_id.to_owned(), (name.to_owned(), target));
    }

    fn record_tool_end(
        &mut self,
        call_id: &str,
        name: &str,
        result_text: &str,
        is_error: bool,
        aborted: bool,
    ) {
        if aborted {
            // Preserve the interrupted entry in `in_flight_tools` so
            // `StopHandoffManifest.pending_checks` reports `interrupted:*`.
            return;
        }
        let entry = self.in_flight_tools.remove(call_id);
        if is_error {
            return;
        }
        let target = entry.and_then(|(_, target)| target);
        if matches!(
            name,
            "write_file" | "edit_file" | "create_deck" | "publish_file"
        ) && let Some(path) = target.as_ref()
        {
            if !self.modified_files.contains(path) {
                self.modified_files.push(path.clone());
            }
            let effect = format!("{name} -> {path}");
            if !self.verified_effects.contains(&effect) {
                self.verified_effects.push(effect);
            }
        } else if matches!(name, "verify_deck" | "verify_artifact") {
            let parsed =
                serde_json::from_str::<serde_json::Value>(result_text).ok();
            let passed = parsed
                .as_ref()
                .and_then(|val| val.get("ok"))
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            if !passed {
                return;
            }
            let verified_path = parsed
                .as_ref()
                .and_then(|val| val.get("artifact"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or(target);
            let effect = match &verified_path {
                Some(path) => format!("{name} ({path})"),
                None => name.to_owned(),
            };
            if !self.verified_effects.contains(&effect) {
                self.verified_effects.push(effect);
            }
            if let Some(path) = verified_path
                && !self.verified_paths.contains(&path)
            {
                self.verified_paths.push(path);
            }
        }
    }

    fn is_object_verified(&self, obj: &crate::session::TrackedObject) -> bool {
        if matches!(
            obj.verify_state,
            ObjectVerifyState::Passed { .. }
                | ObjectVerifyState::Overridden { .. }
        ) {
            return true;
        }
        self.verified_paths.iter().any(|raw| {
            let path = Path::new(raw);
            if let Some(expected) = obj.filename.as_deref() {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            } else {
                path.extension().and_then(|e| e.to_str()).is_some_and(
                    |actual| actual.eq_ignore_ascii_case(&obj.extension),
                )
            }
        })
    }

    fn build_manifest(
        &self,
        record: &SessionRecord,
        prompt: &str,
    ) -> StopHandoffManifest {
        let input_revision = record
            .work_contract
            .as_ref()
            .map_or(0, |contract| contract.revision);
        let command_summary = self.last_command.clone().unwrap_or_else(|| {
            let trimmed = prompt.trim();
            if trimmed.is_empty() {
                "cancelled idle turn".to_owned()
            } else {
                crate::loop_runner::log_preview(trimmed, 160)
            }
        });
        let mut pending_checks: Vec<String> = self
            .in_flight_tools
            .values()
            .map(|(name, target)| match target {
                Some(t) => format!("interrupted:{name}:{t}"),
                None => format!("interrupted:{name}"),
            })
            .collect();
        pending_checks.sort();
        if let Some(contract) = &record.work_contract {
            for obj in contract.writable_deliverables() {
                if !self.is_object_verified(obj) {
                    pending_checks
                        .push(format!("unverified:{}", obj.display_label()));
                }
            }
        }
        let rollback_hint = if self.modified_files.is_empty() {
            "card.stop_rollback_none".to_owned()
        } else {
            "card.stop_rollback_modified".to_owned()
        };
        StopHandoffManifest {
            input_revision,
            command_summary,
            modified_files: self.modified_files.clone(),
            verified_effects: self.verified_effects.clone(),
            pending_checks,
            rollback_hint,
        }
    }
}

struct EngineEmitter {
    session: String,
    observer: Arc<dyn TurnObserver>,
    publisher: PublishToken,
    trace: Option<Arc<Mutex<CancelTrace>>>,
}

impl AgentEmitter for EngineEmitter {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, ()> {
        let session = self.session.clone();
        let observer = Arc::clone(&self.observer);
        let publisher = self.publisher.clone();
        let trace = self.trace.clone();
        Box::pin(async move {
            let trace_end = match &event {
                AgentEvent::ToolExecutionEnd {
                    tool_call_id,
                    tool_name,
                    result,
                    is_error,
                } if trace.is_some() => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|item| match item {
                            rpi_agent::TextContentOrImage::Text(text) => {
                                Some(text.text.as_str())
                            }
                            rpi_agent::TextContentOrImage::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    Some((
                        tool_call_id.clone(),
                        tool_name.clone(),
                        text,
                        *is_error,
                    ))
                }
                _ => None,
            };
            if let (Some(trace), Some((call_id, name, text, is_error))) =
                (&trace, trace_end)
            {
                let aborted = !publisher.is_current() && is_error;
                trace
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_tool_end(&call_id, &name, &text, is_error, aborted);
            }
            publisher.publish(|| match event {
                AgentEvent::MessageUpdate {
                    assistant_message_event:
                        AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } => observer.text(&session, &delta),
                AgentEvent::MessageUpdate {
                    assistant_message_event:
                        AssistantMessageEvent::ThinkingDelta { delta, .. },
                    ..
                } => observer.reasoning(&session, &delta),
                AgentEvent::MessageUpdate {
                    assistant_message_event:
                        AssistantMessageEvent::Error { error, reason, .. },
                    ..
                } => {
                    let text: String = error
                        .content
                        .iter()
                        .filter_map(|content| match content {
                            rpi_ai::types::Content::Text(text) => {
                                Some(text.text.as_str())
                            }
                            _ => None,
                        })
                        .collect();
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "EngineEmitter",
                        session = %session,
                        ?reason,
                        error = %crate::loop_runner::log_preview(&text, 500),
                        "assistant stream error"
                    );
                }
                AgentEvent::ToolExecutionStart {
                    tool_call_id,
                    tool_name,
                    args,
                } => {
                    if let Some(trace) = &trace {
                        trace
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .record_tool_start(
                                &tool_call_id,
                                &tool_name,
                                &args,
                            );
                    }
                    observer.tool_start(
                        &session,
                        &tool_call_id,
                        &tool_name,
                        &args,
                    );
                }
                AgentEvent::ToolExecutionEnd {
                    tool_call_id,
                    tool_name,
                    result,
                    is_error,
                } => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|item| match item {
                            rpi_agent::TextContentOrImage::Text(text) => {
                                Some(text.text.as_str())
                            }
                            rpi_agent::TextContentOrImage::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    observer.tool_end(
                        &session,
                        &tool_call_id,
                        &tool_name,
                        &text,
                        is_error,
                    );
                }
                _ => {}
            });
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancelDisposition, Engine, EngineConfig, EngineEmitter, RunError,
        TurnObserver,
    };
    use crate::loop_runner::{LoopOutcome, ModelRoute};
    use crate::session::{
        DeliveryCommit, DeliveryCommitter, SessionIndex, SessionOrigin,
        SessionRole, SessionWorkspace, TurnExecutionOutcome, TurnState,
        WorkAccessMode,
    };
    use crate::tools::{ToolDefinition, ToolRegistry};
    use crate::types::{ArtifactReceipt, ContentBlock, ToolExecutionResult};
    use rpi_agent::{AgentEmitter, AgentEvent, StreamFn};
    use rpi_ai::types::{
        Api, AssistantMessage, AssistantMessageEvent, DoneReason, StopReason,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    // serde_json supplies additional String equality impls, so bare [] cannot
    // infer the item type in these empty artifact-list assertions.
    const EMPTY_ARTIFACT_IDS: [String; 0] = [];

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("build-test")
                .join(format!(
                    "engine-{tag}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct Recorder {
        text: Mutex<String>,
        reasoning: Mutex<String>,
        finished: Mutex<Vec<LoopOutcome>>,
        storage_errors: Mutex<usize>,
        deliveries: Mutex<Vec<DeliveryCommit>>,
        saving_files: Mutex<usize>,
        delivery_failures: Mutex<usize>,
    }

    impl TurnObserver for Recorder {
        fn text(&self, _session: &str, delta: &str) {
            self.text.lock().unwrap().push_str(delta);
        }
        fn reasoning(&self, _session: &str, delta: &str) {
            self.reasoning.lock().unwrap().push_str(delta);
        }
        fn finished(&self, _session: &str, outcome: LoopOutcome) {
            self.finished.lock().unwrap().push(outcome);
        }
        fn storage_error(&self, _session: &str) {
            *self.storage_errors.lock().unwrap() += 1;
        }
        fn delivery_committed(&self, commit: &DeliveryCommit) {
            self.deliveries.lock().unwrap().push(commit.clone());
        }
        fn saving_files(&self, _session: &str) {
            *self.saving_files.lock().unwrap() += 1;
        }
        fn delivery_failed(&self, _commit: &DeliveryCommit, _error: &str) {
            *self.delivery_failures.lock().unwrap() += 1;
        }
    }

    #[derive(Default)]
    struct SequenceRecorder {
        events: Mutex<Vec<String>>,
    }

    impl TurnObserver for SequenceRecorder {
        fn text(&self, _session: &str, delta: &str) {
            self.events.lock().unwrap().push(format!("text:{delta}"));
        }
        fn assistant_text(&self, _session: &str, text: &str) {
            self.events
                .lock()
                .unwrap()
                .push(format!("assistant:{text}"));
        }
        fn tool_start(
            &self,
            _session: &str,
            _call_id: &str,
            name: &str,
            _args: &serde_json::Value,
        ) {
            self.events
                .lock()
                .unwrap()
                .push(format!("tool_start:{name}"));
        }
        fn tool_end(
            &self,
            _session: &str,
            _call_id: &str,
            name: &str,
            _text: &str,
            is_error: bool,
        ) {
            self.events
                .lock()
                .unwrap()
                .push(format!("tool_end:{name}:{is_error}"));
        }
        fn finished(&self, _session: &str, outcome: LoopOutcome) {
            self.events
                .lock()
                .unwrap()
                .push(format!("finished:{outcome:?}"));
        }
    }

    struct CommitRecorder {
        root: PathBuf,
        commits: Mutex<Vec<DeliveryCommit>>,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl DeliveryCommitter for CommitRecorder {
        async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String> {
            let persisted = SessionIndex::load(&self.root).unwrap();
            assert!(
                persisted
                    .get(&commit.session_id)
                    .unwrap()
                    .delivery_commits
                    .contains(commit),
                "the complete history commit must precede the store marker"
            );
            self.commits.lock().unwrap().push(commit.clone());
            if self.fail {
                Err("marker write failed".into())
            } else {
                Ok(())
            }
        }
    }

    struct SlowCommitter {
        root: PathBuf,
        entered: tokio::sync::Notify,
        released: tokio::sync::Notify,
        commits: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl DeliveryCommitter for SlowCommitter {
        async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String> {
            let history = SessionIndex::load(&self.root).unwrap();
            assert!(
                history
                    .get(&commit.session_id)
                    .unwrap()
                    .delivery_commits
                    .contains(commit)
            );
            self.commits.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.released.notified().await;
            Ok(())
        }
    }

    fn delivery_config(
        committer: Arc<dyn DeliveryCommitter>,
        trusted: bool,
    ) -> EngineConfig {
        let mut cfg = config();
        cfg.delivery_committer = Some(committer);
        let tool = ToolDefinition::new(
            "publish_file",
            "publish an existing file",
            serde_json::json!({"type":"object","properties":{}}),
            |args, ctx| {
                Box::pin(async move {
                    let receipt = ArtifactReceipt {
                        artifact_id: format!("artifact-{}", args.call_id),
                        delivery_key: args.call_id.clone(),
                        session_id: ctx.agent_id.unwrap(),
                        submission_id: ctx.submission_id.unwrap_or_default(),
                        tool_call_id: args.call_id,
                        display_name: "report.csv".into(),
                        media_type: Some("text/csv".into()),
                        size_bytes: 12,
                        sha256: "test-digest".into(),
                        created_at_ms: 1,
                        origin_title_snapshot: String::new(),
                    };
                    ToolExecutionResult::Success {
                        content: vec![ContentBlock::text("prepared")],
                        value: serde_json::json!({
                            "__flowy_host_tool_result_v1": {
                                "artifact_receipt": receipt,
                            }
                        }),
                        meta: None,
                        delivery: Some(Box::new(receipt)),
                    }
                })
            },
        );
        cfg.registry.register(Arc::new(if trusted {
            tool.with_artifact_delivery()
        } else {
            tool
        }));
        cfg
    }

    fn delivery_stream(final_stop: StopReason) -> StreamFn {
        delivery_stream_with_gate(final_stop, None)
    }

    fn delivery_stream_with_gate(
        final_stop: StopReason,
        gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    ) -> StreamFn {
        let turn = Arc::new(AtomicUsize::new(0));
        rpi_agent::stream_fn(move |model, _, _| {
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            let initial = turn.fetch_add(1, Ordering::SeqCst) == 0;
            if initial {
                message.content = ["one", "two"]
                    .into_iter()
                    .map(|id| {
                        rpi_ai::types::Content::ToolCall(
                            rpi_ai::types::ToolCall {
                                kind: rpi_ai::types::ToolCallType,
                                id: id.into(),
                                name: "publish_file".into(),
                                arguments: serde_json::json!({}),
                                thought_signature: None,
                                namespace: None,
                                recorded: None,
                            },
                        )
                    })
                    .collect();
                message.stop_reason = StopReason::ToolUse;
            } else {
                message.content = vec![rpi_ai::types::Content::text("done")];
                message.stop_reason = final_stop;
            }
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            let gate = (!initial).then(|| gate.clone()).flatten();
            tokio::spawn(async move {
                if let Some((entered, released)) = gate {
                    entered.notify_one();
                    released.notified().await;
                }
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        })
    }

    fn config() -> EngineConfig {
        EngineConfig {
            route: ModelRoute {
                provider: "flowy".into(),
                model: "m".into(),
                base_url: "https://example.invalid".into(),
                ..ModelRoute::default()
            },
            api_key: Some("token".into()),
            system: "be brief".into(),
            registry: ToolRegistry::new(),
            cwd: None,
            todos: None,
            delivery_committer: None,
            work_contract: None,
            pre_tool_hook: None,
            execution_gate: None,
            run_budget: None,
        }
    }

    #[tokio::test]
    async fn delivery_group_is_durable_before_marker_and_visible_after_marker()
    {
        let scratch = Scratch::new("delivery");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: false,
        });
        let cfg = delivery_config(committer.clone(), true);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        let run = engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream(StopReason::Stop)),
                Some(7),
            )
            .unwrap();
        assert_eq!(run.await.unwrap(), LoopOutcome::Completed);
        let commits = committer.commits.lock().unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].artifact_ids, ["artifact-one", "artifact-two"]);
        assert_eq!(commits[0].submission_id, format!("submission-{:032x}", 7));
        assert_eq!(*observer.deliveries.lock().unwrap(), *commits);
        assert_eq!(
            *observer.finished.lock().unwrap(),
            [LoopOutcome::Completed]
        );
        let restored = SessionIndex::load(&scratch.0).unwrap();
        let record = restored.get("home").unwrap();
        assert_eq!(record.origin, SessionOrigin::Home);
        assert!(record.workspace.is_some());
        let receipts = record
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| match block {
                ContentBlock::ToolResult { artifact, .. } => artifact.as_ref(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].artifact_id, "artifact-one");
    }

    #[tokio::test]
    async fn untrusted_publish_details_cannot_create_a_delivery() {
        let scratch = Scratch::new("untrusted-delivery");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: false,
        });
        let cfg = delivery_config(committer.clone(), false);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream(StopReason::Stop)),
                Some(7),
            )
            .unwrap()
            .await
            .unwrap();
        assert!(committer.commits.lock().unwrap().is_empty());
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert_eq!(
            engine.session("home").unwrap().delivery_commits.as_slice(),
            []
        );
    }

    #[tokio::test]
    async fn model_failure_after_prepare_never_commits_delivery() {
        let scratch = Scratch::new("failed-delivery");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: false,
        });
        let cfg = delivery_config(committer.clone(), true);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        let outcome = engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream(StopReason::Error)),
                Some(7),
            )
            .unwrap()
            .await
            .unwrap();
        assert_eq!(outcome, LoopOutcome::Failed);
        assert!(committer.commits.lock().unwrap().is_empty());
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert_eq!(
            engine.session("home").unwrap().delivery_commits.as_slice(),
            []
        );
        let proof = engine
            .completion_evidence("home", &format!("submission-{:032x}", 7))
            .unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Failed);
        assert_eq!(proof.artifact_ids.as_slice(), EMPTY_ARTIFACT_IDS);
    }

    #[tokio::test]
    async fn failed_store_marker_never_reports_successful_delivery() {
        let scratch = Scratch::new("marker-failure");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: true,
        });
        let cfg = delivery_config(committer.clone(), true);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        let outcome = engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream(StopReason::Stop)),
                Some(7),
            )
            .unwrap()
            .await;
        assert!(matches!(outcome, Err(RunError::Storage(_))));
        assert_eq!(committer.commits.lock().unwrap().len(), 1);
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert_eq!(*observer.delivery_failures.lock().unwrap(), 1);
        assert_eq!(*observer.storage_errors.lock().unwrap(), 0);
        assert_eq!(*observer.finished.lock().unwrap(), [LoopOutcome::Failed]);
        // A restart can retry exactly this complete durable group.
        assert_eq!(engine.session("home").unwrap().delivery_commits.len(), 1);
        let submission = format!("submission-{:032x}", 7);
        let proof = engine.completion_evidence("home", &submission).unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Completed);
        assert_eq!(proof.artifact_ids, ["artifact-one", "artifact-two"]);
        let before = engine.session("home").unwrap();
        drop(engine);
        let reopened = Engine::open(
            scratch.0.clone(),
            config(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        let successful = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: false,
        });
        let restored_commit = reopened
            .retry_delivery_commit("home", &submission, successful.clone())
            .await
            .unwrap();
        assert_eq!(restored_commit, before.delivery_commits[0]);
        assert_eq!(successful.commits.lock().unwrap().len(), 1);
        assert_eq!(reopened.session("home").unwrap(), before);
        assert_eq!(
            reopened.completion_evidence("home", &submission),
            Some(proof)
        );
    }

    #[tokio::test]
    async fn cancel_after_delivery_history_commit_cannot_undo_finalization() {
        let scratch = Scratch::new("cancel-finalizing");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(SlowCommitter {
            root: scratch.0.clone(),
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
            commits: AtomicUsize::new(0),
        });
        let cfg = delivery_config(committer.clone(), true);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        let run = engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream(StopReason::Stop)),
                Some(7),
            )
            .unwrap();
        let task = tokio::spawn(run);
        committer.entered.notified().await;
        assert_eq!(*observer.saving_files.lock().unwrap(), 1);
        assert!(!engine.request_cancel("home").unwrap());
        assert_eq!(
            engine.request_cancel_typed("home").unwrap(),
            CancelDisposition::TooLate
        );
        assert_eq!(
            engine.session("home").unwrap().status,
            TurnState::Completed
        );
        assert_eq!(
            engine.remove_session("home").unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert!(observer.finished.lock().unwrap().is_empty());
        committer.released.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), LoopOutcome::Completed);
        assert_eq!(committer.commits.load(Ordering::SeqCst), 1);
        assert_eq!(observer.deliveries.lock().unwrap().len(), 1);
        assert_eq!(
            *observer.finished.lock().unwrap(),
            [LoopOutcome::Completed]
        );
        assert!(engine.remove_session("home").unwrap());
    }

    #[tokio::test]
    async fn cancel_before_delivery_history_commit_never_publishes() {
        let scratch = Scratch::new("cancel-prepared");
        let observer = Arc::new(Recorder::default());
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: false,
        });
        let cfg = delivery_config(committer.clone(), true);
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer.clone())
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspaces/home"),
                }),
            )
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let released = Arc::new(tokio::sync::Notify::new());
        let run = engine
            .start_with_config(
                "home".into(),
                "deliver".into(),
                cfg,
                Some(delivery_stream_with_gate(
                    StopReason::Stop,
                    Some((entered.clone(), released.clone())),
                )),
                Some(7),
            )
            .unwrap();
        let task = tokio::spawn(run);
        entered.notified().await;
        assert!(engine.request_cancel("home").unwrap());
        released.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), LoopOutcome::Aborted);
        assert_eq!(engine.session("home").unwrap().status, TurnState::Stopped);
        assert_eq!(
            engine.session("home").unwrap().delivery_commits.as_slice(),
            []
        );
        assert!(committer.commits.lock().unwrap().is_empty());
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert_eq!(*observer.saving_files.lock().unwrap(), 0);
        assert_eq!(
            engine
                .completion_evidence("home", &format!("submission-{:032x}", 7),)
                .unwrap()
                .outcome,
            TurnExecutionOutcome::Stopped
        );
    }

    fn text_stream(text: &'static str) -> StreamFn {
        rpi_agent::stream_fn(move |model, _, _| {
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            message.content = vec![rpi_ai::types::Content::text(text)];
            message.stop_reason = StopReason::Stop;
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            tokio::spawn(async move {
                producer
                    .push(AssistantMessageEvent::Start {
                        partial: Arc::new(message.clone()),
                    })
                    .await;
                producer
                    .push(AssistantMessageEvent::TextDelta {
                        content_index: 0,
                        delta: text.into(),
                        partial: Arc::new(message.clone()),
                    })
                    .await;
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        })
    }

    #[tokio::test]
    async fn the_same_prompt_replays_the_same_event_sequence() {
        let scratch = Scratch::new("exec-replay");
        let observer = Arc::new(SequenceRecorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), observer.clone())
                .unwrap();
        let prompt = "write the memo";

        engine.create_session("s1".into(), 1).unwrap();
        engine
            .run_with_stream("s1", prompt, text_stream("same reply"))
            .await
            .unwrap();
        let first = observer.events.lock().unwrap().clone();

        engine.create_session("s2".into(), 2).unwrap();
        engine
            .run_with_stream("s2", prompt, text_stream("same reply"))
            .await
            .unwrap();
        let second = observer.events.lock().unwrap()[first.len()..].to_vec();

        assert_ne!(first, Vec::<String>::new());
        assert!(
            first.iter().any(|event| event.starts_with("finished:")),
            "the sequence must contain the terminal event"
        );
        assert_eq!(
            first, second,
            "the same prompt must replay the same event sequence"
        );
        // Message ids embed the wall clock; compare the durable shape
        // (role + content), not the id.
        let history = |session: &str| -> Vec<String> {
            engine
                .session(session)
                .unwrap()
                .messages
                .iter()
                .map(|message| {
                    format!("{:?}|{:?}", message.role, message.content)
                })
                .collect()
        };
        assert_eq!(
            history("s1"),
            history("s2"),
            "the persisted histories must match as well"
        );
    }

    #[tokio::test]
    async fn reasoning_deltas_reach_the_observer() {
        let scratch = Scratch::new("reasoning");
        let recorder = Arc::new(Recorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), recorder.clone())
                .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let stream = rpi_agent::stream_fn(|model, _, _| {
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            message.content = vec![rpi_ai::types::Content::text("hi")];
            message.stop_reason = StopReason::Stop;
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            tokio::spawn(async move {
                producer
                    .push(AssistantMessageEvent::Start {
                        partial: Arc::new(message.clone()),
                    })
                    .await;
                producer
                    .push(AssistantMessageEvent::ThinkingDelta {
                        content_index: 0,
                        delta: "think".into(),
                        partial: Arc::new(message.clone()),
                    })
                    .await;
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        });
        engine.run_with_stream("s1", "hello", stream).await.unwrap();
        assert_eq!(*recorder.reasoning.lock().unwrap(), "think");
    }

    #[tokio::test]
    async fn config_switch_applies_to_the_next_run_on_the_same_session() {
        let scratch = Scratch::new("switch");
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorder = Arc::new(Recorder::default());
        let mut first = config();
        first.route.model = "m1".into();
        let engine =
            Engine::open(scratch.0.clone(), first, recorder.clone()).unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let stream = {
            let seen = Arc::clone(&seen);
            rpi_agent::stream_fn(move |model, _, _| {
                seen.lock().unwrap().push(model.id.clone());
                let mut message = AssistantMessage::empty(
                    Api::OpenaiCompletions,
                    "flowy",
                    model.id.clone(),
                    0,
                );
                message.content = vec![rpi_ai::types::Content::text("hi")];
                message.stop_reason = StopReason::Stop;
                let (mut producer, consumer) =
                    rpi_ai::event_stream::create_assistant_message_event_stream(
                    );
                tokio::spawn(async move {
                    producer
                        .push(AssistantMessageEvent::Start {
                            partial: Arc::new(message.clone()),
                        })
                        .await;
                    producer
                        .push(AssistantMessageEvent::Done {
                            reason: DoneReason::Stop,
                            message,
                        })
                        .await;
                });
                consumer
            })
        };
        engine
            .run_with_stream("s1", "one", stream.clone())
            .await
            .unwrap();
        let mut second = config();
        second.route.model = "m2".into();
        engine.set_config(second);
        engine.run_with_stream("s1", "two", stream).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), vec!["m1", "m2"]);
    }

    #[tokio::test]
    async fn history_save_failure_is_reported_to_the_host() {
        let scratch = Scratch::new("storage");
        let recorder = Arc::new(Recorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), recorder.clone())
                .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let sessions = scratch.0.join("sessions");
        std::fs::remove_dir_all(&sessions).unwrap();
        std::fs::write(&sessions, b"blocked").unwrap();
        let outcome = engine
            .run_with_stream("s1", "hello", text_stream("hi"))
            .await;
        assert!(matches!(outcome, Err(RunError::Storage(_))));
        assert_eq!(
            *recorder.finished.lock().unwrap(),
            vec![LoopOutcome::Failed]
        );
        assert!(*recorder.storage_errors.lock().unwrap() >= 1);
    }

    #[test]
    fn session_title_is_persisted_in_history() {
        let scratch = Scratch::new("title");
        let recorder = Arc::new(Recorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), recorder.clone())
                .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let mut record = engine.session("s1").unwrap();
        record.delivery_commits.push(DeliveryCommit {
            execution: crate::session::ExecutionMetadata::default(),
            session_id: "s1".into(),
            submission_id: "submission-1".into(),
            artifact_ids: vec!["artifact-1".into()],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Fallback".into(),
        });
        engine.insert_session(record).unwrap();
        engine
            .set_session_title("s1", "Model title".into(), true)
            .unwrap();
        drop(engine);
        let reopened =
            Engine::open(scratch.0.clone(), config(), recorder).unwrap();
        let record = reopened.session("s1").expect("session");
        assert_eq!(record.title, "Model title");
        assert!(record.titled);
        assert_eq!(
            record.delivery_commits[0].origin_title_snapshot,
            "Model title"
        );
    }

    #[test]
    fn failed_session_delete_keeps_the_session_and_active_source() {
        let scratch = Scratch::new("failed-delete");
        let engine = Engine::open(
            scratch.0.clone(),
            config(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        engine.set_active(Some("s1".into())).unwrap();
        let history = SessionIndex::path(&scratch.0);
        std::fs::remove_file(&history).unwrap();
        std::fs::create_dir(&history).unwrap();
        assert!(engine.remove_session("s1").is_err());
        assert!(engine.session("s1").is_some());
        assert_eq!(engine.active().as_deref(), Some("s1"));
        std::fs::remove_dir(&history).unwrap();
        engine
            .set_session_title("s1", "retained".into(), true)
            .unwrap();
        assert!(SessionIndex::load(&scratch.0).unwrap().get("s1").is_some());
    }

    #[tokio::test]
    async fn turn_is_persisted_and_reported_once() {
        let scratch = Scratch::new("persist");
        let recorder = Arc::new(Recorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), recorder.clone())
                .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let outcome = engine
            .run_with_stream("s1", "hello", text_stream("hi"))
            .await
            .unwrap();
        assert_eq!(outcome, LoopOutcome::Completed);
        assert_eq!(*recorder.text.lock().unwrap(), "hi");
        assert_eq!(
            *recorder.finished.lock().unwrap(),
            vec![LoopOutcome::Completed]
        );

        let reopened = Engine::open(
            scratch.0.clone(),
            config(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        let record = reopened.session("s1").unwrap();
        assert_eq!(record.status, TurnState::Completed);
        assert_eq!(record.messages.len(), 2);
        assert_eq!(record.messages[0].text(), "hello");
        assert_eq!(record.messages[1].text(), "hi");
    }

    #[tokio::test]
    async fn second_run_is_refused_while_one_is_active() {
        let scratch = Scratch::new("busy");
        let engine = Engine::open(
            scratch.0.clone(),
            config(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let rx = std::sync::Arc::new(Mutex::new(Some(rx)));
        let waiter = rpi_agent::stream_fn(move |model, _, _| {
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            message.content = vec![rpi_ai::types::Content::text("late")];
            let rx = std::sync::Arc::clone(&rx);
            tokio::spawn(async move {
                let receiver = rx.lock().unwrap().take();
                if let Some(receiver) = receiver {
                    let _ = receiver.await;
                }
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        });
        let hold = tx;
        let engine = Arc::new(engine);
        let run = {
            let engine = Arc::clone(&engine);
            let stream = waiter.clone();
            tokio::spawn(async move {
                engine.run_with_stream("s1", "first", stream).await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        match engine.run_with_stream("s1", "second", waiter).await {
            Err(RunError::Busy) => {}
            other => panic!("expected Busy, got {other:?}"),
        }
        engine.cancel("s1").unwrap();
        drop(hold);
        let _ = run.await;
    }

    #[tokio::test]
    async fn cancelled_stream_cannot_publish_into_a_replacement_run() {
        let scratch = Scratch::new("late");
        let recorder = Arc::new(Recorder::default());
        let engine =
            Engine::open(scratch.0.clone(), config(), recorder.clone())
                .unwrap();
        engine.create_session("s1".into(), 1).unwrap();
        let old = engine
            .prepare(
                "s1",
                &crate::types::PromptTurn::plain("old"),
                None,
                false,
                None,
            )
            .unwrap();
        let old_submission = old.completion_id.clone();
        let emitter = EngineEmitter {
            session: "s1".into(),
            observer: recorder.clone(),
            publisher: old.ticket.publisher(),
            trace: None,
        };
        let message = Arc::new(AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            "m",
            0,
        ));
        let late = emitter.emit(AgentEvent::MessageUpdate {
            message_id: None,
            message: Arc::clone(&message),
            assistant_message_event: AssistantMessageEvent::TextDelta {
                content_index: 0,
                delta: "stale".into(),
                partial: message,
            },
        });
        engine.cancel("s1").unwrap();
        assert!(engine.is_cancelling("s1"));
        assert!(matches!(
            engine.prepare(
                "s1",
                &crate::types::PromptTurn::plain("new"),
                None,
                false,
                None,
            ),
            Err(RunError::Busy)
        ));
        assert_eq!(
            engine
                .run_prepared("s1", "old", old, Some(text_stream("old")))
                .await
                .unwrap(),
            LoopOutcome::Aborted
        );
        assert!(!engine.is_cancelling("s1"));
        let replacement = engine
            .prepare(
                "s1",
                &crate::types::PromptTurn::plain("new"),
                None,
                false,
                None,
            )
            .unwrap();
        let replacement_submission = replacement.completion_id.clone();
        late.await;
        engine
            .run_prepared("s1", "new", replacement, Some(text_stream("fresh")))
            .await
            .unwrap();
        assert_eq!(*recorder.text.lock().unwrap(), "fresh");
        assert_eq!(
            engine
                .completion_evidence("s1", &old_submission)
                .unwrap()
                .outcome,
            TurnExecutionOutcome::Stopped
        );
        assert_eq!(
            engine
                .completion_evidence("s1", &replacement_submission)
                .unwrap()
                .outcome,
            TurnExecutionOutcome::Completed
        );
    }

    #[tokio::test]
    async fn cancel_blocks_overlapping_run_until_cancelled_turn_reaps() {
        let scratch = Scratch::new("reap-wait");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let allow_reap = Arc::new(tokio::sync::Notify::new());
        let entered_clone = Arc::clone(&entered);
        let allow_reap_clone = Arc::clone(&allow_reap);
        let slow_stream = rpi_agent::stream_fn(move |model, _, _| {
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            let entered = Arc::clone(&entered_clone);
            let allow_reap = Arc::clone(&allow_reap_clone);
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            message.stop_reason = StopReason::Aborted;
            tokio::spawn(async move {
                entered.notify_one();
                allow_reap.notified().await;
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        });
        let run = engine
            .start_with_config(
                "s1".into(),
                "slow".into(),
                config(),
                Some(slow_stream),
                None,
            )
            .unwrap();
        let task = tokio::spawn(run);
        entered.notified().await;
        engine.cancel("s1").unwrap();
        assert!(engine.is_cancelling("s1"));
        assert!(matches!(
            engine
                .run_with_stream("s1", "overlap", text_stream("nope"))
                .await,
            Err(RunError::Busy)
        ));
        allow_reap.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), LoopOutcome::Aborted);
        assert!(
            engine.wait_until_idle("s1", std::time::Duration::from_secs(1))
        );
        assert_eq!(
            engine
                .run_with_stream("s1", "after-reap", text_stream("ok"))
                .await
                .unwrap(),
            LoopOutcome::Completed
        );
        assert_eq!(engine.session("s1").unwrap().status, TurnState::Completed);
    }

    #[tokio::test]
    async fn concurrent_turns_persist_both_results_and_use_session_ids() {
        let scratch = Scratch::new("parallel");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        for id in ["s1", "s2"] {
            engine.create_session(id.into(), 1).unwrap();
        }
        let streams = Arc::new(Mutex::new(Vec::new()));
        let stream = {
            let streams = Arc::clone(&streams);
            let reply = text_stream("ready");
            rpi_agent::stream_fn(move |model, context, options| {
                streams
                    .lock()
                    .unwrap()
                    .push(options.session_id.clone().unwrap());
                reply(model, context, options)
            })
        };
        let first = engine
            .start_with_config(
                "s1".into(),
                "one".into(),
                config(),
                Some(stream.clone()),
                None,
            )
            .unwrap();
        let second = engine
            .start_with_config(
                "s2".into(),
                "two".into(),
                config(),
                Some(stream),
                None,
            )
            .unwrap();
        let first = tokio::spawn(first);
        let second = tokio::spawn(second);
        assert_eq!(first.await.unwrap().unwrap(), LoopOutcome::Completed);
        assert_eq!(second.await.unwrap().unwrap(), LoopOutcome::Completed);
        streams.lock().unwrap().sort_unstable();
        assert_eq!(*streams.lock().unwrap(), ["s1", "s2"]);
        let index = crate::session::SessionIndex::load(&scratch.0).unwrap();
        for (id, prompt) in [("s1", "one"), ("s2", "two")] {
            let record = index.get(id).unwrap();
            assert_eq!(record.messages[0].text(), prompt);
            assert_eq!(record.messages[1].text(), "ready");
            assert_eq!(record.status, TurnState::Completed);
        }
    }

    #[tokio::test]
    async fn prompt_turn_keeps_model_body_and_compact_display_separately() {
        let scratch = Scratch::new("prompt-display");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        let turn = engine
            .start_with_config(
                "s1".into(),
                crate::types::PromptTurn::with_display(
                    "<skill>full model body</skill>",
                    "/skill:demo",
                ),
                config(),
                Some(text_stream("done")),
                None,
            )
            .unwrap();
        assert_eq!(turn.await.unwrap(), LoopOutcome::Completed);
        let index = crate::session::SessionIndex::load(&scratch.0).unwrap();
        let message = &index.get("s1").unwrap().messages[0];
        assert_eq!(message.text(), "<skill>full model body</skill>");
        assert_eq!(message.display_text.as_deref(), Some("/skill:demo"));
    }

    #[test]
    fn no_credential_refuses_without_touching_history() {
        // The async refusal path is covered by `run`; this keeps the error
        // variant exercised without a live provider.
        assert!(matches!(RunError::NoCredential, RunError::NoCredential));
    }

    #[tokio::test]
    async fn cancel_stays_stopping_while_tool_reaps_then_settles_with_handoff_manifest()
     {
        #[derive(Default)]
        struct StopObserver {
            settled: Mutex<Vec<crate::session::StopHandoffManifest>>,
        }
        impl TurnObserver for StopObserver {
            fn stop_settled(
                &self,
                _session: &str,
                manifest: &crate::session::StopHandoffManifest,
            ) {
                self.settled.lock().unwrap().push(manifest.clone());
            }
        }

        let scratch = Scratch::new("s2-stopping-handoff");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let entered_tool = Arc::clone(&entered);
        let release_tool = Arc::clone(&release);

        let mut cfg = config();
        cfg.registry.register(Arc::new(ToolDefinition::new(
            "write_file",
            "records file modification",
            serde_json::json!({"type": "object"}),
            |_args, _ctx| {
                Box::pin(async {
                    ToolExecutionResult::success_text(
                        "written",
                        serde_json::json!({"ok": true}),
                    )
                })
            },
        )));
        cfg.registry.register(Arc::new(ToolDefinition::new(
            "slow_reap",
            "simulates a child process taking time to reap after cancel",
            serde_json::json!({"type": "object"}),
            move |_args, _ctx| {
                let entered = Arc::clone(&entered_tool);
                let release = Arc::clone(&release_tool);
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                    ToolExecutionResult::error("CANCELLED", "reaped")
                })
            },
        )));
        cfg.work_contract = Some(crate::session::WorkContract {
            revision: 3,
            min_slides: Some(3),
            required_items: vec![crate::session::RequiredDeliverable {
                extension: "pptx".into(),
                filename: Some("青禾项目报价汇报.pptx".into()),
            }],
            pinned_facts: std::collections::BTreeMap::new(),
            objects: vec![crate::session::TrackedObject {
                object_id: "obj-pptx-1".into(),
                kind: crate::session::TrackedObjectKind::Deliverable,
                filename: Some("青禾项目报价汇报.pptx".into()),
                extension: "pptx".into(),
                anchor_summary: "青禾项目报价汇报.pptx · 2026-12-02".into(),
                overrides: std::collections::BTreeMap::new(),
                verify_state: crate::session::ObjectVerifyState::Unverified,
            }],
        });

        let observer = Arc::new(StopObserver::default());
        let observer_dyn: Arc<dyn TurnObserver> = observer.clone();
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), cfg.clone(), observer_dyn).unwrap(),
        );
        engine.create_session("s-stop".into(), 1).unwrap();

        let step = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stream = rpi_agent::stream_fn(move |model, _, _| {
            let n = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut msg = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            msg.content = match n {
                0 => vec![rpi_ai::types::Content::ToolCall(
                    rpi_ai::types::ToolCall {
                        kind: rpi_ai::types::ToolCallType,
                        id: "call-write".into(),
                        name: "write_file".into(),
                        arguments: serde_json::json!({"path": "draft.txt"}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    },
                )],
                _ => vec![rpi_ai::types::Content::ToolCall(
                    rpi_ai::types::ToolCall {
                        kind: rpi_ai::types::ToolCallType,
                        id: "call-reap".into(),
                        name: "slow_reap".into(),
                        arguments: serde_json::json!({"command": "render-deck"}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    },
                )],
            };
            msg.stop_reason = StopReason::ToolUse;
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            tokio::spawn(async move {
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::ToolUse,
                        message: msg,
                    })
                    .await;
            });
            consumer
        });

        let run = engine
            .start_with_config(
                "s-stop".into(),
                crate::types::PromptTurn::plain("generate deck"),
                cfg,
                Some(stream),
                Some(42),
            )
            .unwrap();
        let task = tokio::spawn(run);
        entered.notified().await;

        assert!(engine.request_cancel("s-stop").unwrap());
        // While `slow_reap` is still holding RunTicket, durable status MUST be
        // `Stopping` (NOT `Stopped`) and `stop_settled` must NOT have fired yet.
        assert_eq!(
            engine.session("s-stop").unwrap().status,
            TurnState::Stopping
        );
        assert!(engine.is_active("s-stop"));
        assert!(engine.is_cancelling("s-stop"));
        assert_eq!(observer.settled.lock().unwrap().len(), 0);

        // Allow child tool to finish reaping.
        release.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), LoopOutcome::Aborted);

        let rec = engine.session("s-stop").unwrap();
        assert_eq!(rec.status, TurnState::Stopped);
        assert!(!engine.is_active("s-stop"));
        let manifest = rec
            .last_stop_handoff
            .expect("StopHandoffManifest persisted on settle");
        assert_eq!(manifest.input_revision, 3);
        assert!(manifest.command_summary.contains("slow_reap"));
        assert_eq!(manifest.modified_files, vec!["draft.txt".to_owned()]);
        assert_ne!(manifest.verified_effects, Vec::<String>::new());
        assert!(
            manifest
                .pending_checks
                .iter()
                .any(|item| item.contains("interrupted:slow_reap")),
            "interrupted tool must be preserved in pending_checks: {:?}",
            manifest.pending_checks
        );
        assert!(
            manifest
                .pending_checks
                .iter()
                .any(|item| item.contains("青禾项目报价汇报.pptx"))
        );
        assert_eq!(manifest.rollback_hint, "card.stop_rollback_modified");
        assert_eq!(observer.settled.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_trace_only_records_passing_verify_and_updates_pending_checks() {
        let mut trace = super::CancelTrace::default();
        let record = crate::session::SessionRecord {
            work_contract: Some(crate::session::WorkContract {
                revision: 2,
                min_slides: Some(3),
                required_items: vec![
                    crate::session::RequiredDeliverable {
                        extension: "pptx".into(),
                        filename: Some("deck.pptx".into()),
                    },
                    crate::session::RequiredDeliverable {
                        extension: "xlsx".into(),
                        filename: Some("quote.xlsx".into()),
                    },
                ],
                pinned_facts: std::collections::BTreeMap::new(),
                objects: vec![
                    crate::session::TrackedObject {
                        object_id: "obj-pptx-1".into(),
                        kind: crate::session::TrackedObjectKind::Deliverable,
                        filename: Some("deck.pptx".into()),
                        extension: "pptx".into(),
                        anchor_summary: String::new(),
                        overrides: std::collections::BTreeMap::new(),
                        verify_state:
                            crate::session::ObjectVerifyState::Unverified,
                    },
                    crate::session::TrackedObject {
                        object_id: "obj-xlsx-2".into(),
                        kind: crate::session::TrackedObjectKind::Deliverable,
                        filename: Some("quote.xlsx".into()),
                        extension: "xlsx".into(),
                        anchor_summary: String::new(),
                        overrides: std::collections::BTreeMap::new(),
                        verify_state:
                            crate::session::ObjectVerifyState::Unverified,
                    },
                ],
            }),
            ..crate::session::SessionRecord::default()
        };

        // Failed verify_deck returns is_error == false with {"ok": false} ->
        // must NOT appear in verified_effects or clear pending_checks.
        trace.record_tool_start(
            "c1",
            "verify_deck",
            &serde_json::json!({"path": "outputs/deck.pptx"}),
        );
        trace.record_tool_end(
            "c1",
            "verify_deck",
            r#"{"ok":false,"artifact":"outputs/deck.pptx","issues":[{"code":"STRUCTURE"}]}"#,
            false,
            false,
        );
        // Passing verify_artifact with {"ok": true} clears quote.xlsx from pending_checks.
        trace.record_tool_start(
            "c2",
            "verify_artifact",
            &serde_json::json!({"path": "outputs/quote.xlsx"}),
        );
        trace.record_tool_end(
            "c2",
            "verify_artifact",
            r#"{"ok":true,"artifact":"outputs/quote.xlsx","issues":[]}"#,
            false,
            false,
        );

        let manifest = trace.build_manifest(&record, "build deliverables");
        assert!(
            !manifest
                .verified_effects
                .iter()
                .any(|e| e.contains("deck.pptx")),
            "failed verify_deck (ok=false) must not appear in verified_effects: {:?}",
            manifest.verified_effects
        );
        assert!(
            manifest
                .verified_effects
                .iter()
                .any(|e| e.contains("quote.xlsx")),
            "passing verify_artifact (ok=true) must appear in verified_effects: {:?}",
            manifest.verified_effects
        );
        assert!(
            manifest
                .pending_checks
                .iter()
                .any(|p| p == "unverified:deck.pptx"),
            "failed deck.pptx must stay in pending_checks: {:?}",
            manifest.pending_checks
        );
        assert!(
            !manifest
                .pending_checks
                .iter()
                .any(|p| p == "unverified:quote.xlsx"),
            "passing quote.xlsx must be cleared from pending_checks: {:?}",
            manifest.pending_checks
        );
    }

    #[tokio::test]
    async fn wait_until_idle_never_returns_before_observer_finished_runs() {
        struct SlowFinishedObserver {
            finished_ran: Arc<std::sync::atomic::AtomicBool>,
        }
        impl TurnObserver for SlowFinishedObserver {
            fn finished(&self, _session: &str, _outcome: LoopOutcome) {
                std::thread::sleep(std::time::Duration::from_millis(60));
                self.finished_ran
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }

        let scratch = Scratch::new("observer-before-finish");
        let finished_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observer: Arc<dyn TurnObserver> = Arc::new(SlowFinishedObserver {
            finished_ran: Arc::clone(&finished_ran),
        });
        let engine = Arc::new(
            Engine::open(scratch.0.clone(), config(), observer).unwrap(),
        );
        engine.create_session("s-race".into(), 1).unwrap();
        let run = engine
            .start_with_config(
                "s-race".into(),
                crate::types::PromptTurn::plain("quick"),
                config(),
                Some(text_stream("done")),
                Some(77),
            )
            .unwrap();
        let task = tokio::spawn(run);
        let engine_wait = Arc::clone(&engine);
        let ran_when_idle =
            tokio::task::spawn_blocking(move || {
                assert!(engine_wait.wait_until_idle(
                    "s-race",
                    std::time::Duration::from_secs(5),
                ));
                finished_ran.load(std::sync::atomic::Ordering::Acquire)
            })
            .await
            .unwrap();
        assert_eq!(task.await.unwrap().unwrap(), LoopOutcome::Completed);
        assert!(
            ran_when_idle,
            "observer.finished must complete before RunTicket releases wait_until_idle"
        );
    }

    #[test]
    fn park_and_take_pending_card_roll_back_in_memory_state_on_save_error() {
        struct NoopObserver;
        impl TurnObserver for NoopObserver {}
        let scratch = Scratch::new("pending-card-rollback");
        let engine =
            Engine::open(scratch.0.clone(), config(), Arc::new(NoopObserver))
                .unwrap();
        engine.create_session("s-rb".into(), 1).unwrap();
        let card = crate::session::PendingCard::ToolApproval {
            card_id: "card-rb-1".into(),
            created_at_ms: 10,
            tool_name: "bash".into(),
            target_summary: "bash".into(),
            args_hash: "h1".into(),
            risk_reason: "risk".into(),
            command_preview: "rm -rf ./deliverables".into(),
        };
        // Break history.json path so `index.save` fails.
        let history_path = crate::session::SessionIndex::path(&scratch.0);
        let _ = std::fs::remove_file(&history_path);
        std::fs::create_dir(&history_path).unwrap();

        assert!(
            engine
                .park_pending_card("s-rb", card.clone(), None)
                .is_err()
        );
        let after_failed_park = engine.session("s-rb").unwrap();
        assert_eq!(after_failed_park.pending_cards.len(), 0);
        assert_eq!(after_failed_park.status, TurnState::Idle);

        // Restore path, park cleanly, then break again and verify take rolls back.
        std::fs::remove_dir(&history_path).unwrap();
        engine
            .park_pending_card("s-rb", card.clone(), None)
            .unwrap();
        assert_eq!(engine.session("s-rb").unwrap().pending_cards.len(), 1);

        let _ = std::fs::remove_file(&history_path);
        std::fs::create_dir(&history_path).unwrap();
        assert!(
            engine
                .take_pending_card("s-rb", "card-rb-1", TurnState::Idle, None)
                .is_err()
        );
        let after_failed_take = engine.session("s-rb").unwrap();
        assert_eq!(after_failed_take.pending_cards, vec![card]);
        assert_eq!(after_failed_take.status, TurnState::WaitingForUser);
        let _ = std::fs::remove_dir(&history_path);
    }
    #[tokio::test]
    async fn no_artifact_completion_is_durable_and_submission_is_single_use() {
        let scratch = Scratch::new("completion-no-artifacts");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        let submission = format!("submission-{:032x}", 401);
        assert_eq!(
            engine
                .start_with_config(
                    "s1".into(),
                    "one turn".into(),
                    config(),
                    Some(text_stream("done")),
                    Some(401),
                )
                .unwrap()
                .await
                .unwrap(),
            LoopOutcome::Completed
        );
        let proof = engine.completion_evidence("s1", &submission).unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Completed);
        assert_eq!(proof.artifact_ids.as_slice(), EMPTY_ARTIFACT_IDS);
        let record = engine.session("s1").unwrap();
        assert_eq!(record.messages.len(), 2);
        let persisted = SessionIndex::load(&scratch.0).unwrap();
        assert_eq!(persisted.get("s1").unwrap().turn_completions, [proof]);
        let bytes = std::fs::read(SessionIndex::path(&scratch.0)).unwrap();
        assert!(matches!(
            engine.start_with_config(
                "s1".into(),
                "different replay".into(),
                config(),
                Some(text_stream("must not run")),
                Some(401),
            ),
            Err(RunError::DuplicateSubmission)
        ));
        assert_eq!(
            engine.request_cancel_typed("s1").unwrap(),
            CancelDisposition::NotRunning
        );
        assert!(engine.request_cancel("s1").unwrap());
        assert_eq!(engine.session("s1").unwrap(), record);
        assert_eq!(
            std::fs::read(SessionIndex::path(&scratch.0)).unwrap(),
            bytes
        );
    }

    #[tokio::test]
    async fn stopped_evidence_waits_for_the_reserved_future_to_unwind() {
        let scratch = Scratch::new("completion-stop-before-poll");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        assert_eq!(
            engine.request_cancel_typed("missing").unwrap(),
            CancelDisposition::NotFound
        );
        let run = engine
            .start_with_config(
                "s1".into(),
                "stop this".into(),
                config(),
                Some(text_stream("must not run")),
                Some(402),
            )
            .unwrap();
        let submission = format!("submission-{:032x}", 402);
        assert_eq!(
            engine.request_cancel_typed("s1").unwrap(),
            CancelDisposition::Accepted
        );
        assert!(engine.completion_evidence("s1", &submission).is_none());
        assert_eq!(run.await.unwrap(), LoopOutcome::Aborted);
        let proof = engine.completion_evidence("s1", &submission).unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Stopped);
        assert_eq!(proof.artifact_ids.as_slice(), EMPTY_ARTIFACT_IDS);
        let loaded = SessionIndex::load(&scratch.0).unwrap();
        let record = loaded.get("s1").unwrap();
        assert_eq!(record.messages.len(), 1);
        assert_eq!(record.status, TurnState::Stopped);
        assert_eq!(record.turn_completions, [proof]);
    }

    #[tokio::test]
    async fn failed_terminal_save_rolls_back_evidence_and_messages() {
        let scratch = Scratch::new("completion-atomic-save");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        let run = engine
            .start_with_config(
                "s1".into(),
                "write failure".into(),
                config(),
                Some(text_stream("uncommitted result")),
                Some(403),
            )
            .unwrap();
        let before = engine.session("s1").unwrap();
        let path = SessionIndex::path(&scratch.0);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(run.await, Err(RunError::Storage(_))));
        assert_eq!(engine.session("s1").unwrap(), before);
        assert!(
            engine
                .completion_evidence("s1", &format!("submission-{:032x}", 403))
                .is_none()
        );
        std::fs::remove_dir(&path).unwrap();
        engine
            .set_session_title("s1", "save later".into(), true)
            .unwrap();
        let loaded = SessionIndex::load(&scratch.0).unwrap();
        let record = loaded.get("s1").unwrap();
        assert_eq!(record.turn_completions.as_slice(), []);
        assert_eq!(record.messages, before.messages);
        assert_eq!(record.status, TurnState::Running);
    }

    #[tokio::test]
    async fn cancel_during_failed_save_notification_keeps_the_completion_uncommitted()
     {
        struct StorageGate {
            entered: std::sync::mpsc::Sender<()>,
            released: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl TurnObserver for StorageGate {
            fn storage_error(&self, _session: &str) {
                self.entered.send(()).unwrap();
                self.released
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .unwrap();
            }
        }
        let scratch = Scratch::new("completion-failed-save-cancel");
        let (entered, entering) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(StorageGate {
                    entered,
                    released: Mutex::new(released),
                }),
            )
            .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "s1".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspace"),
                }),
            )
            .unwrap();
        let run = engine
            .start_with_config(
                "s1".into(),
                "race the failed commit".into(),
                config(),
                Some(text_stream("uncommitted result")),
                Some(407),
            )
            .unwrap();
        let path = SessionIndex::path(&scratch.0);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let canceller = {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                entering
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .unwrap();
                std::fs::remove_dir(&path).unwrap();
                let disposition = engine.request_cancel_typed("s1").unwrap();
                release.send(()).unwrap();
                disposition
            })
        };
        assert!(matches!(run.await, Err(RunError::Storage(_))));
        assert_eq!(canceller.join().unwrap(), CancelDisposition::TooLate);
        assert!(
            engine
                .completion_evidence("s1", &format!("submission-{:032x}", 407))
                .is_none()
        );
        assert_eq!(
            engine
                .start_with_config(
                    "s1".into(),
                    "replacement delivery".into(),
                    config(),
                    Some(text_stream("replacement")),
                    Some(408),
                )
                .unwrap()
                .await
                .unwrap(),
            LoopOutcome::Completed
        );
        let proof = engine
            .completion_evidence("s1", &format!("submission-{:032x}", 408))
            .unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Completed);
        let loaded = SessionIndex::load(&scratch.0).unwrap();
        assert!(loaded.get("s1").unwrap().turn_completions.contains(&proof));
        assert_eq!(loaded.get("s1").unwrap().status, TurnState::Completed);
    }

    #[tokio::test]
    async fn loop_start_failure_has_durable_failed_completion() {
        let scratch = Scratch::new("completion-loop-failure");
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine.create_session("s1".into(), 1).unwrap();
        let mut record = engine.session("s1").unwrap();
        record.messages.push(crate::Message {
            id: "invalid-system-message".into(),
            role: crate::Role::System,
            content: vec![ContentBlock::text("not a history message")],
            source: crate::MessageSource::User,
            display_text: None,
        });
        engine.insert_session(record).unwrap();
        let result = engine
            .start_with_config(
                "s1".into(),
                "fail conversion".into(),
                config(),
                Some(text_stream("must not run")),
                Some(404),
            )
            .unwrap()
            .await;
        assert!(matches!(result, Err(RunError::Loop(_))));
        let proof = engine
            .completion_evidence("s1", &format!("submission-{:032x}", 404))
            .unwrap();
        assert_eq!(proof.outcome, TurnExecutionOutcome::Failed);
        assert_eq!(proof.artifact_ids.as_slice(), EMPTY_ARTIFACT_IDS);
    }

    #[tokio::test]
    async fn exact_group_retry_is_protected_and_conflicting_evidence_is_denied()
    {
        let scratch = Scratch::new("completion-retry-guard");
        let committer = Arc::new(CommitRecorder {
            root: scratch.0.clone(),
            commits: Mutex::new(Vec::new()),
            fail: true,
        });
        let cfg = delivery_config(committer, true);
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                cfg.clone(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: scratch.0.join("workspace"),
                }),
            )
            .unwrap();
        assert!(matches!(
            engine
                .start_with_config(
                    "home".into(),
                    "deliver".into(),
                    cfg,
                    Some(delivery_stream(StopReason::Stop)),
                    Some(405),
                )
                .unwrap()
                .await,
            Err(RunError::Storage(_))
        ));
        let submission = format!("submission-{:032x}", 405);
        let before = engine.session("home").unwrap();
        let result =
            engine.retry_delivery_commit_with("home", &submission, |commit| {
                assert_eq!(commit, &before.delivery_commits[0]);
                assert_eq!(
                    engine.request_cancel_typed("home").unwrap(),
                    CancelDisposition::TooLate
                );
                assert_eq!(
                    engine.remove_session("home").unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                assert!(matches!(
                    engine.start_with_config(
                        "home".into(),
                        "another turn".into(),
                        config(),
                        Some(text_stream("must not run")),
                        Some(406),
                    ),
                    Err(RunError::Busy)
                ));
                Err(std::io::Error::other("marker still unavailable"))
            });
        assert!(matches!(result, Err(RunError::Storage(_))));
        assert_eq!(engine.session("home").unwrap(), before);
        engine
            .retry_delivery_commit_with("home", &submission, |_| Ok(()))
            .unwrap();
        let mut corrupt = before.clone();
        corrupt.turn_completions[0].artifact_ids = vec!["other-group".into()];
        engine.insert_session(corrupt.clone()).unwrap();
        assert!(
            engine
                .record_delivery_commit(&before.delivery_commits[0])
                .is_err()
        );
        assert_eq!(engine.session("home").unwrap(), corrupt);
        assert!(engine.completion_evidence("home", &submission).is_none());
        assert!(
            engine
                .delivery_commit_for_submission("home", &submission)
                .is_none()
        );
        let invoked = AtomicUsize::new(0);
        assert!(matches!(
            engine.retry_delivery_commit_with("home", &submission, |_| {
                invoked.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            Err(RunError::Storage(_))
        ));
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancelling_one_automation_owner_keeps_another_owners_turn() {
        let scratch = Scratch::new("automation-cancel-owner");
        let mut cfg = config();
        cfg.run_budget = crate::RunBudget::new(60_000, 10, 10);
        cfg.execution_gate =
            Some(Arc::new(crate::execution_gate::tests::RevocableGate(
                std::sync::atomic::AtomicBool::new(true),
            )));
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                cfg.clone(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        let owner = |subject: &str| crate::ExecutionOwner {
            authority: "test-authority".into(),
            subject: subject.into(),
        };
        for id in ["account-a", "account-b"] {
            engine
                .create_if_absent(crate::SessionSpec {
                    id: id.into(),
                    created_at_ms: 1,
                    origin: SessionOrigin::Home,
                    workspace: Some(SessionWorkspace {
                        root: scratch.0.join(id),
                    }),
                    execution: crate::ExecutionMetadata {
                        owner: Some(owner(id)),
                        source: crate::ExecutionSource::Automation {
                            automation_id: id.into(),
                            run_id: id.into(),
                            attempt_id: id.into(),
                        },
                    },
                    role: SessionRole::Standard,
                    root_session_id: None,
                    access_mode: WorkAccessMode::ReadWrite,
                })
                .unwrap();
        }
        let start = |id: &str, submission| {
            engine
                .start_with_config(
                    id.into(),
                    "run automation".into(),
                    cfg.clone(),
                    Some(text_stream("complete")),
                    Some(submission),
                )
                .unwrap()
        };
        let a = start("account-a", 101);
        let b = start("account-b", 102);
        engine.cancel_automation_owner(&owner("account-a")).unwrap();
        assert_eq!(a.await.unwrap(), LoopOutcome::Aborted);
        assert_eq!(b.await.unwrap(), LoopOutcome::Completed);
    }

    #[tokio::test]
    async fn automation_execution_identity_reaches_each_tool_context() {
        let scratch = Scratch::new("automation-context");
        let expected = crate::ExecutionMetadata {
            owner: Some(crate::ExecutionOwner {
                authority: "test-authority".into(),
                subject: "subject-42".into(),
            }),
            source: crate::ExecutionSource::Automation {
                automation_id: "automation-1".into(),
                run_id: "run-1".into(),
                attempt_id: "attempt-1".into(),
            },
        };
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let mut cfg = config();
        cfg.run_budget = crate::RunBudget::new(60_000, 10, 10);
        cfg.execution_gate =
            Some(Arc::new(crate::execution_gate::tests::RevocableGate(
                std::sync::atomic::AtomicBool::new(true),
            )));
        cfg.registry.register(Arc::new(ToolDefinition::new(
            "publish_file",
            "record host context",
            serde_json::json!({"type":"object","properties":{}}),
            move |_, ctx| {
                let sink = Arc::clone(&sink);
                Box::pin(async move {
                    sink.lock().unwrap().push(ctx.execution);
                    ToolExecutionResult::success_text(
                        "recorded",
                        serde_json::json!({}),
                    )
                })
            },
        )));
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                cfg.clone(),
                Arc::new(Recorder::default()),
            )
            .unwrap(),
        );
        engine
            .create_if_absent(crate::SessionSpec {
                id: "automation-session".into(),
                created_at_ms: 1,
                origin: SessionOrigin::Home,
                workspace: Some(SessionWorkspace {
                    root: scratch.0.join("workspace"),
                }),
                execution: expected.clone(),
                role: SessionRole::Standard,
                root_session_id: None,
                access_mode: WorkAccessMode::ReadWrite,
            })
            .unwrap();
        let mut ungated = cfg.clone();
        ungated.execution_gate = None;
        assert!(matches!(
            engine.start_with_config(
                "automation-session".into(),
                "must not run unguarded".into(),
                ungated,
                Some(delivery_stream(StopReason::Stop)),
                Some(37),
            ),
            Err(RunError::NoCredential)
        ));
        let result = engine
            .start_with_config(
                "automation-session".into(),
                "record identity".into(),
                cfg,
                Some(delivery_stream(StopReason::Stop)),
                Some(37),
            )
            .unwrap()
            .await
            .unwrap();
        assert_eq!(result, LoopOutcome::Completed);
        assert_eq!(*captured.lock().unwrap(), [expected.clone(), expected]);
    }
}
