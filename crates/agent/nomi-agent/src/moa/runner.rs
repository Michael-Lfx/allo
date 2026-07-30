//! Reference fan-out: parallel advisory calls with cadence-aware caching.
//!
//! Cancellation: the engine owns no polling abort flag — a user interrupt
//! drops the turn future, which drops the [`tokio::task::JoinSet`] here and
//! aborts every in-flight reference call automatically.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use nomi_config::config::Config;
use nomi_providers::{LlmProvider, create_provider};
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::message::{ContentBlock, Message, Role, TokenUsage};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::advisory_view::{build_advisory_view, trim_view_to_window};
use super::prompts::REFERENCE_SYSTEM_PROMPT;
use super::{FanoutCadence, MoaAdvice, MoaSlotTurnUsage, MoaState};

/// Upper bound on simultaneous reference calls.
const MAX_CONCURRENT_REFERENCES: usize = 8;

/// Result of one fan-out opportunity.
#[derive(Debug, Clone)]
pub struct MoaOutcome {
    /// One entry per configured slot, in slot order. Failed slots carry a
    /// `"[failed: …]"` sentinel text.
    pub advices: Vec<MoaAdvice>,
    /// Per-slot advisor usage for this round, in slot order (failed slots
    /// carry zeros). Empty on cache hits.
    pub slot_usage: Vec<MoaSlotTurnUsage>,
    /// Summed advisor token usage (zero on cache hits).
    pub usage: TokenUsage,
    /// The advisory view actually sent to the references (before per-slot
    /// window trimming). Empty on cache hits — nothing was sent.
    pub advisory_input: Vec<Message>,
    /// True when the advices were served from the signature cache without
    /// re-running the references (the engine skips re-emitting events).
    pub from_cache: bool,
}

/// Injectable provider factory — tests swap in scripted providers.
pub type ProviderFactory = Arc<dyn Fn(&Config) -> Arc<dyn LlmProvider> + Send + Sync>;

/// Runs the reference fan-out against a [`MoaState`].
pub struct MoaRunner {
    provider_factory: ProviderFactory,
}

impl Default for MoaRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl MoaRunner {
    pub fn new() -> Self {
        Self {
            provider_factory: Arc::new(|config: &Config| create_provider(config)),
        }
    }

    /// Test seam: build references through a custom factory.
    pub fn with_provider_factory(provider_factory: ProviderFactory) -> Self {
        Self { provider_factory }
    }

    /// One fan-out opportunity. Returns `None` when MoA is inactive, when the
    /// cadence skips this iteration with no cache to reuse, or when every
    /// reference failed (the caller then proceeds single-model and may
    /// surface an informational notice).
    pub async fn run(&self, state: &mut MoaState, messages: &[Message]) -> Option<MoaOutcome> {
        if !state.is_active() {
            return None;
        }
        state.iteration_count += 1;

        let labels = state
            .slots
            .iter()
            .map(|s| s.label.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let signature = turn_signature(messages, &labels);

        match state.cadence() {
            FanoutCadence::UserTurn => {
                // Same turn → the prefix through the last real user message
                // is unchanged → serve the cached advice without re-running.
                if state.last_run_signature == Some(signature) {
                    if let Some(cached) = &state.cached_advices {
                        return Some(MoaOutcome {
                            advices: cached.clone(),
                            slot_usage: Vec::new(),
                            usage: TokenUsage::default(),
                            advisory_input: Vec::new(),
                            from_cache: true,
                        });
                    }
                }
            }
            FanoutCadence::PerIteration => {}
            FanoutCadence::EveryN(n) => {
                // Advise on iteration 1, then every Nth; off-cycle iterations
                // reuse the last cache when present (else fall through and run).
                let scheduled = (state.iteration_count - 1) % n.max(1) == 0;
                if !scheduled {
                    if let Some(cached) = &state.cached_advices {
                        return Some(MoaOutcome {
                            advices: cached.clone(),
                            slot_usage: Vec::new(),
                            usage: TokenUsage::default(),
                            advisory_input: Vec::new(),
                            from_cache: true,
                        });
                    }
                }
            }
        }

        let (advices, slot_usage, usage, advisory_input) = self.fan_out(state, messages).await;
        if advices.iter().all(|a| a.text.starts_with("[failed:")) {
            // Aggregator-alone round: don't cache failure sentinels, and keep
            // the old signature so a later iteration may retry. Nothing useful
            // ran, so per-slot turn usage is not extended either.
            return None;
        }
        state.last_run_signature = Some(signature);
        state.cached_advices = Some(advices.clone());
        state.turn_slot_usage.extend(slot_usage.iter().cloned());
        Some(MoaOutcome {
            advices,
            slot_usage,
            usage,
            advisory_input,
            from_cache: false,
        })
    }

    async fn fan_out(
        &self,
        state: &MoaState,
        messages: &[Message],
    ) -> (Vec<MoaAdvice>, Vec<MoaSlotTurnUsage>, TokenUsage, Vec<Message>) {
        let view = build_advisory_view(messages);
        let timeout = Duration::from_secs(state.config.reference_timeout_secs.max(1));
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REFERENCES));
        let mut join_set: JoinSet<(usize, Result<(String, TokenUsage), String>)> = JoinSet::new();

