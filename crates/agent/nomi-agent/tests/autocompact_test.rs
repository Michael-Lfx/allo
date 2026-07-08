//! Black-box integration tests for the autocompact subsystem.
//!
//! These tests correspond to TC-2.4-* in the test plan.
//! They exercise the public autocompact API with a mock LLM provider,
//! validating trigger logic, summary formatting, boundary markers,
//! circuit breaker, and PTL retry behaviour.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc;

use nomi_agent::compact::auto::{
    CompactError, autocompact, extract_compact_metadata, is_compact_boundary, should_autocompact,
};
use nomi_agent::compact::prompt::{
    build_compact_prompt, build_summary_content, format_compact_summary,
};
use nomi_agent::compact::state::CompactState;
use nomi_config::compact::CompactConfig;
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::compact::CompactTrigger;
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::message::{ContentBlock, Message, Role, StopReason, TokenUsage};

// ── Mock provider ───────────────────────────────────────────────────────────

/// A mock LLM provider that returns pre-configured responses in order.
struct MockProvider {
    responses: Mutex<VecDeque<Result<Vec<LlmEvent>, ProviderError>>>,
}

impl MockProvider {
    fn new(responses: Vec<Result<Vec<LlmEvent>, ProviderError>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }

    /// Create a provider that returns a successful summary response.
    fn with_summary(summary: &str) -> Self {
        Self::new(vec![Ok(vec![
            LlmEvent::TextDelta(summary.to_string()),
            LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage {
                    input_tokens: 50_000,
                    output_tokens: 2_000,
                    ..Default::default()
                },
            },
        ])])
    }

    /// Create a provider that returns an error.
    fn with_error(error: ProviderError) -> Self {
        Self::new(vec![Err(error)])
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn stream(
        &self,
        _request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockProvider: no more responses queued");

        match response {
            Ok(events) => {
                let (tx, rx) = mpsc::channel(events.len() + 1);
                for event in events {
                    tx.send(event).await.ok();
                }
                Ok(rx)
            }
            Err(e) => Err(e),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn text_msg(role: Role, content: &str) -> Message {
    Message::new(
        role,
        vec![ContentBlock::Text {
            text: content.to_string(),
        }],
    )
}

fn sample_conversation(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            let role = if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            };
            text_msg(role, &format!("message-{i}"))
        })
        .collect()
}

fn default_config() -> CompactConfig {
    CompactConfig::default()
}

// ── TC-2.4-01: Watermark above threshold triggers ───────────────────────────

#[test]
fn tc_2_4_01_above_threshold_triggers() {
    // effective_window = 200k - 20k = 180k, threshold = 180k - 13k = 167k
    assert!(should_autocompact(170_000, &default_config()));
}

// ── TC-2.4-02: Below threshold does not trigger ─────────────────────────────

#[test]
fn tc_2_4_02_below_threshold_does_not_trigger() {
    assert!(!should_autocompact(160_000, &default_config()));
}

// ── TC-2.4-03: Exact threshold triggers ─────────────────────────────────────

#[test]
fn tc_2_4_03_at_exact_threshold_triggers() {
    assert!(should_autocompact(167_000, &default_config()));
}

// ── TC-2.4-04: Circuit breaker initial state ────────────────────────────────

#[test]
fn tc_2_4_04_initial_state_not_broken() {
    let state = CompactState::new();
    assert_eq!(state.consecutive_failures, 0);
    assert!(!state.is_circuit_broken(&default_config()));
}

// ── TC-2.4-05: Circuit breaker trips ────────────────────────────────────────

#[test]
fn tc_2_4_05_circuit_breaker_trips() {
    let config = default_config();
    let mut state = CompactState::new();
    state.record_failure();
    state.record_failure();
    state.record_failure();
    assert!(state.is_circuit_broken(&config));
}

// ── TC-2.4-06: Circuit breaker resets ───────────────────────────────────────

#[test]
fn tc_2_4_06_circuit_breaker_resets_on_success() {
    let config = default_config();
    let mut state = CompactState::new();
    state.record_failure();
    state.record_failure();
    state.record_success();
    assert_eq!(state.consecutive_failures, 0);
    assert!(!state.is_circuit_broken(&config));
}

// ── TC-2.4-07: Circuit breaker blocks autocompact ───────────────────────────

#[tokio::test]
async fn tc_2_4_07_circuit_breaker_blocks_autocompact() {
    let provider = MockProvider::with_summary("<summary>should not be called</summary>");
    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();
    state.record_failure();
    state.record_failure();
    state.record_failure();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state).await;
    assert!(matches!(result, Err(CompactError::CircuitBroken { .. })));
}

