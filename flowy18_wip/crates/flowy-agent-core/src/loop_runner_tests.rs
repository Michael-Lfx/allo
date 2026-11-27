use super::{
    LoopOutcome, ModelRoute, TurnRequest, log_preview,
    repair_tool_call_history, run_turn, to_agent_message, to_host_messages,
};
use crate::types::{CancelToken, ContentBlock, Message, MessageSource, Role};
use rpi_agent::{AgentMessage, CollectorEmitter};
use rpi_ai::types::{
    Api, AssistantMessage, AssistantMessageEvent, Content, DoneReason,
    ErrorReason, StopReason,
};
use std::sync::Arc;

fn route() -> ModelRoute {
    ModelRoute {
        provider: "flowy".into(),
        model: "AIPC-deepseek-v4-flash".into(),
        base_url: "https://example.invalid/claw/v1".into(),
        ..ModelRoute::default()
    }
}

#[test]
fn catalog_limits_reach_the_pi_model() {
    let route = ModelRoute {
        context_window: 1234,
        max_tokens: 567,
        ..route()
    };
    let model = route.rpi_model();
    assert_eq!(model.context_window, 1234);
    assert_eq!(model.max_tokens, 567);
}

#[test]
fn orphan_tool_call_gets_placeholder_and_orphan_results_are_dropped() {
    let assistant = Message {
        id: "a1".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.txt"}"#.into(),
        }],
        source: MessageSource::Model {
            provider: "flowy".into(),
            model: "m".into(),
        },
        display_text: None,
        dispatch_receipt_id: None,
    };
    let user = Message {
        id: "u1".into(),
        role: Role::User,
        content: vec![ContentBlock::text("next")],
        source: MessageSource::User,
        display_text: None,
        dispatch_receipt_id: None,
    };
    let mut history: Vec<AgentMessage> = Vec::new();
    for message in [&assistant, &user] {
        if let Some(converted) = to_agent_message(message).unwrap() {
            history.push(converted);
        }
    }
    let repaired = repair_tool_call_history(history);
    assert_eq!(repaired.len(), 3);
    let AgentMessage::ToolResult(result) = &repaired[1] else {
        panic!("placeholder tool result expected right after assistant");
    };
    assert_eq!(result.tool_call_id, "call-1");
    assert_eq!(result.tool_name, "read_file");
    assert!(result.is_error);
    let AgentMessage::User(next) = &repaired[2] else {
        panic!("user message must stay after the repaired batch");
    };
    assert_eq!(super::user_text(&next.content), "next");
}

#[test]
fn unmatched_tool_result_is_dropped_from_provider_history() {
    let mut history: Vec<AgentMessage> = Vec::new();
    let user = Message {
        id: "u1".into(),
        role: Role::User,
        content: vec![ContentBlock::text("hi")],
        source: MessageSource::User,
        display_text: None,
        dispatch_receipt_id: None,
    };
    history.push(to_agent_message(&user).unwrap().unwrap());
    history.push(AgentMessage::ToolResult(Box::new(
        rpi_ai::types::ToolResultMessage {
            role: rpi_ai::types::ToolResultRole,
            tool_call_id: "call-9".into(),
            tool_name: "read_file".into(),
            content: vec![Content::text("stale")],
            details: None,
            usage: None,
            added_tool_names: Vec::new(),
            is_error: false,
            timestamp: 0,
        },
    )));
    let repaired = repair_tool_call_history(history);
    assert_eq!(repaired.len(), 1);
    assert!(matches!(repaired[0], AgentMessage::User(_)));
}

#[test]
fn invalid_tool_arguments_degrade_to_an_empty_object() {
    let assistant = Message {
        id: "a1".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: "{not json".into(),
        }],
        source: MessageSource::Model {
            provider: "flowy".into(),
            model: "m".into(),
        },
        display_text: None,
        dispatch_receipt_id: None,
    };
    let converted = to_agent_message(&assistant).unwrap().unwrap();
    let AgentMessage::Assistant(assistant) = converted else {
        panic!("assistant message expected");
    };
    let Content::ToolCall(call) = &assistant.content[0] else {
        panic!("tool call expected");
    };
    assert!(call.arguments.is_object());
    assert_eq!(call.arguments, serde_json::json!({}));
}