        for (idx, slot) in state.slots.iter().enumerate() {
            let provider = (self.provider_factory)(&slot.config);
            let request = LlmRequest {
                model: slot.config.model.clone(),
                system: REFERENCE_SYSTEM_PROMPT.to_string(),
                messages: trim_view_to_window(view.clone(), slot.context_window_tokens),
                tools: Vec::new(),
                max_tokens: slot.max_tokens.unwrap_or(state.config.reference_max_tokens),
                thinking: None,
                reasoning_effort: None,
                temperature: slot.temperature,
            };
            let semaphore = Arc::clone(&semaphore);
            join_set.spawn(async move {
                let _permit = semaphore.acquire_owned().await;
                let result =
                    match tokio::time::timeout(timeout, collect_advice(provider, &request)).await {
                        Ok(result) => result,
                        Err(_) => Err(format!("timeout after {}s", timeout.as_secs())),
                    };
                (idx, result)
            });
        }

        // A panicked/aborted task keeps its pre-filled failure entry; one bad
        // slot never takes the round down.
        let mut results: Vec<Result<(String, TokenUsage), String>> = state
            .slots
            .iter()
            .map(|_| Err("reference task aborted".to_string()))
            .collect();
        while let Some(joined) = join_set.join_next().await {
            if let Ok((idx, result)) = joined {
                results[idx] = result;
            }
        }

        let mut usage = TokenUsage::default();
        let mut slot_usage = Vec::with_capacity(state.slots.len());
        let advices = state
            .slots
            .iter()
            .zip(results)
            .map(|(slot, result)| {
                let text = match result {
                    Ok((text, this_usage)) => {
                        usage.input_tokens += this_usage.input_tokens;
                        usage.output_tokens += this_usage.output_tokens;
                        usage.cache_creation_tokens += this_usage.cache_creation_tokens;
                        usage.cache_read_tokens += this_usage.cache_read_tokens;
                        slot_usage.push(MoaSlotTurnUsage {
                            label: slot.label.clone(),
                            input_tokens: this_usage.input_tokens,
                            output_tokens: this_usage.output_tokens,
                        });
                        text
                    }
                    Err(reason) => {
                        // Failed slot: keep the list aligned with a zero entry.
                        slot_usage.push(MoaSlotTurnUsage {
                            label: slot.label.clone(),
                            input_tokens: 0,
                            output_tokens: 0,
                        });
                        format!("[failed: {reason}]")
                    }
                };
                MoaAdvice {
                    label: slot.label.clone(),
                    text,
                }
            })
            .collect();
        (advices, slot_usage, usage, view)
    }
}

