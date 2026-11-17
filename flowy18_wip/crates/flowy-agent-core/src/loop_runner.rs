//! One turn of the pi loop with host-owned messages.
//!
//! The loop is provider-agnostic: [`run_turn`] receives a [`StreamFn`] and
//! converts between the stored [`Message`] vocabulary and pi's agent messages.
//! Tool execution is wired by the embedding host (see `tools`).

use std::sync::Arc;

use rpi_agent::hooks::AgentLoopConfig;
use rpi_agent::{AgentContext, AgentEmitter, AgentMessage, AgentTool, StreamFn, run_agent_loop};
use rpi_ai::types::{
    Api, AssistantMessage, Content, StopReason, ThinkingContent, ToolCall, ToolCallType,
    ToolResultMessage, ToolResultRole, UserContent, UserMessage,
};

use crate::types::{CancelToken, ContentBlock, Message, MessageSource, Role};

/// Provider route for one run. Credentials live in the stream function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

impl ModelRoute {
    #[must_use]
    pub fn rpi_model(&self) -> rpi_ai::Model {
        rpi_ai::Model::new(
            self.model.clone(),
            self.model.clone(),
            Api::OpenaiCompletions,
            self.provider.clone(),
            self.base_url.clone(),
        )
    }
}

/// Inputs for one turn.
pub struct TurnRequest {
    pub system: String,
    pub history: Vec<Message>,
    pub prompt: String,
    pub route: ModelRoute,
    pub tools: Vec<Arc<dyn AgentTool>>,
    pub signal: CancelToken,
}

impl std::fmt::Debug for TurnRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnRequest")
            .field("history", &self.history.len())
            .field("prompt_len", &self.prompt.len())
            .field("route", &self.route)
            .field("tools", &self.tools.len())
            .finish_non_exhaustive()
    }
}

/// What the loop produced, in host vocabulary.
#[derive(Debug)]
pub struct TurnResult {
    pub messages: Vec<Message>,
    pub outcome: LoopOutcome,
}

/// Terminal state of one turn, derived from the last assistant message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopOutcome {
    Completed,
    Failed,
    Aborted,
}

/// Run one turn. `Err` means the loop itself failed to start or returned an
/// error; a provider failure that settled as a terminal message yields
/// `Ok(TurnResult { outcome: Failed, .. })` so callers cannot mistake it for
/// success.
pub async fn run_turn(
    request: TurnRequest,
    stream_fn: StreamFn,
    emit: Arc<dyn AgentEmitter>,
) -> Result<TurnResult, String> {
    let prompts = vec![AgentMessage::User(UserMessage::new(
        UserContent::Text(request.prompt),
        now_ms(),
    ))];
    let mut history = Vec::new();
    for message in &request.history {
        if let Some(converted) = to_agent_message(message)? {
            history.push(converted);
        }
    }
    let context = AgentContext {
        system_prompt: request.system,
        messages: history,
        tools: request.tools,
    };
    let pi_model = request.route.rpi_model();
    let signal = request.signal.inner();
    let config = AgentLoopConfig {
        model: pi_model,
        convert_to_llm: rpi_agent::hooks::default_convert_to_llm_fn(),
        convert_to_llm_incremental: None,
        transform_context: None,
        get_api_key: None,
        should_stop_after_turn: None,
        prepare_next_turn: None,
        after_tool_results: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        pending_message_count: None,
        before_tool_call: None,
        after_tool_call: None,
        tool_execution: rpi_agent::ToolExecutionMode::Sequential,
        thinking_level: rpi_ai::types::ThinkingLevel::Off,
        api_key: None,
        timeout: None,
        stream_idle_timeout: None,
        max_retries: Some(0),
        max_retry_delay: None,
        cache_retention: rpi_ai::provider::CacheRetention::default(),
        session_id: None,
        signal,
    };
    let new_messages = run_agent_loop(prompts, context, config, emit, stream_fn)
        .await
        .map_err(|error| error.to_string())?;
    let outcome = outcome_of(&new_messages);
    let messages = to_host_messages(&new_messages);
    Ok(TurnResult { messages, outcome })
}

