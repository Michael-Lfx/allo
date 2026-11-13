use cordis::Context as CordisContext;
use dsh_rs::api::events::{self, SessionEventPayload};
use dsh_rs::api::services::{
    AgentRegistryService, AgentView, LlmAdapterApi, LlmService, SessionPersistenceApi,
};
use dsh_rs::bundle::{install_base, BaseConfig};
use dsh_rs::types::{
    stream_from_chunks, AgentCancelCause, AgentOptions, BoxStream, ContentBlock, FinishReason,
    GenerateOptions, LlmError, Message, MessageSource, Role, SessionEventData, StreamChunk,
    TurnEndReason,
};
use futures::StreamExt;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::LlmConfig;
use crate::index::{SessionIndex, SessionRecord};
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
    Session { id: String },
}

struct LiveSession {
    agent: Arc<dyn AgentView>,
    projection: ChatProjection,
    title: String,
    titled: bool,
    status: TurnStatus,
    updated_at: u64,
}

struct Shared {
    live: Mutex<HashMap<String, LiveSession>>,
    active: Mutex<Option<String>>,
    updates: Mutex<VecDeque<AgentUpdate>>,
    store_root: PathBuf,
}

struct Booted {
    _ctx: CordisContext,
    _listener: cordis::fiber::EffectGuard,
    agents: AgentRegistryService,
    persistence: Option<Arc<dyn SessionPersistenceApi>>,
    llm: LlmService,
    config: LlmConfig,
    cwd: PathBuf,
    shared: Arc<Shared>,
}

struct Harness {
    runtime: tokio::runtime::Runtime,
    store_root: PathBuf,
    config: LlmConfig,
    booted: Option<Booted>,
}