// ── TC-2.4-08: Prompt contains all 7 sections ──────────────────────────────

#[test]
fn tc_2_4_08_prompt_contains_all_sections() {
    let prompt = build_compact_prompt();
    for i in 1..=7 {
        assert!(prompt.contains(&format!("{i}.")), "Missing section {i}");
    }
    assert!(prompt.contains("CRITICAL: Respond with TEXT ONLY"));
}

// ── TC-2.4-09: Summary formatting (normal) ──────────────────────────────────

#[test]
fn tc_2_4_09_format_strips_analysis_extracts_summary() {
    let raw = "<analysis>thinking</analysis>\n<summary>result</summary>";
    assert_eq!(format_compact_summary(raw), "Summary:\nresult");
}

// ── TC-2.4-10: Summary formatting (no analysis) ────────────────────────────

#[test]
fn tc_2_4_10_format_without_analysis() {
    let raw = "<summary>result</summary>";
    assert_eq!(format_compact_summary(raw), "Summary:\nresult");
}

// ── TC-2.4-11: Summary formatting (no tags) ────────────────────────────────

#[test]
fn tc_2_4_11_format_graceful_degradation() {
    let raw = "plain text without tags";
    assert_eq!(format_compact_summary(raw), "plain text without tags");
}

// ── TC-2.4-12: Post-compact message structure ───────────────────────────────

#[tokio::test]
async fn tc_2_4_12_post_compact_message_structure() {
    let summary = "<analysis>thinking</analysis>\n<summary>Detailed summary here</summary>";
    let provider = MockProvider::with_summary(summary);
    let messages = sample_conversation(20);
    let config = default_config();
    let mut state = CompactState::new();
    state.last_input_tokens = 170_000;

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed");

    // Should have 2 messages: boundary + summary
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages_summarized, 20);

    // First message is the boundary marker
    assert!(is_compact_boundary(&result.messages[0]));
    assert_eq!(result.messages[0].role, Role::User);

    // Second message is the summary
    assert_eq!(result.messages[1].role, Role::User);
    match &result.messages[1].content[0] {
        ContentBlock::Text { text } => {
            assert!(text.contains("Detailed summary here"));
            assert!(text.contains("This session is being continued"));
        }
        _ => panic!("expected Text block"),
    }
}

// ── TC-2.4-13: Boundary marker metadata ─────────────────────────────────────

#[tokio::test]
async fn tc_2_4_13_boundary_metadata() {
    let provider = MockProvider::with_summary("<summary>summary</summary>");
    let messages = sample_conversation(15);
    let config = default_config();
    let mut state = CompactState::new();
    state.last_input_tokens = 170_000;

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed");

    let metadata = extract_compact_metadata(&result.messages[0]).expect("should have metadata");
    assert_eq!(metadata.trigger, CompactTrigger::Auto);
    assert_eq!(metadata.pre_compact_tokens, 170_000);
    assert_eq!(metadata.messages_summarized, 15);
}

// ── TC-2.4-14: Disabled config skips (tested via should_autocompact) ────────

#[test]
fn tc_2_4_14_disabled_config_skips() {
    let config = CompactConfig {
        enabled: false,
        ..default_config()
    };
    assert!(!should_autocompact(999_999, &config));
}

// ── TC-2.4-15: Prompt forbids tool calls ────────────────────────────────────

#[test]
fn tc_2_4_15_prompt_forbids_tool_calls() {
    let prompt = build_compact_prompt();
    assert!(prompt.contains("Do NOT call any tools"));
}

// ── TC-2.4-16: Success resets failure counter ───────────────────────────────

#[tokio::test]
async fn tc_2_4_16_success_resets_failure_counter() {
    let provider = MockProvider::with_summary("<summary>summary</summary>");
    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();
    state.consecutive_failures = 2;
    state.last_input_tokens = 170_000;

    autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed");

    assert_eq!(state.consecutive_failures, 0);
}

// ── TC-2.4-17: Provider error triggers mechanical fold ─────────────────────

#[tokio::test]
async fn tc_2_4_17_provider_error_mechanical_fold() {
    // Two error responses: the transient retry retries once before
    // falling to mechanical fold.
    let provider = MockProvider::new(vec![
        Err(ProviderError::Api {
            status: 500,
            message: "Internal error".to_string(),
        }),
        Err(ProviderError::Api {
            status: 500,
            message: "Internal error".to_string(),
        }),
    ]);
    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed with mechanical fold");

    // Mechanical fold: context is freed even when summarizer is unreachable
    assert!(result.mechanical_fold);
    assert_eq!(result.messages_summarized, 10);
    // record_success is called — this is a graceful degradation, not a failure
    assert_eq!(state.consecutive_failures, 0);
}