#[test]
fn legacy_tool_result_without_tool_source_is_skipped() {
    let message = Message {
        id: "t-legacy".into(),
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_call_id: "call-9".into(),
            content: vec![ContentBlock::text("data")],
            is_error: Some(false),
            artifact: None,
        }],
        source: MessageSource::User,
        display_text: None,
        dispatch_receipt_id: None,
    };
    assert!(to_agent_message(&message).unwrap().is_none());
}

#[test]
fn tool_history_converts_both_ways_without_losing_pairing_or_arguments() {
    let history = vec![
        Message {
            id: "u1".into(),
            role: Role::User,
            content: vec![ContentBlock::text("read it")],
            source: MessageSource::User,
            display_text: None,
            dispatch_receipt_id: None,
        },
        Message {
            id: "a1".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall {
                id: "call-1".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"a.txt","limit":10}"#.into(),
            }],
            source: MessageSource::Model {
                provider: "flowy".into(),
                model: "m".into(),
            },
            display_text: None,
            dispatch_receipt_id: None,
        },
        Message {
            id: "t1".into(),
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_call_id: "call-1".into(),
                content: vec![ContentBlock::text("data")],
                is_error: Some(false),
                artifact: None,
            }],
            source: MessageSource::Tool {
                tool: "read_file".into(),
            },
            display_text: None,
            dispatch_receipt_id: None,
        },
    ];
    for message in &history {
        assert!(to_agent_message(message).unwrap().is_some());
    }
    let assistant = to_agent_message(&history[1]).unwrap().unwrap();
    let AgentMessage::Assistant(assistant) = assistant else {
        panic!("expected assistant message");
    };
    let Content::ToolCall(call) = &assistant.content[0] else {
        panic!("expected tool call");
    };
    assert_eq!(call.id, "call-1");
    assert_eq!(call.arguments["path"], "a.txt");
    assert_eq!(assistant.stop_reason, StopReason::ToolUse);

    let back = to_host_messages(&[AgentMessage::Assistant(assistant)]);
    let ContentBlock::ToolCall { arguments, .. } = &back[0].content[0] else {
        panic!("expected tool call after round trip");
    };
    let parsed: serde_json::Value = serde_json::from_str(arguments).unwrap();
    assert_eq!(parsed["path"], "a.txt");
}

fn scripted_stream_capturing(
    text: &'static str,
    stop: StopReason,
    captured: Arc<std::sync::Mutex<Vec<rpi_ai::types::Message>>>,
) -> rpi_agent::StreamFn {
    rpi_agent::stream_fn(move |model, context, _| {
        if let Ok(mut guard) = captured.lock() {
            *guard = context.messages.clone();
        }
        let mut message = AssistantMessage::empty(
            Api::Other("test".into()),
            "flowy",
            model.id.clone(),
            0,
        );
        message.content = vec![Content::text(text)];
        message.stop_reason = stop;
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
}

fn scripted_stream(
    text: &'static str,
    stop: StopReason,
) -> rpi_agent::StreamFn {
    rpi_agent::stream_fn(move |model, _, _| {
        let mut message = AssistantMessage::empty(
            Api::Other("test".into()),
            "flowy",
            model.id.clone(),
            0,
        );
        message.content = vec![Content::text(text)];
        message.stop_reason = stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Start {
                    partial: Arc::new(message.clone()),
                })
                .await;
            match stop {
                StopReason::Error | StopReason::Aborted => {
                    producer
                        .push(AssistantMessageEvent::Error {
                            reason: ErrorReason::Error,
                            error: message,
                        })
                        .await;
                }
                _ => {
                    producer
                        .push(AssistantMessageEvent::Done {
                            reason: DoneReason::Stop,
                            message,
                        })
                        .await;
                }
            }
        });
        consumer
    })
}