#[derive(Clone)]
pub struct AgentHandle {
    inner: Arc<Mutex<Harness>>,
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
            .thread_name("flowy-dsh")
            .build()
            .expect("tokio runtime for dsh harness");
        Self {
            inner: Arc::new(Mutex::new(Harness {
                runtime,
                store_root,
                config,
                booted: None,
            })),
        }
    }

    pub fn apply_config(&self, config: LlmConfig) -> Result<(), &'static str> {
        let mut guard = lock(&self.inner);
        guard.config = config.clone();
        let Some(booted) = guard.booted.as_mut() else {
            return Ok(());
        };
        apply_live_config(booted, &config)
    }

    pub fn drain_updates(&self) -> Vec<AgentUpdate> {
        let guard = lock(&self.inner);
        let Some(booted) = guard.booted.as_ref() else {
            return Vec::new();
        };
        booted
            .shared
            .updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let mut guard = lock(&self.inner);
        if guard.ensure_booted().is_err() {
            return Vec::new();
        }
        let booted = guard.booted.as_ref().expect("booted");
        let live = lock(&booted.shared.live);
        let mut rows: Vec<_> = live
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
        let mut guard = lock(&self.inner);
        let _ = guard.ensure_booted();
        guard.booted.as_ref().and_then(|booted| {
            lock(&booted.shared.active).clone()
        })
    }

    pub fn has_messages(&self) -> bool {
        !self.lines().is_empty()
    }

    pub fn lines(&self) -> Vec<ChatLine> {
        let mut guard = lock(&self.inner);
        let _ = guard.ensure_booted();
        let Some(booted) = guard.booted.as_ref() else {
            return Vec::new();
        };
        let active = lock(&booted.shared.active).clone();
        let Some(id) = active else {
            return Vec::new();
        };
        lock(&booted.shared.live)
            .get(&id)
            .map(|session| session.projection.visible_lines())
            .unwrap_or_default()
    }

    pub fn turn_status(&self) -> TurnStatus {
        let mut guard = lock(&self.inner);
        let _ = guard.ensure_booted();
        let Some(booted) = guard.booted.as_ref() else {
            return TurnStatus::Idle;
        };
        let active = lock(&booted.shared.active).clone();
        let Some(id) = active else {
            return TurnStatus::Idle;
        };
        lock(&booted.shared.live)
            .get(&id)
            .map(|session| session.status.clone())
            .unwrap_or_default()
    }

    pub fn session_title(&self) -> String {
        let mut guard = lock(&self.inner);
        let _ = guard.ensure_booted();
        let Some(booted) = guard.booted.as_ref() else {
            return String::new();
        };
        let active = lock(&booted.shared.active).clone();
        let Some(id) = active else {
            return String::new();
        };
        lock(&booted.shared.live)
            .get(&id)
            .map(|session| session.title.clone())
            .unwrap_or_default()
    }

    pub fn create_session(&self) -> Result<String, &'static str> {
        let mut guard = lock(&self.inner);
        guard.ensure_booted()?;
        let id = guard.spawn_session(None, String::new(), Vec::new(), false)?;
        guard.set_active(&id);
        guard.persist_index();
        guard.push_update(AgentUpdate::Sessions);
        Ok(id)
    }

    pub fn select_session(&self, id: &str) -> Result<(), &'static str> {
        let mut guard = lock(&self.inner);
        guard.ensure_booted()?;
        {
            let booted = guard.booted.as_ref().ok_or("notice.agent_failed")?;
            if !lock(&booted.shared.live).contains_key(id) {
                return Err("notice.agent_failed");
            }
        }
        guard.set_active(id);
        guard.persist_index();
        guard.push_update(AgentUpdate::Sessions);
        guard.push_update(AgentUpdate::Session {
            id: id.to_owned(),
        });
        Ok(())
    }

    pub fn send(&self, text: &str) -> Result<(), &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        let handle = self.clone();
        let mut guard = lock(&self.inner);
        guard.ensure_booted()?;
        if guard.active_id().is_none() {
            let id = guard.spawn_session(None, String::new(), Vec::new(), false)?;
            guard.set_active(&id);
        }
        let session_id = guard.active_id().ok_or("notice.agent_failed")?;
        let (agent, need_title) = {
            let booted = guard.booted.as_mut().ok_or("notice.agent_failed")?;
            let mut live = lock(&booted.shared.live);
            let session = live.get_mut(&session_id).ok_or("notice.agent_failed")?;
            if matches!(session.status, TurnStatus::Running) {
                return Err("notice.agent_busy");
            }
            let need_title = !session.titled;
            session.status = TurnStatus::Running;
            session.updated_at = now_ms();
            session.projection.lines.push(ChatLine::User {
                text: text.to_owned(),
            });
            (Arc::clone(&session.agent), need_title)
        };
        guard.persist_index();
        guard.push_update(AgentUpdate::Session {
            id: session_id.clone(),
        });
        guard.push_update(AgentUpdate::Sessions);

        let message_id = format!("u-{}", next_stamp());
        let runtime = &guard.runtime;
        runtime.spawn({
            let session_id = session_id.clone();
            let handle = handle.clone();
            let text = text.to_owned();
            async move {
                agent.followup(Message::user(
                    message_id,
                    vec![ContentBlock::text(text)],
                ));
                agent.when_idle().await;
                let guard = lock(&handle.inner);
                if let Some(booted) = guard.booted.as_ref() {
                    let mut live = lock(&booted.shared.live);
                    if let Some(session) = live.get_mut(&session_id) {
                        session.projection.flush_pending();
                        if matches!(session.status, TurnStatus::Running) {
                            session.status = TurnStatus::Completed;
                        }
                        session.updated_at = now_ms();
                    }
                    drop(live);
                    persist_shared(booted);
                    push_shared(booted, AgentUpdate::Session { id: session_id });
                }
            }
        });

        if need_title {
            let prompt = text.to_owned();
            let session_id = session_id.clone();
            runtime.spawn({
                let handle = handle.clone();
                async move {
                    handle.autotitle(session_id, prompt).await;
                }
            });
        }
        Ok(())
    }

    pub fn stop(&self) {
        let mut guard = lock(&self.inner);
        let _ = guard.ensure_booted();
        let Some(booted) = guard.booted.as_ref() else {
            return;
        };
        let Some(id) = lock(&booted.shared.active).clone() else {
            return;
        };
        {
            let _enter = guard.runtime.enter();
            let mut live = lock(&booted.shared.live);
            if let Some(session) = live.get_mut(&id) {
                session.agent.cancel(AgentCancelCause::User, false);
                session.status = TurnStatus::Stopped;
                session.updated_at = now_ms();
            }
        }
        persist_shared(booted);
        push_shared(booted, AgentUpdate::Session { id });
    }

    async fn autotitle(&self, session_id: String, prompt: String) {
        let (llm, config) = {
            let guard = lock(&self.inner);
            let Some(booted) = guard.booted.as_ref() else {
                return;
            };
            {
                let live = lock(&booted.shared.live);
                if live.get(&session_id).is_some_and(|session| session.titled) {
                    return;
                }
            }
            (booted.llm.clone(), booted.config.clone())
        };

        let mut title = String::new();
        if config.uses_live() {
            let options = GenerateOptions {
                provider: config.provider.clone(),
                model: config.model.clone(),
                messages: vec![Message::user(
                    format!("title-{}", next_stamp()),
                    vec![ContentBlock::text(
                        prompt.chars().take(2000).collect::<String>(),
                    )],
                )],
                system: Some(title::title_system_prompt().to_owned()),
                tools: None,
                temperature: Some(0.2),
                max_tokens: Some(64),
                stop: None,
                session_id: None,
            };
            if let Ok(stream) = llm.stream(options).await {
                let mut text = String::new();
                let mut stream = stream;
                while let Some(chunk) = stream.next().await {
                    if let StreamChunk::TextDelta { text: delta, .. } = chunk {
                        text.push_str(&delta);
                    }
                }
                title = title::clamp_title(&text, TITLE_MAX_CHARS);
            }
        }
        if title.is_empty() {
            title = title::fallback_title(&prompt, TITLE_MAX_CHARS);
        }
        if title.is_empty() {
            return;
        }

        let guard = lock(&self.inner);
        let Some(booted) = guard.booted.as_ref() else {
            return;
        };
        {
            let mut live = lock(&booted.shared.live);
            let Some(session) = live.get_mut(&session_id) else {
                return;
            };
            if session.titled {
                return;
            }
            session.title = title;
            session.titled = true;
            session.updated_at = now_ms();
        }
        persist_shared(booted);
        push_shared(booted, AgentUpdate::Sessions);
        push_shared(
            booted,
            AgentUpdate::Session {
                id: session_id,
            },
        );
    }
}