fn outcome_of(new_messages: &[AgentMessage]) -> LoopOutcome {
    let last = new_messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(assistant) => Some(assistant),
        _ => None,
    });
    match last.map(|assistant| assistant.stop_reason) {
        Some(StopReason::Error) => LoopOutcome::Failed,
        Some(StopReason::Aborted) => LoopOutcome::Aborted,
        Some(_) | None => LoopOutcome::Completed,
    }
}

/// Convert stored messages to pi agent messages. Tool results must carry the
/// originating tool name via [`MessageSource::Tool`].
pub fn to_agent_message(message: &Message) -> Result<Option<AgentMessage>, String> {
    if let Some(tool_result) = message
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => Some((tool_call_id, content, is_error)),
            _ => None,
        })
    {
        let tool_name = match &message.source {
            MessageSource::Tool { tool } => tool.clone(),
            _ => return Err("tool result message is missing its tool name".into()),
        };
        let (tool_call_id, content, is_error) = tool_result;
        return Ok(Some(AgentMessage::ToolResult(Box::new(ToolResultMessage {
            role: ToolResultRole,
            tool_call_id: tool_call_id.clone(),
            tool_name,
            content: to_pi_content(content)?,
            details: None,
            usage: None,
            added_tool_names: Vec::new(),
            is_error: is_error.unwrap_or(false),
            timestamp: now_ms(),
        }))));
    }
    match message.role {
        Role::System => Err("system messages belong in the system prompt".into()),
        Role::User => Ok(Some(AgentMessage::User(UserMessage::new(
            UserContent::Text(message.text()),
            now_ms(),
        )))),
        Role::Assistant => {
            let (provider, model) = match &message.source {
                MessageSource::Model { provider, model } => {
                    (provider.clone(), model.clone())
                }
                _ => ("flowy".to_owned(), String::new()),
            };
            let mut assistant = AssistantMessage::empty(
                Api::OpenaiCompletions,
                provider,
                model,
                now_ms(),
            );
            assistant.content = to_pi_content(&message.content)?;
            assistant.stop_reason = if assistant
                .content
                .iter()
                .any(|content| matches!(content, Content::ToolCall(_)))
            {
                StopReason::ToolUse
            } else {
                StopReason::Stop
            };
            Ok(Some(AgentMessage::Assistant(Box::new(assistant))))
        }
    }
}

fn to_pi_content(blocks: &[ContentBlock]) -> Result<Vec<Content>, String> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => Ok(Content::text(text.clone())),
            ContentBlock::Reasoning { text } => Ok(Content::Thinking(ThinkingContent {
                kind: rpi_ai::types::ThinkingContentType,
                thinking: text.clone(),
                thinking_signature: None,
                redacted: false,
            })),
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
            } => {
                let arguments: serde_json::Value = serde_json::from_str(arguments)
                    .map_err(|error| format!("tool call {id} has invalid JSON arguments: {error}"))?;
                Ok(Content::ToolCall(ToolCall {
                    kind: ToolCallType,
                    id: id.clone(),
                    name: name.clone(),
                    arguments,
                    thought_signature: None,
                    namespace: None,
                    recorded: None,
                }))
            }
            ContentBlock::ToolResult { .. } => {
                Err("tool result blocks must be their own message".into())
            }
        })
        .collect()
}

