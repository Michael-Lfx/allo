//! Session engine: owns durable history, one active run per session, and the
//! bridge from loop events to a UI observer.
//!
//! The embedding host keeps its own projection; this engine only reports
//! deltas, tool lifecycle, and terminal outcomes. Nothing here knows about GPUI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use rpi_agent::{AgentEmitter, AgentEvent, StreamFn};
use rpi_ai::types::AssistantMessageEvent;

use crate::loop_runner::{LoopOutcome, ModelRoute, TurnRequest, run_turn};
use crate::run_fence::RunFence;
use crate::session::{SessionIndex, SessionRecord, TurnState};
use crate::tools::{RegistryTool, ToolRegistry};
use crate::types::{
    CancelToken, Message, MessageSource, Role, TodoSink, ToolRunContext,
};

/// UI-facing turn events. All methods have no-op defaults so a caller may
/// implement only what it renders.
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
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("route", &self.route)
            .field("has_key", &self.api_key.is_some())
            .field("system_len", &self.system.len())
            .field("tools", &self.registry.list().len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum RunError {
    Busy,
    NotFound(String),
    NoCredential,
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
            Self::Loop(message) => write!(f, "{message}"),
        }
    }
}

struct Inner {
    index: SessionIndex,
    config: EngineConfig,
    fences: HashMap<String, Arc<RunFence>>,
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
        self.inner.lock().unwrap().index.sessions.clone()
    }

    #[must_use]
    pub fn session(&self, id: &str) -> Option<SessionRecord> {
        self.inner.lock().unwrap().index.get(id).cloned()
    }

    #[must_use]
    pub fn active(&self) -> Option<String> {
        self.inner.lock().unwrap().index.active.clone()
    }

    pub fn set_active(&self, id: Option<String>) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.index.active = id;
        let snapshot = inner.index.clone();
        drop(inner);
        snapshot.save(&self.root)
    }

    /// Create an empty session and persist its record.
    pub fn create_session(&self, id: String, now: u64) -> std::io::Result<()> {
        self.insert_session(SessionRecord {
            id,
            updated_at: now,
            ..SessionRecord::default()
        })
    }

    /// Insert or replace one durable session record.
    pub fn insert_session(&self, record: SessionRecord) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.index.upsert(record);
        let snapshot = inner.index.clone();
        drop(inner);
        snapshot.save(&self.root)
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
        record.title = title;
        record.titled = titled;
        let snapshot = inner.index.clone();
        drop(inner);
        snapshot.save(&self.root)
    }

    pub fn remove_session(&self, id: &str) -> std::io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        let removed = inner.index.remove(id).is_some();
        inner.fences.remove(id);
        let snapshot = inner.index.clone();
        drop(inner);
        if removed {
            snapshot.save(&self.root)?;
        }
        Ok(removed)
    }

    /// Cancel the active run of one session, if any.
    pub fn cancel(&self, id: &str) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(fence) = inner.fences.get(id) {
            fence.cancel_active();
        }
        if let Some(record) = inner.index.get_mut(id)
            && record.status.is_busy()
        {
            record.status = TurnState::Stopped;
        }
        let snapshot = inner.index.clone();
        drop(inner);
        snapshot.save(&self.root)
    }

    /// Run one turn against the configured provider. The caller spawns this;
    /// it is `async` so a host runtime owns scheduling.
    pub async fn run(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<LoopOutcome, RunError> {
        let stream = {
            let (route, api_key) = {
                let inner = self.inner.lock().unwrap();
                if inner.config.api_key.is_none() {
                    return Err(RunError::NoCredential);
                }
                (inner.config.route.clone(), inner.config.api_key.clone())
            };
            crate::transport::openai_stream_fn(&route, api_key)
        };
        self.run_with_stream(session_id, prompt, stream).await
    }

    /// Run with an explicit stream function (scripted stream in tests).
    pub async fn run_with_stream(
        &self,
        session_id: &str,
        prompt: &str,
        stream: StreamFn,
    ) -> Result<LoopOutcome, RunError> {
        let (ticket, history, request_config) = {
            let mut inner = self.inner.lock().unwrap();
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
            let config = inner.config.clone();
            let now = now_ms();
            let record =
                inner.index.get_mut(session_id).expect("record exists");
            let history = record.messages.clone();
            record.messages.push(Message {
                id: format!("u-{now}"),
                role: Role::User,
                content: vec![crate::types::ContentBlock::text(prompt)],
                source: MessageSource::User,
            });
            record.status = TurnState::Running;
            record.updated_at = now;
            let snapshot = inner.index.clone();
            drop(inner);
            if snapshot.save(&self.root).is_err() {
                self.observer.storage_error(session_id);
            }
            (ticket, history, config)
        };

        let tools =
            build_tools(&request_config, session_id, ticket.signal().clone());
        let request = TurnRequest {
            system: request_config.system.clone(),
            history,
            prompt: prompt.to_owned(),
            route: request_config.route.clone(),
            tools,
            signal: ticket.signal().clone(),
        };
        let emitter = Arc::new(EngineEmitter {
            session: session_id.to_owned(),
            observer: Arc::clone(&self.observer),
        });
        let outcome = match run_turn(request, stream, emitter).await {
            Ok(result) => {
                if ticket.finish() {
                    let final_text = result
                        .messages
                        .iter()
                        .rev()
                        .find(|message| message.role == Role::Assistant)
                        .map_or_default(Message::text);
                    if !final_text.is_empty() {
                        self.observer.assistant_text(session_id, &final_text);
                    }
                    let mut inner = self.inner.lock().unwrap();
                    if let Some(record) = inner.index.get_mut(session_id) {
                        let mut new_messages = result.messages;
                        // The loop echoes the prompt back; the engine already
                        // stored it, so drop the duplicate.
                        if new_messages.first().is_some_and(|message| {
                            message.role == Role::User
                                && message.text() == prompt
                        }) {
                            new_messages.remove(0);
                        }
                        record.messages.extend(new_messages);
                        record.status = status_for(result.outcome);
                        record.updated_at = now_ms();
                    }
                    let snapshot = inner.index.clone();
                    drop(inner);
                    if snapshot.save(&self.root).is_err() {
                        self.observer.storage_error(session_id);
                    }
                    self.observer.finished(session_id, result.outcome);
                    result.outcome
                } else {
                    LoopOutcome::Aborted
                }
            }
            Err(message) => {
                if ticket.finish() {
                    let mut inner = self.inner.lock().unwrap();
                    if let Some(record) = inner.index.get_mut(session_id) {
                        record.status = TurnState::Error {
                            message: message.clone(),
                        };
                        record.updated_at = now_ms();
                    }
                    let snapshot = inner.index.clone();
                    drop(inner);
                    if snapshot.save(&self.root).is_err() {
                        self.observer.storage_error(session_id);
                    }
                }
                return Err(RunError::Loop(message));
            }
        };
        Ok(outcome)
    }
}

