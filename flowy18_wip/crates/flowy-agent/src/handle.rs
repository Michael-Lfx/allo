//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    Engine, EngineConfig, LoopOutcome, ModelRoute, RunError, SessionRecord, TurnObserver,
    TurnState, echo_stream_fn,
    types::{ContentBlock, Message, MessageSource, Role},
};

use crate::config::LlmConfig;
use crate::index::{SessionIndex, SessionRecord as IndexRecord};
use crate::projection::{ChatLine, ChatProjection, TurnStatus};
use crate::title::{self, TITLE_MAX_CHARS};

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
}

#[derive(Clone, Debug)]
pub enum AgentUpdate {
    Sessions,
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
    status: TurnStatus,
    updated_at: u64,
    turn_started_at: Option<u64>,
}

struct UiState {
    root: PathBuf,
    live: Mutex<HashMap<String, Live>>,
    active: Mutex<Option<String>>,
    updates: Mutex<VecDeque<AgentUpdate>>,
    load_failed: Mutex<bool>,
}

impl UiState {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            live: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            updates: Mutex::new(VecDeque::new()),
            load_failed: Mutex::new(false),
        }
    }

    fn push_update(&self, update: AgentUpdate) {
        lock(&self.updates).push_back(update);
    }

    fn report_storage_error(&self) {
        self.push_update(AgentUpdate::StorageError);
    }
}

