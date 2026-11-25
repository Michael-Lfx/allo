//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    DeliveryCommit, Engine, EngineConfig, LoopOutcome, ModelRoute, PromptTurn,
    RunError, SessionOrigin, SessionRecord, SessionWorkspace, TurnObserver,
    TurnState, complete_text, echo_stream_fn, log_preview,
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
};
use crate::title::{self, TITLE_MAX_CHARS};

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
    Skills,
    Artifacts,
    Session {
        id: String,
    },
    /// The session index could not be loaded or saved.
    StorageError,
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

struct UiState {
    root: PathBuf,
    live: Mutex<HashMap<String, Live>>,
    /// File-tool arguments held between `tool_start` and `tool_end`, keyed by
    /// session and call id, so the change card never re-reads the file.
    tool_args: Mutex<HashMap<String, serde_json::Value>>,
    active: Mutex<Option<String>>,
    workspace_for_new_sessions: Mutex<Option<AgentWorkspace>>,
    origin_for_new_sessions: Mutex<SessionOrigin>,
    artifact_store: OnceLock<ArtifactStore>,
    initializing: Mutex<bool>,
    context_generation: AtomicU64,
    updates: Mutex<VecDeque<AgentUpdate>>,
    load_failed: Mutex<bool>,
    history_unavailable: Mutex<bool>,
    dispatch: Mutex<()>,
    index_write: Mutex<()>,
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
            workspace_for_new_sessions: Mutex::new(None),
            origin_for_new_sessions: Mutex::new(SessionOrigin::Unknown),
            artifact_store: store_slot,
            initializing: Mutex::new(true),
            context_generation: AtomicU64::new(0),
            updates: Mutex::new(VecDeque::new()),
            load_failed: Mutex::new(false),
            history_unavailable: Mutex::new(false),
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

struct Harness {
    runtime: tokio::runtime::Runtime,
    state: Arc<UiState>,
    engine: Mutex<Option<Arc<Engine>>>,
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
        let state = Arc::new(UiState::new(store_root, None));
        Self {
            inner: Arc::new(Harness {
                runtime,
                state: Arc::clone(&state),
                engine: Mutex::new(None),
                config: Mutex::new(config),
                skills: Mutex::new(Vec::new()),
            }),
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
        self.boot(config);
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
        lock(&self.inner.skills).clone()
    }

    fn boot(&self, config: LlmConfig) {
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
                    return;
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
                return;
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
                    return;
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
                        return;
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
                return;
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
        *lock(&self.inner.engine) = Some(engine);
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::boot",
            sessions = lock(&state.live).len(),
            "done"
        );
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
            return Err("notice.storage_error");
        }
        let Some(engine) = lock(&self.inner.engine).clone() else {
            return Err("notice.agent_failed");
        };
        let cwd = ensure_tool_cwd(&self.inner.state.root);
        engine.set_config(engine_config(&config, &cwd));
        *lock(&self.inner.config) = config;
        Ok(())
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