impl Harness {
    fn ensure_booted(&mut self) -> Result<(), &'static str> {
        if self.booted.is_some() {
            return Ok(());
        }
        let shared = Arc::new(Shared {
            live: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            updates: Mutex::new(VecDeque::new()),
            store_root: self.store_root.clone(),
        });
        let shared_boot = Arc::clone(&shared);
        let config = self.config.clone();
        let booted = self
            .runtime
            .block_on(boot(self.store_root.clone(), shared_boot, config))
            .map_err(|_| "notice.agent_failed")?;
        self.booted = Some(booted);
        self.restore_from_index()?;
        Ok(())
    }

    fn active_id(&self) -> Option<String> {
        self.booted
            .as_ref()
            .and_then(|booted| lock(&booted.shared.active).clone())
    }

    fn set_active(&self, id: &str) {
        if let Some(booted) = self.booted.as_ref() {
            *lock(&booted.shared.active) = Some(id.to_owned());
        }
    }

    fn spawn_session(
        &self,
        id: Option<String>,
        title: String,
        lines: Vec<ChatLine>,
        titled: bool,
    ) -> Result<String, &'static str> {
        let booted = self.booted.as_ref().ok_or("notice.agent_failed")?;
        let options = AgentOptions {
            provider: booted.config.agent_provider().to_owned(),
            model: booted.config.agent_model().to_owned(),
            max_tokens: None,
        };
        let cwd = booted.cwd.display().to_string();
        let _enter = self.runtime.enter();
        let agent = booted
            .agents
            .create(id, options, Some(cwd), None)
            .map_err(|_| "notice.agent_failed")?;
        let session_id = agent.id().to_owned();
        if !lines.is_empty() {
            replay_lines(agent.as_ref(), &lines);
        }
        lock(&booted.shared.live).insert(
            session_id.clone(),
            LiveSession {
                agent,
                projection: ChatProjection::from_lines(lines),
                title,
                titled,
                status: TurnStatus::Idle,
                updated_at: now_ms(),
            },
        );
        Ok(session_id)
    }

    fn restore_from_index(&mut self) -> Result<(), &'static str> {
        let index = SessionIndex::load(&self.store_root);
        for record in index.sessions {
            let exists = self.booted.as_ref().is_some_and(|booted| {
                lock(&booted.shared.live).contains_key(&record.id)
            });
            if exists {
                continue;
            }
            let id = self.spawn_session(
                Some(record.id.clone()),
                record.title,
                record.lines,
                record.titled,
            )?;
            if let Some(booted) = self.booted.as_ref() {
                if let Some(session) = lock(&booted.shared.live).get_mut(&id) {
                    session.status = record.status;
                    session.updated_at = record.updated_at;
                }
            }
        }
        if let Some(booted) = self.booted.as_ref() {
            if let Some(active) = index.active {
                if lock(&booted.shared.live).contains_key(&active) {
                    *lock(&booted.shared.active) = Some(active);
                }
            }
            if lock(&booted.shared.active).is_none() {
                let first = lock(&booted.shared.live).keys().next().cloned();
                *lock(&booted.shared.active) = first;
            }
        }
        Ok(())
    }

    fn persist_index(&self) {
        if let Some(booted) = self.booted.as_ref() {
            persist_shared(booted);
        }
    }

    fn push_update(&self, update: AgentUpdate) {
        if let Some(booted) = self.booted.as_ref() {
            push_shared(booted, update);
        }
    }
}

