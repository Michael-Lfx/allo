use super::*;
use crate::tools::ToolDefinition;
use crate::{BudgetStopReason, RunBudget};
use rpi_ai::types::{
    Api, AssistantMessage, AssistantMessageEvent, Content, DoneReason,
    StopReason, ToolCall, ToolCallType,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join(format!(
                "budget-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
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
struct Observer(AtomicUsize);

impl TurnObserver for Observer {
    fn storage_error(&self, _: &str) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn config(budget: RunBudget) -> EngineConfig {
    EngineConfig {
        route: ModelRoute {
            provider: "fixture".into(),
            model: "fixture".into(),
            ..ModelRoute::default()
        },
        api_key: None,
        system: String::new(),
        registry: ToolRegistry::new(),
        cwd: None,
        todos: None,
        delivery_committer: None,
        work_contract: None,
        pre_tool_hook: None,
        execution_gate: None,
        run_budget: Some(budget),
    }
}

fn setup(config: &EngineConfig) -> (Scratch, Arc<Engine>, Arc<Observer>) {
    let root = Scratch::new();
    let observer = Arc::new(Observer::default());
    let engine = Arc::new(
        Engine::open(root.0.clone(), config.clone(), observer.clone()).unwrap(),
    );
    engine
        .create_session_with_metadata(
            "budget".into(),
            1,
            SessionOrigin::Home,
            Some(SessionWorkspace {
                root: root.0.join("workspace"),
            }),
        )
        .unwrap();
    (root, engine, observer)
}

fn submission() -> String {
    format!("submission-{:032x}", 1)
}

fn tool_stream(
    calls: Arc<AtomicUsize>,
    name: &'static str,
    tool_calls: usize,
    final_text: bool,
) -> StreamFn {
    rpi_agent::stream_fn(move |model, _, _| {
        let index = calls.fetch_add(1, Ordering::SeqCst);
        let mut message = AssistantMessage::empty(
            Api::OpenaiCompletions,
            model.provider.clone(),
            model.id.clone(),
            0,
        );
        if index > 0 && final_text {
            message.content = vec![Content::text("done")];
            message.stop_reason = StopReason::Stop;
        } else {
            message.content = (0..tool_calls)
                .map(|call| {
                    Content::ToolCall(ToolCall {
                        kind: ToolCallType,
                        id: format!("call-{index}-{call}"),
                        name: name.into(),
                        arguments: serde_json::json!({}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    })
                })
                .collect();
            message.stop_reason = StopReason::ToolUse;
        }
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
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

fn counting_tool(calls: Arc<AtomicUsize>) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "tick",
        "count actual dispatches",
        serde_json::json!({"type":"object","properties":{}}),
        move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                crate::ToolExecutionResult::success_text(
                    "tick",
                    serde_json::json!({}),
                )
            })
        },
    ))
}

#[tokio::test]
async fn an_infinite_loop_stops_at_the_model_limit_and_recovers_its_counts() {
    let cfg = config(RunBudget::new(60_000, 3, 20).unwrap());
    let tools = Arc::new(AtomicUsize::new(0));
    cfg.registry.register(counting_tool(Arc::clone(&tools)));
    let (root, engine, _) = setup(&cfg);
    let models = Arc::new(AtomicUsize::new(0));
    let run = engine
        .start_with_config(
            "budget".into(),
            "keep working".into(),
            cfg.clone(),
            Some(tool_stream(Arc::clone(&models), "tick", 1, false)),
            Some(1),
        )
        .unwrap();
    assert_eq!(run.await.unwrap(), LoopOutcome::Aborted);
    assert_eq!(models.load(Ordering::SeqCst), 3);
    assert_eq!(tools.load(Ordering::SeqCst), 3);
    let checkpoint = engine
        .run_budget_checkpoint("budget", &submission())
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.model_calls, 3);
    assert_eq!(checkpoint.tool_calls, 3);
    assert_eq!(
        checkpoint.stop_reason,
        Some(BudgetStopReason::ModelCallBudget)
    );
    let reopened =
        Engine::open(root.0.clone(), cfg, Arc::new(Observer::default()))
            .unwrap();
    assert_eq!(
        reopened
            .run_budget_checkpoint("budget", &submission())
            .unwrap(),
        Some(checkpoint)
    );
    assert_eq!(
        reopened
            .completion_evidence("budget", &submission())
            .unwrap()
            .outcome,
        TurnExecutionOutcome::Stopped
    );
}

#[tokio::test]
async fn each_tool_in_one_model_response_is_counted_separately() {
    let cfg = config(RunBudget::new(60_000, 5, 2).unwrap());
    let tools = Arc::new(AtomicUsize::new(0));
    cfg.registry.register(counting_tool(Arc::clone(&tools)));
    let (_root, engine, _) = setup(&cfg);
    let models = Arc::new(AtomicUsize::new(0));
    let run = engine
        .start_with_config(
            "budget".into(),
            "several tools".into(),
            cfg,
            Some(tool_stream(Arc::clone(&models), "tick", 5, false)),
            Some(1),
        )
        .unwrap();
    assert_eq!(run.await.unwrap(), LoopOutcome::Aborted);
    assert_eq!(models.load(Ordering::SeqCst), 1);
    assert_eq!(tools.load(Ordering::SeqCst), 2);
    let checkpoint = engine
        .run_budget_checkpoint("budget", &submission())
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.tool_calls, 2);
    assert_eq!(
        checkpoint.stop_reason,
        Some(BudgetStopReason::ToolCallBudget)
    );
}

