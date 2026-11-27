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
                    rpi_ai::types::Content::ToolCall(rpi_ai::types::ToolCall {
                        kind: rpi_ai::types::ToolCallType,
                        id: id.into(),
                        name: "publish_file".into(),
                        arguments: serde_json::json!({}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    })
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
    }
}

#[tokio::test]
async fn delivery_group_is_durable_before_marker_and_visible_after_marker() {
    let scratch = Scratch::new("delivery");
    let observer = Arc::new(Recorder::default());
    let committer = Arc::new(CommitRecorder {
        root: scratch.0.clone(),
        commits: Mutex::new(Vec::new()),
        fail: false,
    });
    let cfg = delivery_config(committer.clone(), true);
    let engine = Arc::new(
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
    assert_eq!(*observer.finished.lock().unwrap(), [LoopOutcome::Completed]);
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
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
    assert_eq!(engine.session("home").unwrap().delivery_commits, Vec::new());
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
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
    assert_eq!(engine.session("home").unwrap().delivery_commits, Vec::new());
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
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
    assert_eq!(engine.session("home").unwrap().status, TurnState::Completed);
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
    assert_eq!(*observer.finished.lock().unwrap(), [LoopOutcome::Completed]);
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
        Engine::open(scratch.0.clone(), cfg.clone(), observer.clone()).unwrap(),
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
    assert_eq!(engine.session("home").unwrap().delivery_commits, Vec::new());
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
        Engine::open(scratch.0.clone(), config(), observer.clone()).unwrap();
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
            .map(|message| format!("{:?}|{:?}", message.role, message.content))
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
        Engine::open(scratch.0.clone(), config(), recorder.clone()).unwrap();
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
                rpi_ai::event_stream::create_assistant_message_event_stream();
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
        Engine::open(scratch.0.clone(), config(), recorder.clone()).unwrap();
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
        Engine::open(scratch.0.clone(), config(), recorder.clone()).unwrap();
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
    let reopened = Engine::open(scratch.0.clone(), config(), recorder).unwrap();
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
        Engine::open(scratch.0.clone(), config(), recorder.clone()).unwrap();
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
        Engine::open(scratch.0.clone(), config(), recorder.clone()).unwrap();
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
    assert!(engine.wait_until_idle("s1", std::time::Duration::from_secs(1)));
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
                    verify_state: crate::session::ObjectVerifyState::Unverified,
                },
                crate::session::TrackedObject {
                    object_id: "obj-xlsx-2".into(),
                    kind: crate::session::TrackedObjectKind::Deliverable,
                    filename: Some("quote.xlsx".into()),
                    extension: "xlsx".into(),
                    anchor_summary: String::new(),
                    overrides: std::collections::BTreeMap::new(),
                    verify_state: crate::session::ObjectVerifyState::Unverified,
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
    let engine =
        Arc::new(Engine::open(scratch.0.clone(), config(), observer).unwrap());
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
    let ran_when_idle = tokio::task::spawn_blocking(move || {
        assert!(
            engine_wait
                .wait_until_idle("s-race", std::time::Duration::from_secs(5),)
        );
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

#[test]
fn child_projections_match_child_sessions_across_multiple_workers() {
    use std::collections::BTreeMap;

    use crate::session::{
        DeliveryCommit, FactEntry, FactStatus, ObjectVerifyState,
        SessionOrigin, SessionRecord, SessionRole, TrackedObject,
        TrackedObjectKind, TurnState, WorkContract,
    };

    struct NoopObserver;
    impl TurnObserver for NoopObserver {}

    let scratch = Scratch::new("child-projection-parity");
    let engine =
        Engine::open(scratch.0.clone(), config(), Arc::new(NoopObserver))
            .unwrap();

    engine
        .insert_session(SessionRecord {
            id: "router-1".into(),
            role: SessionRole::Router,
            updated_at: 10,
            ..SessionRecord::default()
        })
        .unwrap();
    // Other root worker and non-worker session must be ignored.
    engine
        .insert_session(SessionRecord {
            id: "other-worker".into(),
            role: SessionRole::Worker,
            root_session_id: Some("router-2".into()),
            updated_at: 99,
            status: TurnState::Running,
            work_contract: Some(WorkContract {
                objects: vec![TrackedObject {
                    object_id: "obj-shared".into(),
                    kind: TrackedObjectKind::Deliverable,
                    filename: Some("other.pptx".into()),
                    extension: "pptx".into(),
                    anchor_summary: String::new(),
                    overrides: BTreeMap::new(),
                    verify_state: ObjectVerifyState::Unverified,
                }],
                ..WorkContract::default()
            }),
            ..SessionRecord::default()
        })
        .unwrap();
    engine
        .insert_session(SessionRecord {
            id: "standard-child-id".into(),
            role: SessionRole::Standard,
            root_session_id: Some("router-1".into()),
            updated_at: 98,
            ..SessionRecord::default()
        })
        .unwrap();

    // Older worker (updated_at = 20)
    engine
        .insert_session(SessionRecord {
            id: "w-older".into(),
            role: SessionRole::Worker,
            root_session_id: Some("router-1".into()),
            updated_at: 20,
            status: TurnState::Completed,
            delivery_commits: vec![DeliveryCommit {
                session_id: "w-older".into(),
                submission_id: "sub-1".into(),
                artifact_ids: vec!["art-a".into(), "art-b".into()],
                completed_successfully: true,
                committed_at_ms: 20,
                origin: SessionOrigin::Home,
                origin_title_snapshot: String::new(),
            }],
            work_contract: Some(WorkContract {
                revision: 2,
                pinned_facts: BTreeMap::from([
                    ("client".into(), "Acme".into()),
                    ("date".into(), "2026-01-01".into()),
                ]),
                objects: vec![
                    TrackedObject {
                        object_id: "obj-1".into(),
                        kind: TrackedObjectKind::Deliverable,
                        filename: Some("deck.pptx".into()),
                        extension: "pptx".into(),
                        anchor_summary: "deck".into(),
                        overrides: BTreeMap::from([
                            (
                                "date".into(),
                                FactEntry {
                                    value: "2026-02-02".into(),
                                    status: FactStatus::Confirmed,
                                    updated_at_ms: 21,
                                },
                            ),
                            (
                                "client".into(),
                                FactEntry {
                                    value: "PendingCorp".into(),
                                    status: FactStatus::PendingProposal,
                                    updated_at_ms: 22,
                                },
                            ),
                        ]),
                        verify_state: ObjectVerifyState::Unverified,
                    },
                    TrackedObject {
                        object_id: "obj-shared".into(),
                        kind: TrackedObjectKind::Record,
                        filename: None,
                        extension: "md".into(),
                        anchor_summary: "older".into(),
                        overrides: BTreeMap::new(),
                        verify_state: ObjectVerifyState::Unverified,
                    },
                ],
                ..WorkContract::default()
            }),
            ..SessionRecord::default()
        })
        .unwrap();

    // Newer worker (updated_at = 50) also tracking `obj-shared`
    engine
        .insert_session(SessionRecord {
            id: "w-newer".into(),
            role: SessionRole::Worker,
            root_session_id: Some("router-1".into()),
            updated_at: 50,
            status: TurnState::Running,
            delivery_commits: vec![
                DeliveryCommit {
                    session_id: "w-newer".into(),
                    submission_id: "sub-2".into(),
                    artifact_ids: vec!["art-c".into()],
                    completed_successfully: true,
                    committed_at_ms: 50,
                    origin: SessionOrigin::Home,
                    origin_title_snapshot: String::new(),
                },
                DeliveryCommit {
                    session_id: "w-newer".into(),
                    submission_id: "sub-3".into(),
                    artifact_ids: vec!["art-d".into()],
                    completed_successfully: false,
                    committed_at_ms: 51,
                    origin: SessionOrigin::Home,
                    origin_title_snapshot: String::new(),
                },
            ],
            work_contract: Some(WorkContract {
                revision: 1,
                pinned_facts: BTreeMap::from([("topic".into(), "Q4".into())]),
                objects: vec![TrackedObject {
                    object_id: "obj-shared".into(),
                    kind: TrackedObjectKind::Deliverable,
                    filename: Some("newer.pptx".into()),
                    extension: "pptx".into(),
                    anchor_summary: "newer".into(),
                    overrides: BTreeMap::new(),
                    verify_state: ObjectVerifyState::Unverified,
                }],
                ..WorkContract::default()
            }),
            ..SessionRecord::default()
        })
        .unwrap();

    let full_children = engine.child_sessions("router-1");
    for oid in ["obj-1", "obj-shared", "obj-missing"] {
        let expected = full_children.iter().find_map(|child| {
            let wc = child.work_contract.as_ref()?;
            let obj = wc.find_object(oid)?;
            let facts: Vec<(String, String)> =
                wc.confirmed_facts_for(oid).into_iter().collect();
            Some((child.id.clone(), obj.clone(), facts))
        });
        assert_eq!(
            engine.find_child_tracked_object("router-1", oid),
            expected,
            "parity mismatch for object {oid}"
        );
    }
    // `obj-shared` resolves to `w-newer` first (higher `updated_at`).
    assert_eq!(
        engine
            .find_child_tracked_object("router-1", "obj-shared")
            .map(|(id, _, _)| id),
        Some("w-newer".to_owned())
    );

    let expected_summaries: Vec<super::ChildDeleteSummary> = full_children
        .iter()
        .map(|child| super::ChildDeleteSummary {
            id: child.id.clone(),
            is_busy: child.status.is_busy(),
            artifact_ids: child
                .delivery_commits
                .iter()
                .flat_map(|c| c.artifact_ids.iter().cloned())
                .collect(),
            submission_ids: child
                .delivery_commits
                .iter()
                .map(|c| c.submission_id.clone())
                .collect(),
        })
        .collect();
    assert_eq!(
        engine.child_delete_summaries("router-1"),
        expected_summaries
    );
}
