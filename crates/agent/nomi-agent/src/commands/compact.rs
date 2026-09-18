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
                // 摘要在移动 `messages` 之前取出：`mechanical_fold` 为真时，这段历史
                // 不是被"总结"而是被丢弃（确定性占位符），用户必须知道。
                let mechanical_placeholder = result.mechanical_fold.then(|| {
                    result
                        .mechanical_reason
                        .clone()
                        .unwrap_or_else(|| "no reason recorded".to_string())
                });
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

                if msgs_summarized > 0 {
                    ctx.output.emit_info(&format!(
                        "Context compacted: {}k → compact ({} messages summarized)",
                        pre_tokens / 1000,
                        msgs_summarized
                    ));
                } else {
                    // 与自动路径一致（`engine::run_autocompact` 只在
                    // `messages_summarized > 0` 时打印成功行）：没有折到任何消息就不是
                    // "压缩成功"。空操作有两种来源——没有可折叠区，或可折叠区低于
                    // `auto::MIN_FOLD_TOKENS`（400 token）预算。复用上面那条同义文案，
                    // 保持单一措辞。
                    ctx.output.emit_info("Context is already compact");
                }

                if let Some(reason) = mechanical_placeholder {
                    // 非致命：上下文确实释放了，值得成功行，但摘要降级必须可见
                    // （`CompactResult::mechanical_fold` 的契约）。
                    ctx.output.emit_warning(&format!(
                        "Context compacted with a placeholder summary ({reason}); \
                         details from before that point are gone"
                    ));
                }
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
    use std::sync::{Arc, Mutex};

    use nomi_providers::{LlmProvider, ProviderError};
    use nomi_types::llm::{LlmEvent, LlmRequest};
    use nomi_types::message::{ContentBlock, Message, Role, StopReason};

    use super::*;
    use crate::commands::{CommandContext, CommandRegistry};
    use crate::compact::state::CompactState;
    use crate::output::OutputSink;
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

    /// Answers the summarization request the way a healthy provider does:
    /// one text delta carrying a `<summary>` block, then a terminal `EndTurn`.
    struct SummaryProvider;
    #[async_trait::async_trait]
    impl LlmProvider for SummaryProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tx.try_send(LlmEvent::TextDelta(
                "<summary>earlier work folded</summary>".to_string(),
            ))
            .unwrap();
            tx.try_send(LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: Default::default(),
            })
            .unwrap();
            drop(tx);
            Ok(rx)
        }
    }

    /// Records info and warning lines separately so a test can assert on exactly
    /// what the user is told, at which severity. `OutputSink::emit_warning`
    /// defaults to routing into `emit_info`; overriding it here keeps the two
    /// channels distinguishable.
    #[derive(Default)]
    struct CaptureSink {
        info: Mutex<Vec<String>>,
        warnings: Mutex<Vec<String>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self::default()
        }

        fn info(&self) -> Vec<String> {
            self.info.lock().unwrap().clone()
        }

        fn warnings(&self) -> Vec<String> {
            self.warnings.lock().unwrap().clone()
        }
    }

    impl OutputSink for CaptureSink {
        fn emit_text_delta(&self, _: &str, _: &str) {}
        fn emit_thinking(&self, _: &str, _: &str) {}
        fn emit_tool_call(&self, _: &str, _: &str, _: &str) {}
        fn emit_tool_result(&self, _: &str, _: &str, _: bool, _: &str) {}
        fn emit_stream_start(&self, _: &str) {}
        fn emit_output_discarded(&self, _: &str, _: u32) {}
        fn emit_stream_end(&self, _: &str, _: usize, _: u64, _: u64, _: u64, _: u64) {}
        fn emit_error(&self, _: &str) {}
        fn emit_info(&self, msg: &str) {
            self.info.lock().unwrap().push(msg.to_string());
        }
        fn emit_warning(&self, msg: &str) {
            self.warnings.lock().unwrap().push(msg.to_string());
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

    #[tokio::test]
    async fn no_op_compact_does_not_claim_success() {
        let provider: Arc<dyn LlmProvider> = Arc::new(NullProvider);
        let registry = CommandRegistry::new();
        let output = CaptureSink::new();
        // 6 条短消息：越过 `messages.len() <= 2` 的守卫，但折叠区远低于
        // `auto::MIN_FOLD_TOKENS`（400 token），因此必然空操作。
        let mut messages: Vec<Message> = (0..6)
            .map(|i| {
                Message::new(
                    if i % 2 == 0 {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    vec![ContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                )
            })
            .collect();
        let before = messages.len();
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

        CompactCommand.execute(&mut ctx, "").await.unwrap();

        let info = output.info();
        assert!(
            !info.iter().any(|line| line.contains("Context compacted")),
            "a no-op must not report success, got: {info:?}"
        );
        assert!(
            info.iter().any(|line| line.contains("already compact")),
            "a no-op must say why nothing happened, got: {info:?}"
        );
        // 空操作既不重写历史，也不留下压缩边界。
        assert_eq!(ctx.messages.len(), before);
        assert!(
            !ctx.messages.iter().any(|m| auto::is_compact_boundary(m)),
            "a no-op must not leave a compact boundary behind"
        );
    }

    /// 一个"确实有东西可折、且本次压缩会被 force"的手动 /compact 现场：8 条消息
    /// （首条是可钉住的小 user turn；assistant 各约 2000 字符 ≈ 500 token，远超
    /// `auto::MIN_FOLD_TOKENS` 的 400），window 8k，水位越过 autocompact 阈值，
    /// 从而走通 计划 → 折叠 → 摘要 全链路。
    fn compactible_fixture() -> (
        Vec<Message>,
        nomi_config::compact::CompactConfig,
        CompactState,
    ) {
        let long = || ContentBlock::Text {
            text: "a".repeat(2_000),
        };
        let short = |text: &str| ContentBlock::Text {
            text: text.to_string(),
        };
        let messages: Vec<Message> = vec![
            Message::new(Role::User, vec![short("ship it")]),
            Message::new(Role::Assistant, vec![long()]),
            Message::new(Role::User, vec![short("ok")]),
            Message::new(Role::Assistant, vec![long()]),
            Message::new(Role::User, vec![short("ok")]),
            Message::new(Role::Assistant, vec![long()]),
            Message::new(Role::User, vec![short("ok")]),
            Message::new(Role::Assistant, vec![long()]),
        ];

        let config = nomi_config::compact::CompactConfig {
            context_window: 8_000,
            output_reserve: 1_000,
            autocompact_buffer: 500,
            ..nomi_config::compact::CompactConfig::default()
        };
        let mut state = CompactState::new();
        state.last_input_tokens = 7_000;
        (messages, config, state)
    }

    /// 真的折到了消息时，成功行必须照常出现——别让上面的守卫把它一并吃掉。
    #[tokio::test]
    async fn real_compact_reports_summarized_messages() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryProvider);
        let registry = CommandRegistry::new();
        let output = CaptureSink::new();
        let (mut messages, config, mut state) = compactible_fixture();

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

        CompactCommand.execute(&mut ctx, "").await.unwrap();

        let info = output.info();
        assert!(
            info.iter().any(|line| line.contains("Context compacted")),
            "a real compaction must still report success, got: {info:?}"
        );
        assert!(
            info.iter().any(|line| line.contains("messages summarized")),
            "the success line must carry the summarized count, got: {info:?}"
        );
        assert!(
            ctx.messages.iter().any(|m| auto::is_compact_boundary(m)),
            "a real compaction must insert the boundary marker"
        );
        // 摘要是真的，就不该报占位符警告。
        assert!(
            output.warnings().is_empty(),
            "a healthy LLM summary must not warn, got: {:?}",
            output.warnings()
        );
    }

    /// 摘要器不可用时，上下文仍然释放（成功行照旧），但必须明确告知摘要是占位符——
    /// 否则用户以为这段历史被总结了，实际上它被丢弃了。
    #[tokio::test]
    async fn mechanical_fold_warns_about_placeholder_summary() {
        // NullProvider 立刻关闭流：摘要调用必然失败 → 机械折叠。
        let provider: Arc<dyn LlmProvider> = Arc::new(NullProvider);
        let registry = CommandRegistry::new();
        let output = CaptureSink::new();
        let (mut messages, config, mut state) = compactible_fixture();

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

        CompactCommand.execute(&mut ctx, "").await.unwrap();

        // 折叠发生了 → 成功行照旧（上下文确实被释放）。
        assert!(
            output
                .info()
                .iter()
                .any(|line| line.contains("Context compacted")),
            "a mechanical fold still frees context and must report it, got: {:?}",
            output.info()
        );
        // 但摘要降级必须可见。
        let warnings = output.warnings();
        assert!(
            warnings
                .iter()
                .any(|line| line.contains("placeholder summary")),
            "a mechanical fold must warn about the placeholder summary, got: {warnings:?}"
        );
    }
}