    /// The host-side artifact catalog API. A missing store means startup
    /// storage initialization failed; callers should surface a storage error.
    pub fn artifact_store(&self) -> Result<ArtifactStore, &'static str> {
        self.inner
            .state
            .artifact_store
            .get()
            .cloned()
            .ok_or("notice.storage_error")
    }

    /// A confirmed source deletion is separate from a temporarily missing
    /// history record. Store reads happen on the caller's background executor.
    pub fn source_deleted(&self, session_id: &str) -> bool {
        if *lock(&self.inner.state.load_failed) {
            return false;
        }
        if lock(&self.inner.engine)
            .as_ref()
            .is_some_and(|engine| engine.session(session_id).is_some())
        {
            return false;
        }
        self.inner
            .state
            .artifact_store
            .get()
            .and_then(|store| store.source_deleted(session_id).ok())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn source_available(&self, session_id: &str) -> bool {
        if *lock(&self.inner.state.load_failed) {
            return false;
        }
        let record = lock(&self.inner.engine)
            .as_ref()
            .and_then(|engine| engine.session(session_id));
        record.is_some_and(|record| {
            !*lock(&self.inner.state.history_unavailable)
                || record.origin != SessionOrigin::Unknown
        })
    }

    /// Remove a conversation and every artifact delivered by it. A durable
    /// session-deletion plan spans the history/index commit and idempotent
    /// artifact cleanup so a failed cleanup cannot resurrect or orphan files.
    pub fn delete_session(&self, session_id: &str) -> Result<(), &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine.session(session_id).ok_or("notice.agent_failed")?;
        if record.status.is_busy() {
            return Err("notice.agent_busy");
        }
        let store = self.artifact_store()?;
        let artifact_ids = record
            .delivery_commits
            .iter()
            .flat_map(|commit| commit.artifact_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let submission_ids = record
            .delivery_commits
            .iter()
            .map(|commit| commit.submission_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        store
            .prepare_session_deletion(
                session_id,
                &artifact_ids,
                &submission_ids,
            )
            .map_err(|error| {
                log_storage_io_error(
                    "prepare session deletion",
                    &self.inner.state.root.join("artifacts"),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.storage_error"
            })?;

        // Stop a title request before removing the session. `generate_title`
        // takes the same dispatch lock, so it cannot write a title after the
        // deletion becomes observable.
        if let Some(live) = lock(&self.inner.state.live).get(session_id) {
            live.title_signal.cancel();
        }
        if let Err(error) = engine.remove_session(session_id) {
            log_storage_io_error(
                "remove structured session",
                &flowy_agent_core::SessionIndex::path(&self.inner.state.root),
                &error,
            );
            if let Err(cancel_error) = store.cancel_session_deletion(session_id)
            {
                log_storage_io_error(
                    "cancel prepared session deletion",
                    &self.inner.state.root.join("artifacts"),
                    &cancel_error,
                );
                self.inner.state.report_storage_error();
            }
            return Err("notice.storage_error");
        }
        let mut live = lock(&self.inner.state.live);
        let removed = live.remove(session_id);
        drop(live);
        let previous_active = lock(&self.inner.state.active).clone();
        if previous_active.as_deref() == Some(session_id) {
            *lock(&self.inner.state.active) = None;
        }
        if let Err(error) = persist_index(&self.inner.state) {
            // The UI cache is derived. Restore the complete structured record
            // when its cache mutation failed, so a failed deletion cannot be
            // rehydrated from text-only display rows later.
            if engine.insert_session(record).is_err() {
                self.inner.state.report_storage_error();
                self.inner.state.push_update(AgentUpdate::Sessions);
                self.inner.state.push_update(AgentUpdate::Artifacts);
                return Err("notice.storage_error");
            }
            if engine.set_active(previous_active.clone()).is_err() {
                self.inner.state.report_storage_error();
            }
            if let Some(removed) = removed {
                lock(&self.inner.state.live)
                    .insert(session_id.to_owned(), removed);
            }
            *lock(&self.inner.state.active) = previous_active;
            if let Err(cancel_error) = store.cancel_session_deletion(session_id)
            {
                log_storage_io_error(
                    "cancel prepared session deletion",
                    &self.inner.state.root.join("artifacts"),
                    &cancel_error,
                );
                self.inner.state.report_storage_error();
            }
            self.inner.state.report_storage_error();
            return Err(error);
        }

        // The conversation removal is the logical deletion commit point. The
        // committed plan keeps artifacts hidden and makes physical cleanup
        // retryable even if the process exits midway.
        if let Err(error) = store.commit_session_deletion(session_id) {
            log_storage_io_error(
                "commit session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        } else if let Err(error) = store.cleanup_session_deletion(session_id) {
            log_storage_io_error(
                "clean up session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        } else if let Err(error) = store.complete_session_deletion(session_id) {
            log_storage_io_error(
                "complete session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner.state.push_update(AgentUpdate::Artifacts);
        Ok(())
    }

    /// Retry the independent artifact-store marker for a durable delivery
    /// commit after the turn itself has already completed. The turn is never
    /// re-run and no new submission or artifact ID is created.
    pub fn retry_delivery(
        &self,
        session_id: Option<&str>,
    ) -> Result<(), &'static str> {
        let session_id = session_id.ok_or("chat.status_delivery_failed")?;
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine
            .session(session_id)
            .ok_or("chat.status_delivery_failed")?;
        if record.delivery_commits.is_empty() {
            return Err("chat.status_delivery_failed");
        }
        if record.status.is_busy()
            || lock(&self.inner.state.live)
                .get(session_id)
                .is_some_and(|session| session.status.is_busy())
        {
            return Err("notice.agent_busy");
        }
        if !lock(&self.inner.state.live).get(session_id).is_some_and(
            |session| matches!(session.status, TurnStatus::DeliveryFailed),
        ) {
            return Err("chat.status_delivery_failed");
        }
        let store = self.artifact_store()?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.status = TurnStatus::SavingFiles;
            session.turn_started_at = Some(now_ms());
        }
        self.inner.state.stream_changed(session_id);
        if let Err(error) = store.recover_commits(&record.delivery_commits) {
            log_storage_io_error(
                "retry delivered artifacts",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.status = TurnStatus::DeliveryFailed;
                session.turn_started_at = None;
            }
            self.inner.state.stream_changed(session_id);
            return Err("chat.status_delivery_failed");
        }
        for commit in &record.delivery_commits {
            apply_delivery_commit_to_live(&self.inner.state, commit);
        }
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.projection.lines.retain(|line| {
                !matches!(
                    line,
                    ChatLine::Status { text }
                        if text == "chat.status_delivery_failed"
                )
            });
            session.status = TurnStatus::Completed;
            session.turn_started_at = None;
            session.updated_at = now_ms();
        }
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner.state.stream_changed(session_id);
        self.inner.state.push_update(AgentUpdate::Artifacts);
        Ok(())
    }

    pub fn artifacts(&self) -> Result<Vec<ArtifactRecord>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .list()
    }

    pub fn artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<ArtifactRecord>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .get(artifact_id)
    }

    pub fn set_artifact_favorite(
        &self,
        artifact_id: &str,
        favorite: bool,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .set_favorite(artifact_id, favorite)
    }

    pub fn trash_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .trash(artifact_id)
    }

    pub fn restore_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .restore_from_trash(artifact_id)
    }

    pub fn permanently_delete_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .permanently_delete(artifact_id)
    }

    pub fn read_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<u8>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .read_payload(artifact_id)
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
            return Err("notice.storage_error");
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
                    "notice.storage_error"
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
            .map_err(|_| "notice.storage_error")?;
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
            return Err("notice.storage_error");
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

    fn submit_message(
        &self,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
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
        let state = &self.inner.state;
        if *lock(&state.load_failed) {
            return Err("notice.storage_error");
        }
        let _dispatch = lock(&state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        if let Some(session) =
            submission.and_then(|id| engine.submitted_session(id))
        {
            return Ok(session);
        }
        if selection.is_some_and(|selection| {
            self.context_generation() != selection.generation
                || lock(&state.active).as_deref() != selection.active
        }) {
            return Err("notice.agent_busy");
        }
        let id = self.ensure_active_session()?;
        if lock(&state.live)
            .get(&id)
            .is_some_and(|session| session.status.is_busy())
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
        let config = lock(&self.inner.config).clone();
        let skills = self.skill_catalog();
        let (prompt, active_skills) = prepare_skill_prompt(text, &skills)?;
        let title_prompt = text.to_owned();
        let stream = config
            .api_key
            .is_none()
            .then(|| echo_stream_fn(text.to_owned()));
        let workspace = lock(&state.live)
            .get(&id)
            .and_then(|session| session.workspace.clone());
        let core_session = engine.session(&id).ok_or("notice.agent_failed")?;
        let mut run_config = engine_config(
            &config,
            &session_working_directory(&state.root, workspace.as_ref()),
        );
        let cwd = core_session.workspace.as_ref().map_or_else(
            || session_working_directory(&state.root, workspace.as_ref()),
            |workspace| workspace.root.clone(),
        );
        run_config.cwd = Some(cwd.display().to_string());
        run_config.registry = tool_registry(&cwd, &skills, active_skills);
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
                "notice.storage_error"
            })?;
            run_config
                .registry
                .register(crate::publish_tool::definition(store.clone()));
            run_config.delivery_committer = Some(Arc::new(store));
            run_config.system = format!(
                "Files to deliver to this user must be created in the current turn output directory: {}. Use write_file with a path inside that directory, then explicitly call publish_file with a path relative to the output directory. publish_file only stages files; delivery occurs after the whole turn completes successfully. Do not treat ordinary text links or intermediate files as delivered artifacts. Maximum 8 files, 128 MiB each, 256 MiB per turn.",
                output_root.display()
            );
        }
        run_config.system = append_skill_listing(&run_config.system, &skills);
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
                    RunError::Storage(_) => "notice.storage_error",
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
        state.push_update(AgentUpdate::Session { id: id.clone() });
        state.push_update(AgentUpdate::Sessions);
        let state = Arc::clone(state);
        let receipt = id.clone();
        let completed_submission = submission;
        self.inner.runtime.spawn(async move {
            let outcome = turn.await;
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
            capture_turn_artifacts(&engine, &state, &id, completed_submission);
            if matches!(outcome, Ok(LoopOutcome::Completed))
                && let Some(signal) = title_signal
            {
                tracing::debug!(
                    target: "flowy.prompt",
                    stage = "AgentHandle::generate_title",
                    session = %id,
                    "sidecar LLM"
                );
                generate_title(engine, state, config, signal, id, title_prompt)
                    .await;
            }
        });
        Ok(receipt)
    }

    pub fn stop(&self) {
        let _dispatch = lock(&self.inner.state.dispatch);
        self.stop_locked();
    }

    pub fn stop_if_active(&self, expected: Option<&str>) {
        let _dispatch = lock(&self.inner.state.dispatch);
        if lock(&self.inner.state.active).as_deref() == expected {
            self.stop_locked();
        }
    }

    fn stop_locked(&self) {
        let Some(id) = lock(&self.inner.state.active).clone() else {
            return;
        };
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = engine {
            match engine.request_cancel(&id) {
                Ok(false) => return,
                Ok(true) => {}
                Err(error) => {
                    log_storage_io_error(
                        "cancel agent turn",
                        &flowy_agent_core::SessionIndex::path(
                            &self.inner.state.root,
                        ),
                        &error,
                    );
                    self.inner.state.report_storage_error();
                    return;
                }
            }
        }
        {
            let mut live = lock(&self.inner.state.live);
            if let Some(session) = live.get_mut(&id) {
                session.title_signal.cancel();
                session.projection.flush();
                if session.status.is_busy() {
                    session.status = TurnStatus::Stopped;
                    session.projection.lines.push(ChatLine::Status {
                        text: session.status.key().into(),
                    });
                }
                session.turn_started_at = None;
                session.updated_at = now_ms();
            }
        }
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Session { id });
    }

    fn ensure_active_session(&self) -> Result<String, &'static str> {
        let active = lock(&self.inner.state.active).clone();
        if let Some(id) = active
            && lock(&self.inner.state.live).contains_key(&id)
        {
            return Ok(id);
        }
        self.create_session()
    }
}

struct UiObserver {
    state: Arc<UiState>,
}

fn mark_live_delivery_failed(session: &mut Live) {
    session.status = TurnStatus::DeliveryFailed;
    session.turn_started_at = None;
    if !session.projection.lines.iter().any(|line| {
        matches!(
            line,
            ChatLine::Status { text }
                if text == "chat.status_delivery_failed"
        )
    }) {
        session.projection.lines.push(ChatLine::Status {
            text: "chat.status_delivery_failed".into(),
        });
    }
}

fn mark_delivery_failed(state: &UiState, session_id: &str) {
    if let Some(session) = lock(&state.live).get_mut(session_id) {
        mark_live_delivery_failed(session);
    }
    state.stream_changed(session_id);
}