/// Drain one reference stream into (advice text, usage).
async fn collect_advice(
    provider: Arc<dyn LlmProvider>,
    request: &LlmRequest,
) -> Result<(String, TokenUsage), String> {
    let mut rx = provider.stream(request).await.map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut usage = TokenUsage::default();
    while let Some(event) = rx.recv().await {
        match event {
            LlmEvent::TextDelta(delta) => text.push_str(&delta),
            LlmEvent::Done { usage: done, .. } => usage = done,
            LlmEvent::Error(message) => return Err(message),
            // References carry no tools; thinking/tool noise is dropped.
            _ => {}
        }
    }
    if text.trim().is_empty() {
        return Err("empty response".to_string());
    }
    Ok((text, usage))
}

/// Turn signature: hash of the history prefix through the last REAL user
/// message (typed user content — tool-result carrier frames don't count),
/// plus the slot labels. Later iterations of the same turn only append
/// frames after that point, so the signature is stable within a turn and
/// naturally changes on the next user turn.
pub(crate) fn turn_signature(messages: &[Message], slot_labels: &str) -> u64 {
    let mut end = messages.len();
    for (idx, msg) in messages.iter().enumerate().rev() {
        if msg.role == Role::User && is_real_user_message(msg) {
            end = idx + 1;
            break;
        }
    }
    let mut hasher = DefaultHasher::new();
    for msg in &messages[..end] {
        role_tag(msg.role).hash(&mut hasher);
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => text.hash(&mut hasher),
                ContentBlock::ToolUse { name, input, .. } => {
                    name.hash(&mut hasher);
                    input.to_string().hash(&mut hasher);
                }
                ContentBlock::ToolResult { content, .. } => content.hash(&mut hasher),
                ContentBlock::Thinking { .. } => {}
                ContentBlock::Image { data, .. } => data.len().hash(&mut hasher),
            }
        }
    }
    slot_labels.hash(&mut hasher);
    hasher.finish()
}

fn is_real_user_message(msg: &Message) -> bool {
    let carries_tool_result = msg
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolResult { .. }));
    let carries_user_payload = msg
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { .. } | ContentBlock::Image { .. }));
    !carries_tool_result && carries_user_payload
}