struct Harness {
    runtime: tokio::runtime::Runtime,
    state: Arc<UiState>,
    engine: Mutex<Option<Arc<Engine>>>,
    config: Mutex<LlmConfig>,
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("flowy-agent")
            .build()
            .expect("tokio runtime for agent harness");
        let state = Arc::new(UiState::new(store_root));
        let handle = Self {
            inner: Arc::new(Harness {
                runtime,
                state: Arc::clone(&state),
                engine: Mutex::new(None),
                config: Mutex::new(config.clone()),
            }),
        };
        handle.boot(config);
        handle
    }

    fn boot(&self, config: LlmConfig) {
        let state = Arc::clone(&self.inner.state);
        let index = match SessionIndex::load(&state.root) {
            Ok(index) => index,
            Err(_) => {
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return;
            }
        };
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let engine = match Engine::open(state.root.clone(), engine_config(&config), observer) {
            Ok(engine) => Arc::new(engine),
            Err(_) => {
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return;
            }
        };
        let SessionIndex { active, sessions } = index;
        {
            let mut live = lock(&state.live);
            for record in sessions {
                let mut status = record.status;
                let mut lines = record.lines;
                if status == TurnStatus::Running {
                    status = TurnStatus::Stopped;
                    finish_interrupted_lines(&mut lines);
                }
                live.insert(
                    record.id,
                    Live {
                        projection: ChatProjection::from_lines(lines),
                        title: record.title,
                        titled: record.titled,
                        status,
                        updated_at: record.updated_at,
                        turn_started_at: None,
                    },
                );
            }
            *lock(&state.active) = active;
        }
        // A crash leaves the old status in the file; persist the repaired view.
        let _ = persist_index(&state);
        hydrate_engine(&engine, &state, &config);
        *lock(&self.inner.engine) = Some(engine);
    }

    pub fn apply_config(&self, config: LlmConfig) -> Result<(), &'static str> {
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = engine {
            engine.set_config(engine_config(&config));
        }
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
            })
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        rows
    }

    pub fn active_session_id(&self) -> Option<String> {
        lock(&self.inner.state.active).clone()
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
        let active = lock(&self.inner.state.active).clone();
        active
            .and_then(|id| {
                lock(&self.inner.state.live)
                    .get(&id)
                    .map(|session| session.projection.visible_lines())
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
        if *lock(&self.inner.state.load_failed) {
            return Err("notice.storage_error");
        }
        let id = format!("s-{}", next_stamp());
        let now = now_ms();
        let engine = lock(&self.inner.engine).clone().ok_or("notice.agent_failed")?;
        // The durable history write may fail on the same broken volume the
        // index uses; the live session still works in memory and the failure is
        // reported through `AgentUpdate::StorageError`.
        let _ = engine.create_session(id.clone(), now);
        lock(&self.inner.state.live).insert(
            id.clone(),
            Live {
                projection: ChatProjection::new(),
                title: String::new(),
                titled: false,
                status: TurnStatus::Idle,
                updated_at: now,
                turn_started_at: None,
            },
        );
        *lock(&self.inner.state.active) = Some(id.clone());
        let _ = persist_index(&self.inner.state);
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(id)
    }

    pub fn select_session(&self, id: &str) -> Result<(), &'static str> {
        if !lock(&self.inner.state.live).contains_key(id) {
            return Err("notice.agent_failed");
        }
        *lock(&self.inner.state.active) = Some(id.to_owned());
        let _ = persist_index(&self.inner.state);
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner
            .state
            .push_update(AgentUpdate::Session { id: id.to_owned() });
        Ok(())
    }

    pub fn send(&self, text: &str) -> Result<(), &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        if *lock(&self.inner.state.load_failed) {
            return Err("notice.storage_error");
        }
        let session_id = self.ensure_active_session()?;
        let config = lock(&self.inner.config).clone();
        {
            let mut live = lock(&self.inner.state.live);
            let session = live.get_mut(&session_id).ok_or("notice.agent_failed")?;
            if session.status.is_busy() {
                return Err("notice.agent_busy");
            }
            let now = now_ms();
            session.status = TurnStatus::Running;
            session.turn_started_at = Some(now);
            session.updated_at = now;
            session.projection.push_user(text);
            if !session.titled {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
            }
        }
        let _ = persist_index(&self.inner.state);
        self.inner
            .state
            .push_update(AgentUpdate::Session { id: session_id.clone() });
        self.inner.state.push_update(AgentUpdate::Sessions);

        let engine = lock(&self.inner.engine).clone().ok_or("notice.agent_failed")?;
        ensure_engine_session(&engine, &session_id);
        let prompt = text.to_owned();
        let id = session_id.clone();
        let state = Arc::clone(&self.inner.state);
        let runtime = &self.inner.runtime;
        if config.api_key.is_some() {
            runtime.spawn(async move {
                if let Err(error) = engine.run(&id, &prompt).await {
                    mark_failure(&state, &id, &error);
                }
            });
        } else {
            let stream = echo_stream_fn(prompt.clone());
            runtime.spawn(async move {
                let _ = engine.run_with_stream(&id, &prompt, stream).await;
            });
        }
        Ok(())
    }

    pub fn stop(&self) {
        let Some(id) = lock(&self.inner.state.active).clone() else {
            return;
        };
        if let Some(engine) = lock(&self.inner.engine).clone() {
            let _ = engine.cancel(&id);
        }
        {
            let mut live = lock(&self.inner.state.live);
            if let Some(session) = live.get_mut(&id) {
                session.projection.flush();
                if session.status.is_busy() {
                    session.status = TurnStatus::Stopped;
                }
                session.turn_started_at = None;
                session.updated_at = now_ms();
            }
        }
        let _ = persist_index(&self.inner.state);
        self.inner
            .state
            .push_update(AgentUpdate::Session { id });
    }

    fn ensure_active_session(&self) -> Result<String, &'static str> {
        if let Some(id) = lock(&self.inner.state.active).clone() {
            if lock(&self.inner.state.live).contains_key(&id) {
                return Ok(id);
            }
        }
        self.create_session()
    }
}

struct UiObserver {
    state: Arc<UiState>,
}