/// Convert the loop's new messages back to host messages for durable storage.
#[must_use]
pub fn to_host_messages(new_messages: &[AgentMessage]) -> Vec<Message> {
    let mut out = Vec::new();
    for message in new_messages {
        match message {
            AgentMessage::User(user) => {
                out.push(Message {
                    id: next_id(),
                    role: Role::User,
                    content: vec![ContentBlock::text(user_text(&user.content))],
                    source: MessageSource::User,
                });
            }
            AgentMessage::Assistant(assistant) => {
                let content: Vec<ContentBlock> = assistant
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        Content::Text(text) => Some(ContentBlock::text(text.text.clone())),
                        Content::Thinking(thinking) => Some(ContentBlock::Reasoning {
                            text: thinking.thinking.clone(),
                        }),
                        Content::ToolCall(call) => Some(ContentBlock::ToolCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.to_string(),
                        }),
                        Content::Image(_) => None,
                    })
                    .collect();
                out.push(Message {
                    id: next_id(),
                    role: Role::Assistant,
                    content,
                    source: MessageSource::Model {
                        provider: assistant.provider.clone(),
                        model: assistant.model.clone(),
                    },
                });
            }
            AgentMessage::ToolResult(result) => {
                let content = result
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        Content::Text(text) => Some(ContentBlock::text(text.text.clone())),
                        Content::Thinking(thinking) => Some(ContentBlock::text(
                            thinking.thinking.clone(),
                        )),
                        _ => None,
                    })
                    .collect();
                out.push(Message {
                    id: next_id(),
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_call_id: result.tool_call_id.clone(),
                        content,
                        is_error: Some(result.is_error),
                    }],
                    source: MessageSource::Tool {
                        tool: result.tool_name.clone(),
                    },
                });
            }
            AgentMessage::Custom(_) => {}
        }
    }
    out
}

fn user_text(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Content::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn next_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("m-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{ModelRoute, TurnRequest, to_agent_message, to_host_messages, run_turn, LoopOutcome};
    use crate::types::{
        CancelToken, ContentBlock, Message, MessageSource, Role,
    };
    use rpi_agent::{AgentMessage, CollectorEmitter};
    use rpi_ai::types::{
        Api, AssistantMessage, AssistantMessageEvent, Content, DoneReason, ErrorReason,
        StopReason,
    };
    use std::sync::Arc;

    fn route() -> ModelRoute {
        ModelRoute {
            provider: "flowy".into(),
            model: "AIPC-deepseek-v4-flash".into(),
            base_url: "https://example.invalid/claw/v1".into(),
        }
    }

    #[test]
    fn tool_history_converts_both_ways_without_losing_pairing_or_arguments() {
        let history = vec![
            Message {
                id: "u1".into(),
                role: Role::User,
                content: vec![ContentBlock::text("read it")],
                source: MessageSource::User,
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
            },
            Message {
                id: "t1".into(),
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_call_id: "call-1".into(),
                    content: vec![ContentBlock::text("data")],
                    is_error: Some(false),
                }],
                source: MessageSource::Tool {
                    tool: "read_file".into(),
                },
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

    fn scripted_stream(text: &'static str, stop: StopReason) -> rpi_agent::StreamFn {
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
    async fn successful_turn_returns_host_messages_not_raw_ok() {
        let (emitter, _events) = CollectorEmitter::new();
        let result = run_turn(
            TurnRequest {
                system: "be brief".into(),
                history: vec![],
                prompt: "hi".into(),
                route: route(),
                tools: Vec::new(),
                signal: CancelToken::new(),
            },
            scripted_stream("hello", StopReason::Stop),
            Arc::new(emitter),
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, LoopOutcome::Completed);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[1].content, vec![ContentBlock::text("hello")]);
    }

    #[tokio::test]
    async fn provider_error_is_failed_not_completed() {
        let (emitter, _events) = CollectorEmitter::new();
        let result = run_turn(
            TurnRequest {
                system: String::new(),
                history: vec![],
                prompt: "hi".into(),
                route: route(),
                tools: Vec::new(),
                signal: CancelToken::new(),
            },
            scripted_stream("boom", StopReason::Error),
            Arc::new(emitter),
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, LoopOutcome::Failed);
    }
}