fn apply_delivery_commit_to_live(state: &UiState, commit: &DeliveryCommit) {
    if let Some(session) = lock(&state.live).get_mut(&commit.session_id) {
        session.projection.flush();
        for artifact_id in &commit.artifact_ids {
            if !session.projection.lines.iter().any(|line| {
                matches!(
                    line,
                    ChatLine::Artifact { artifact_id: existing }
                        if existing == artifact_id
                )
            }) {
                session.projection.lines.push(ChatLine::Artifact {
                    artifact_id: artifact_id.clone(),
                });
            }
        }
    }
}

impl TurnObserver for UiObserver {
    fn saving_files(&self, session_id: &str) {
        if let Some(session) = lock(&self.state.live).get_mut(session_id) {
            session.status = TurnStatus::SavingFiles;
        }
        self.state.stream_changed(session_id);
    }

    fn delivery_committed(&self, commit: &DeliveryCommit) {
        apply_delivery_commit_to_live(&self.state, commit);
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.stream_changed(&commit.session_id);
        self.state.push_update(AgentUpdate::Artifacts);
    }

    fn delivery_failed(&self, commit: &DeliveryCommit, error: &str) {
        tracing::error!(
            target: "flowy.storage",
            operation = "commit delivered artifacts",
            session = %commit.session_id,
            submission = %commit.submission_id,
            error = %error,
            "delivery marker failed"
        );
        if let Some(session) =
            lock(&self.state.live).get_mut(&commit.session_id)
        {
            session.status = TurnStatus::DeliveryFailed;
            session.turn_started_at = None;
        }
        self.state.stream_changed(&commit.session_id);
    }

    fn text(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_text(delta);
        }
        self.state.stream_changed(session);
    }

    fn reasoning(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_reasoning(delta);
        }
        self.state.stream_changed(session);
    }

    fn assistant_text(&self, session: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.flush();
            let duplicate = matches!(
                live.projection.lines.last(),
                Some(ChatLine::Assistant { text: last }) if last == text
            );
            if !duplicate {
                live.projection.lines.push(ChatLine::Assistant {
                    text: text.to_owned(),
                });
            }
        }
    }

    fn tool_start(
        &self,
        session: &str,
        call_id: &str,
        name: &str,
        args: &serde_json::Value,
    ) {
        // Only file-writing tools can produce a change row; keep their
        // arguments until the call settles.
        if matches!(name, "write_file" | "edit_file") {
            lock(&self.state.tool_args)
                .insert(tool_arg_key(session, call_id), args.clone());
        }
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection
                .push_tool_call(name, tool_target(args), call_id);
            live.updated_at = now_ms();
        }
        self.state.stream_changed(session);
    }

    fn tool_end(
        &self,
        session: &str,
        call_id: &str,
        name: &str,
        text: &str,
        is_error: bool,
    ) {
        // Tool paths may be relative; resolve them against the session working
        // directory so the card can open the real folder later.
        let cwd = lock(&self.state.live)
            .get(session)
            .and_then(|live| {
                live.workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
            })
            .unwrap_or_else(|| self.state.root.join("sessions").join("cwd"));
        let display_text = tool_result_display_text(name, text);
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection
                .finish_tool_call(call_id, &display_text, is_error);
            // Drop the cached arguments either way; a failed call leaves the
            // file untouched, so only settled calls get a change row.
            let change =
                take_file_change(&self.state.tool_args, session, call_id, &cwd);
            if !is_error && let Some(change) = change {
                live.projection.push_file_change(
                    change.path,
                    change.kind,
                    change.added,
                    change.removed,
                    change.binary,
                );
            }
            live.updated_at = now_ms();
        }
        self.state.stream_changed(session);
    }

    fn finished(&self, session: &str, outcome: LoopOutcome) {
        {
            let mut live = lock(&self.state.live);
            if let Some(live) = live.get_mut(session) {
                live.projection.flush();
                live.status = match outcome {
                    LoopOutcome::Completed => TurnStatus::Completed,
                    LoopOutcome::Aborted => TurnStatus::Stopped,
                    // `delivery_failed` ran before this terminal callback. Keep
                    // the dedicated status so the retry action remains visible.
                    LoopOutcome::Failed
                        if matches!(
                            live.status,
                            TurnStatus::DeliveryFailed
                        ) =>
                    {
                        TurnStatus::DeliveryFailed
                    }
                    // Store an i18n key, never a raw English message: this value
                    // is persisted in the index and may be rendered later.
                    LoopOutcome::Failed => TurnStatus::Error {
                        message: "chat.status_error".into(),
                    },
                };
                if outcome != LoopOutcome::Completed {
                    live.auto_title_pending = false;
                    live.projection.lines.push(ChatLine::Status {
                        text: live.status.key().into(),
                    });
                }
                live.turn_started_at = None;
                live.updated_at = now_ms();
            }
        }
        // Interrupted turns can leave cached tool arguments behind.
        lock(&self.state.tool_args)
            .retain(|key, _| !key.starts_with(&format!("{session}\u{1}")));
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.push_update(AgentUpdate::Session {
            id: session.to_owned(),
        });
        self.state.push_update(AgentUpdate::Sessions);
    }

    fn storage_error(&self, session: &str) {
        tracing::error!(
            target: "flowy.storage",
            operation = "persist structured session history",
            session = %session,
            path = %flowy_agent_core::SessionIndex::path(&self.state.root).display(),
            "agent reported a storage failure"
        );
        self.state.report_storage_error();
    }
}

async fn generate_title(
    engine: Arc<Engine>,
    state: Arc<UiState>,
    config: LlmConfig,
    signal: CancelToken,
    id: String,
    prompt: String,
) {
    let pending = lock(&state.live)
        .get(&id)
        .is_some_and(|session| session.auto_title_pending);
    clear_auto_title(&state, &id);
    if !pending || signal.is_cancelled() {
        return;
    }
    let prompt: String = prompt.chars().take(2000).collect();
    let generated = complete_text(
        Some(format!("title-{id}")),
        title::title_system_prompt(),
        &prompt,
        model_route(&config),
        config.api_key.clone(),
        signal.clone(),
    )
    .await;
    if signal.is_cancelled() {
        return;
    }
    let title = generated
        .ok()
        .map(|text| title::clamp_title(&text, TITLE_MAX_CHARS))
        .filter(|text| !text.is_empty());
    let Some(title) = title else {
        return;
    };
    // Serialize the title commit with session deletion. Once the dispatcher
    // has removed the source, this task must not write the title back.
    let _dispatch = lock(&state.dispatch);
    if signal.is_cancelled() {
        return;
    }
    if engine.set_session_title(&id, title.clone(), true).is_ok() {
        sync_artifact_title(&state, &engine, &id);
    } else {
        state.report_storage_error();
    }
    if let Some(session) = lock(&state.live).get_mut(&id) {
        session.title = title;
        session.updated_at = now_ms();
    }
    if persist_index(&state).is_err() {
        state.report_storage_error();
    }
    state.push_update(AgentUpdate::Session { id });
    state.push_update(AgentUpdate::Sessions);
}

