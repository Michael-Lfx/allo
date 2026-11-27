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
    LoopOutcome, ModelRoute, PreToolHook, TurnRequest, run_turn,
};
use crate::run_fence::{PublishToken, RunFence, RunTicket};
use crate::session::{
    DeliveryCommit, DeliveryCommitter, DispatchRecord, ObjectVerifyState,
    PendingCard, PendingRouterFollowup, SessionIndex, SessionOrigin,
    SessionRecord, SessionRole, SessionWorkspace, StopHandoffManifest,
    SubmissionOutcome, TrackedObject, TurnState, WorkAccessMode, WorkContract,
};
use crate::tools::{
    ActivatedToolSet, RegistryTool, TOOL_SEARCH_NAME, ToolExposure,
    ToolRegistry, format_deferred_tool_catalog, tool_search_definition,
};
use crate::types::{
    ArtifactReceipt, CancelToken, ContentBlock, Message, MessageSource,
    PromptTurn, Role, TodoSink, ToolRunContext,
};

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
    /// Deferred tool names pre-activated at Step 1 of this turn (used e.g. when
    /// resuming after `PendingCard::ToolApproval`).
    pub pre_activated_tools: Vec<String>,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("route", &self.route)
            .field("has_key", &self.api_key.is_some())
            .field("system_len", &self.system.len())
            .field("tools", &self.registry.list().len())
            .field("pre_activated_tools", &self.pre_activated_tools.len())
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

/// Lightweight summary of a durable Worker child session for cascade deletion
/// without cloning full `messages` or event histories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildDeleteSummary {
    pub id: String,
    pub is_busy: bool,
    pub artifact_ids: Vec<String>,
    pub submission_ids: Vec<String>,
}

#[derive(Debug)]
pub enum RunError {
    Busy,
    NotFound(String),
    NoCredential,
    Storage(std::io::Error),
    Loop(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "session already has an active run"),
            Self::NotFound(id) => write!(f, "session {id} not found"),
            Self::NoCredential => {
                write!(f, "no account credential for this run")
            }
            Self::Storage(error) => {
                write!(f, "could not save the submission: {error}")
            }
            Self::Loop(message) => write!(f, "{message}"),
        }
    }
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
    origin: SessionOrigin,
    output_root: Option<PathBuf>,
}