// ── TC-2.4-18: PTL retry succeeds on second attempt ────────────────────────

#[tokio::test]
async fn tc_2_4_18_ptl_retry_succeeds() {
    let provider = MockProvider::new(vec![
        // First attempt: prompt too long
        Err(ProviderError::PromptTooLong(
            "prompt exceeds limit".to_string(),
        )),
        // Second attempt (after truncation): success
        Ok(vec![
            LlmEvent::TextDelta("<summary>retried summary</summary>".to_string()),
            LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            },
        ]),
    ]);

    let messages = sample_conversation(20);
    let config = default_config();
    let mut state = CompactState::new();
    state.last_input_tokens = 170_000;

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed after retry");

    assert_eq!(result.messages.len(), 2);
    assert_eq!(state.consecutive_failures, 0);

    // Verify summary content
    match &result.messages[1].content[0] {
        ContentBlock::Text { text } => {
            assert!(text.contains("retried summary"));
        }
        _ => panic!("expected Text block"),
    }
}

// ── TC-2.4-19: PTL retry exhausted → mechanical fold ────────────────────────

#[tokio::test]
async fn tc_2_4_19_ptl_retry_exhausted_mechanical_fold() {
    let provider = MockProvider::new(vec![
        Err(ProviderError::PromptTooLong("too long 1".to_string())),
        Err(ProviderError::PromptTooLong("too long 2".to_string())),
        Err(ProviderError::PromptTooLong("too long 3".to_string())),
    ]);

    let messages = sample_conversation(20);
    let config = default_config();
    let mut state = CompactState::new();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed with mechanical fold after PTL exhausted");

    assert!(result.mechanical_fold);
    assert_eq!(state.consecutive_failures, 0);
}

// ── TC-2.4-20: PTL retry truncates messages ─────────────────────────────────

#[tokio::test]
async fn tc_2_4_20_ptl_retry_truncates_messages() {
    // Track the request message count on each attempt
    let request_counts: std::sync::Arc<Mutex<Vec<usize>>> =
        std::sync::Arc::new(Mutex::new(Vec::new()));
    let counts_clone = request_counts.clone();

    // Custom mock that records message counts
    struct CountingProvider {
        counts: std::sync::Arc<Mutex<Vec<usize>>>,
        attempt: Mutex<u32>,
    }

    #[async_trait]
    impl LlmProvider for CountingProvider {
        async fn stream(
            &self,
            request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            // Scope the lock so the MutexGuard is dropped before the await
            let current_attempt = {
                let mut attempt = self.attempt.lock().unwrap();
                self.counts.lock().unwrap().push(request.messages.len());
                let val = *attempt;
                *attempt += 1;
                val
            };

            if current_attempt == 0 {
                return Err(ProviderError::PromptTooLong("too long".to_string()));
            }

            // Second attempt: succeed
            let (tx, rx) = mpsc::channel(2);
            tx.send(LlmEvent::TextDelta(
                "<summary>truncated summary</summary>".to_string(),
            ))
            .await
            .ok();
            tx.send(LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            })
            .await
            .ok();
            Ok(rx)
        }
    }

    let provider = CountingProvider {
        counts: counts_clone,
        attempt: Mutex::new(0),
    };

    let messages = sample_conversation(20);
    let config = default_config();
    let mut state = CompactState::new();
    state.last_input_tokens = 170_000;

    autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("should succeed after retry");

    let counts = request_counts.lock().unwrap();
    assert_eq!(counts.len(), 2, "should have 2 attempts");

    // First attempt: 20 conversation + 1 prompt = 21
    assert_eq!(counts[0], 21);

    // Second attempt: truncated (~20% dropped from 20 = 4 dropped) + 1 prompt
    // 20 - 4 = 16, + 1 prompt = 17
    assert_eq!(counts[1], 17);
}

// ── Additional edge cases ───────────────────────────────────────────────────

#[tokio::test]
async fn empty_response_mechanical_fold() {
    // Two empty responses: the transient retry retries once before
    // falling to mechanical fold.
    let provider = MockProvider::new(vec![
        Ok(vec![LlmEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }]),
        Ok(vec![LlmEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }]),
    ]);

    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed with mechanical fold on empty response");

    assert!(result.mechanical_fold);
    assert_eq!(state.consecutive_failures, 0);
}