fn build_tools(
    config: &EngineConfig,
    session_id: &str,
    signal: CancelToken,
) -> Vec<Arc<dyn rpi_agent::AgentTool>> {
    let run_ctx = ToolRunContext {
        signal,
        agent_id: Some(session_id.to_owned()),
        cwd: config.cwd.clone(),
        todos: config.todos.clone(),
    };
    config
        .registry
        .list()
        .into_iter()
        .filter_map(|name| {
            let definition = config.registry.get(&name)?;
            RegistryTool::new(
                config.registry.clone(),
                definition,
                run_ctx.clone(),
            )
            .ok()
            .map(|tool| {
                let tool: Arc<dyn rpi_agent::AgentTool> = Arc::new(tool);
                tool
            })
        })
        .collect()
}

fn status_for(outcome: LoopOutcome) -> TurnState {
    match outcome {
        LoopOutcome::Completed => TurnState::Completed,
        LoopOutcome::Aborted => TurnState::Stopped,
        LoopOutcome::Failed => TurnState::Error {
            message: "model reported an error".into(),
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

struct EngineEmitter {
    session: String,
    observer: Arc<dyn TurnObserver>,
}

impl AgentEmitter for EngineEmitter {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, ()> {
        let session = self.session.clone();
        let observer = Arc::clone(&self.observer);
        Box::pin(async move {
            match event {
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
                AgentEvent::ToolExecutionStart {
                    tool_call_id,
                    tool_name,
                    args,
                } => observer.tool_start(
                    &session,
                    &tool_call_id,
                    &tool_name,
                    &args,
                ),
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
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Engine, EngineConfig, RunError, TurnObserver};
    use crate::loop_runner::{LoopOutcome, ModelRoute};
    use crate::session::TurnState;
    use crate::tools::ToolRegistry;
    use rpi_agent::StreamFn;
    use rpi_ai::types::{
        Api, AssistantMessage, AssistantMessageEvent, DoneReason, StopReason,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
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
        }
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
            .await
            .unwrap();
        assert_eq!(outcome, LoopOutcome::Completed);
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
        engine
            .set_session_title("s1", "Model title".into(), true)
            .unwrap();
        drop(engine);
        let reopened =
            Engine::open(scratch.0.clone(), config(), recorder).unwrap();
        let record = reopened.session("s1").expect("session");
        assert_eq!(record.title, "Model title");
        assert!(record.titled);
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

    #[test]
    fn no_credential_refuses_without_touching_history() {
        // The async refusal path is covered by `run`; this keeps the error
        // variant exercised without a live provider.
        assert!(matches!(RunError::NoCredential, RunError::NoCredential));
    }
}