#[tokio::test]
async fn orphaned_tool_call_is_repaired_before_the_provider_request() {
    let assistant = Message {
        id: "a1".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.txt"}"#.into(),
        }],
        source: MessageSource::Model {
            provider: "flowy".into(),
            model: "m".into(),
        },
        display_text: None,
        dispatch_receipt_id: None,
    };
    // Legacy row without MessageSource::Tool: converted to None, so the
    // assistant tool call would otherwise reach the provider unanswered.
    let legacy_result = Message {
        id: "t1".into(),
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_call_id: "call-1".into(),
            content: vec![ContentBlock::text("data")],
            is_error: Some(false),
            artifact: None,
        }],
        source: MessageSource::User,
        display_text: None,
        dispatch_receipt_id: None,
    };
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: vec![assistant, legacy_result],
            prompt: "hi".into(),
            route: route(),
            tools: Vec::new(),
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        scripted_stream_capturing(
            "ok",
            StopReason::Stop,
            Arc::clone(&captured),
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Completed);
    let messages = captured.lock().unwrap().clone();
    assert!(messages.iter().any(|message| matches!(
        message,
        rpi_ai::types::Message::Assistant(assistant)
            if assistant.content.iter().any(|content| matches!(
                content,
                Content::ToolCall(call) if call.id == "call-1"
            ))
    )));
    let repaired = messages
        .iter()
        .find_map(|message| match message {
            rpi_ai::types::Message::ToolResult(result)
                if result.tool_call_id == "call-1" =>
            {
                Some(result)
            }
            _ => None,
        })
        .expect("provider request must carry a result for call-1");
    assert_eq!(repaired.tool_name, "read_file");
    assert!(repaired.is_error);
    assert!(
        Content::text_only(&repaired.content, "").contains("not re-executed")
    );
}

#[tokio::test]
async fn successful_turn_returns_host_messages_not_raw_ok() {
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: "be brief".into(),
            history: vec![],
            prompt: "hi".into(),
            route: route(),
            tools: Vec::new(),
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        scripted_stream("hello", StopReason::Stop),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Completed);
    assert_eq!(result.messages.len(), 2);
    assert_eq!(
        result.messages[1].content,
        vec![ContentBlock::text("hello")]
    );
}

#[tokio::test]
async fn provider_error_is_failed_not_completed() {
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: vec![],
            prompt: "hi".into(),
            route: route(),
            tools: Vec::new(),
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        scripted_stream("boom", StopReason::Error),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Failed);
    assert_eq!(result.error.as_deref(), Some("boom"));
}

#[test]
fn log_preview_truncates_by_char_count() {
    assert_eq!(log_preview("hi", 10), "hi");
    assert_eq!(log_preview("abcdefghij", 5), "abcde…");
    assert_eq!(log_preview("你好世界", 2), "你好…");
}

#[test]
fn compact_context_truncates_oversized_tool_result_and_preserves_tool_pairing()
{
    let huge = "x".repeat(super::MAX_TOOL_RESULT_CHARS + 500);
    let messages = vec![
        AgentMessage::User(rpi_ai::types::UserMessage::new(
            rpi_ai::types::UserContent::Text("old question".into()),
            1,
        )),
        AgentMessage::Assistant(Box::new({
            let mut msg = AssistantMessage::empty(
                Api::Other("test".into()),
                "flowy",
                "m",
                2,
            );
            msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
                kind: rpi_ai::types::ToolCallType,
                id: "call-old".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({}),
                thought_signature: None,
                namespace: None,
                recorded: None,
            })];
            msg
        })),
        AgentMessage::ToolResult(Box::new(rpi_ai::types::ToolResultMessage {
            role: rpi_ai::types::ToolResultRole,
            tool_call_id: "call-old".into(),
            tool_name: "read_file".into(),
            content: vec![Content::text(huge)],
            details: None,
            usage: None,
            added_tool_names: Vec::new(),
            is_error: false,
            timestamp: 3,
        })),
        AgentMessage::User(rpi_ai::types::UserMessage::new(
            rpi_ai::types::UserContent::Text("latest question".into()),
            4,
        )),
        AgentMessage::Assistant(Box::new({
            let mut msg = AssistantMessage::empty(
                Api::Other("test".into()),
                "flowy",
                "m",
                5,
            );
            msg.content = vec![Content::text("latest answer")];
            msg
        })),
    ];

    // Large budget keeps all messages but still truncates the giant tool output.
    let truncated_only = super::compact_context_messages(
        messages.clone(),
        super::DEFAULT_CONTEXT_CHAR_BUDGET,
    );
    assert_eq!(truncated_only.len(), 5);
    let AgentMessage::ToolResult(tr) = &truncated_only[2] else {
        panic!("expected tool result at index 2");
    };
    let Content::Text(text) = &tr.content[0] else {
        panic!("expected text block");
    };
    assert!(text.text.contains("truncated by host context guard"));
    assert!(text.text.chars().count() < super::MAX_TOOL_RESULT_CHARS + 100);

    // Tight budget drops oldest turns cleanly without leaving an orphaned ToolResult.
    let tightened = super::compact_context_messages(messages, 100);
    assert!(tightened.len() <= 3);
    assert!(!matches!(
        tightened.first(),
        Some(AgentMessage::ToolResult(_))
    ));
}