fn recover_session_deletions(
    store: &ArtifactStore,
    engine: &Engine,
    state: &UiState,
) -> BTreeSet<String> {
    let mut deleted = BTreeSet::new();
    let plans = match store.session_deletions() {
        Ok(plans) => plans,
        Err(error) => {
            log_storage_io_error(
                "load session deletion plans",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
            return deleted;
        }
    };
    for plan in plans {
        let session_exists = engine.session(&plan.session_id).is_some();
        let result = match plan.state {
            SessionDeletionState::Prepared if session_exists => {
                store.cancel_session_deletion(&plan.session_id)
            }
            SessionDeletionState::Prepared => {
                deleted.insert(plan.session_id.clone());
                store
                    .commit_session_deletion(&plan.session_id)
                    .and_then(|()| {
                        store.cleanup_session_deletion(&plan.session_id)
                    })
            }
            SessionDeletionState::Committed => {
                deleted.insert(plan.session_id.clone());
                if session_exists
                    && let Err(error) = engine.remove_session(&plan.session_id)
                {
                    log_storage_io_error(
                        "remove session for committed deletion",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        &error,
                    );
                    state.report_storage_error();
                }
                store.cleanup_session_deletion(&plan.session_id)
            }
        };
        if let Err(error) = result {
            log_storage_io_error(
                "recover session deletion",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
    deleted
}

fn migrate_legacy_source_deletions(
    store: &ArtifactStore,
    engine: &Engine,
    state: &UiState,
) -> BTreeSet<String> {
    let mut deleted = BTreeSet::new();
    let sessions = match store.source_deleted_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            log_storage_io_error(
                "load legacy source deletion markers",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
            return deleted;
        }
    };
    for session_id in sessions {
        if engine.session(&session_id).is_some() {
            if let Err(error) = store.clear_source_deleted(&session_id) {
                log_storage_io_error(
                    "clear stale source deletion marker",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
            }
            continue;
        }
        deleted.insert(session_id.clone());
        let result = store
            .prepare_session_deletion(&session_id, &[], &[])
            .and_then(|()| store.commit_session_deletion(&session_id))
            .and_then(|()| store.cleanup_session_deletion(&session_id));
        if let Err(error) = result {
            log_storage_io_error(
                "migrate legacy source deletion",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
    deleted
}

fn backfill_conversation_artifacts(state: &UiState, engine: &Engine) {
    let Some(store) = state.artifact_store.get().cloned() else {
        return;
    };
    for session in engine.sessions() {
        if !crate::conversation_artifacts::session_supports_import(&session) {
            continue;
        }
        match store.source_deleted(&session.id) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                log_storage_io_error(
                    "read source deletion marker before artifact backfill",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
                continue;
            }
        }
        let writes = crate::conversation_artifacts::parse_successful_writes(
            &session.messages,
        );
        if writes.is_empty() {
            continue;
        }
        let commits =
            crate::conversation_artifacts::prepare_conversation_artifacts(
                &store, &session, &writes,
            );
        persist_conversation_commits(engine, &store, state, commits);
    }
}

fn capture_turn_artifacts(
    engine: &Engine,
    state: &UiState,
    session_id: &str,
    submission: Option<u128>,
) {
    let Some(submission) = submission else {
        return;
    };
    let Some(session) = engine.session(session_id) else {
        return;
    };
    let turn_id =
        crate::conversation_artifacts::turn_id_for_submission(submission);
    if !session.messages.iter().any(|message| message.id == turn_id) {
        tracing::warn!(
            target: "flowy.storage",
            session = %session_id,
            turn = %turn_id,
            "conversation artifact import could not locate the finished turn"
        );
        return;
    }
    let writes =
        crate::conversation_artifacts::parse_successful_writes_for_turn(
            &session.messages,
            &turn_id,
        );
    if writes.is_empty() {
        return;
    }
    let Some(store) = state.artifact_store.get().cloned() else {
        state.report_storage_error();
        return;
    };
    let commits = crate::conversation_artifacts::prepare_conversation_artifacts(
        &store, &session, &writes,
    );
    if persist_conversation_commits(engine, &store, state, commits) {
        state.push_update(AgentUpdate::Artifacts);
    }
}

fn persist_conversation_commits(
    engine: &Engine,
    store: &ArtifactStore,
    state: &UiState,
    commits: Vec<DeliveryCommit>,
) -> bool {
    let mut imported = false;
    for commit in commits {
        if let Err(error) = engine.record_delivery_commit(&commit) {
            log_storage_io_error(
                "persist conversation artifact commit",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            store.record_import_diagnostic(format!(
                "Could not persist artifact commit for {}: {error}",
                commit.session_id
            ));
            state.report_storage_error();
            continue;
        }
        if let Err(error) = store.commit_prepared(&commit) {
            log_storage_io_error(
                "commit conversation artifact",
                &state.root.join("artifacts"),
                &error,
            );
            store.record_import_diagnostic(format!(
                "Could not commit conversation artifact for {}: {error}",
                commit.session_id
            ));
            state.report_storage_error();
            continue;
        }
        imported = true;
    }
    imported
}

fn sync_artifact_title(state: &UiState, engine: &Engine, id: &str) {
    if let Some(store) = state.artifact_store.get() {
        let commits = engine
            .session(id)
            .map_or_default(|record| record.delivery_commits);
        if let Err(error) = store.recover_commits(&commits) {
            log_storage_io_error(
                "update artifact source title",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
}

fn clear_auto_title(state: &UiState, id: &str) {
    if let Some(session) = lock(&state.live).get_mut(id) {
        session.auto_title_pending = false;
    }
}

fn persist_index(state: &UiState) -> Result<(), &'static str> {
    // Serialize snapshots and replacements without holding the read model
    // mutex while the filesystem syncs, so UI reads remain memory-only.
    let _write = lock(&state.index_write);
    let live = lock(&state.live);
    let mut sessions: Vec<IndexRecord> = live
        .iter()
        .map(|(id, session)| IndexRecord {
            id: id.clone(),
            title: session.title.clone(),
            updated_at: session.updated_at,
            lines: session.projection.lines.clone(),
            status: session.status.clone(),
            titled: session.titled,
            workspace: session.workspace.clone(),
            origin: session.origin.clone(),
            session_workspace: session.session_workspace.clone(),
        })
        .collect();
    sessions.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
    let index = SessionIndex {
        active: lock(&state.active).clone(),
        sessions,
    };
    drop(live);
    index.save(&state.root).map_err(|_| "notice.storage_error")
}

fn finish_interrupted_lines(lines: &mut [ChatLine]) {
    for line in lines {
        match line {
            ChatLine::Reasoning { live, .. } => *live = false,
            ChatLine::ToolCall { running, .. } => *running = false,
            _ => {}
        }
    }
}

fn ensure_engine_session(engine: &Engine, id: &str) -> std::io::Result<()> {
    if engine.session(id).is_none() {
        engine.insert_session(SessionRecord {
            id: id.to_owned(),
            ..SessionRecord::default()
        })?;
    }
    Ok(())
}

fn import_engine_sessions(
    engine: &Engine,
    state: &UiState,
    deleted_sessions: &BTreeSet<String>,
) {
    let records = engine.sessions();
    let committed = state
        .artifact_store
        .get()
        .and_then(|store| store.committed_ids().ok())
        .unwrap_or_default();
    let mut live = lock(&state.live);
    for record in records {
        if deleted_sessions.contains(&record.id) {
            continue;
        }
        let allowed = record
            .delivery_commits
            .iter()
            .flat_map(|commit| &commit.artifact_ids)
            .filter(|id| committed.contains(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let pending_delivery = record
            .delivery_commits
            .iter()
            .filter(|commit| commit.completed_successfully)
            .flat_map(|commit| &commit.artifact_ids)
            .any(|id| !committed.contains(id));
        if let Some(existing) = live.get_mut(&record.id) {
            existing.origin = record.origin.clone();
            existing.session_workspace.clone_from(&record.workspace);
            let existing_ids = existing
                .projection
                .lines
                .iter()
                .filter_map(|line| {
                    if let ChatLine::Artifact { artifact_id } = line {
                        Some(artifact_id.clone())
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>();
            if existing_ids != allowed {
                let mut projected =
                    lines_from_messages(&record.messages, &allowed);
                projected.extend(
                    existing
                        .projection
                        .lines
                        .iter()
                        .filter(|line| {
                            matches!(line, ChatLine::TaskReceipt { .. })
                        })
                        .cloned(),
                );
                existing.projection = ChatProjection::from_lines(projected);
                existing.status = live_status(record.status.clone());
                existing.updated_at = record.updated_at;
            }
            restore_existing_history(existing, record, &allowed);
            if pending_delivery {
                mark_live_delivery_failed(existing);
            }
            continue;
        }
        let lines = lines_from_messages(&record.messages, &allowed);
        let title = if record.title.is_empty() {
            lines
                .iter()
                .find_map(|line| match line {
                    ChatLine::User { text } => {
                        Some(title::fallback_title(text, TITLE_MAX_CHARS))
                    }
                    _ => None,
                })
                .unwrap_or_default()
        } else {
            record.title.clone()
        };
        let titled = record.titled || !title.is_empty();
        let status = if pending_delivery {
            TurnStatus::DeliveryFailed
        } else {
            live_status(record.status)
        };
        live.insert(
            record.id.clone(),
            Live {
                projection: ChatProjection::from_lines(lines),
                title,
                titled,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status,
                updated_at: record.updated_at,
                turn_started_at: None,
                workspace: match &record.origin {
                    SessionOrigin::Project {
                        project_id: Some(project_id),
                    } => record.workspace.as_ref().map(|workspace| {
                        AgentWorkspace {
                            project_id: project_id.clone(),
                            root: workspace.root.clone(),
                        }
                    }),
                    _ => None,
                },
                origin: record.origin,
                session_workspace: record.workspace,
            },
        );
    }
}

fn restore_existing_history(
    existing: &mut Live,
    record: SessionRecord,
    allowed: &BTreeSet<String>,
) {
    let lines = &mut existing.projection.lines;
    let mut visible = lines.iter().enumerate().filter_map(|(index, line)| {
        if let ChatLine::User { text } = line {
            Some((index, text))
        } else {
            None
        }
    });
    let mut latest = None;
    for (index, message) in
        record.messages.iter().enumerate().filter(|(_, message)| {
            message.role == Role::User
                && matches!(message.source, MessageSource::User)
        })
    {
        let display = message
            .display_text
            .clone()
            .unwrap_or_else(|| message.text());
        match visible.next() {
            Some((row, text)) if *text == display => {
                latest = Some((row, index));
            }
            None if message.id.starts_with("submission-") => {
                lines.extend(lines_from_messages(
                    &record.messages[index..],
                    allowed,
                ));
                existing.status = live_status(record.status);
                existing.updated_at = record.updated_at;
                return;
            }
            _ => return,
        }
    }
    // The completed turn can reach disk before the UI index. Match both
    // prompt sequences so an older reply cannot complete a later rerun.
    let Some((row, index)) = latest else {
        return;
    };
    if visible.next().is_some()
        || existing.status != TurnStatus::Stopped
        || record.status != TurnState::Completed
        || record.updated_at < existing.updated_at
    {
        return;
    }

    let previous = lines.split_off(row + 1);
    let mut timings = previous
        .iter()
        .filter_map(|line| match line {
            ChatLine::Reasoning {
                text,
                duration_secs,
                ..
            } => Some((text, *duration_secs)),
            _ => None,
        })
        .peekable();
    let mut recovered =
        lines_from_messages(&record.messages[index + 1..], allowed);
    for line in &mut recovered {
        if let ChatLine::Reasoning {
            text,
            duration_secs,
            ..
        } = line
            && let Some((_, elapsed)) =
                timings.next_if(|(saved, _)| *saved == text)
        {
            *duration_secs = elapsed;
        }
    }
    lines.extend(recovered);
    // Task receipts belong to the UI and have no structured message.
    lines.extend(
        previous
            .into_iter()
            .filter(|line| matches!(line, ChatLine::TaskReceipt { .. })),
    );
    existing.status = TurnStatus::Completed;
    existing.updated_at = record.updated_at;
}

fn lines_from_messages(
    messages: &[Message],
    allowed: &BTreeSet<String>,
) -> Vec<ChatLine> {
    let mut lines = Vec::new();
    let mut pending_artifacts = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {}
            Role::User => {
                if matches!(message.source, MessageSource::User) {
                    for artifact_id in std::mem::take(&mut pending_artifacts) {
                        lines.push(ChatLine::Artifact { artifact_id });
                    }
                }
                let text = message
                    .display_text
                    .clone()
                    .unwrap_or_else(|| message.text());
                if !text.is_empty() {
                    lines.push(ChatLine::User { text });
                }
                let tool_name = match &message.source {
                    MessageSource::Tool { tool } => Some(tool.as_str()),
                    _ => None,
                };
                for block in &message.content {
                    if let ContentBlock::ToolResult {
                        content,
                        is_error,
                        artifact,
                        ..
                    } = block
                    {
                        let text = content
                            .iter()
                            .filter_map(ContentBlock::as_text)
                            .collect::<Vec<_>>()
                            .join("\n");
                        lines.push(ChatLine::ToolResult {
                            text: tool_result_display_text(
                                tool_name.unwrap_or_default(),
                                &text,
                            ),
                            is_error: is_error.unwrap_or(false),
                        });
                        if let Some(receipt) = artifact
                            && allowed.contains(&receipt.artifact_id)
                            && !pending_artifacts.contains(&receipt.artifact_id)
                        {
                            pending_artifacts.push(receipt.artifact_id.clone());
                        }
                    }
                }
            }
            Role::Assistant => {
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                lines.push(ChatLine::Assistant {
                                    text: text.clone(),
                                });
                            }
                        }
                        ContentBlock::Reasoning { text } => {
                            lines.push(ChatLine::Reasoning {
                                text: text.clone(),
                                duration_secs: 0,
                                live: false,
                            });
                        }
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            let arguments = serde_json::from_str(arguments)
                                .unwrap_or(serde_json::Value::Null);
                            lines.push(ChatLine::ToolCall {
                                name: name.clone(),
                                target: tool_target(&arguments),
                                call_id: id.clone(),
                                running: false,
                            });
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } => {
                            let text = content
                                .iter()
                                .filter_map(ContentBlock::as_text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tool_name = match &message.source {
                                MessageSource::Tool { tool } => tool.as_str(),
                                _ => "",
                            };
                            lines.push(ChatLine::ToolResult {
                                text: tool_result_display_text(
                                    tool_name, &text,
                                ),
                                is_error: is_error.unwrap_or(false),
                            });
                        }
                    }
                }
            }
        }
    }
    for artifact_id in pending_artifacts {
        lines.push(ChatLine::Artifact { artifact_id });
    }
    lines
}

fn tool_result_display_text(tool_name: &str, text: &str) -> String {
    if tool_name == "publish_file"
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(key) = value.get("ui_key").and_then(|value| value.as_str())
        && key.starts_with("chat.publish.")
    {
        return key.to_owned();
    }
    text.to_owned()
}

fn live_status(state: TurnState) -> TurnStatus {
    match state {
        TurnState::Idle => TurnStatus::Idle,
        TurnState::Running | TurnState::Stopped => TurnStatus::Stopped,
        TurnState::Completed => TurnStatus::Completed,
        TurnState::Error { message } => TurnStatus::Error { message },
    }
}

fn sync_engine_titles(engine: &Engine, state: &UiState) {
    let live = lock(&state.live);
    for (id, session) in live.iter() {
        if session.title.is_empty() {
            continue;
        }
        let Some(record) = engine.session(id) else {
            continue;
        };
        if record.title == session.title && record.titled == session.titled {
            continue;
        }
        if engine
            .set_session_title(id, session.title.clone(), session.titled)
            .is_ok()
        {
            sync_artifact_title(state, engine, id);
        } else {
            state.report_storage_error();
        }
    }
}

fn hydrate_engine(engine: &Engine, state: &UiState, config: &LlmConfig) {
    let known: std::collections::HashSet<String> = engine
        .sessions()
        .into_iter()
        .map(|record| record.id)
        .collect();
    let live = lock(&state.live);
    for (id, session) in live.iter() {
        if known.contains(id) {
            continue;
        }
        if engine
            .insert_session(SessionRecord {
                id: id.clone(),
                title: session.title.clone(),
                titled: session.titled,
                updated_at: session.updated_at,
                status: TurnState::Stopped,
                messages: messages_from_lines(
                    &session.projection.lines,
                    config,
                ),
                todos: Vec::new(),
                origin: SessionOrigin::Unknown,
                workspace: None,
                delivery_commits: Vec::new(),
            })
            .is_err()
        {
            state.report_storage_error();
        }
    }
}

fn messages_from_lines(lines: &[ChatLine], config: &LlmConfig) -> Vec<Message> {
    let mut messages = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match line {
            ChatLine::User { text } if !text.is_empty() => {
                messages.push(Message {
                    id: format!("legacy-u-{index}"),
                    role: Role::User,
                    content: vec![ContentBlock::text(text.clone())],
                    source: MessageSource::User,
                    display_text: None,
                });
            }
            ChatLine::Assistant { text } => messages.push(Message {
                id: format!("legacy-a-{index}"),
                role: Role::Assistant,
                content: vec![ContentBlock::text(text.clone())],
                source: MessageSource::Model {
                    provider: config.provider.clone(),
                    model: config.model.clone(),
                },
                display_text: None,
            }),
            _ => {}
        }
    }
    messages
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

fn ensure_tool_cwd(root: &Path) -> PathBuf {
    let dir = root.join("sessions").join("cwd");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log_storage_io_error("create tool working directory", &dir, &error);
    }
    dir
}

fn session_working_directory(
    root: &Path,
    workspace: Option<&AgentWorkspace>,
) -> PathBuf {
    workspace.map_or_else(|| ensure_tool_cwd(root), |item| item.root.clone())
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

fn tool_registry(
    cwd: &Path,
    skills: &[InstalledSkill],
    active_skills: Vec<SkillId>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.register(pi_shell_definition(cwd));
    // The app has no todo surface yet; expose only tools whose results the
    // host can satisfy instead of advertising one that always fails.
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let active = Arc::new(Mutex::new(skill_scope(skills, active_skills)));
    let skills_for_paths = skills.to_vec();
    let path_active = Arc::clone(&active);
    let guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        path_guard(&root, &skills_for_paths, &path_active, args)
    });
    registry.add_guard(guard);
    let skills_for_shell = skills.to_vec();
    let shell_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_guard(&skills_for_shell, &active, args)
    });
    registry.add_guard(shell_guard);
    registry
}

/// Host policy for file tools. The platform shell is handled by a separate
/// guard because it executes commands rather than host file paths.
fn path_guard(
    root: &Path,
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    let key = match args.name.as_str() {
        "read_file" | "write_file" | "edit_file" | "glob" | "grep" => "path",
        _ => return None,
    };
    let raw = args.arguments.get(key).and_then(|value| value.as_str())?;
    if raw.trim().is_empty() {
        return None;
    }
    let writable = matches!(args.name.as_str(), "write_file" | "edit_file");
    if confined_to(root, raw) {
        return None;
    }
    if writable {
        return Some(format!("path escapes the working directory: {raw}"));
    }
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        root.join(raw)
    };
    let Some(skill) = skills
        .iter()
        .find(|skill| confined_to(&skill.root, &path.to_string_lossy()))
    else {
        return Some(format!("path escapes the working directory: {raw}"));
    };
    if matches!(args.name.as_str(), "read_file" | "grep") {
        lock(active).insert(skill.id.clone());
    }
    None
}

fn skill_scope(
    skills: &[InstalledSkill],
    explicit: Vec<SkillId>,
) -> HashSet<SkillId> {
    let mut active = explicit.into_iter().collect::<HashSet<_>>();
    active.extend(
        skills
            .iter()
            .filter(|skill| skill.auto_invoke)
            .map(|skill| skill.id.clone()),
    );
    active
}

fn shell_guard(
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    if !matches!(args.name.as_str(), "bash" | "powershell") {
        return None;
    }
    let active = lock(active);
    let blocked = skills
        .iter()
        .find(|skill| active.contains(&skill.id) && !skill.scripts_enabled);
    blocked.map(|skill| {
        format!(
            "skill \"{}\" does not have script permission; enable it in My Skills before using a shell",
            skill.name
        )
    })
}

fn confined_to(root: &Path, raw: &str) -> bool {
    let path = Path::new(raw);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let root =
        std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(candidate) = std::fs::canonicalize(&normalized) {
        return within(&root, &candidate);
    }
    if let (Some(parent), Some(name)) =
        (normalized.parent(), normalized.file_name())
        && let Ok(parent) = std::fs::canonicalize(parent)
    {
        return within(&root, &parent.join(name));
    }
    within(&root, &normalized)
}

fn within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = root.to_string_lossy().replace('/', "\\").to_lowercase();
        let candidate = candidate
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        candidate == root || candidate.starts_with(&format!("{root}\\"))
    }
    #[cfg(not(windows))]
    {
        candidate.starts_with(root)
    }
}

fn tool_target(args: &serde_json::Value) -> String {
    for key in [
        "description",
        "path",
        "file_path",
        "command",
        "query",
        "pattern",
        "target",
    ] {
        if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
            return value.to_owned();
        }
    }
    String::new()
}

fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let stamp = now_ms().saturating_mul(1000);
    stamp + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn now_ms() -> u64 {
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

/// Cache key for one tool call: call ids repeat across sessions.
fn tool_arg_key(session: &str, call_id: &str) -> String {
    format!("{session}\u{1}{call_id}")
}

struct FileChangeDraft {
    path: String,
    kind: FileChangeKind,
    added: u32,
    removed: u32,
    binary: bool,
}

/// Resolve a tool path against the session directory: `write_file` and
/// `edit_file` accept relative paths and resolve them inside the tool, so the
/// raw argument is what reaches this row.
fn resolve_change_path(raw: &str, cwd: &Path) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    Some(absolute.to_string_lossy().into_owned())
}

/// Build the change row from the tool arguments, then drop them.
fn take_file_change(
    args: &Mutex<HashMap<String, serde_json::Value>>,
    session: &str,
    call_id: &str,
    cwd: &Path,
) -> Option<FileChangeDraft> {
    let value = lock(args).remove(&tool_arg_key(session, call_id))?;
    let path = resolve_change_path(value.get("path")?.as_str()?, cwd)?;
    if let (Some(old), Some(new)) = (
        value.get("old_string").and_then(|v| v.as_str()),
        value.get("new_string").and_then(|v| v.as_str()),
    ) {
        let counted = if is_binary(old) || is_binary(new) {
            None
        } else {
            line_delta(old, new)
        };
        let (added, removed) = counted.unwrap_or((0, 0));
        return Some(FileChangeDraft {
            path,
            kind: FileChangeKind::Update,
            added,
            removed,
            binary: counted.is_none(),
        });
    }
    // `write_file` replaces the whole file, so the previous content is not in
    // the arguments; the card reports the written size and no removals.
    let content = value.get("content")?.as_str()?;
    let binary = is_binary(content);
    Some(FileChangeDraft {
        path,
        kind: FileChangeKind::Add,
        added: if binary {
            0
        } else {
            content.lines().count() as u32
        },
        removed: 0,
        binary,
    })
}