#[tokio::test]
async fn deadline_waits_for_the_original_tool_to_finish_reaping() {
    let cfg = config(RunBudget::new(200, 10, 10).unwrap());
    let started = Arc::new(tokio::sync::Notify::new());
    let cancelled = Arc::new(tokio::sync::Notify::new());
    let released = Arc::new(tokio::sync::Notify::new());
    let (tool_started, tool_cancelled, tool_released) = (
        Arc::clone(&started),
        Arc::clone(&cancelled),
        Arc::clone(&released),
    );
    cfg.registry.register(Arc::new(ToolDefinition::new(
        "slow",
        "cooperative reaping fixture",
        serde_json::json!({"type":"object","properties":{}}),
        move |_, ctx| {
            let (started, cancelled, released) = (
                Arc::clone(&tool_started),
                Arc::clone(&tool_cancelled),
                Arc::clone(&tool_released),
            );
            Box::pin(async move {
                started.notify_one();
                ctx.signal.cancelled().await;
                cancelled.notify_one();
                released.notified().await;
                crate::ToolExecutionResult::success_text(
                    "reaped",
                    serde_json::json!({}),
                )
            })
        },
    )));
    let (_root, engine, _) = setup(&cfg);
    let run = engine
        .start_with_config(
            "budget".into(),
            "wait for tool".into(),
            cfg,
            Some(tool_stream(Arc::new(AtomicUsize::new(0)), "slow", 1, false)),
            Some(1),
        )
        .unwrap();
    let run = tokio::spawn(run);
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), cancelled.notified())
        .await
        .unwrap();
    assert!(!run.is_finished());
    assert!(engine.is_cancelling("budget"));
    assert!(engine.is_active("budget"));
    released.notify_one();
    assert_eq!(run.await.unwrap().unwrap(), LoopOutcome::Aborted);
    assert!(!engine.is_active("budget"));
    assert_eq!(
        engine
            .run_budget_checkpoint("budget", &submission())
            .unwrap()
            .unwrap()
            .stop_reason,
        Some(BudgetStopReason::Deadline)
    );
}

#[tokio::test]
async fn deadline_during_a_stalled_provider_does_not_need_another_dispatch() {
    let cfg = config(RunBudget::new(100, 10, 10).unwrap());
    let (_root, engine, _) = setup(&cfg);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let stream = rpi_agent::stream_fn(move |_, _, options| {
        counted.fetch_add(1, Ordering::SeqCst);
        let (producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        let signal = options.signal.clone();
        tokio::spawn(async move {
            signal.cancelled().await;
            drop(producer);
        });
        consumer
    });
    let run = engine
        .start_with_config(
            "budget".into(),
            "stalled".into(),
            cfg,
            Some(stream),
            Some(1),
        )
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), run)
            .await
            .unwrap()
            .unwrap(),
        LoopOutcome::Aborted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine
            .run_budget_checkpoint("budget", &submission())
            .unwrap()
            .unwrap()
            .stop_reason,
        Some(BudgetStopReason::Deadline)
    );
}