#[tokio::test]
async fn run_turn_stops_at_max_tool_turns_cap() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_ref = Arc::clone(&calls);
    let stream = rpi_agent::stream_fn(move |model, _context, _| {
        let idx = calls_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut msg = AssistantMessage::empty(
            Api::Other("test".into()),
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
            kind: rpi_ai::types::ToolCallType,
            id: format!("loop-call-{idx}"),
            name: "noop_tool".into(),
            arguments: serde_json::json!({}),
            thought_signature: None,
            namespace: None,
            recorded: None,
        })];
        msg.stop_reason = StopReason::ToolUse;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
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
    });
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "noop_tool",
        "always succeeds",
        serde_json::json!({"type": "object"}),
        |_args, _ctx| {
            Box::pin(async {
                crate::types::ToolExecutionResult::success_text(
                    "ok",
                    serde_json::json!({"ok": true}),
                )
            })
        },
    ));
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: Vec::new(),
            prompt: "loop forever".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        stream,
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Completed);
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        super::MAX_TOOL_TURNS_PER_SUBMISSION
    );
}

#[tokio::test]
async fn complete_structured_extracts_tool_call_arguments_in_one_request() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_ref = Arc::clone(&calls);
    let stream = rpi_agent::stream_fn(move |model, context, _| {
        calls_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(context.tools.len(), 1);
        assert_eq!(context.tools[0].name, "submit_verdict");
        let mut msg = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
            kind: rpi_ai::types::ToolCallType,
            id: "call-struct-1".into(),
            name: "submit_verdict".into(),
            arguments: serde_json::json!({
                "ok": false,
                "issues": [{"where": "S0", "what": "wrong region", "why": "brief says East"}]
            }),
            thought_signature: None,
            namespace: None,
            recorded: None,
        })];
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
    let value = super::complete_structured_with_stream(
        super::StructuredRequest {
            session_id: Some("struct-1".into()),
            system: "judge".into(),
            prompt: "check".into(),
            tool_name: "submit_verdict".into(),
            tool_description: "submit verdict".into(),
            schema: serde_json::json!({"type": "object"}),
            route: route(),
            signal: CancelToken::new(),
        },
        stream,
    )
    .await
    .expect("structured value");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["issues"][0]["where"], "S0");
}