fn role_tag(role: Role) -> u8 {
    match role {
        Role::User => 0,
        Role::Assistant => 1,
        Role::System => 2,
        Role::Tool => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use nomi_config::config::{MoaConfig, ProviderType};
    use nomi_providers::ProviderError;
    use nomi_types::message::StopReason;
    use tokio::sync::mpsc;

    use super::super::MoaResolvedSlot;
    use super::*;

    /// Scripted stand-in provider: replays a fixed event list, or fails to
    /// connect. Mirrors the MockLlmProvider pattern in tests/common.
    struct ScriptedProvider {
        events: Vec<LlmEvent>,
        fail_connect: bool,
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(
            &self,
            _request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            if self.fail_connect {
                return Err(ProviderError::Api {
                    status: 500,
                    message: "connect refused".into(),
                });
            }
            let (tx, rx) = mpsc::channel(64);
            let events = self.events.clone();
            tokio::spawn(async move {
                for event in events {
                    let _ = tx.send(event).await;
                }
            });
            Ok(rx)
        }
    }

    fn advice_events(text: &str, output_tokens: u64) -> Vec<LlmEvent> {
        vec![
            LlmEvent::TextDelta(text.to_string()),
            LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens,
                    ..TokenUsage::default()
                },
            },
        ]
    }

    fn test_config(model: &str) -> Config {
        Config {
            provider_label: "mock".into(),
            provider: ProviderType::OpenAI,
            api_key: "test-key".into(),
            base_url: String::new(),
            model: model.into(),
            max_tokens: 1024,
            max_turns: None,
            system_prompt: None,
            project_instructions: Default::default(),
            thinking: None,
            prompt_caching: false,
            compat: Default::default(),
            tools: Default::default(),
            session: Default::default(),
            compact: Default::default(),
            plan: Default::default(),
            file_cache: Default::default(),
            hooks: Default::default(),
            bedrock: None,
            vertex: None,
            mcp: Default::default(),
            logging: Default::default(),
            memory: Default::default(),
            moa: Default::default(),
        }
    }

    fn slot(model: &str) -> MoaResolvedSlot {
        MoaResolvedSlot {
            config: test_config(model),
            label: format!("mock/{model}"),
            max_tokens: None,
            temperature: None,
            context_window_tokens: None,
        }
    }

    fn enabled_state(fanout: &str, slots: Vec<MoaResolvedSlot>) -> MoaState {
        MoaState::new(
            MoaConfig {
                enabled: true,
                fanout: fanout.to_string(),
                reference_timeout_secs: 5,
                ..MoaConfig::default()
            },
            slots,
        )
    }

    /// Stand-in provider whose stream stays open but never yields an event:
    /// the sender side is parked forever, so only the per-slot timeout can
    /// end the call.
    struct StalledProvider;

    #[async_trait::async_trait]
    impl LlmProvider for StalledProvider {
        async fn stream(
            &self,
            _request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _keep_open = tx;
                std::future::pending::<()>().await;
            });
            Ok(rx)
        }
    }

    /// Factory routing "stalls" to a stalled provider, everything else to the
    /// usual scripted advice.
    fn stalling_factory() -> ProviderFactory {
        Arc::new(|config: &Config| {
            let provider: Arc<dyn LlmProvider> = match config.model.as_str() {
                "stalls" => Arc::new(StalledProvider),
                model => Arc::new(ScriptedProvider {
                    events: advice_events(&format!("advice from {model}"), 7),
                    fail_connect: false,
                }),
            };
            provider
        })
    }

    /// Factory that scripts behavior per model name and counts invocations.
    fn counting_factory(calls: Arc<AtomicUsize>) -> ProviderFactory {
        Arc::new(move |config: &Config| {
            calls.fetch_add(1, Ordering::SeqCst);
            let provider: Arc<dyn LlmProvider> = match config.model.as_str() {
                "fails" => Arc::new(ScriptedProvider {
                    events: Vec::new(),
                    fail_connect: true,
                }),
                "errors" => Arc::new(ScriptedProvider {
                    events: vec![LlmEvent::Error("rate limited".into())],
                    fail_connect: false,
                }),
                model => Arc::new(ScriptedProvider {
                    events: advice_events(&format!("advice from {model}"), 7),
                    fail_connect: false,
                }),
            };
            provider
        })
    }

    fn user(text: &str) -> Message {
        Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        )
    }

    fn assistant(text: &str) -> Message {
        Message::new(
            Role::Assistant,
            vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        )
    }

    fn tool_result_frame(content: &str) -> Message {
        Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: content.to_string(),
                is_error: false,
                images: Vec::new(),
            }],
        )
    }

    #[tokio::test]
    async fn all_slots_succeed_and_usage_is_summed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = enabled_state("user_turn", vec![slot("alpha"), slot("beta")]);
        let messages = vec![user("hello")];

        let outcome = runner.run(&mut state, &messages).await.expect("outcome");
        assert!(!outcome.from_cache);
        assert_eq!(outcome.advices.len(), 2);
        assert_eq!(outcome.advices[0].label, "mock/alpha");
        assert_eq!(outcome.advices[0].text, "advice from alpha");
        assert_eq!(outcome.advices[1].text, "advice from beta");
        assert_eq!(outcome.usage.output_tokens, 14);
        assert_eq!(outcome.usage.input_tokens, 20);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn per_slot_usage_is_recorded_with_zeros_for_failed_slots() {
        let runner =
            MoaRunner::with_provider_factory(counting_factory(Arc::new(AtomicUsize::new(0))));
        let mut state = enabled_state("user_turn", vec![slot("alpha"), slot("fails")]);
        let messages = vec![user("hello")];

        let outcome = runner.run(&mut state, &messages).await.expect("outcome");
        assert_eq!(
            outcome.slot_usage,
            vec![
                MoaSlotTurnUsage {
                    label: "mock/alpha".into(),
                    input_tokens: 10,
                    output_tokens: 7,
                },
                MoaSlotTurnUsage {
                    label: "mock/fails".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                },
            ]
        );
        // The real fan-out also lands in the turn-scoped accumulator.
        assert_eq!(state.turn_slot_usage(), outcome.slot_usage.as_slice());
        // The advisory input that was sent rides along for tracing.
        assert!(!outcome.advisory_input.is_empty());
    }

    #[tokio::test]
    async fn cache_hit_does_not_duplicate_turn_slot_usage() {
        let runner =
            MoaRunner::with_provider_factory(counting_factory(Arc::new(AtomicUsize::new(0))));
        let mut state = enabled_state("user_turn", vec![slot("alpha")]);
        let mut messages = vec![user("do the thing")];

        runner.run(&mut state, &messages).await.expect("first run");
        assert_eq!(state.turn_slot_usage().len(), 1);

        messages.push(assistant("calling a tool"));
        messages.push(tool_result_frame("tool output"));
        let second = runner.run(&mut state, &messages).await.expect("cache hit");
        assert!(second.from_cache);
        assert!(second.slot_usage.is_empty());
        assert!(second.advisory_input.is_empty());
        assert_eq!(
            state.turn_slot_usage().len(),
            1,
            "cache-hit rounds must not append per-slot usage"
        );
    }

    #[tokio::test]
    async fn per_iteration_reruns_accumulate_turn_slot_usage() {
        let runner =
            MoaRunner::with_provider_factory(counting_factory(Arc::new(AtomicUsize::new(0))));
        let mut state = enabled_state("per_iteration", vec![slot("alpha")]);
        let messages = vec![user("go")];

        runner.run(&mut state, &messages).await.unwrap();
        runner.run(&mut state, &messages).await.unwrap();
        assert_eq!(state.turn_slot_usage().len(), 2);
        assert_eq!(state.turn_slot_usage()[0].input_tokens, 10);
        assert_eq!(state.turn_slot_usage()[1].output_tokens, 7);

        state.reset_turn();
        assert!(state.turn_slot_usage().is_empty());
    }

    #[tokio::test]
    async fn partial_failure_is_annotated_without_sinking_the_round() {
        let runner =
            MoaRunner::with_provider_factory(counting_factory(Arc::new(AtomicUsize::new(0))));
        let mut state = enabled_state("user_turn", vec![slot("alpha"), slot("fails")]);
        let messages = vec![user("hello")];

        let outcome = runner.run(&mut state, &messages).await.expect("outcome");
        assert_eq!(outcome.advices[0].text, "advice from alpha");
        assert!(outcome.advices[1].text.starts_with("[failed: "));
    }

    #[tokio::test]
    async fn all_failed_returns_none_and_caches_nothing() {
        let runner =
            MoaRunner::with_provider_factory(counting_factory(Arc::new(AtomicUsize::new(0))));
        let mut state = enabled_state("user_turn", vec![slot("fails"), slot("errors")]);
        let messages = vec![user("hello")];

        assert!(runner.run(&mut state, &messages).await.is_none());
        assert!(state.cached_advices.is_none());
        assert!(state.last_run_signature.is_none());
    }

    #[tokio::test]
    async fn all_slots_timing_out_returns_none_and_caches_nothing() {
        let runner = MoaRunner::with_provider_factory(stalling_factory());
        let mut state = enabled_state("user_turn", vec![slot("stalls"), slot("stalls")]);
        // Shortest allowed timeout keeps the real wait around one second.
        state.config.reference_timeout_secs = 1;
        let messages = vec![user("hello")];

        assert!(runner.run(&mut state, &messages).await.is_none());
        assert!(state.cached_advices.is_none());
        assert!(state.last_run_signature.is_none());
    }

    #[tokio::test]
    async fn timed_out_slot_is_annotated_while_healthy_slot_survives() {
        let runner = MoaRunner::with_provider_factory(stalling_factory());
        let mut state = enabled_state("user_turn", vec![slot("alpha"), slot("stalls")]);
        state.config.reference_timeout_secs = 1;
        let messages = vec![user("hello")];

        let outcome = runner.run(&mut state, &messages).await.expect("outcome");
        assert_eq!(outcome.advices[0].text, "advice from alpha");
        assert!(outcome.advices[1].text.starts_with("[failed:"));
        assert!(outcome.advices[1].text.contains("timeout"));
    }

    #[tokio::test]
    async fn user_turn_cadence_hits_cache_within_a_turn() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = enabled_state("user_turn", vec![slot("alpha")]);
        let mut messages = vec![user("do the thing")];

        let first = runner.run(&mut state, &messages).await.expect("first run");
        assert!(!first.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Same turn, later iteration: tool traffic lands AFTER the last real
        // user message, so the signature is unchanged → cache HIT, no re-run.
        messages.push(assistant("calling a tool"));
        messages.push(tool_result_frame("tool output"));
        let second = runner.run(&mut state, &messages).await.expect("cache hit");
        assert!(second.from_cache);
        assert_eq!(second.advices, first.advices);
        assert_eq!(second.usage, TokenUsage::default());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "references must not re-run");
    }

    #[tokio::test]
    async fn new_user_turn_misses_cache_and_reruns() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = enabled_state("user_turn", vec![slot("alpha")]);
        let mut messages = vec![user("first request")];

        runner.run(&mut state, &messages).await.expect("first run");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Next user turn: reset turn counters, append a new real user message.
        state.reset_turn();
        messages.push(assistant("done"));
        messages.push(user("second request"));
        let rerun = runner.run(&mut state, &messages).await.expect("rerun");
        assert!(!rerun.from_cache, "new signature must MISS the cache");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn every_n_cadence_reuses_cache_off_cycle() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = enabled_state("every_n:3", vec![slot("alpha")]);
        let messages = vec![user("go")];

        // Iterations 1..=4: scheduled on 1 and 4 ((iteration - 1) % 3 == 0).
        let r1 = runner.run(&mut state, &messages).await.unwrap();
        assert!(!r1.from_cache);
        let r2 = runner.run(&mut state, &messages).await.unwrap();
        assert!(r2.from_cache);
        let r3 = runner.run(&mut state, &messages).await.unwrap();
        assert!(r3.from_cache);
        let r4 = runner.run(&mut state, &messages).await.unwrap();
        assert!(!r4.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn per_iteration_cadence_always_reruns() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = enabled_state("per_iteration", vec![slot("alpha")]);
        let messages = vec![user("go")];

        assert!(!runner.run(&mut state, &messages).await.unwrap().from_cache);
        assert!(!runner.run(&mut state, &messages).await.unwrap().from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn inactive_state_returns_none_without_calls() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MoaRunner::with_provider_factory(counting_factory(Arc::clone(&calls)));
        let mut state = MoaState::new(MoaConfig::default(), vec![slot("alpha")]);
        assert!(runner.run(&mut state, &[user("hi")]).await.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn signature_ignores_frames_after_last_real_user_message() {
        let base = vec![user("q1"), assistant("a1"), user("q2")];
        let sig_base = turn_signature(&base, "labels");

        let mut extended = base.clone();
        extended.push(assistant("working"));
        extended.push(tool_result_frame("output"));
        assert_eq!(sig_base, turn_signature(&extended, "labels"));

        let mut new_turn = extended.clone();
        new_turn.push(user("q3"));
        assert_ne!(sig_base, turn_signature(&new_turn, "labels"));
        assert_ne!(sig_base, turn_signature(&base, "other-labels"));
    }
}
