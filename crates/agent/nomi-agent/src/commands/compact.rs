use async_trait::async_trait;

use super::{CommandContext, CommandResult, SlashCommand};
use crate::compact::auto;
use crate::compact::estimate::estimate_tokens_from_messages;
use nomi_types::compact::CompactTrigger;

pub struct CompactCommand;

#[async_trait]
impl SlashCommand for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self) -> &str {
        "Compress conversation context. Optional args become Compact Instructions \
         for the summarizer (e.g. /compact keep the API contract)."
    }

    async fn execute(
        &self,
        ctx: &mut CommandContext<'_>,
        args: &str,
    ) -> anyhow::Result<CommandResult> {
        if ctx.messages.len() <= 2 {
            ctx.output.emit_info("Context is already compact");
            return Ok(CommandResult::Continue);
        }

        // Reset circuit breaker — manual intent overrides protection
        ctx.compact_state.consecutive_failures = 0;

        let pre_tokens = ctx.compact_state.last_input_tokens;

        match auto::autocompact_with(
            ctx.provider.as_ref(),
            ctx.messages,
            ctx.model,
            ctx.compact_config,
            ctx.compact_state,
            auto::AutocompactRequest {
                force_mechanical: false,
                observation: ctx.observation.clone(),
                focus: Some(args.trim()).filter(|s| !s.is_empty()),
                trigger: Some(CompactTrigger::Manual),
                archive_cwd: ctx.workspace_cwd.as_deref(),
                session_id: ctx.session_id.as_deref(),
            },
        )
        .await
        {
            Ok(result) => {
                let msgs_summarized = result.messages_summarized;
                *ctx.messages = result.messages;

                if let Some(boundary) = ctx
                    .messages
                    .iter_mut()
                    .rev()
                    .find(|message| auto::is_compact_boundary(message))
                {
                    for block in &mut boundary.content {
                        if let nomi_types::message::ContentBlock::Text { text } = block
                            && text.starts_with(auto::BOUNDARY_PREFIX)
                        {
                            let metadata = nomi_types::compact::CompactMetadata {
                                trigger: CompactTrigger::Manual,
                                pre_compact_tokens: pre_tokens,
                                messages_summarized: msgs_summarized,
                            };
                            *text = format!(
                                "{}\n{}",
                                auto::BOUNDARY_PREFIX,
                                serde_json::to_string(&metadata)
                                    .expect("metadata serialization cannot fail")
                            );
                        }
                    }
                }

                ctx.compact_state.set_watermark(
                    estimate_tokens_from_messages(ctx.messages),
                    ctx.compact_config,
                );

                ctx.output.emit_info(&format!(
                    "Context compacted: {}k → compact ({} messages summarized)",
                    pre_tokens / 1000,
                    msgs_summarized
                ));
            }
            Err(e) => {
                ctx.output.emit_warning(&format!("Compact failed: {}", e));
            }
        }

        Ok(CommandResult::Continue)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nomi_providers::{LlmProvider, ProviderError};
    use nomi_types::llm::{LlmEvent, LlmRequest};
    use nomi_types::message::{ContentBlock, Message, Role};

    use super::*;
    use crate::commands::{CommandContext, CommandRegistry};
    use crate::compact::state::CompactState;
    use crate::output::null_sink::NullSink;

    struct NullProvider;
    #[async_trait::async_trait]
    impl LlmProvider for NullProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, ProviderError> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn compact_already_compact_guard() {
        let provider: Arc<dyn LlmProvider> = Arc::new(NullProvider);
        let registry = CommandRegistry::new();
        let output = NullSink;
        let mut messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text { text: "hi".into() }],
        )];
        let mut state = CompactState::new();
        let config = nomi_config::compact::CompactConfig::default();

        let mut ctx = CommandContext {
            messages: &mut messages,
            compact_state: &mut state,
            compact_config: &config,
            provider,
            model: "test-model",
            output: &output,
            registry: &registry,
            observation: None,
            workspace_cwd: None,
            session_id: None,
        };

        let cmd = CompactCommand;
        let result = cmd.execute(&mut ctx, "").await.unwrap();
        assert_eq!(result, CommandResult::Continue);
        assert_eq!(ctx.messages.len(), 1);
    }

    #[tokio::test]
    async fn compact_resets_circuit_breaker() {
        let provider: Arc<dyn LlmProvider> = Arc::new(NullProvider);
        let registry = CommandRegistry::new();
        let output = NullSink;
        let mut messages: Vec<Message> = (0..10)
            .map(|i| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                Message::new(
                    role,
                    vec![ContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                )
            })
            .collect();
        let mut state = CompactState::new();
        state.consecutive_failures = 5;
        let config = nomi_config::compact::CompactConfig::default();

        let mut ctx = CommandContext {
            messages: &mut messages,
            compact_state: &mut state,
            compact_config: &config,
            provider,
            model: "test-model",
            output: &output,
            registry: &registry,
            observation: None,
            workspace_cwd: None,
            session_id: None,
        };

        let cmd = CompactCommand;
        let _ = cmd.execute(&mut ctx, "").await;
        // Circuit breaker was reset to 0 before the call, then failure increments it
        assert!(ctx.compact_state.consecutive_failures <= 1);
    }

    struct RecordingProvider {
        last_prompt: std::sync::Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for RecordingProvider {
        async fn stream(
            &self,
            request: &LlmRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, ProviderError> {
            let prompt = request
                .messages
                .last()
                .and_then(|m| {
                    m.content.iter().find_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_default();
            *self.last_prompt.lock().unwrap() = Some(prompt);
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let _ = tx
                .try_send(LlmEvent::TextDelta(
                    "<summary>Standing facts preserved</summary>".into(),
                ));
            let _ = tx.try_send(LlmEvent::Done {
                stop_reason: nomi_types::message::StopReason::EndTurn,
                usage: Default::default(),
            });
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn compact_args_reach_summarizer_prompt() {
        let recorder = Arc::new(RecordingProvider {
            last_prompt: std::sync::Mutex::new(None),
        });
        let provider: Arc<dyn LlmProvider> = recorder.clone();
        let registry = CommandRegistry::new();
        let output = NullSink;
        let blob = "x".repeat(2_000);
        let mut messages: Vec<Message> = (0..40)
            .map(|i| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                Message::new(
                    role,
                    vec![ContentBlock::Text {
                        text: format!("msg-{i} {blob}"),
                    }],
                )
            })
            .collect();
        let mut state = CompactState::new();
        state.last_input_tokens = 50_000;
        let config = nomi_config::compact::CompactConfig::default();

        let mut ctx = CommandContext {
            messages: &mut messages,
            compact_state: &mut state,
            compact_config: &config,
            provider,
            model: "test-model",
            output: &output,
            registry: &registry,
            observation: None,
            workspace_cwd: None,
            session_id: None,
        };

        let cmd = CompactCommand;
        let _ = cmd.execute(&mut ctx, "keep the API contract").await;
        let prompt = recorder.last_prompt.lock().unwrap().clone().unwrap_or_default();
        assert!(
            prompt.contains("Compact Instructions"),
            "summarizer prompt should include Compact Instructions, got: {prompt}"
        );
        assert!(prompt.contains("keep the API contract"));
    }
}