#[tokio::test]
async fn complete_structured_falls_back_to_json_text_and_rejects_garbage() {
    let json_stream = rpi_agent::stream_fn(|model, _, _| {
        let mut msg = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::text(
            "```json\n{\"ok\": true, \"issues\": []}\n```",
        )];
        msg.stop_reason = StopReason::Stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Done {
                    reason: DoneReason::Stop,
                    message: msg,
                })
                .await;
        });
        consumer
    });
    let req = || super::StructuredRequest {
        session_id: None,
        system: "judge".into(),
        prompt: "check".into(),
        tool_name: "submit_verdict".into(),
        tool_description: "submit verdict".into(),
        schema: serde_json::json!({"type": "object"}),
        route: route(),
        signal: CancelToken::new(),
    };
    let value = super::complete_structured_with_stream(req(), json_stream)
        .await
        .expect("json fallback");
    assert_eq!(value["ok"], true);

    let bad_stream = rpi_agent::stream_fn(|model, _, _| {
        let mut msg = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::text("not valid json")];
        msg.stop_reason = StopReason::Stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Done {
                    reason: DoneReason::Stop,
                    message: msg,
                })
                .await;
        });
        consumer
    });
    assert!(
        super::complete_structured_with_stream(req(), bad_stream)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn complete_structured_rejects_length_truncation_non_object_and_cancelled_signal()
 {
    let make_req = |signal: CancelToken| super::StructuredRequest {
        session_id: None,
        system: "judge".into(),
        prompt: "check".into(),
        tool_name: "submit_verdict".into(),
        tool_description: "submit verdict".into(),
        schema: serde_json::json!({"type": "object"}),
        route: route(),
        signal,
    };

    // 1) Truncated `DoneReason::Length` must fail closed even if partial
    //    tool arguments look like `{"ok": true}`.
    let truncated_stream = rpi_agent::stream_fn(|model, _, _| {
        let mut msg = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
            kind: rpi_ai::types::ToolCallType,
            id: "tc-trunc".into(),
            name: "submit_verdict".into(),
            arguments: serde_json::json!({"ok": true}),
            thought_signature: None,
            namespace: None,
            recorded: None,
        })];
        msg.stop_reason = StopReason::Length;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Done {
                    reason: DoneReason::Length,
                    message: msg,
                })
                .await;
        });
        consumer
    });
    assert!(
        super::complete_structured_with_stream(
            make_req(CancelToken::new()),
            truncated_stream,
        )
        .await
        .is_err()
    );

    // 2) Non-object JSON fallback (e.g. `true` or `[1,2]`) must be rejected.
    let scalar_stream = rpi_agent::stream_fn(|model, _, _| {
        let mut msg = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::text("true")];
        msg.stop_reason = StopReason::Stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Done {
                    reason: DoneReason::Stop,
                    message: msg,
                })
                .await;
        });
        consumer
    });
    assert!(
        super::complete_structured_with_stream(
            make_req(CancelToken::new()),
            scalar_stream,
        )
        .await
        .is_err()
    );

    // 3) Pre-cancelled token returns error immediately without waiting on stream.
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let hanging_stream = rpi_agent::stream_fn(|_, _, _| {
        let (_producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        consumer
    });
    assert!(
        super::complete_structured_with_stream(
            make_req(cancelled),
            hanging_stream,
        )
        .await
        .is_err()
    );
}

fn make_test_tool(
    def: crate::tools::ToolDefinition,
) -> Arc<dyn rpi_agent::AgentTool> {
    let registry = crate::tools::ToolRegistry::new();
    let def = Arc::new(def);
    registry.register(Arc::clone(&def));
    Arc::new(
        crate::tools::RegistryTool::new(
            registry,
            def,
            crate::types::ToolRunContext {
                signal: CancelToken::new(),
                agent_id: None,
                cwd: None,
                todos: None,
                submission_id: None,
                session_origin: crate::session::SessionOrigin::Unknown,
                output_root: None,
                artifact_delivery_available: false,
            },
        )
        .unwrap(),
    )
}

fn make_tool_stream(
    tool_name: &'static str,
    args: serde_json::Value,
    calls: Arc<std::sync::atomic::AtomicUsize>,
) -> rpi_agent::StreamFn {
    rpi_agent::stream_fn(move |model, _context, _| {
        let idx = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut msg = AssistantMessage::empty(
            Api::Other("test".into()),
            "flowy",
            model.id.clone(),
            0,
        );
        msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
            kind: rpi_ai::types::ToolCallType,
            id: format!("cb-call-{idx}"),
            name: tool_name.into(),
            arguments: args.clone(),
            thought_signature: None,
            namespace: None,
            recorded: None,
        })];
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
    })
}

#[tokio::test]
async fn same_non_transient_error_trips_circuit_breaker_without_burning_24_turns()
 {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "write_file",
        "always returns permission denied",
        serde_json::json!({"type": "object"}),
        |_args, _ctx| {
            Box::pin(async {
                crate::types::ToolExecutionResult::error(
                    "PERMISSION_DENIED",
                    "cannot write outside workspace",
                )
            })
        },
    ));
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: Vec::new(),
            prompt: "write protected file".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        make_tool_stream(
            "write_file",
            serde_json::json!({"path": "/etc/hosts"}),
            Arc::clone(&calls),
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Failed);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("SAME_ERROR_CIRCUIT_BREAKER")),
        "circuit breaker trip must return Failed with diagnostic error: {:?}",
        result.error
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "identical non-transient error must trip on 2nd identical failure (0 extra retries), never burning all 24"
    );
}