fn persist_shared(booted: &Booted) {
    let live = lock(&booted.shared.live);
    let active = lock(&booted.shared.active).clone();
    let mut index = SessionIndex {
        active,
        sessions: Vec::new(),
    };
    for (id, session) in live.iter() {
        index.upsert(SessionRecord {
            id: id.clone(),
            title: session.title.clone(),
            updated_at: session.updated_at,
            lines: session.projection.visible_lines(),
            status: session.status.clone(),
            titled: session.titled,
        });
    }
    drop(live);
    index.save(&booted.shared.store_root);
    if let Some(persistence) = &booted.persistence {
        if let Some(id) = lock(&booted.shared.active).clone() {
            let _ = persistence.flush(&id);
        }
    }
}

fn push_shared(booted: &Booted, update: AgentUpdate) {
    lock(&booted.shared.updates).push_back(update);
}

fn apply_event(shared: &Shared, session_id: &str, event: &dsh_rs::types::SessionEvent) {
    let mut live = lock(&shared.live);
    let Some(session) = live.get_mut(session_id) else {
        return;
    };
    // Skip duplicate optimistic user bubble.
    if let SessionEventData::UserMessage { message } = &event.data {
        let text = message.text();
        if session
            .projection
            .lines
            .last()
            .is_some_and(|line| matches!(line, ChatLine::User { text: existing } if *existing == text))
        {
            return;
        }
    }
    if session.projection.apply(event) {
        session.updated_at = now_ms();
        if let SessionEventData::TurnEnd {
            reason: TurnEndReason::Error { message, .. },
            ..
        } = &event.data
        {
            session.status = TurnStatus::Error {
                message: message.clone(),
            };
        }
        drop(live);
        lock(&shared.updates).push_back(AgentUpdate::Session {
            id: session_id.to_owned(),
        });
    }
}

fn replay_lines(agent: &dyn AgentView, lines: &[ChatLine]) {
    let session = agent.session();
    let mut turn = 1_u64;
    for line in lines {
        match line {
            ChatLine::User { text } => {
                session.append(SessionEventData::TurnStart { turn });
                session.append(SessionEventData::UserMessage {
                    message: Message::user(
                        format!("replay-u-{turn}"),
                        vec![ContentBlock::text(text.clone())],
                    ),
                });
            }
            ChatLine::Assistant { text } => {
                session.append(SessionEventData::AssistantMessage {
                    turn,
                    step: 0,
                    message: Message {
                        id: format!("replay-a-{turn}"),
                        role: Role::Assistant,
                        content: vec![ContentBlock::text(text.clone())],
                        source: MessageSource::Model {
                            provider: "replay".into(),
                            model: "replay".into(),
                        },
                    },
                    usage: None,
                    interrupted: None,
                });
                session.append(SessionEventData::TurnEnd {
                    turn,
                    reason: TurnEndReason::Completed,
                });
                turn = turn.saturating_add(1);
            }
            ChatLine::Tool { text } | ChatLine::Status { text } => {
                session.append(SessionEventData::ToolResult {
                    turn,
                    step: 0,
                    message: Message {
                        id: format!("replay-t-{turn}"),
                        role: Role::User,
                        content: vec![ContentBlock::text(text.clone())],
                        source: MessageSource::User,
                    },
                });
            }
        }
    }
}