impl TurnObserver for UiObserver {
    fn text(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_text(delta);
        }
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
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection
                .push_tool_call(name, tool_target(args), call_id);
            live.updated_at = now_ms();
        }
    }

    fn tool_end(
        &self,
        session: &str,
        call_id: &str,
        _name: &str,
        text: &str,
        is_error: bool,
    ) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.finish_tool_call(call_id, text, is_error);
            live.updated_at = now_ms();
        }
    }

    fn finished(&self, session: &str, outcome: LoopOutcome) {
        {
            let mut live = lock(&self.state.live);
            if let Some(live) = live.get_mut(session) {
                live.projection.flush();
                live.status = match outcome {
                    LoopOutcome::Completed => TurnStatus::Completed,
                    LoopOutcome::Aborted => TurnStatus::Stopped,
                    LoopOutcome::Failed => TurnStatus::Error {
                        message: "notice.agent_failed".into(),
                    },
                };
                live.turn_started_at = None;
                live.updated_at = now_ms();
            }
        }
        let _ = persist_index(&self.state);
        self.state
            .push_update(AgentUpdate::Session {
                id: session.to_owned(),
            });
        self.state.push_update(AgentUpdate::Sessions);
    }
}

fn mark_failure(state: &UiState, session_id: &str, error: &RunError) {
    if matches!(error, RunError::Busy) {
        return;
    }
    {
        let mut live = lock(&state.live);
        if let Some(session) = live.get_mut(session_id) {
            session.projection.flush();
            session.status = TurnStatus::Error {
                message: error.to_string(),
            };
            session.turn_started_at = None;
            session.updated_at = now_ms();
        }
    }
    let _ = persist_index(state);
    state.push_update(AgentUpdate::Session {
        id: session_id.to_owned(),
    });
}

fn persist_index(state: &UiState) -> Result<(), &'static str> {
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
        })
        .collect();
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let index = SessionIndex {
        active: lock(&state.active).clone(),
        sessions,
    };
    drop(live);
    if index.save(&state.root).is_err() {
        state.report_storage_error();
        return Err("notice.storage_error");
    }
    Ok(())
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

fn ensure_engine_session(engine: &Engine, id: &str) {
    if engine.session(id).is_none() {
        let _ = engine.insert_session(SessionRecord {
            id: id.to_owned(),
            ..SessionRecord::default()
        });
    }
}

fn hydrate_engine(engine: &Engine, state: &UiState, config: &LlmConfig) {    let known: std::collections::HashSet<String> = engine
        .sessions()
        .into_iter()
        .map(|record| record.id)
        .collect();
    let live = lock(&state.live);
    for (id, session) in live.iter() {
        if known.contains(id) {
            continue;
        }
        let _ = engine.insert_session(SessionRecord {
            id: id.clone(),
            title: session.title.clone(),
            titled: session.titled,
            updated_at: session.updated_at,
            status: TurnState::Stopped,
            messages: messages_from_lines(&session.projection.lines, config),
            todos: Vec::new(),
        });
    }
}

fn messages_from_lines(lines: &[ChatLine], config: &LlmConfig) -> Vec<Message> {
    let mut messages = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match line {
            ChatLine::User { text } if !text.is_empty() => messages.push(Message {
                id: format!("legacy-u-{index}"),
                role: Role::User,
                content: vec![ContentBlock::text(text.clone())],
                source: MessageSource::User,
            }),
            ChatLine::Assistant { text } => messages.push(Message {
                id: format!("legacy-a-{index}"),
                role: Role::Assistant,
                content: vec![ContentBlock::text(text.clone())],
                source: MessageSource::Model {
                    provider: config.provider.clone(),
                    model: config.model.clone(),
                },
            }),
            _ => {}
        }
    }
    messages
}

fn engine_config(config: &LlmConfig) -> EngineConfig {
    EngineConfig {
        route: ModelRoute {
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
        },
        api_key: config.api_key.clone(),
        system: String::new(),
        registry: flowy_agent_core::tools::ToolRegistry::new(),
        cwd: None,
        todos: None,
    }
}

fn tool_target(args: &serde_json::Value) -> String {
    for key in ["path", "file_path", "command", "query", "pattern", "target"] {
        if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
            return value.to_owned();
        }
    }
    String::new()
}

fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = now_ms().saturating_mul(1000);
    stamp + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