#[tokio::test]
async fn transient_readonly_tool_error_allows_one_retry_before_tripping() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "read_file",
        "always times out transiently",
        serde_json::json!({"type": "object"}),
        |_args, _ctx| {
            Box::pin(async {
                crate::types::ToolExecutionResult::error(
                    "IO_ERROR",
                    "file temporarily unavailable: sharing violation (try again)",
                )
            })
        },
    ));
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: Vec::new(),
            prompt: "read busy file".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        make_tool_stream(
            "read_file",
            serde_json::json!({"path": "report.xlsx"}),
            Arc::clone(&calls),
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Failed);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("SAME_ERROR_CIRCUIT_BREAKER")),
        "transient circuit breaker trip must return Failed with diagnostic error: {:?}",
        result.error
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "transient read-only error gets 1 identical retry (attempt 2) and trips on attempt 3"
    );
}

#[tokio::test]
async fn different_args_on_same_file_do_not_false_trip_circuit_breaker() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_stream = Arc::clone(&calls);
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "edit_file",
        "succeeds only when old_string matches target line",
        serde_json::json!({"type": "object"}),
        |args, _ctx| {
            let matched = args
                .arguments
                .get("old_string")
                .and_then(serde_json::Value::as_str)
                == Some("needle_3");
            Box::pin(async move {
                if matched {
                    crate::types::ToolExecutionResult::success_text(
                        "edited",
                        serde_json::json!({"ok": true}),
                    )
                } else {
                    crate::types::ToolExecutionResult::error(
                        "NOT_FOUND",
                        "old_string not found",
                    )
                }
            })
        },
    ));
    let stream = rpi_agent::stream_fn(move |model, _, _| {
        let idx =
            calls_stream.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut msg = AssistantMessage::empty(
            Api::Other("test".into()),
            "flowy",
            model.id.clone(),
            0,
        );
        let (reason, stop) = if idx < 3 {
            msg.content = vec![Content::ToolCall(rpi_ai::types::ToolCall {
                kind: rpi_ai::types::ToolCallType,
                id: format!("edit-call-{idx}"),
                name: "edit_file".into(),
                arguments: serde_json::json!({
                    "path": "src/lib.rs",
                    "old_string": format!("needle_{}", idx + 1),
                    "new_string": "replacement",
                }),
                thought_signature: None,
                namespace: None,
                recorded: None,
            })];
            (DoneReason::ToolUse, StopReason::ToolUse)
        } else {
            msg.content = vec![Content::text("edit applied")];
            (DoneReason::Stop, StopReason::Stop)
        };
        msg.stop_reason = stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Done {
                    reason,
                    message: msg,
                })
                .await;
        });
        consumer
    });
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: Vec::new(),
            prompt: "edit src/lib.rs".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        stream,
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Completed);
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        4,
        "different edit_file arguments on the same path must never false-trip the breaker"
    );
}