/// Owns the durable index and the per-session run fences.
pub struct Engine {
    root: PathBuf,
    inner: Mutex<Inner>,
    observer: Arc<dyn TurnObserver>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("root", &self.root)
            .field("sessions", &self.lock_inner().index.sessions.len())
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
            inner: Mutex::new(Inner {
                index,
                config,
                fences: HashMap::new(),
                finalizing: HashSet::new(),
            }),
            observer,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_config(&self, config: EngineConfig) {
        self.lock_inner().config = config;
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

    #[must_use]
    pub fn active(&self) -> Option<String> {
        self.lock_inner().index.active.clone()
    }

    #[must_use]
    pub fn submitted_session(&self, submission: u128) -> Option<String> {
        let message_id = format!("submission-{submission:032x}");
        self.lock_inner().index.sessions.iter().find_map(|record| {
            let in_history = record.messages.iter().any(|message| {
                message.role == Role::User && message.id == message_id
            });
            let in_followups = record
                .pending_router_followups
                .iter()
                .any(|item| item.submission_id == message_id);
            (in_history || in_followups).then(|| record.id.clone())
        })
    }

    pub fn set_active(&self, id: Option<String>) -> std::io::Result<()> {
        let mut inner = self.lock_inner();
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

    /// Finds the first durable Worker session owned by `root_id` (in index
    /// order) whose `WorkContract` tracks `object_id`, returning `(child_id,
    /// tracked_object, confirmed_facts)` without cloning sibling `messages` or
    /// histories.
    #[must_use]
    #[expect(
        clippy::type_complexity,
        reason = "preserves the explicit 3-tuple child object projection signature for callers"
    )]
    pub fn find_child_tracked_object(
        &self,
        root_id: &str,
        object_id: &str,
    ) -> Option<(String, TrackedObject, Vec<(String, String)>)> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let found = inner
            .index
            .sessions
            .iter()
            .filter(|rec| {
                rec.role == SessionRole::Worker
                    && rec.root_session_id.as_deref() == Some(root_id)
            })
            .find_map(|child| {
                let wc = child.work_contract.as_ref()?;
                let obj = wc.find_object(object_id)?;
                let facts = wc
                    .confirmed_facts_for(object_id)
                    .into_iter()
                    .collect::<Vec<_>>();
                Some((child.id.clone(), obj.clone(), facts))
            });
        drop(inner);
        found
    }

    /// Returns lightweight deletion summaries for all durable Worker sessions
    /// owned by `root_id` without cloning full `messages` or event vectors.
    #[must_use]
    pub fn child_delete_summaries(
        &self,
        root_id: &str,
    ) -> Vec<ChildDeleteSummary> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let summaries = inner
            .index
            .sessions
            .iter()
            .filter(|rec| {
                rec.role == SessionRole::Worker
                    && rec.root_session_id.as_deref() == Some(root_id)
            })
            .map(|child| {
                let mut artifact_ids = Vec::new();
                let mut submission_ids =
                    Vec::with_capacity(child.delivery_commits.len());
                for commit in &child.delivery_commits {
                    artifact_ids.extend(commit.artifact_ids.iter().cloned());
                    submission_ids.push(commit.submission_id.clone());
                }
                ChildDeleteSummary {
                    id: child.id.clone(),
                    is_busy: child.status.is_busy(),
                    artifact_ids,
                    submission_ids,
                }
            })
            .collect();
        drop(inner);
        summaries
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

    /// Persist origin/workspace metadata on an existing session.
    pub fn set_session_origin_workspace(
        &self,
        id: &str,
        origin: SessionOrigin,
        workspace: Option<SessionWorkspace>,
    ) -> std::io::Result<()> {
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
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

    /// Persist one host-derived delivery commit. Repeated calls with the same
    /// commit are idempotent; a different commit for the same submission
    /// replaces the previous receipt set.
    pub fn record_delivery_commit(
        &self,
        commit: &DeliveryCommit,
    ) -> std::io::Result<()> {
        let mut inner = self.lock_inner();
        let previous = inner
            .index
            .get(&commit.session_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("session {} not found", commit.session_id),
                )
            })?
            .delivery_commits
            .clone();
        let saved = {
            let record = inner
                .index
                .get_mut(&commit.session_id)
                .expect("session still exists under the engine lock");
            if record
                .delivery_commits
                .iter()
                .any(|existing| existing == commit)
            {
                return Ok(());
            }
            record.delivery_commits.retain(|existing| {
                existing.submission_id != commit.submission_id
            });
            record.delivery_commits.push(commit.clone());
            inner.index.save(&self.root)
        };
        if let Err(error) = saved {
            if let Some(record) = inner.index.get_mut(&commit.session_id) {
                record.delivery_commits = previous;
            }
            return Err(error);
        }
        drop(inner);
        Ok(())
    }

    pub fn remove_session(&self, id: &str) -> std::io::Result<bool> {
        let mut inner = self.lock_inner();
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
        let fence = self.lock_inner().fences.get(id).cloned();
        fence.is_some_and(|fence| fence.is_cancelling())
    }

    /// True while session `id` holds an active run fence (either running or
    /// cancelling/reaping child tools).
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        let fence = self.lock_inner().fences.get(id).cloned();
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

    /// Wait up to `timeout` for session `id`'s active/cancelling run to finish
    /// reaping and release its fence. Returns `true` if idle.
    #[must_use]
    pub fn wait_until_idle(&self, id: &str, timeout: Duration) -> bool {
        let fence = self.lock_inner().fences.get(id).cloned();
        fence.is_none_or(|fence| fence.wait_until_idle(timeout))
    }

    /// Request cancellation before a successful delivery history commit.
    /// `false` means the delivery crossed its durable linearization point and
    /// is completing its independent store marker; callers must keep the
    /// saving state visible until the terminal notification arrives.
    pub fn request_cancel(&self, id: &str) -> std::io::Result<bool> {
        let mut inner = self.lock_inner();
        if inner.finalizing.contains(id) {
            return Ok(false);
        }
        let fence_active =
            inner.fences.get(id).is_some_and(|fence| fence.is_active());
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
        inner.index.save(&self.root).map(|()| true)
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
            let mut inner = self.lock_inner();
            let config = config.unwrap_or_else(|| inner.config.clone());
            if needs_credential && config.api_key.is_none() {
                return Err(RunError::NoCredential);
            }
            if inner.index.get(session_id).is_none() {
                return Err(RunError::NotFound(session_id.to_owned()));
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
            let record =
                inner.index.get_mut(session_id).expect("record exists");
            let history = record.messages.clone();
            let origin = record.origin.clone();
            let submission_id =
                submission.map(|id| format!("submission-{id:032x}"));
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
            );
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
                id: submission.map_or_else(
                    || format!("u-{now}"),
                    |id| format!("submission-{id:032x}"),
                ),
                role: Role::User,
                content: vec![crate::types::ContentBlock::text(&prompt.model)],
                source: prompt.source.clone(),
                display_text: prompt.display.clone(),
                dispatch_receipt_id: prompt.dispatch_receipt_id.clone(),
            });
            record.status = TurnState::Running;
            record.updated_at = now;
            let saved = inner.index.save(&self.root);
            if submission.is_some()
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
                origin,
                output_root,
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
        prepared: PreparedTurn,
        stream: Option<StreamFn>,
    ) -> Result<LoopOutcome, RunError> {
        let PreparedTurn {
            ticket,
            history,
            config,
            submission_id,
            origin,
            output_root,
        } = prepared;
        let cancel_trace = Arc::new(Mutex::new(CancelTrace::default()));
        if !ticket.is_current() {
            self.settle_cancelled_run(
                session_id,
                prompt,
                &cancel_trace,
                &ticket,
            );
            return Ok(LoopOutcome::Aborted);
        }
        let stream = stream.unwrap_or_else(|| {
            crate::transport::openai_stream_fn(
                &config.route,
                config.api_key.clone(),
            )
        });
        let delivery_committer = config.delivery_committer.clone();
        let artifact_receipts = Arc::new(Mutex::new(Vec::new()));
        let prepared_tools = prepare_turn_tools(
            &config,
            session_id,
            ticket.signal().clone(),
            submission_id.clone(),
            origin,
            output_root,
            Arc::clone(&artifact_receipts),
        );
        tracing::debug!(
            target: "flowy.prompt",
            stage = "Engine::run_prepared",
            session = %session_id,
            history = history.len(),
            tools = prepared_tools.eager_tools.len(),
            deferred_tools = prepared_tools.deferred_tools.len(),
            model = %config.route.model,
            base_url = %config.route.base_url,
            "→ run_turn"
        );
        let request = TurnRequest {
            session_id: Some(session_id.to_owned()),
            system: prepared_tools.system,
            history,
            prompt: prompt.to_owned(),
            route: config.route,
            tools: prepared_tools.eager_tools,
            deferred_tools: prepared_tools.deferred_tools,
            activated_tools: prepared_tools.activated_tools,
            read_only_tools: prepared_tools.read_only_tools,
            signal: ticket.signal().clone(),
            pre_tool_hook: config.pre_tool_hook,
        };
        let emitter = Arc::new(EngineEmitter {
            session: session_id.to_owned(),
            observer: Arc::clone(&self.observer),
            publisher: ticket.publisher(),
            trace: Some(Arc::clone(&cancel_trace)),
        });
        let result = run_turn(request, stream, emitter).await;
        let stored = {
            let mut inner = self.lock_inner();
            if !ticket.is_current() {
                drop(inner);
                self.settle_cancelled_run(
                    session_id,
                    prompt,
                    &cancel_trace,
                    &ticket,
                );
                return Ok(LoopOutcome::Aborted);
            }
            let Some(record) = inner.index.get_mut(session_id) else {
                return Ok(LoopOutcome::Aborted);
            };
            let mut before = record.clone();
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
                            &artifact_receipts.lock().unwrap_or_else(
                                std::sync::PoisonError::into_inner,
                            ),
                            session_id,
                            submission_id.as_deref(),
                        );
                    }
                    let mut new_messages = result.messages;
                    // The loop echoes the prompt back; the engine already
                    // stored it, so drop the duplicate.
                    if new_messages.first().is_some_and(|message| {
                        message.role == Role::User && message.text() == prompt
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
            if let Err(error) = inner.index.save(&self.root) {
                // A later successful save must never resurrect this turn's
                // uncommitted receipts or its DeliveryCommit.
                before.status = TurnState::Error {
                    message: "could not save turn history".to_owned(),
                };
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
    })
}