/// Content carrying a NUL byte is not text; counting its lines is meaningless.
fn is_binary(text: &str) -> bool {
    text.as_bytes().contains(&0)
}

/// Line counts are a hint on the card, so a very large edit is left uncounted
/// instead of spending seconds inside the diff on the turn path.
const MAX_DIFF_BYTES: usize = 200 * 1024;

/// Added and removed line counts for one change; `None` when the edit is too
/// large to count cheaply.
fn line_delta(old: &str, new: &str) -> Option<(u32, u32)> {
    if old.len().saturating_add(new.len()) > MAX_DIFF_BYTES {
        return None;
    }
    use similar::{ChangeTag, TextDiff};
    let mut added = 0u32;
    let mut removed = 0u32;
    for change in TextDiff::from_lines(old, new).iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }
    Some((added, removed))
}

#[cfg(test)]
mod background_init_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn empty_selection_revision_prevents_sending_home_draft_into_a_new_project()
    {
        let root = std::env::temp_dir()
            .join(format!("flowy-context-{}", next_stamp()));
        let handle = AgentHandle::with_config(
            root.clone(),
            LlmConfig {
                api_key: None,
                ..LlmConfig::default()
            },
        );
        handle.set_origin_for_new_sessions(SessionOrigin::Home);
        let captured = handle.context_generation();
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        handle.set_workspace_for_new_sessions(Some(AgentWorkspace {
            project_id: "project-1".into(),
            root: project,
        }));
        handle.set_origin_for_new_sessions(SessionOrigin::Project {
            project_id: Some("project-1".into()),
        });
        assert_eq!(handle.active_session_id(), None);
        assert_eq!(
            handle.submit_chat_if_active(1, "Home draft", None, captured),
            Err("notice.agent_busy")
        );
        assert!(handle.list_sessions().is_empty());
        assert_eq!(handle.lines().as_slice(), []);
        drop(handle);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retry_delivery_commits_the_durable_group_and_clears_failure() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-retry-delivery-{}", next_stamp()));
        let config = LlmConfig::default();
        let handle = AgentHandle::uninitialized(root.clone(), config.clone());
        let state = Arc::clone(&handle.inner.state);
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let engine = Arc::new(
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap(),
        );
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: root.join("workspace"),
                }),
            )
            .unwrap();
        let output = root.join("outputs").join("submission-1");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("report.md"), b"# report\n").unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "home".into(),
                submission_id: "submission-1".into(),
                tool_call_id: "call-1".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        let mut record = engine.session("home").unwrap();
        record.delivery_commits.push(DeliveryCommit {
            session_id: "home".into(),
            submission_id: "submission-1".into(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Home".into(),
        });
        engine.insert_session(record).unwrap();
        *lock(&handle.inner.engine) = Some(engine);
        let _ = state.artifact_store.set(store.clone());
        *lock(&state.active) = Some("home".into());
        lock(&state.live).insert(
            "home".into(),
            Live {
                projection: ChatProjection::from_lines(vec![
                    ChatLine::Status {
                        text: "chat.status_delivery_failed".into(),
                    },
                ]),
                title: "Home".into(),
                titled: true,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: TurnStatus::DeliveryFailed,
                updated_at: 1,
                turn_started_at: None,
                workspace: None,
                origin: SessionOrigin::Home,
                session_workspace: None,
            },
        );

        if let Some(live) = lock(&state.live).get_mut("home") {
            live.status = TurnStatus::Running;
        }
        assert_eq!(
            handle.retry_delivery(Some("home")),
            Err("notice.agent_busy")
        );
        if let Some(live) = lock(&state.live).get_mut("home") {
            live.status = TurnStatus::DeliveryFailed;
        }
        handle.retry_delivery(Some("home")).unwrap();
        // Read under the guard, then release it: the lock is a significant
        // drop, and holding it across the assertions serves nothing.
        let live_state = lock(&state.live);
        let live = live_state.get("home").unwrap();
        let is_completed = live.status == TurnStatus::Completed;
        let has_failure_line = live.projection.lines.iter().any(|line| {
            matches!(line, ChatLine::Status { text } if text == "chat.status_delivery_failed")
        });
        let has_artifact_line = live.projection.lines.iter().any(|line| {
            matches!(line, ChatLine::Artifact { artifact_id } if artifact_id == &receipt.artifact_id)
        });
        drop(live_state);
        assert!(is_completed);
        assert!(!has_failure_line);
        assert!(has_artifact_line);
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"# report\n"
        );
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_completes_a_prepared_session_deletion_after_crash() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-session-delete-recovery-{}", next_stamp()));
        let config = LlmConfig::default();
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::new(UiState::new(root.clone(), None)),
        });
        let engine =
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap();
        engine
            .create_session_with_metadata(
                "home".into(),
                1,
                SessionOrigin::Home,
                Some(SessionWorkspace {
                    root: root.join("workspace"),
                }),
            )
            .unwrap();
        let output = root.join("outputs").join("submission-1");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join("report.md"),
            b"# report
