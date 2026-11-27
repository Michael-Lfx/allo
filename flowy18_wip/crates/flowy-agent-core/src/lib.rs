//! Host-owned agent core seams: messages, tool definitions, guarded execution.
//!
//! This crate intentionally has no agent kernel, plugin system, or transport
//! dependency. The loop and HTTP provider are added separately so the tool and
//! session layer can be tested without a running model.

pub mod acceptance;
pub mod engine;
pub mod loop_runner;
pub mod run_fence;
pub mod session;
pub mod tools;
pub mod transport;
pub mod types;

pub use acceptance::{
    AcceptanceQueue, AcceptedMessage, DraftTarget, Lease, ReceiptState,
};
pub use engine::{
    Engine, EngineConfig, EnqueueOutcome, RunError, TurnObserver,
};
pub use loop_runner::{
    LoopOutcome, ModelRoute, PreToolDecision, PreToolHook, StructuredRequest,
    TurnRequest, TurnResult, canonical_tool_args_json, complete_structured,
    complete_structured_with_stream, complete_text, log_preview, run_turn,
};
pub use run_fence::{PublishToken, RunFence, RunTicket};
pub use session::{
    BranchHeartbeat, DeliveryCommit, DeliveryCommitter, DisambiguationOption,
    DispatchRecord, FactEntry, FactStatus, ObjectVerifyState, PendingCard,
    PendingRouterFollowup, RequiredDeliverable, SessionIndex, SessionOrigin,
    SessionRecord, SessionRole, SessionWorkspace, StopHandoffManifest,
    SubmissionOutcome, TrackedObject, TrackedObjectKind, TurnState,
    WorkAccessMode, WorkContract,
};
pub use tools::{
    ActivatedToolSet, MAX_ACTIVATED_TOOLS_PER_TURN, SearchableToolEntry,
    TOOL_SEARCH_NAME, ToolDefinition, ToolExposure, ToolPolicyContext,
    ToolProvider, ToolProviderContext, ToolRegistry, ToolSource,
};
pub use transport::{
    echo_stream_fn, openai_stream_fn, openai_structured_stream_fn,
};

pub use types::{
    ArtifactReceipt, CancelToken, ContentBlock, Message, MessageId,
    MessageSource, PromptTurn, Role, TodoItem, TodoSink, TodoStatus,
    ToolCallArgs, ToolExecutionResult, ToolRunContext, ToolSchema,
};