#[tokio::test]
async fn intervening_successful_tool_evidence_resets_error_breaker() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let exec_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let exec_ref = Arc::clone(&exec_count);
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "write_file",
        "fails on turns 0, 2, 3; succeeds on turn 1",
        serde_json::json!({"type": "object"}),
        move |_args, _ctx| {
            let n = exec_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if n == 1 {
                    crate::types::ToolExecutionResult::success_text(
                        "ok",
                        serde_json::json!({"ok": true}),
                    )
                } else {
                    crate::types::ToolExecutionResult::error(
                        "CONFLICT",
                        "stale lock",
                    )
                }
            })
        },
    ));
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: None,
            system: String::new(),
            history: Vec::new(),
            prompt: "retry after evidence".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: None,
        },
        make_tool_stream(
            "write_file",
            serde_json::json!({"path": "out.txt"}),
            Arc::clone(&calls),
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Failed);
    // Turn 0: err (streak=1) -> Turn 1: ok (streak=0) -> Turn 2: err (streak=1) -> Turn 3: err (streak=2 -> stop)
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[tokio::test]
async fn before_tool_call_ask_suspends_cleanly_without_executing_tool() {
    let executed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executed_ref = Arc::clone(&executed);
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "powershell",
        "risky shell command",
        serde_json::json!({"type": "object"}),
        move |_args, _ctx| {
            executed_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                crate::types::ToolExecutionResult::success_text(
                    "ran",
                    serde_json::json!({"ok": true}),
                )
            })
        },
    ));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook: super::PreToolHook = Arc::new(|name, args| {
        let canonical = super::canonical_tool_args_json(name, args);
        super::PreToolDecision::Ask {
            target_summary: name.to_owned(),
            args_hash: format!("hash:{canonical}"),
            risk_reason: "destructive workspace delete".to_owned(),
            command_preview: "Remove-Item -Recurse -Force ./deliverables"
                .to_owned(),
        }
    });
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: Some("s-ask".into()),
            system: String::new(),
            history: Vec::new(),
            prompt: "clean deliverables".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal: CancelToken::new(),
            pre_tool_hook: Some(hook),
        },
        make_tool_stream(
            "powershell",
            serde_json::json!({"command": "Remove-Item -Recurse -Force ./deliverables"}),
            Arc::clone(&calls),
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::WaitingForUser);
    assert_eq!(
        executed.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "tool body must NEVER execute when pre_tool_hook returns Ask"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    match result.pending_approval {
        Some(crate::session::PendingCard::ToolApproval {
            tool_name,
            risk_reason,
            ..
        }) => {
            assert_eq!(tool_name, "powershell");
            assert_eq!(risk_reason, "destructive workspace delete");
        }
        other => {
            panic!("expected PendingCard::ToolApproval, got {other:?}")
        }
    }
}

#[test]
fn canonical_args_strip_shell_description_and_breaker_respects_turns_and_trailing_success()
 {
    // 1. Shell `description` drift and float-vs-int timeout normalize identically.
    let a = serde_json::json!({
        "command": "rm -rf ./deliverables",
        "description": "first wording",
        "timeout": 120,
    });
    let b = serde_json::json!({
        "timeout": 120.0,
        "description": "different wording",
        "command": "rm -rf ./deliverables",
    });
    assert_eq!(
        super::canonical_tool_args_json("bash", &a),
        super::canonical_tool_args_json("bash", &b)
    );
    // Non-shell tool preserves `description`.
    assert_ne!(
        super::canonical_tool_args_json("write_file", &a),
        super::canonical_tool_args_json("write_file", &b)
    );

    // 2. Bare digits like line 503 or 429 bytes are not transient HTTP errors.
    assert!(!super::is_transient_tool_error("parse error at line 503"));
    assert!(!super::is_transient_tool_error("wrote 429 bytes"));
    assert!(super::is_transient_tool_error(
        "HTTP 503 Service Unavailable"
    ));
    assert!(super::is_transient_tool_error("status=429 rate exceeded"));

    // 3. Same-turn parallel failures do not trip, and a trailing success resets.
    let mk_assistant = |calls: &[(&str, &str)]| {
        let mut msg =
            AssistantMessage::empty(Api::Other("test".into()), "flowy", "m", 0);
        msg.content = calls
            .iter()
            .map(|(id, path)| {
                Content::ToolCall(rpi_ai::types::ToolCall {
                    kind: rpi_ai::types::ToolCallType,
                    id: (*id).into(),
                    name: "write_file".into(),
                    arguments: serde_json::json!({"path": path}),
                    thought_signature: None,
                    namespace: None,
                    recorded: None,
                })
            })
            .collect();
        AgentMessage::Assistant(Box::new(msg))
    };
    let mk_res = |id: &str, is_error: bool, text: &str| {
        AgentMessage::ToolResult(Box::new(rpi_ai::types::ToolResultMessage {
            role: rpi_ai::types::ToolResultRole,
            tool_call_id: id.into(),
            tool_name: "write_file".into(),
            content: vec![Content::text(text)],
            details: None,
            usage: None,
            added_tool_names: Vec::new(),
            is_error,
            timestamp: 0,
        }))
    };
    let same_turn_batch = vec![
        mk_assistant(&[("c1", "a.txt"), ("c2", "a.txt")]),
        mk_res("c1", true, "denied"),
        mk_res("c2", true, "denied"),
    ];
    assert!(
        super::should_trip_same_error_breaker(&same_turn_batch).is_none(),
        "duplicate calls inside a single assistant turn must not trip before the model sees the error"
    );
    let trailing_ok = vec![
        mk_assistant(&[("c1", "a.txt")]),
        mk_res("c1", true, "denied"),
        mk_assistant(&[("c2", "a.txt"), ("c3", "b.txt")]),
        mk_res("c2", true, "denied"),
        mk_res("c3", false, "ok"),
    ];
    assert!(
        super::should_trip_same_error_breaker(&trailing_ok).is_none(),
        "trailing successful tool result provides new evidence and must clear prior streak"
    );
    let real_repeat = vec![
        mk_assistant(&[("c1", "a.txt")]),
        mk_res("c1", true, "denied"),
        mk_assistant(&[("c2", "a.txt")]),
        mk_res("c2", true, "denied"),
    ];
    assert!(
        super::should_trip_same_error_breaker(&real_repeat).is_some(),
        "two consecutive assistant turns with identical error must trip"
    );
}