#[tokio::test]
async fn stream_error_mechanical_fold() {
    // Two stream-error responses: the transient retry retries once
    // before falling to mechanical fold.
    let provider = MockProvider::new(vec![
        Ok(vec![
            LlmEvent::TextDelta("partial".to_string()),
            LlmEvent::Error("connection reset".to_string()),
        ]),
        Ok(vec![
            LlmEvent::TextDelta("partial".to_string()),
            LlmEvent::Error("connection reset".to_string()),
        ]),
    ]);

    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed with mechanical fold on stream error");

    assert!(result.mechanical_fold);
    assert_eq!(state.consecutive_failures, 0);
}

#[test]
fn summary_content_auto_has_continuation() {
    let content = build_summary_content("Summary:\ntest", true);
    assert!(content.contains("Continue the conversation"));
    assert!(content.contains("as if the break never happened"));
}

#[test]
fn summary_content_manual_no_continuation() {
    let content = build_summary_content("Summary:\ntest", false);
    assert!(!content.contains("Continue the conversation"));
}

// ── P5: Transient error retry ──────────────────────────────────────────────

#[tokio::test]
async fn transient_error_retry_succeeds() {
    // First attempt fails with a transient error, second succeeds.
    // Mirrors Reasonix's `summarizeWithRetry` which retries one non-timeout
    // failure.
    let provider = MockProvider::new(vec![
        Err(ProviderError::Api {
            status: 429,
            message: "rate limited".to_string(),
        }),
        Ok(vec![
            LlmEvent::TextDelta("<summary>retried successfully</summary>".to_string()),
            LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            },
        ]),
    ]);

    let messages = sample_conversation(10);
    let config = default_config();
    let mut state = CompactState::new();

    let result = autocompact(&provider, &messages, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed after transient retry");

    assert!(!result.mechanical_fold);
    assert_eq!(state.consecutive_failures, 0);

    // Verify summary content from the second attempt
    match &result.messages[1].content[0] {
        ContentBlock::Text { text } => {
            assert!(text.contains("retried successfully"));
        }
        _ => panic!("expected Text block"),
    }
}

// ── P5: Force compaction ratio bypasses fold economics ─────────────────────

#[tokio::test]
async fn force_ratio_bypasses_fold_economics() {
    // When the prompt is at 90% of the window, compaction is forced even
    // if the foldable region is small (fold_economics would skip it).
    // Mirrors Reasonix's `compactForceRatio = 0.9`.
    let provider = MockProvider::with_summary("<summary>forced</summary>");

    // Build a conversation with >= 20 messages (tail-preserving mode).
    // 1 pinned user + 16 short messages (foldable, < 400 tokens) +
    // 4 large messages (tail, fill the budget).
    let mut msgs = vec![text_msg(Role::User, "Start")];
    for i in 0..8 {
        msgs.push(text_msg(Role::Assistant, &format!("a{i}")));
        msgs.push(text_msg(Role::User, &format!("u{i}")));
    }
    for _ in 0..2 {
        msgs.push(text_msg(Role::Assistant, &"x".repeat(4000)));
        msgs.push(text_msg(Role::User, &"y".repeat(4000)));
    }
    // Total: 1 + 16 + 4 = 21 messages

    // Small window so the tail budget is tiny and the 4 large messages
    // fill it, leaving the 16 short messages as a small foldable region.
    let config = CompactConfig {
        context_window: 1_000,
        ..default_config()
    };
    let mut state = CompactState::new();
    // 90% of 1000 = 900 → force = true
    state.last_input_tokens = 900;

    let result = autocompact(&provider, &msgs, "test-model", &config, &mut state)
        .await
        .expect("autocompact should proceed with force ratio");

    // Compaction happened despite small foldable region
    assert!(result.messages_summarized > 0);
    assert!(!result.mechanical_fold);
}

#[tokio::test]
async fn no_force_ratio_skips_small_fold() {
    // When the prompt is below 90% of the window, fold_economics still
    // skips compaction for a small foldable region.
    let provider = MockProvider::with_summary("<summary>should not happen</summary>");

    let mut msgs = vec![text_msg(Role::User, "Start")];
    for i in 0..8 {
        msgs.push(text_msg(Role::Assistant, &format!("a{i}")));
        msgs.push(text_msg(Role::User, &format!("u{i}")));
    }
    for _ in 0..2 {
        msgs.push(text_msg(Role::Assistant, &"x".repeat(4000)));
        msgs.push(text_msg(Role::User, &"y".repeat(4000)));
    }

    let config = CompactConfig {
        context_window: 1_000,
        ..default_config()
    };
    let mut state = CompactState::new();
    // 80% of 1000 = 800 → force = false (below 90%)
    state.last_input_tokens = 800;

    let result = autocompact(&provider, &msgs, "test-model", &config, &mut state)
        .await
        .expect("autocompact should succeed (no-op)");

    // Fold economics skipped compaction — no messages summarized
    assert_eq!(result.messages_summarized, 0);
}