#[cfg(test)]
mod router_core_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "flowy-core-router-{tag}-{}-{}",
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

    struct NoopObserver;
    impl TurnObserver for NoopObserver {}

    #[test]
    fn legacy_session_and_message_source_roundtrip_compat() {
        let legacy: SessionRecord =
            serde_json::from_str(r#"{"id":"legacy-1","messages":[]}"#).unwrap();
        assert_eq!(legacy.role, SessionRole::Standard);
        assert_eq!(legacy.access_mode, WorkAccessMode::ReadWrite);
        assert_eq!(legacy.pending_router_followups, Vec::new());
        assert_eq!(legacy.dispatch_requests.len(), 0);

        let sources = [
            MessageSource::User,
            MessageSource::CardResume,
            MessageSource::SystemEvent {
                event_kind: "work_event_batch".into(),
            },
        ];
        for src in sources {
            let json = serde_json::to_string(&src).unwrap();
            let parsed: MessageSource = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, src);
        }

        let sys = PromptTurn::system_event("work_event_batch", "[batch]");
        assert_eq!(sys.display.as_deref(), Some(""));
        assert_eq!(
            sys.source,
            MessageSource::SystemEvent {
                event_kind: "work_event_batch".into()
            }
        );
        let resume = PromptTurn::card_resume("[resumed]", Some(String::new()));
        assert_eq!(resume.source, MessageSource::CardResume);
    }

    #[test]
    fn accept_strict_roundtrips_and_preserves_binding_across_retry_dead() {
        let scratch = Scratch::new("accept-strict");
        let queue = AcceptanceQueue::new(&scratch.0);
        let legacy_receipt = queue.accept(Some("s-1"), "normal", 10).unwrap();
        assert!(!legacy_receipt.strict_target);
        assert!(legacy_receipt.target_session_id.is_none());

        let strict = queue
            .accept_strict("w-1", "heavy task", Some("req-1"), 20)
            .unwrap();
        assert!(strict.strict_target);
        assert_eq!(strict.target_session_id.as_deref(), Some("w-1"));
        assert_eq!(strict.dispatch_request_id.as_deref(), Some("req-1"));

        let replayed = queue.get(&strict.id).unwrap().unwrap();
        assert!(replayed.strict_target);
        assert_eq!(replayed.target_session_id.as_deref(), Some("w-1"));
        assert_eq!(replayed.dispatch_request_id.as_deref(), Some("req-1"));

        // Exhaust and retry_dead -> strict_target and dispatch_request_id stay pinned.
        let taken = queue.take(&strict.id, "att-1", 25, 5000).unwrap().unwrap();
        assert!(taken.strict_target);
        assert!(queue.give_up(&strict.id, "att-1", 30).unwrap());
        let requeued = queue.retry_dead(&strict.id, 35).unwrap().unwrap();
        assert!(requeued.strict_target);
        assert_eq!(requeued.target_session_id.as_deref(), Some("w-1"));
        assert_eq!(requeued.dispatch_request_id.as_deref(), Some("req-1"));
    }

    #[test]
    fn engine_router_followups_dispatch_ledger_and_atomic_pop_work() {
        let scratch = Scratch::new("engine-router");
        let config = EngineConfig {
            route: ModelRoute::default(),
            api_key: None,
            system: String::new(),
            registry: tools::ToolRegistry::new(),
            cwd: None,
            todos: None,
            delivery_committer: None,
            work_contract: None,
            pre_tool_hook: None,
            pre_activated_tools: Vec::new(),
        };
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config.clone(),
                Arc::new(NoopObserver),
            )
            .unwrap(),
        );
        engine
            .create_session_with_role(
                "r-1".into(),
                1,
                SessionOrigin::Home,
                None,
                SessionRole::Router,
                None,
                WorkAccessMode::ReadWrite,
            )
            .unwrap();
        engine
            .create_session_with_role(
                "w-1".into(),
                2,
                SessionOrigin::Home,
                None,
                SessionRole::Worker,
                Some("r-1".into()),
                WorkAccessMode::ReadOnly,
            )
            .unwrap();
        assert_eq!(engine.child_sessions("r-1").len(), 1);
        assert_eq!(engine.child_sessions("r-1")[0].id, "w-1");
        assert_eq!(
            engine.child_sessions("r-1")[0].access_mode,
            WorkAccessMode::ReadOnly
        );
        assert_eq!(engine.child_session_ids("r-1"), vec!["w-1".to_owned()]);

        let sub_id = format!("submission-{:032x}", 42_u128);
        let item = PendingRouterFollowup {
            submission_id: sub_id,
            text: "second question".into(),
            reply_to_object_id: None,
            created_at_ms: 10,
        };
        assert_eq!(
            engine
                .enqueue_router_followup("r-1", item.clone(), 1)
                .unwrap(),
            EnqueueOutcome::Enqueued
        );
        // Same submission_id is idempotent even when at capacity.
        assert_eq!(
            engine.enqueue_router_followup("r-1", item, 1).unwrap(),
            EnqueueOutcome::Duplicate
        );
        // Different submission_id respects max_len.
        assert_eq!(
            engine
                .enqueue_router_followup(
                    "r-1",
                    PendingRouterFollowup {
                        submission_id: format!("submission-{:032x}", 43_u128),
                        text: "third".into(),
                        reply_to_object_id: None,
                        created_at_ms: 11,
                    },
                    1,
                )
                .unwrap(),
            EnqueueOutcome::QueueFull
        );
        assert_eq!(engine.submitted_session(42).as_deref(), Some("r-1"));

        // Record dispatch idempotency entry and survive reload.
        engine
            .record_dispatch_request(
                "r-1",
                DispatchRecord {
                    request_id: "req-a".into(),
                    fingerprint: "fp-a".into(),
                    worker_id: "w-1".into(),
                    receipt_id: "rcpt-1".into(),
                    created_at_ms: 12,
                },
            )
            .unwrap();
        let reopened = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config.clone(),
                Arc::new(NoopObserver),
            )
            .unwrap(),
        );
        assert_eq!(
            reopened
                .lookup_dispatch_request("r-1", "req-a")
                .unwrap()
                .worker_id,
            "w-1"
        );
        assert_eq!(reopened.pending_router_followups("r-1").len(), 1);

        // Starting the queued follow-up pops it atomically in Engine::prepare.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let fut = reopened
            .start_with_config(
                "r-1".into(),
                PromptTurn::plain("second question"),
                config,
                Some(echo_stream_fn("answered".into())),
                Some(42),
            )
            .unwrap();
        assert_eq!(
            reopened.pending_router_followups("r-1"),
            Vec::new(),
            "Engine::prepare must atomically pop matched pending_router_followups"
        );
        let outcome = rt.block_on(fut).unwrap();
        assert_eq!(outcome, LoopOutcome::Completed);
        assert_eq!(reopened.submitted_session(42).as_deref(), Some("r-1"));
    }

    #[test]
    fn record_dispatch_request_bounds_capacity_and_evicts_oldest() {
        let scratch = Scratch::new("dispatch-cap");
        let config = EngineConfig {
            route: ModelRoute::default(),
            api_key: None,
            system: String::new(),
            registry: tools::ToolRegistry::new(),
            cwd: None,
            todos: None,
            delivery_committer: None,
            work_contract: None,
            pre_tool_hook: None,
            pre_activated_tools: Vec::new(),
        };
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config.clone(),
                Arc::new(NoopObserver),
            )
            .unwrap(),
        );
        engine
            .create_session_with_role(
                "r-cap".into(),
                1,
                SessionOrigin::Home,
                None,
                SessionRole::Router,
                None,
                WorkAccessMode::ReadWrite,
            )
            .unwrap();
        let total = engine::MAX_DISPATCH_REQUESTS + 5;
        for idx in 0..total {
            engine
                .record_dispatch_request(
                    "r-cap",
                    DispatchRecord {
                        request_id: format!("req-{idx:04}"),
                        fingerprint: format!("fp-{idx:04}"),
                        worker_id: format!("w-{idx:04}"),
                        receipt_id: format!("rcpt-{idx:04}"),
                        created_at_ms: 1_000 + idx as u64,
                    },
                )
                .unwrap();
        }
        let reopened =
            Engine::open(scratch.0.clone(), config, Arc::new(NoopObserver))
                .unwrap();
        let persisted = reopened.session("r-cap").unwrap();
        assert_eq!(
            persisted.dispatch_requests.len(),
            engine::MAX_DISPATCH_REQUESTS
        );
        for evicted in 0..5 {
            assert!(
                reopened
                    .lookup_dispatch_request(
                        "r-cap",
                        &format!("req-{evicted:04}")
                    )
                    .is_none(),
                "oldest dispatch record req-{evicted:04} should be evicted"
            );
        }
        for retained in 5..total {
            assert!(
                reopened
                    .lookup_dispatch_request(
                        "r-cap",
                        &format!("req-{retained:04}")
                    )
                    .is_some(),
                "newest dispatch record req-{retained:04} should remain"
            );
        }
    }

    #[test]
    fn suspend_with_card_halts_tool_batch_and_persists_disambiguation_card() {
        let scratch = Scratch::new("suspend-card");
        let second_tool_runs = Arc::new(AtomicUsize::new(0));
        let runs_c = Arc::clone(&second_tool_runs);

        let registry = tools::ToolRegistry::new();
        registry.register(Arc::new(tools::ToolDefinition::new(
            "ask_user_choice",
            "ask choice",
            serde_json::json!({"type":"object"}),
            |_args, _ctx| {
                Box::pin(async {
                    ToolExecutionResult::error(
                        "NEVER",
                        "should be suspended by pre_tool_hook",
                    )
                })
            },
        )));
        registry.register(Arc::new(tools::ToolDefinition::new(
            "dispatch_work",
            "dispatch work",
            serde_json::json!({"type":"object"}),
            move |_args, _ctx| {
                runs_c.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {
                    ToolExecutionResult::success_value(
                        serde_json::json!({"ok":true}),
                    )
                })
            },
        )));

        let hook: PreToolHook = Arc::new(|name, _args| {
            if name == "ask_user_choice" {
                PreToolDecision::SuspendWithCard {
                    card: PendingCard::Disambiguation {
                        card_id: "choice-1".into(),
                        created_at_ms: 100,
                        prompt: "Which deck?".into(),
                        impact_boundary: "Only edit selected".into(),
                        options: vec![DisambiguationOption {
                            object_id: "obj-ppt-1".into(),
                            label: "formal.pptx".into(),
                            anchor_summary: "formal.pptx".into(),
                            extension: "pptx".into(),
                        }],
                        allow_multi: false,
                        dispatch_receipt_id: None,
                    },
                    reason: "USER_CHOICE_REQUIRED".into(),
                }
            } else {
                PreToolDecision::Allow
            }
        });

        let config = EngineConfig {
            route: ModelRoute::default(),
            api_key: None,
            system: String::new(),
            registry,
            cwd: None,
            todos: None,
            delivery_committer: None,
            work_contract: None,
            pre_tool_hook: Some(hook),
            pre_activated_tools: Vec::new(),
        };
        let engine = Arc::new(
            Engine::open(
                scratch.0.clone(),
                config.clone(),
                Arc::new(NoopObserver),
            )
            .unwrap(),
        );
        engine.create_session("r-choice".into(), 1).unwrap();

        // Stream emits BOTH `ask_user_choice` AND `dispatch_work` in the same batch;
        // `SuspendWithCard` MUST terminate the batch so `dispatch_work` runs 0 times!
        let stream: rpi_agent::StreamFn = rpi_agent::stream_fn(
            |_model, _ctx, _opts| {
                use rpi_ai::types::{
                    Api, AssistantMessage, AssistantMessageEvent, Content,
                    DoneReason, StopReason, ToolCall, ToolCallType,
                };
                let mut msg = AssistantMessage::empty(
                    Api::OpenaiCompletions,
                    "test",
                    "test",
                    1,
                );
                msg.stop_reason = StopReason::ToolUse;
                msg.content = vec![
                    Content::ToolCall(ToolCall {
                        kind: ToolCallType,
                        id: "call-choice".into(),
                        name: "ask_user_choice".into(),
                        arguments: serde_json::json!({}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    }),
                    Content::ToolCall(ToolCall {
                        kind: ToolCallType,
                        id: "call-dispatch".into(),
                        name: "dispatch_work".into(),
                        arguments: serde_json::json!({"brief": "should not run"}),
                        thought_signature: None,
                        namespace: None,
                        recorded: None,
                    }),
                ];
                let (mut producer, consumer) =
                    rpi_ai::event_stream::create_assistant_message_event_stream(
                    );
                tokio::spawn(async move {
                    producer
                        .push(AssistantMessageEvent::Start {
                            partial: Arc::new(msg.clone()),
                        })
                        .await;
                    producer
                        .push(AssistantMessageEvent::Done {
                            reason: DoneReason::ToolUse,
                            message: msg,
                        })
                        .await;
                });
                consumer
            },
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let fut = engine
            .start_with_config(
                "r-choice".into(),
                PromptTurn::plain("change the date"),
                config,
                Some(stream),
                Some(99),
            )
            .unwrap();
        let outcome = rt.block_on(fut).unwrap();
        assert_eq!(outcome, LoopOutcome::WaitingForUser);
        assert_eq!(
            second_tool_runs.load(Ordering::SeqCst),
            0,
            "SuspendWithCard must halt remaining tools in the same batch"
        );
        let rec = engine.session("r-choice").unwrap();
        assert_eq!(rec.status, TurnState::WaitingForUser);
        assert_eq!(rec.pending_cards.len(), 1);
        assert_eq!(rec.pending_cards[0].card_id(), "choice-1");
    }
}