struct PreparedTurnTools {
    eager_tools: Vec<Arc<dyn rpi_agent::AgentTool>>,
    deferred_tools: Vec<Arc<dyn rpi_agent::AgentTool>>,
    activated_tools: Option<ActivatedToolSet>,
    read_only_tools: HashSet<String>,
    system: String,
}

fn prepare_turn_tools(
    config: &EngineConfig,
    session_id: &str,
    signal: CancelToken,
    submission_id: Option<String>,
    session_origin: SessionOrigin,
    output_root: Option<PathBuf>,
    artifact_receipts: Arc<Mutex<Vec<ArtifactReceipt>>>,
) -> PreparedTurnTools {
    let run_ctx = ToolRunContext {
        signal,
        agent_id: Some(session_id.to_owned()),
        cwd: config.cwd.clone(),
        todos: config.todos.clone(),
        submission_id,
        session_origin,
        output_root,
        artifact_delivery_available: config.delivery_committer.is_some(),
    };
    if !config.registry.has_deferred() {
        let eager_tools = config
            .registry
            .definitions()
            .into_iter()
            .filter_map(|definition| {
                RegistryTool::new_with_artifact_sink(
                    config.registry.clone(),
                    definition,
                    run_ctx.clone(),
                    Some(Arc::clone(&artifact_receipts)),
                )
                .ok()
                .map(|tool| -> Arc<dyn rpi_agent::AgentTool> { Arc::new(tool) })
            })
            .collect();
        return PreparedTurnTools {
            eager_tools,
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: config.registry.read_only_tool_names(),
            system: config.system.clone(),
        };
    }

    let turn_registry = config.registry.fork();
    let activated =
        ActivatedToolSet::new(config.pre_activated_tools.iter().cloned());
    if turn_registry.get(TOOL_SEARCH_NAME).is_none() {
        turn_registry.register(tool_search_definition(
            &turn_registry,
            activated.clone(),
        ));
    }

    let mut eager_tools = Vec::new();
    let mut deferred_tools = Vec::new();
    for definition in turn_registry.definitions() {
        let is_deferred = definition.exposure() == ToolExposure::Deferred
            && !activated.contains(&definition.name);
        let Ok(tool) = RegistryTool::new_with_artifact_sink(
            turn_registry.clone(),
            definition,
            run_ctx.clone(),
            Some(Arc::clone(&artifact_receipts)),
        ) else {
            continue;
        };
        let boxed: Arc<dyn rpi_agent::AgentTool> = Arc::new(tool);
        if is_deferred {
            deferred_tools.push(boxed);
        } else {
            eager_tools.push(boxed);
        }
    }

    let mut system = config.system.clone();
    if let Some(catalog) = format_deferred_tool_catalog(&turn_registry)
        && !system.contains("[DeferredToolsAvailable]")
    {
        if !system.trim().is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(&catalog);
    }

    PreparedTurnTools {
        eager_tools,
        deferred_tools,
        activated_tools: Some(activated),
        read_only_tools: turn_registry.read_only_tool_names(),
        system,
    }
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
#[path = "engine_tests.rs"]
mod tests;