#[tokio::test]
async fn cancelled_turn_takes_priority_over_pre_tool_approval() {
    let signal = CancelToken::new();
    let hook_signal = signal.clone();
    let tool = make_test_tool(crate::tools::ToolDefinition::new(
        "powershell",
        "risky shell command",
        serde_json::json!({"type": "object"}),
        |_args, _ctx| {
            Box::pin(async {
                crate::types::ToolExecutionResult::success_text(
                    "ran",
                    serde_json::json!({"ok": true}),
                )
            })
        },
    ));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook: super::PreToolHook = Arc::new(move |name, args| {
        // Simulate cancel arriving right as before_tool_call runs.
        hook_signal.cancel();
        super::PreToolDecision::Ask {
            target_summary: name.to_owned(),
            args_hash: super::canonical_tool_args_json(name, args),
            risk_reason: "risky".to_owned(),
            command_preview: "rm -rf ./deliverables".to_owned(),
        }
    });
    let (emitter, _events) = CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id: Some("s-abort-wins".into()),
            system: String::new(),
            history: Vec::new(),
            prompt: "clean".into(),
            route: route(),
            tools: vec![tool],
            deferred_tools: Vec::new(),
            activated_tools: None,
            read_only_tools: std::collections::HashSet::new(),
            signal,
            pre_tool_hook: Some(hook),
        },
        make_tool_stream(
            "powershell",
            serde_json::json!({"command": "rm -rf ./deliverables"}),
            calls,
        ),
        Arc::new(emitter),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, LoopOutcome::Aborted);
    assert!(
        result.pending_approval.is_none(),
        "cancelled turn must discard pending_approval instead of parking WaitingForUser"
    );
}

#[test]
fn failure_detail_prefers_error_message_and_falls_back_to_plain_text() {
    let mut msg =
        AssistantMessage::empty(Api::Other("test".into()), "flowy", "m", 0);
    msg.stop_reason = StopReason::Error;
    msg.error_message = Some("HTTP 429 rate limit".into());
    let with_err_msg = [AgentMessage::Assistant(Box::new(msg))];
    assert_eq!(
        super::failure_detail(&with_err_msg).as_deref(),
        Some("HTTP 429 rate limit")
    );

    let mut blank_err_msg =
        AssistantMessage::empty(Api::Other("test".into()), "flowy", "m", 0);
    blank_err_msg.stop_reason = StopReason::Error;
    blank_err_msg.error_message = Some("   ".into());
    blank_err_msg.content = vec![Content::text("upstream closed stream")];
    let fallback_text = [AgentMessage::Assistant(Box::new(blank_err_msg))];
    assert_eq!(
        super::failure_detail(&fallback_text).as_deref(),
        Some("upstream closed stream")
    );
}