",
        )
        .unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "home".into(),
                submission_id: "submission-1".into(),
                tool_call_id: "call-1".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        let delivery = DeliveryCommit {
            session_id: "home".into(),
            submission_id: "submission-1".into(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            committed_at_ms: 1,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Home".into(),
        };
        store
            .recover_commits(std::slice::from_ref(&delivery))
            .unwrap();
        store
            .prepare_session_deletion(
                "home",
                &delivery.artifact_ids,
                std::slice::from_ref(&delivery.submission_id),
            )
            .unwrap();
        engine.remove_session("home").unwrap();
        crate::index::SessionIndex {
            active: Some("home".into()),
            sessions: vec![IndexRecord {
                id: "home".into(),
                title: "stale".into(),
                updated_at: 1,
                lines: vec![ChatLine::User {
                    text: "stale row".into(),
                }],
                status: TurnStatus::Completed,
                titled: true,
                workspace: None,
                origin: SessionOrigin::Home,
                session_workspace: None,
            }],
        }
        .save(&root)
        .unwrap();
        drop(store);
        drop(engine);

        let handle = AgentHandle::with_config(root.clone(), config);
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.is_initializing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!handle.is_initializing());
        assert!(handle.list_sessions().is_empty());
        assert!(
            crate::index::SessionIndex::load(&root)
                .unwrap()
                .sessions
                .is_empty()
        );
        let store = handle.artifact_store().unwrap();
        assert_eq!(store.list().unwrap().as_slice(), []);
        assert!(store.get(&receipt.artifact_id).unwrap().is_none());
        assert!(store.session_deletions().unwrap().is_empty());
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_source_deletion_marker_is_migrated_to_cascade_cleanup() {
        use crate::artifact_store::PrepareArtifact;

        let root = std::env::temp_dir()
            .join(format!("flowy-session-delete-legacy-{}", next_stamp()));
        let config = LlmConfig::default();
        let state = Arc::new(UiState::new(root.clone(), None));
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let engine =
            Engine::open(root.clone(), engine_config(&config, &root), observer)
                .unwrap();
        let output = root.join("outputs").join("submission-legacy");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join("report.md"),
            b"# report
",
        )
        .unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.md".into(),
                source_relative_path: "report.md".into(),
                conversation_import: false,
                session_id: "legacy".into(),
                submission_id: "submission-legacy".into(),
                tool_call_id: "call-legacy".into(),
                delivery_key: "report".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        store
            .recover_commits(std::slice::from_ref(&DeliveryCommit {
                session_id: "legacy".into(),
                submission_id: "submission-legacy".into(),
                artifact_ids: vec![receipt.artifact_id.clone()],
                completed_successfully: true,
                committed_at_ms: 1,
                origin: SessionOrigin::Home,
                origin_title_snapshot: "Legacy".into(),
            }))
            .unwrap();
        std::fs::write(
            store.root().join("sources-deleted/legacy.json"),
            br#"{"schema_version":1,"session_id":"legacy","deleted_at_ms":1}"#,
        )
        .unwrap();

        let deleted = migrate_legacy_source_deletions(&store, &engine, &state);
        assert!(deleted.contains("legacy"));
        assert_eq!(store.list().unwrap().as_slice(), []);
        assert!(store.get(&receipt.artifact_id).unwrap().is_none());
        assert_eq!(store.session_deletions().unwrap().len(), 1);
        store.complete_session_deletion("legacy").unwrap();
        assert!(!store.root().join("sources-deleted/legacy.json").exists());
        assert!(store.session_deletions().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn background_initialization_restores_sessions_and_announces_readiness() {
        let root = std::env::temp_dir()
            .join(format!("flowy-background-{}", next_stamp()));
        let config = LlmConfig {
            api_key: None,
            ..LlmConfig::default()
        };
        let seed = AgentHandle::with_config(root.clone(), config.clone());
        let id = seed.create_home_session().unwrap();
        drop(seed);
        let handle = AgentHandle::with_config_background(root.clone(), config);
        handle
            .apply_config(LlmConfig {
                api_key: None,
                model: "latest-model".into(),
                ..LlmConfig::default()
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.is_initializing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !handle.is_initializing(),
            "storage initialization did not finish"
        );
        assert_eq!(handle.active_session_id().as_deref(), Some(id.as_str()));
        assert!(handle.source_available(&id));
        assert_eq!(lock(&handle.inner.config).model, "latest-model");
        let updates = handle.drain_updates();
        assert_eq!(
            updates
                .iter()
                .filter(|update| matches!(update, AgentUpdate::Initialized))
                .count(),
            1
        );
        assert!(
            !updates
                .iter()
                .any(|update| matches!(update, AgentUpdate::StorageError))
        );
        drop(handle);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod guard_tests {
    use super::{
        AgentWorkspace, append_skill_listing, confined_to, path_guard,
        prepare_skill_prompt, session_working_directory, shell_guard,
        skill_scope, take_file_change, tool_arg_key, tool_registry,
        tool_result_display_text,
    };
    use crate::projection::FileChangeKind;
    use flowy_agent_core::types::ToolCallArgs;
    use flowy_skills::{InstallOrigin, InstalledSkill, parse_skill_id};
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    fn args(name: &str, arguments: serde_json::Value) -> ToolCallArgs {
        ToolCallArgs {
            call_id: "call-1".into(),
            name: name.into(),
            arguments,
        }
    }

    #[test]
    fn file_tools_stay_inside_the_tool_working_directory() {
        let root = Path::new("/work");
        assert!(confined_to(root, "a/b"));
        assert!(confined_to(root, "./a/../b"));
        assert!(!confined_to(root, "../outside"));
        assert!(!confined_to(root, "/etc/passwd"));
    }

    fn shell_name() -> &'static str {
        if cfg!(windows) { "powershell" } else { "bash" }
    }

    #[test]
    fn shell_is_not_treated_as_a_file_path() {
        let root = Path::new("/work");
        let active = Arc::new(Mutex::new(HashSet::new()));
        for name in ["bash", "powershell"] {
            let shell = args(name, json!({ "command": "cd .. && pwd" }));
            assert_eq!(path_guard(root, &[], &active, &shell), None);
        }
        let read = args("read_file", json!({ "path": "../outside" }));
        assert!(path_guard(root, &[], &active, &read).is_some());
    }

    #[test]
    fn production_registry_exposes_file_and_shell_tools_without_todos() {
        let registry = tool_registry(Path::new("/work"), &[], Vec::new());
        let names = registry.list();
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(names.iter().any(|name| name == shell_name()));
        let other = if cfg!(windows) { "bash" } else { "powershell" };
        assert!(!names.iter().any(|name| name == other));
        assert!(!names.iter().any(|name| name == "todo_write"));
    }

    fn installed_skill(scripts_enabled: bool) -> InstalledSkill {
        InstalledSkill {
            id: parse_skill_id("owner", "demo", "1.0.0").unwrap(),
            name: "demo".into(),
            description: "Demo <skill> & helpers".into(),
            root: Path::new("/work/skills/demo").to_path_buf(),
            skill_path: Path::new("/work/skills/demo/SKILL.md").to_path_buf(),
            content: "---\nname: demo\n---\nUse the demo.".into(),
            auto_invoke: true,
            scripts_enabled,
            origin: InstallOrigin::Community,
            latest_version: Some("1.0.0".into()),
        }
    }

    #[test]
    fn slash_skill_injects_full_body_and_keeps_display_text() {
        let skill = installed_skill(false);
        let (prompt, active) =
            prepare_skill_prompt("/skill:demo summarize this", &[skill])
                .unwrap();
        assert!(prompt.model.contains("<skill name=\"demo\""));
        assert!(prompt.model.contains("Use the demo."));
        assert_eq!(
            prompt.display.as_deref(),
            Some("/skill:demo summarize this")
        );
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn available_skill_listing_is_xml_escaped() {
        let listing = append_skill_listing("base", &[installed_skill(false)]);
        assert!(listing.starts_with("base\n\n"));
        assert!(listing.contains("<available_skills>"));
        assert!(listing.contains("Demo &lt;skill&gt; &amp; helpers"));
        assert!(listing.contains("/work/skills/demo/SKILL.md"));
    }

    #[test]
    fn shell_is_blocked_until_active_skill_scripts_are_enabled() {
        let skill = installed_skill(false);
        let active = Arc::new(Mutex::new(HashSet::from([skill.id.clone()])));
        let shell = args(shell_name(), json!({ "command": "echo hi" }));
        assert!(shell_guard(&[skill.clone()], &active, &shell).is_some());
        let mut enabled = skill;
        enabled.scripts_enabled = true;
        assert!(shell_guard(&[enabled], &active, &shell).is_none());
    }

    #[test]
    fn auto_listed_skills_are_in_shell_scope() {
        let skill = installed_skill(false);
        let active =
            Arc::new(Mutex::new(skill_scope(&[skill.clone()], Vec::new())));
        let shell = args(shell_name(), json!({ "command": "echo hi" }));
        assert!(active.lock().unwrap().contains(&skill.id));
        assert!(shell_guard(&[skill], &active, &shell).is_some());
    }

    #[test]
    fn reading_or_grepping_a_skill_marks_it_active_and_allows_its_root() {
        let base = std::env::temp_dir().join(format!(
            "flowy-agent-skill-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cwd = base.join("cwd");
        let skill_root = base.join("skill");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&skill_root).unwrap();
        std::fs::write(skill_root.join("SKILL.md"), "body").unwrap();
        let mut skill = installed_skill(false);
        skill.root = skill_root.clone();
        skill.skill_path = skill_root.join("SKILL.md");
        let active = Arc::new(Mutex::new(HashSet::new()));
        let read = args(
            "read_file",
            json!({ "path": skill.skill_path.display().to_string() }),
        );
        assert_eq!(path_guard(&cwd, &[skill.clone()], &active, &read), None);
        assert!(active.lock().unwrap().contains(&skill.id));

        let grep_active = Arc::new(Mutex::new(HashSet::new()));
        let grep = args(
            "grep",
            json!({
                "path": skill_root.display().to_string(),
                "pattern": "body"
            }),
        );
        assert_eq!(
            path_guard(&cwd, &[skill.clone()], &grep_active, &grep),
            None
        );
        assert!(grep_active.lock().unwrap().contains(&skill.id));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn session_workspace_overrides_default_tool_directory() {
        let root = Path::new("/flowy-data");
        let workspace = AgentWorkspace {
            project_id: "project-a".into(),
            root: Path::new("/work/project-a").to_path_buf(),
        };
        assert_eq!(
            session_working_directory(root, Some(&workspace)),
            workspace.root,
        );
        assert_eq!(
            session_working_directory(root, None),
            root.join("sessions").join("cwd"),
        );
    }

    #[test]
    fn publish_tool_result_uses_ui_key_without_changing_other_tools() {
        let payload = r#"{"status":"staged","message":"File staged.","ui_key":"chat.publish.staged"}"#;
        assert_eq!(
            tool_result_display_text("publish_file", payload),
            "chat.publish.staged"
        );
        assert_eq!(tool_result_display_text("write_file", payload), payload);
    }

    #[test]
    fn file_change_counts_changed_lines_for_edits() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c1"),
            json!({
                "path": "src/a.rs",
                "old_string": "a\nb\nc",
                "new_string": "a\nB\nc",
            }),
        );
        let edit = take_file_change(&args, "s", "c1", Path::new("/work"))
            .expect("edit change");
        assert!(edit.path.ends_with("a.rs"), "path={}", edit.path);
        assert_eq!(edit.kind, FileChangeKind::Update);
        assert_eq!((edit.added, edit.removed), (1, 1));
        assert!(!edit.binary);
        // The cached arguments are dropped once the row is built.
        assert!(
            take_file_change(&args, "s", "c1", Path::new("/work")).is_none()
        );
    }

    #[test]
    fn file_change_reports_only_additions_for_whole_file_writes() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c2"),
            json!({ "path": "new.md", "content": "one\ntwo\n" }),
        );
        let write = take_file_change(&args, "s", "c2", Path::new("/work"))
            .expect("write change");
        assert_eq!(write.kind, FileChangeKind::Add);
        assert_eq!((write.added, write.removed), (2, 0));
    }

    #[test]
    fn relative_tool_paths_resolve_against_the_session_directory() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c3"),
            json!({ "path": "notes/a.md", "content": "x\n" }),
        );
        let change = take_file_change(&args, "s", "c3", Path::new("/work"))
            .expect("change");
        assert!(change.path.contains("notes"), "path={}", change.path);
        assert!(change.path.ends_with("a.md"));
    }

    #[test]
    fn blank_tool_paths_produce_no_row() {
        let args = Mutex::new(HashMap::new());
        args.lock().expect("lock").insert(
            tool_arg_key("s", "c4"),
            json!({ "path": "   ", "content": "x" }),
        );
        assert!(
            take_file_change(&args, "s", "c4", Path::new("/work")).is_none()
        );
    }
}
