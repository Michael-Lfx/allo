//! Agent host port. UI depends on this crate only — never on an agent kernel.
//!
//! Turns run through `flowy-agent-core` (pi loop + provider transport). Chat
//! rows are projected from loop events and persisted next to the structured
//! history, so the UI keeps its snapshot semantics.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::{
    Engine, EngineConfig, LoopOutcome, ModelRoute, RunError, SessionRecord,
    TurnObserver, TurnState, complete_text, echo_stream_fn,
    tools::{
        ToolGuard, ToolRegistry, pi_shell_definition, register_builtin_tools,
    },
    types::{
        CancelToken, ContentBlock, Message, MessageSource, Role, ToolCallArgs,
    },
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
    /// A sidecar title request is queued for this session.
    auto_title_pending: bool,
    /// Cancels the in-flight title request on stop.
    title_signal: CancelToken,
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
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                // Keep the damaged file, but do not let it lock the whole app.
                if recover_corrupt_index(&state.root).is_err() {
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return;
                }
                state.report_storage_error();
                SessionIndex::default()
            }
            Err(_) => {
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
                if recover_corrupt_history(&state.root).is_err() {
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return;
                }
                state.report_storage_error();
                if let Ok(engine) = Engine::open(
                    state.root.clone(),
                    engine_config(&config, &tool_cwd),
                    observer,
                ) {
                    Arc::new(engine)
                } else {
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return;
                }
            }
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
                        auto_title_pending: false,
                        title_signal: CancelToken::new(),
                        status,
                        updated_at: record.updated_at,
                        turn_started_at: None,
                    },
                );
            }
            drop(live);
            *lock(&state.active) = active;
        }
        // If the UI index was lost or quarantined, rebuild the visible sessions
        // from the structured history instead of hiding them.
        import_engine_sessions(&engine, &state);
        // A crash leaves the old status in the file; persist the repaired view.
        if persist_index(&state).is_err() {
            state.report_storage_error();
        }
        hydrate_engine(&engine, &state, &config);
        sync_engine_titles(&engine, &state);
        *lock(&self.inner.engine) = Some(engine);
    }

    pub fn apply_config(&self, config: LlmConfig) -> Result<(), &'static str> {
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
            })
            .collect();
        drop(live);
        rows.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
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
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        // The durable history write may fail on the same broken volume the
        // index uses; the live session still works in memory and the failure is
        // reported through `AgentUpdate::StorageError`.
        if engine.create_session(id.clone(), now).is_err() {
            self.inner.state.report_storage_error();
        }
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
            },
        );
        *lock(&self.inner.state.active) = Some(id.clone());
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        Ok(id)
    }

    pub fn select_session(&self, id: &str) -> Result<(), &'static str> {
        if !lock(&self.inner.state.live).contains_key(id) {
            return Err("notice.agent_failed");
        }
        *lock(&self.inner.state.active) = Some(id.to_owned());
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
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
        let mut title_signal = None;
        let mut fallback_title = None;
        {
            let mut live = lock(&self.inner.state.live);
            let session =
                live.get_mut(&session_id).ok_or("notice.agent_failed")?;
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
                fallback_title = Some(session.title.clone());
                if config.api_key.is_some() {
                    let signal = CancelToken::new();
                    session.title_signal = signal.clone();
                    session.auto_title_pending = true;
                    title_signal = Some(signal);
                }
            }
            drop(live);
        }
        if persist_index(&self.inner.state).is_err() {
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Session {
            id: session_id.clone(),
        });
        self.inner.state.push_update(AgentUpdate::Sessions);

        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        if ensure_engine_session(&engine, &session_id).is_err() {
            self.inner.state.report_storage_error();
        }
        if let Some(title) = fallback_title
            && engine.set_session_title(&session_id, title, true).is_err()
        {
            self.inner.state.report_storage_error();
        }
        let prompt = text.to_owned();
        let id = session_id;
        let state = Arc::clone(&self.inner.state);
        let is_live = config.api_key.is_some();
        let runtime = &self.inner.runtime;
        runtime.spawn(async move {
            if is_live {
                match engine.run(&id, &prompt).await {
                    Ok(LoopOutcome::Completed) => {
                        if let Some(signal) = title_signal {
                            generate_title(
                                Arc::clone(&engine),
                                Arc::clone(&state),
                                config,
                                signal,
                                id,
                                prompt,
                            )
                            .await;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => mark_failure(&state, &id, &error),
                }
            } else {
                let stream = echo_stream_fn(prompt.clone());
                match engine.run_with_stream(&id, &prompt, stream).await {
                    Ok(_) => clear_auto_title(&state, &id),
                    Err(error) => mark_failure(&state, &id, &error),
                }
            }
        });
        Ok(())
    }

    pub fn stop(&self) {
        let Some(id) = lock(&self.inner.state.active).clone() else {
            return;
        };
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = engine {
            let _ = engine.cancel(&id);
        }
        {
            let mut live = lock(&self.inner.state.live);
            if let Some(session) = live.get_mut(&id) {
                session.title_signal.cancel();
                session.projection.flush();
                if session.status.is_busy() {
                    session.status = TurnStatus::Stopped;
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

impl TurnObserver for UiObserver {
    fn text(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_text(delta);
        }
    }

    fn reasoning(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_reasoning(delta);
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
                    // Store an i18n key, never a raw English message: this value
                    // is persisted in the index and may be rendered later.
                    LoopOutcome::Failed => TurnStatus::Error {
                        message: "chat.status_error".into(),
                    },
                };
                live.turn_started_at = None;
                live.updated_at = now_ms();
            }
        }
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.push_update(AgentUpdate::Session {
            id: session.to_owned(),
        });
        self.state.push_update(AgentUpdate::Sessions);
    }

    fn storage_error(&self, _session: &str) {
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
    if engine.set_session_title(&id, title.clone(), true).is_err() {
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

fn clear_auto_title(state: &UiState, id: &str) {
    if let Some(session) = lock(&state.live).get_mut(id) {
        session.auto_title_pending = false;
    }
}

fn mark_failure(state: &UiState, session_id: &str, error: &RunError) {
    if matches!(error, RunError::Busy) {
        return;
    }
    {
        let mut live = lock(&state.live);
        if let Some(session) = live.get_mut(session_id) {
            // A cancelled run may surface a late error after `stop` already set
            // `Stopped`; never overwrite that terminal state.
            if !session.status.is_busy() {
                return;
            }
            session.projection.flush();
            session.status = TurnStatus::Error {
                message: "chat.status_error".into(),
            };
            session.auto_title_pending = false;
            session.turn_started_at = None;
            session.updated_at = now_ms();
        }
    }
    if persist_index(state).is_err() {
        state.report_storage_error();
    }
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
    sessions.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
    let index = SessionIndex {
        active: lock(&state.active).clone(),
        sessions,
    };
    drop(live);
    if index.save(&state.root).is_err() {
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

fn ensure_engine_session(engine: &Engine, id: &str) -> std::io::Result<()> {
    if engine.session(id).is_none() {
        engine.insert_session(SessionRecord {
            id: id.to_owned(),
            ..SessionRecord::default()
        })?;
    }
    Ok(())
}

fn import_engine_sessions(engine: &Engine, state: &UiState) {
    let records = engine.sessions();
    let mut live = lock(&state.live);
    for record in records {
        if live.contains_key(&record.id) {
            continue;
        }
        let lines = lines_from_messages(&record.messages);
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
        live.insert(
            record.id.clone(),
            Live {
                projection: ChatProjection::from_lines(lines),
                title,
                titled,
                auto_title_pending: false,
                title_signal: CancelToken::new(),
                status: live_status(record.status),
                updated_at: record.updated_at,
                turn_started_at: None,
            },
        );
    }
}

fn lines_from_messages(messages: &[Message]) -> Vec<ChatLine> {
    let mut lines = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {}
            Role::User => {
                let text = message.text();
                if !text.is_empty() {
                    lines.push(ChatLine::User { text });
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
                            lines.push(ChatLine::ToolResult {
                                text,
                                is_error: is_error.unwrap_or(false),
                            });
                        }
                    }
                }
            }
        }
    }
    lines
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
            .is_err()
        {
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

fn engine_config(config: &LlmConfig, cwd: &Path) -> EngineConfig {
    EngineConfig {
        route: model_route(config),
        api_key: config.api_key.clone(),
        system: String::new(),
        registry: tool_registry(cwd),
        cwd: Some(cwd.display().to_string()),
        todos: None,
    }
}

fn ensure_tool_cwd(root: &Path) -> PathBuf {
    let dir = root.join("sessions").join("cwd");
    let _ = std::fs::create_dir_all(&dir);
    dir
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

fn tool_registry(cwd: &Path) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.register(pi_shell_definition(cwd));
    // The app has no todo surface yet; expose only tools whose results the
    // host can satisfy instead of advertising one that always fails.
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let guard: ToolGuard =
        Arc::new(move |args: &ToolCallArgs| path_guard(&root, args));
    registry.add_guard(guard);
    registry
}

/// Host policy for file tools. The platform shell is intentionally exempt: it
/// executes commands, not host file paths, and the host has no OS sandbox for it.
fn path_guard(root: &Path, args: &ToolCallArgs) -> Option<String> {
    let key = match args.name.as_str() {
        "read_file" | "write_file" | "edit_file" | "glob" | "grep" => "path",
        _ => return None,
    };
    let raw = args.arguments.get(key).and_then(|value| value.as_str())?;
    if raw.trim().is_empty() {
        return None;
    }
    (!confined_to(root, raw))
        .then(|| format!("path escapes the working directory: {raw}"))
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

#[cfg(test)]
mod guard_tests {
    use super::{confined_to, path_guard, tool_registry};
    use flowy_agent_core::types::ToolCallArgs;
    use serde_json::json;
    use std::path::Path;

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
        for name in ["bash", "powershell"] {
            let shell = args(name, json!({ "command": "cd .. && pwd" }));
            assert_eq!(path_guard(root, &shell), None);
        }
        let read = args("read_file", json!({ "path": "../outside" }));
        assert!(path_guard(root, &read).is_some());
    }

    #[test]
    fn production_registry_exposes_file_and_shell_tools_without_todos() {
        let registry = tool_registry(Path::new("/work"));
        let names = registry.list();
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(names.iter().any(|name| name == shell_name()));
        let other = if cfg!(windows) { "bash" } else { "powershell" };
        assert!(!names.iter().any(|name| name == other));
        assert!(!names.iter().any(|name| name == "todo_write"));
    }
}