#[tokio::test]
async fn a_failed_model_reservation_never_dispatches_or_increments_accounting()
{
    let cfg = config(RunBudget::new(60_000, 10, 10).unwrap());
    let (root, engine, observer) = setup(&cfg);
    let calls = Arc::new(AtomicUsize::new(0));
    let run = engine
        .start_with_config(
            "budget".into(),
            "must not dispatch".into(),
            cfg,
            Some(tool_stream(Arc::clone(&calls), "tick", 1, false)),
            Some(1),
        )
        .unwrap();
    let history = SessionIndex::path(&root.0);
    let backup = root.0.join("saved-history.json");
    std::fs::rename(&history, &backup).unwrap();
    std::fs::create_dir(&history).unwrap();
    assert!(matches!(run.await, Err(RunError::Storage(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(observer.0.load(Ordering::SeqCst) > 0);
    assert!(!engine.is_active("budget"));
    let checkpoint = engine
        .run_budget_checkpoint("budget", &submission())
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.model_calls, 0);
    assert_eq!(checkpoint.stop_reason, None);
    std::fs::remove_dir(&history).unwrap();
    std::fs::rename(&backup, &history).unwrap();
    assert_eq!(
        SessionIndex::load(&root.0)
            .unwrap()
            .get("budget")
            .unwrap()
            .run_budget_checkpoints[&submission()],
        checkpoint
    );
}

#[tokio::test]
async fn failed_initial_budget_save_rolls_back_the_submission_and_checkpoint() {
    let cfg = config(RunBudget::new(60_000, 10, 10).unwrap());
    let (root, engine, _) = setup(&cfg);
    let history = SessionIndex::path(&root.0);
    let backup = root.0.join("saved-history.json");
    std::fs::rename(&history, &backup).unwrap();
    std::fs::create_dir(&history).unwrap();
    assert!(matches!(
        engine.start_with_config(
            "budget".into(),
            "must not record".into(),
            cfg,
            Some(crate::echo_stream_fn("done".into())),
            Some(1)
        ),
        Err(RunError::Storage(_))
    ));
    let session = engine.session("budget").unwrap();
    assert_eq!(session.messages.len(), 0);
    assert!(session.run_budget_checkpoints.is_empty());
    assert!(!engine.is_active("budget"));
}

struct SlowCommitter {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    commits: AtomicUsize,
}

#[async_trait::async_trait]
impl DeliveryCommitter for SlowCommitter {
    async fn commit(&self, _: &DeliveryCommit) -> Result<(), String> {
        self.entered.notify_one();
        self.release.notified().await;
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn deadline_records_its_reason_without_cancelling_a_committed_delivery() {
    let mut cfg = config(RunBudget::new(2_000, 10, 10).unwrap());
    let committer = Arc::new(SlowCommitter {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        commits: AtomicUsize::new(0),
    });
    cfg.delivery_committer = Some(committer.clone());
    cfg.registry.register(Arc::new(
        ToolDefinition::new(
            "publish_file",
            "host receipt fixture",
            serde_json::json!({"type":"object","properties":{}}),
            |args, ctx| {
                Box::pin(async move {
                    let receipt = ArtifactReceipt {
                        artifact_id: "artifact-budget".into(),
                        delivery_key: args.call_id.clone(),
                        session_id: ctx.agent_id.unwrap(),
                        submission_id: ctx.submission_id.unwrap(),
                        tool_call_id: args.call_id,
                        display_name: "fixture.csv".into(),
                        media_type: Some("text/csv".into()),
                        size_bytes: 1,
                        sha256: "fixture-digest".into(),
                        created_at_ms: 1,
                        origin_title_snapshot: String::new(),
                    };
                    crate::ToolExecutionResult::Success {
                        content: vec![ContentBlock::text("prepared")],
                        value: serde_json::json!({"__flowy_host_tool_result_v1": {"artifact_receipt": receipt}}),
                        meta: None,
                        delivery: Some(Box::new(receipt)),
                    }
                })
            },
        )
        .with_artifact_delivery(),
    ));
    let (_root, engine, _) = setup(&cfg);
    let run = engine
        .start_with_config(
            "budget".into(),
            "deliver".into(),
            cfg,
            Some(tool_stream(
                Arc::new(AtomicUsize::new(0)),
                "publish_file",
                1,
                true,
            )),
            Some(1),
        )
        .unwrap();
    let run = tokio::spawn(run);
    tokio::time::timeout(Duration::from_secs(5), committer.entered.notified())
        .await
        .unwrap();
    let history_at_commit = engine.session("budget").unwrap();
    let latest_user_role = history_at_commit
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .unwrap();
    assert!(
        matches!(latest_user_role.source, MessageSource::Tool { .. }),
        "tool results intentionally share the wire User role"
    );
    assert!(
        latest_user_role
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
    );
    let delivery = engine
        .delivery_commit_for_submission("budget", &submission())
        .unwrap();
    assert_eq!(delivery.artifact_ids, ["artifact-budget"]);
    let checkpoint = engine
        .run_budget_checkpoint("budget", &submission())
        .unwrap()
        .unwrap();
    let remaining = checkpoint
        .deadline_at_ms
        .saturating_sub(i64::try_from(now_ms()).unwrap())
        .max(0);
    tokio::time::sleep(Duration::from_millis(
        u64::try_from(remaining).unwrap() + 50,
    ))
    .await;
    assert!(!run.is_finished());
    assert_eq!(
        engine.request_cancel_typed("budget").unwrap(),
        CancelDisposition::TooLate
    );
    assert_eq!(
        engine
            .run_budget_checkpoint("budget", &submission())
            .unwrap()
            .unwrap()
            .stop_reason,
        Some(BudgetStopReason::Deadline)
    );
    committer.release.notify_one();
    assert_eq!(run.await.unwrap().unwrap(), LoopOutcome::Completed);
    assert_eq!(committer.commits.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine.delivery_commit_for_submission("budget", &submission()),
        Some(delivery)
    );
    assert_eq!(
        engine
            .completion_evidence("budget", &submission())
            .unwrap()
            .outcome,
        TurnExecutionOutcome::Completed
    );
}

#[tokio::test]
async fn automation_sessions_cannot_start_without_a_finite_budget() {
    let mut cfg = config(RunBudget::new(60_000, 10, 10).unwrap());
    cfg.execution_gate =
        Some(Arc::new(crate::execution_gate::tests::RevocableGate(
            std::sync::atomic::AtomicBool::new(true),
        )));
    let (_root, engine, _) = setup(&cfg);
    let mut session = engine.session("budget").unwrap();
    session.execution = ExecutionMetadata {
        owner: Some(crate::ExecutionOwner {
            authority: "fixture".into(),
            subject: "7".into(),
        }),
        source: crate::ExecutionSource::Automation {
            automation_id: "automation".into(),
            run_id: "run".into(),
            attempt_id: "attempt".into(),
        },
    };
    engine.insert_session(session).unwrap();
    cfg.run_budget = None;
    assert!(matches!(
        engine.start_with_config(
            "budget".into(),
            "must have limits".into(),
            cfg,
            Some(crate::echo_stream_fn("no".into())),
            Some(1)
        ),
        Err(RunError::MissingBudget)
    ));
    let session = engine.session("budget").unwrap();
    assert_eq!(session.messages.len(), 0);
    assert!(session.run_budget_checkpoints.is_empty());
}

#[tokio::test]
async fn a_checkpoint_alone_prevents_replaying_and_resetting_a_submission() {
    let limits = RunBudget::new(60_000, 10, 10).unwrap();
    let cfg = config(limits);
    let (_root, engine, _) = setup(&cfg);
    let checkpoint = crate::RunBudgetCheckpoint {
        started_at_ms: 1_000,
        deadline_at_ms: 61_000,
        limits,
        model_calls: 7,
        tool_calls: 4,
        stop_reason: None,
    };
    let mut session = engine.session("budget").unwrap();
    // A parsable but incomplete history is not evidence that work never
    // started. Its remaining durable reservations must never be overwritten.
    session
        .run_budget_checkpoints
        .insert(submission(), checkpoint);
    engine.insert_session(session).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(matches!(
        engine.start_with_config(
            "budget".into(),
            "must not replay".into(),
            cfg,
            Some(tool_stream(Arc::clone(&calls), "tick", 1, false)),
            Some(1)
        ),
        Err(RunError::DuplicateSubmission)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(engine.session("budget").unwrap().messages.len(), 0);
    assert_eq!(
        engine
            .run_budget_checkpoint("budget", &submission())
            .unwrap(),
        Some(checkpoint)
    );
    assert!(!engine.is_active("budget"));
}

#[tokio::test]
async fn tool_result_roles_do_not_authenticate_a_completion_submission() {
    let cfg = config(RunBudget::new(60_000, 10, 10).unwrap());
    let (_root, engine, _) = setup(&cfg);
    let run = engine
        .start_with_config(
            "budget".into(),
            "finish once".into(),
            cfg.clone(),
            Some(crate::echo_stream_fn("done".into())),
            Some(1),
        )
        .unwrap();
    assert_eq!(run.await.unwrap(), LoopOutcome::Completed);
    assert!(
        engine
            .completion_evidence("budget", &submission())
            .is_some()
    );
    let accounting = engine
        .run_budget_checkpoint("budget", &submission())
        .unwrap();
    let mut session = engine.session("budget").unwrap();
    let prompt = session
        .messages
        .iter_mut()
        .find(|message| message.id == submission())
        .unwrap();
    prompt.source = MessageSource::Tool {
        tool: "fixture".into(),
    };
    engine.insert_session(session).unwrap();
    assert!(
        engine
            .completion_evidence("budget", &submission())
            .is_none()
    );
    // Accounting remains evidence that work started, even when its remaining
    // messages cannot authenticate a successful completion. It cannot reset.
    assert_eq!(
        engine
            .run_budget_checkpoint("budget", &submission())
            .unwrap(),
        accounting
    );
    assert!(matches!(
        engine.start_with_config(
            "budget".into(),
            "must not replay".into(),
            cfg,
            Some(crate::echo_stream_fn("no".into())),
            Some(1)
        ),
        Err(RunError::DuplicateSubmission)
    ));
}
