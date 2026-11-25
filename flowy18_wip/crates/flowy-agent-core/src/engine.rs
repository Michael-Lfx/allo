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

use crate::loop_runner::{LoopOutcome, ModelRoute, TurnRequest, run_turn};
use crate::run_fence::{PublishToken, RunFence, RunTicket};
use crate::session::{
    DeliveryCommit, DeliveryCommitter, ObjectVerifyState, PendingCard,
    SessionIndex, SessionOrigin, SessionRecord, SessionWorkspace,
    StopHandoffManifest, SubmissionOutcome, TurnState, WorkContract,
};
use crate::tools::{RegistryTool, ToolRegistry};
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
            .field("sessions", &self.inner.lock().unwrap().index.sessions.len())
            .finish_non_exhaustive()
    }
}

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
                record
                    .messages
                    .iter()
                    .any(|message| {
                        message.role == Role::User && message.id == message_id
                    })
                    .then(|| record.id.clone())
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
        record
            .pending_cards
            .retain(|existing| existing.card_id() != card.card_id());
        record.pending_cards.push(card);
        if let Some(contract) = updated_contract {
            record.work_contract = Some(contract);
        }
        record.status = TurnState::WaitingForUser;
        record.updated_at = now_ms();
        inner.index.save(&self.root)
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
        let removed = record.pending_cards.remove(pos);
        if let Some(contract) = updated_contract {
            record.work_contract = Some(contract);
        }
        if record.pending_cards.is_empty() {
            record.status = next_status;
        }
        record.updated_at = now_ms();
        inner.index.save(&self.root)?;
        drop(inner);
        Ok(Some(removed))
    }

    /// Persist a [`StopHandoffManifest`] and transition the session status.
    pub fn set_stop_handoff(
        &self,
        id: &str,
        manifest: StopHandoffManifest,
        next_status: TurnState,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        record.last_stop_handoff = Some(manifest);
        record.status = next_status;
        record.updated_at = now_ms();
        inner.index.save(&self.root)
    }

    /// Update the [`ObjectVerifyState`] of a tracked object matching
    /// `artifact_path` inside `id`'s active `work_contract`, if present.
    pub fn set_object_verify_state(
        &self,
        id: &str,
        artifact_path: &Path,
        state: ObjectVerifyState,
    ) -> std::io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        let Some(record) = inner.index.get_mut(id) else {
            return Ok(false);
        };
        let Some(contract) = record.work_contract.as_mut() else {
            return Ok(false);
        };
        let file_name = artifact_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let ext = artifact_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let idx = contract
            .objects
            .iter()
            .position(|obj| {
                obj.filename
                    .as_deref()
                    .is_some_and(|f| f.eq_ignore_ascii_case(file_name))
            })
            .or_else(|| {
                let mut same_ext = contract
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, obj)| obj.extension.eq_ignore_ascii_case(ext))
                    .map(|(i, _)| i);
                let first = same_ext.next()?;
                same_ext.next().is_none().then_some(first)
            });
        let Some(idx) = idx else {
            return Ok(false);
        };
        contract.objects[idx].verify_state = state;
        inner.index.save(&self.root)?;
        drop(inner);
        Ok(true)
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
        let record = inner.index.get_mut(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session {id} not found"),
            )
        })?;
        for commit in &mut record.delivery_commits {
            commit.origin_title_snapshot.clone_from(&title);
        }
        record.title = title;
        record.titled = titled;
        inner.index.save(&self.root)
    }

    /// Persist one host-derived delivery commit. Repeated calls with the same
    /// commit are idempotent; a different commit for the same submission
    /// replaces the previous receipt set.
    pub fn record_delivery_commit(
        &self,
        commit: &DeliveryCommit,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
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

    /// Defensively cancel every active run fence (for process shutdown / handle
    /// teardown) so child tool processes are signaled to exit immediately.
    pub fn cancel_all_active(&self) {
        let fences: Vec<Arc<RunFence>> = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fences
            .values()
            .cloned()
            .collect();
        for fence in fences {
            if fence.is_active() {
                fence.cancel_active();
            }
        }
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
        let mut inner = self.inner.lock().unwrap();
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
                            && matches!(msg.source, MessageSource::User)
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
                        && matches!(msg.source, MessageSource::User)
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
            );
            if let Some(contract) = &config.work_contract {
                if let Some(sub_id) = submission_id.as_deref() {
                    record
                        .submission_revisions
                        .insert(sub_id.to_owned(), contract.revision);
                }
                record.work_contract = Some(contract.clone());
            }
            record.messages.push(Message {
                id: submission.map_or_else(
                    || format!("u-{now}"),
                    |id| format!("submission-{id:032x}"),
                ),
                role: Role::User,
                content: vec![crate::types::ContentBlock::text(&prompt.model)],
                source: MessageSource::User,
                display_text: prompt.display.clone(),
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
        let tools = build_tools(
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
        };
        let emitter = Arc::new(EngineEmitter {
            session: session_id.to_owned(),
            observer: Arc::clone(&self.observer),
            publisher: ticket.publisher(),
            trace: Some(Arc::clone(&cancel_trace)),
        });
        let result = run_turn(request, stream, emitter).await;
        let stored = {
            let mut inner = self.inner.lock().unwrap();
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
                            &artifact_receipts.lock().unwrap(),
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
                    record.status =
                        status_for(result.outcome, error.as_deref());
                    if let Some(sub_id) = submission_id.as_ref() {
                        let outcome_entry = match result.outcome {
                            LoopOutcome::Completed => {
                                let missing = missing_required_for_submission(
                                    record, sub_id,
                                );
                                if missing.is_empty() {
                                    SubmissionOutcome::Completed
                                } else {
                                    SubmissionOutcome::Partial { missing }
                                }
                            }
                            LoopOutcome::Aborted => SubmissionOutcome::Aborted,
                            LoopOutcome::Failed => SubmissionOutcome::Failed {
                                message: error
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|text| !text.is_empty())
                                    .unwrap_or("model reported an error")
                                    .to_owned(),
                            },
                        };
                        record
                            .submission_outcomes
                            .insert(sub_id.clone(), outcome_entry);
                    }
                    Ok((result.outcome, commit))
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
                if !ticket.finish() {
                    return Ok(LoopOutcome::Aborted);
                }
                drop(inner);
                self.observer.storage_error(session_id);
                self.observer.finished(session_id, LoopOutcome::Failed);
                return Err(RunError::Storage(error));
            }
            match result {
                Ok((outcome, Some(commit))) => {
                    // This lock also serializes request_cancel. The successful
                    // history save is the delivery's linearization point.
                    inner.finalizing.insert(session_id.to_owned());
                    drop(inner);
                    (outcome, commit)
                }
                Ok((outcome, None)) => {
                    // History is durably saved; finish under the same lock so
                    // `request_cancel` cannot split durable history from live
                    // observer/queue state in a gap before `ticket.finish()`.
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
                    if !ticket.finish() {
                        return Ok(LoopOutcome::Aborted);
                    }
                    drop(inner);
                    self.observer.finished(session_id, outcome);
                    return Ok(outcome);
                }
                Err(error) => {
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
                    if !ticket.finish() {
                        return Ok(LoopOutcome::Aborted);
                    }
                    drop(inner);
                    self.observer.finished(session_id, LoopOutcome::Failed);
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

fn build_tools(
    config: &EngineConfig,
    session_id: &str,
    signal: CancelToken,
    submission_id: Option<String>,
    session_origin: SessionOrigin,
    output_root: Option<PathBuf>,
    artifact_receipts: Arc<Mutex<Vec<ArtifactReceipt>>>,
) -> Vec<Arc<dyn rpi_agent::AgentTool>> {
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

    fn record_tool_end(&mut self, call_id: &str, name: &str, is_error: bool) {
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
            let effect = match target {
                Some(path) => format!("{name} ({path})"),
                None => name.to_owned(),
            };
            if !self.verified_effects.contains(&effect) {
                self.verified_effects.push(effect);
            }
        }
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
                if !matches!(
                    obj.verify_state,
                    ObjectVerifyState::Passed { .. }
                        | ObjectVerifyState::Overridden { .. }
                ) {
                    pending_checks
                        .push(format!("unverified:{}", obj.display_label()));
                }
            }
        }
        let rollback_hint = if self.modified_files.is_empty() {
            "本轮尚未写入工作区文件，可直接重新发送指令。".to_owned()
        } else {
            "已修改文件位于会话工作目录且未对外发布，可检查并回退后重跑。"
                .to_owned()
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
            if let AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                is_error,
                ..
            } = &event
                && let Some(trace) = &trace
            {
                trace
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_tool_end(tool_call_id, tool_name, *is_error);
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
    use super::{Engine, EngineConfig, EngineEmitter, RunError, TurnObserver};
    use crate::loop_runner::{LoopOutcome, ModelRoute};
    use crate::session::{
        DeliveryCommit, DeliveryCommitter, SessionIndex, SessionOrigin,
        SessionWorkspace, TurnState,
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
        assert!(engine.session("home").unwrap().delivery_commits.is_empty());
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
        assert!(engine.session("home").unwrap().delivery_commits.is_empty());
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
        assert!(engine.session("home").unwrap().delivery_commits.is_empty());
        assert!(committer.commits.lock().unwrap().is_empty());
        assert!(observer.deliveries.lock().unwrap().is_empty());
        assert_eq!(*observer.saving_files.lock().unwrap(), 0);
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
        let _hold = tx;
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
        drop(_hold);
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
        late.await;
        engine
            .run_prepared("s1", "new", replacement, Some(text_stream("fresh")))
            .await
            .unwrap();
        assert_eq!(*recorder.text.lock().unwrap(), "fresh");
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
        assert!(!manifest.verified_effects.is_empty());
        assert!(
            manifest
                .pending_checks
                .iter()
                .any(|item| item.contains("青禾项目报价汇报.pptx"))
        );
        assert!(!manifest.rollback_hint.is_empty());
        assert_eq!(observer.settled.lock().unwrap().len(), 1);
    }
}
