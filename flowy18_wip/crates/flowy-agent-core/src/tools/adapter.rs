//! Adapts a host [`ToolDefinition`] to the pi loop's `AgentTool` contract.
//!
//! Every call still goes through [`ToolRegistry::execute`], so the host policy,
//! monotonic guards, cancellation check, and argument handling stay in force.

use std::sync::Arc;

use async_trait::async_trait;
use rpi_agent::{AgentError, AgentTool, AgentToolResult, TextContentOrImage};
use rpi_ai::types::Tool;
use tokio_util::sync::CancellationToken;

use crate::tools::registry::ToolRegistry;
use crate::tools::ToolDefinition;
use crate::types::{CancelToken, ContentBlock, ToolExecutionResult, ToolRunContext};

/// One registered host tool exposed to the loop.
pub struct RegistryTool {
    registry: ToolRegistry,
    definition: Arc<ToolDefinition>,
    schema: Tool,
    run_ctx: ToolRunContext,
}

impl std::fmt::Debug for RegistryTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryTool")
            .field("name", &self.definition.name)
            .finish_non_exhaustive()
    }
}

impl RegistryTool {
    pub fn new(
        registry: ToolRegistry,
        definition: Arc<ToolDefinition>,
        run_ctx: ToolRunContext,
    ) -> Result<Self, String> {
        let core_schema = definition.schema();
        let parameters = serde_json::from_value(core_schema.parameters)
            .map_err(|error| format!("tool {} has an invalid schema: {error}", core_schema.name))?;
        Ok(Self {
            registry,
            definition,
            schema: Tool {
                name: core_schema.name,
                description: core_schema.description,
                parameters,
                constrained_sampling: None,
            },
            run_ctx,
        })
    }
}

#[async_trait]
impl AgentTool for RegistryTool {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        &self.definition.name
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        params: serde_json::Value,
        signal: CancellationToken,
        _on_update: Arc<dyn Fn(rpi_agent::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let mut run_ctx = self.run_ctx.clone();
        run_ctx.signal = CancelToken::from_inner(signal);
        let result = self
            .registry
            .execute(
                tool_call_id.to_owned(),
                self.definition.name.clone(),
                params,
                run_ctx,
            )
            .await;
        match result {
            ToolExecutionResult::Success { content, value, .. } => Ok(AgentToolResult {
                content: to_loop_content(&content),
                details: value,
                ..AgentToolResult::default()
            }),
            ToolExecutionResult::Error {
                message,
                code,
                content,
            } => Err(AgentError::StructuredTool(Box::new(AgentToolResult {
                content: to_loop_content(&content),
                details: serde_json::json!({ "code": code, "message": message }),
                ..AgentToolResult::default()
            }))),
        }
    }
}

fn to_loop_content(content: &[ContentBlock]) -> Vec<TextContentOrImage> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(TextContentOrImage::text(text.clone())),
            ContentBlock::Reasoning { text } => Some(TextContentOrImage::text(text.clone())),
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::RegistryTool;
    use crate::loop_runner::{LoopOutcome, ModelRoute, TurnRequest, run_turn};
    use crate::tools::{ToolDefinition, ToolRegistry};
    use crate::types::{
        CancelToken, ContentBlock, ToolCallArgs, ToolExecutionResult, ToolRunContext,
    };
    use rpi_agent::CollectorEmitter;
    use rpi_ai::types::{
        Api, AssistantMessage, AssistantMessageEvent, Content, DoneReason, StopReason, ToolCall,
        ToolCallType,
    };
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn echo_tool(calls: Arc<AtomicUsize>) -> Arc<ToolDefinition> {
        Arc::new(ToolDefinition::new(
            "echo",
            "echo text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            move |args: ToolCallArgs, _ctx: ToolRunContext| {
                let calls = calls.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let text = args.arguments["text"].as_str().unwrap_or_default().to_owned();
                    ToolExecutionResult::success_text(text, args.arguments)
                })
            },
        ))
    }

    fn ctx() -> ToolRunContext {
        ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("agent".into()),
            cwd: None,
            todos: None,
        }
    }

    #[tokio::test]
    async fn loop_tool_call_executes_through_the_host_registry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = ToolRegistry::new();
        registry.register(echo_tool(calls.clone()));
        let tool = RegistryTool::new(
            registry.clone(),
            registry.get("echo").unwrap(),
            ctx(),
        )
        .unwrap();

        let turn = Arc::new(AtomicUsize::new(0));
        let stream = rpi_agent::stream_fn(move |model, context, _| {
            let index = turn.fetch_add(1, Ordering::SeqCst);
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            let mut message = AssistantMessage::empty(
                Api::OpenaiCompletions,
                "flowy",
                model.id.clone(),
                0,
            );
            if index == 0 {
                message.content = vec![Content::ToolCall(ToolCall {
                    kind: ToolCallType,
                    id: "call-1".into(),
                    name: "echo".into(),
                    arguments: json!({"text":"payload"}),
                    thought_signature: None,
                    namespace: None,
                    recorded: None,
                })];
                message.stop_reason = StopReason::ToolUse;
            } else {
                let rpi_ai::types::Message::ToolResult(result) = context.messages.last().unwrap()
                else {
                    panic!("second request must carry the tool result");
                };
                assert_eq!(result.tool_call_id, "call-1");
                assert!(!result.is_error);
                message.content = vec![Content::text("done")];
                message.stop_reason = StopReason::Stop;
            }
            tokio::spawn(async move {
                producer
                    .push(AssistantMessageEvent::Done {
                        reason: DoneReason::Stop,
                        message,
                    })
                    .await;
            });
            consumer
        });

        let (emitter, _events) = CollectorEmitter::new();
        let result = run_turn(
            TurnRequest {
                system: String::new(),
                history: vec![],
                prompt: "echo payload".into(),
                route: ModelRoute {
                    provider: "flowy".into(),
                    model: "m".into(),
                    base_url: "https://example.invalid".into(),
                },
                tools: vec![Arc::new(tool)],
                signal: CancelToken::new(),
            },
            stream,
            Arc::new(emitter),
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, LoopOutcome::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let tool_result = result
            .messages
            .iter()
            .find(|message| {
                message
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
            })
            .expect("tool result stored in host history");
        assert!(matches!(
            &tool_result.content[0],
            ContentBlock::ToolResult { is_error: Some(false), .. }
        ));
    }
}