fn apply_live_config(booted: &mut Booted, config: &LlmConfig) -> Result<(), &'static str> {
    let previous = booted.config.agent_provider().to_owned();
    if previous != "mock" {
        booted.llm.unregister_adapter(&[&previous]);
    }
    if config.uses_live() {
        let section = config.openai_section().ok_or("notice.agent_failed")?;
        let adapter = dsh_rs::bundle::openai_compatible_adapter(section)
            .map_err(|_| "notice.agent_failed")?;
        let route = config.agent_provider();
        booted.llm.unregister_adapter(&[route]);
        booted
            .llm
            .register_adapter(&[route], adapter)
            .map_err(|_| "notice.agent_failed")?;
    } else {
        booted.llm.unregister_adapter(&["mock"]);
        booted
            .llm
            .register_adapter(&["mock"], Arc::new(ScriptedMock::echo()))
            .map_err(|_| "notice.agent_failed")?;
    }
    booted.config = config.clone();
    Ok(())
}

async fn boot(
    store_root: PathBuf,
    shared: Arc<Shared>,
    config: LlmConfig,
) -> Result<Booted, String> {
    let sessions_dir = store_root.join("sessions");
    let _ = std::fs::create_dir_all(&sessions_dir);
    let cwd = sessions_dir.join("cwd");
    let _ = std::fs::create_dir_all(&cwd);

    let ctx = CordisContext::new();
    install_base(
        &ctx,
        BaseConfig {
            store_dir: Some(sessions_dir),
            openai: config.openai_section(),
            default_provider: Some(config.agent_provider().to_owned()),
            default_model: Some(config.agent_model().to_owned()),
            ..BaseConfig::default()
        },
    )
    .await?;

    if !config.uses_live() {
        let llm = ctx
            .require::<LlmService>(dsh_rs::api::LLM_SERVICE)
            .map_err(|error| error.to_string())?;
        llm.unregister_adapter(&["mock"]);
        llm.register_adapter(&["mock"], Arc::new(ScriptedMock::echo()))
            .map_err(|error| error.to_string())?;
    }

    let agents = ctx
        .require::<AgentRegistryService>(dsh_rs::api::AGENTS_SERVICE)
        .map_err(|error| error.to_string())?;
    let agents = (*agents).clone();
    let persistence = ctx
        .get::<Arc<dyn SessionPersistenceApi>>(dsh_rs::api::SESSION_PERSISTENCE_SERVICE)
        .map(|wrapped| (*wrapped).clone());
    let llm = ctx
        .require::<LlmService>(dsh_rs::api::LLM_SERVICE)
        .map_err(|error| error.to_string())?;
    let llm = (*llm).clone();

    let shared_listener = Arc::clone(&shared);
    let listener = events::on_sync::<SessionEventPayload, _>(&ctx, move |_ctx, payload| {
        let shared_listener = Arc::clone(&shared_listener);
        Box::pin(async move {
            apply_event(&shared_listener, &payload.session, &payload.event);
            Ok(serde_json::Value::Null)
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Booted {
        _ctx: ctx,
        _listener: listener,
        agents,
        persistence,
        llm,
        config,
        cwd,
        shared,
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn next_stamp() -> u64 {
    static STAMP: AtomicU64 = AtomicU64::new(0);
    STAMP.fetch_add(1, Ordering::Relaxed)
}

struct ScriptedMock {
    script: Mutex<VecDeque<Vec<StreamChunk>>>,
}

impl ScriptedMock {
    fn echo() -> Self {
        Self {
            script: Mutex::new(VecDeque::new()),
        }
    }
}

impl LlmAdapterApi for ScriptedMock {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn stream(
        &self,
        options: GenerateOptions,
    ) -> dsh_rs::api::services::BoxFuture<Result<BoxStream<StreamChunk>, LlmError>> {
        let scripted = self
            .script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        Box::pin(async move {
            let chunks = match scripted {
                Some(script) => script,
                None => {
                    let echoed = options
                        .messages
                        .iter()
                        .rev()
                        .find(|message| message.role == Role::User)
                        .map(Message::text)
                        .unwrap_or_default();
                    text_response(&echoed)
                }
            };
            Ok(stream_from_chunks(chunks))
        })
    }
}

fn text_response(text: &str) -> Vec<StreamChunk> {
    vec![
        StreamChunk::BlockStart {
            index: 0,
            block_type: "text".into(),
        },
        StreamChunk::TextDelta {
            index: 0,
            text: text.to_owned(),
        },
        StreamChunk::BlockEnd {
            index: 0,
            block: ContentBlock::Text {
                text: text.to_owned(),
            },
        },
        StreamChunk::Finish {
            reason: FinishReason::Stop,
        },
    ]
}
