//! Prefix-cache curve: request N must replay request N-1's entire payload.
//!
//! DeepSeek automatic prefix cache is byte-identical. The client-side contract
//! is the same as Reasonix `TestCacheHitPrefixStable`: cached prefix chars on
//! request i equal the full size of request i-1. New content only on the tail.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nomi_agent::engine::AgentEngine;
use nomi_agent::output::OutputSink;
use nomi_agent::output::terminal::TerminalSink;
use nomi_providers::{LlmProvider, ProviderError};
use nomi_tools::registry::ToolRegistry;
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::message::{Message, StopReason, TokenUsage};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use common::{MockTool, test_config};

const DIALOGUE_TURNS: usize = 10;
const TOOL_ROUNDS: usize = 8;
const TAIL_WINDOW: usize = 4;
const MIN_TAIL_HIT_PCT: u32 = 80;

const LONG_REASONING: &str = "Let me reason about this carefully. I will weigh the constraints, \
     enumerate the candidate approaches, reject the ones that violate a requirement, \
     and then commit to the most defensible option.";

fn silent_output() -> Arc<dyn OutputSink> {
    Arc::new(TerminalSink::new(true))
}

fn curve_config() -> nomi_config::config::Config {
    let mut config = test_config();
    config.compact.enabled = false;
    config.max_turns = Some(DIALOGUE_TURNS.max(TOOL_ROUNDS) + 2);
    config
}

/// Simulated gateway: every request reports the bytes it replayed from the
/// previous request as `cache_read_tokens`, optionally capped to model a gateway
/// that can only cache a fixed head.
struct CurveProvider {
    requests: Mutex<Vec<LlmRequest>>,
    remaining_tool_rounds: AtomicUsize,
    cache_cap_bytes: Option<usize>,
}

impl CurveProvider {
    fn dialogue() -> Arc<Self> {
        Self::new(0, None)
    }

    fn tool_loop(rounds: usize) -> Arc<Self> {
        Self::new(rounds, None)
    }

    fn head_capped_dialogue(cap_bytes: usize) -> Arc<Self> {
        Self::new(0, Some(cap_bytes))
    }

    fn new(tool_rounds: usize, cache_cap_bytes: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            remaining_tool_rounds: AtomicUsize::new(tool_rounds),
            cache_cap_bytes,
        })
    }

    fn recorded(&self) -> Vec<LlmRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for CurveProvider {
    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        let (idx, usage) = {
            let mut requests = self.requests.lock().unwrap();
            let replayed = requests.last().map_or(0, payload_bytes);
            requests.push(request.clone());
            let cache_read = self.cache_cap_bytes.map_or(replayed, |cap| replayed.min(cap));
            let usage = TokenUsage {
                input_tokens: payload_bytes(request) as u64,
                cache_read_tokens: cache_read as u64,
                ..TokenUsage::default()
            };
            (requests.len(), usage)
        };
        let emit_tool = self
            .remaining_tool_rounds
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();

        let events = if emit_tool {
            vec![
                LlmEvent::ThinkingDelta(LONG_REASONING.to_string()),
                LlmEvent::ToolUse {
                    id: format!("echo-{idx}"),
                    name: "echo".to_string(),
                    input: json!({ "text": format!("round-{idx}") }),
                    extra: None,
                },
                LlmEvent::Done {
                    stop_reason: StopReason::ToolUse,
                    usage,
                },
            ]
        } else {
            vec![
                LlmEvent::ThinkingDelta(LONG_REASONING.to_string()),
                LlmEvent::TextDelta("Done.".to_string()),
                LlmEvent::Done {
                    stop_reason: StopReason::EndTurn,
                    usage,
                },
            ]
        };

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            for event in events {
                let _ = tx.send(event).await;
            }
        });
        Ok(rx)
    }
}

fn user_payload(turn: usize) -> String {
    format!(
        "Turn {turn}: {}",
        "please consider this requirement. ".repeat(6)
    )
}

fn message_content_json(message: &Message) -> Value {
    json!({
        "role": message.role,
        "content": message.content,
    })
}

fn payload_bytes(request: &LlmRequest) -> usize {
    serde_json::to_vec(&json!({
        "system": request.system,
        "tools": request.tools.iter().map(|tool| json!({
            "name": tool.name,
            "description": tool.description,
            "input_schema": tool.input_schema,
            "deferred": tool.deferred,
        })).collect::<Vec<_>>(),
        "messages": request.messages.iter().map(message_content_json).collect::<Vec<_>>(),
    }))
    .expect("prefix payload must serialize")
    .len()
}

fn assert_request_replays_prior(prev: &LlmRequest, next: &LlmRequest, index: usize) {
    assert_eq!(
        next.system, prev.system,
        "PREFIX BROKEN at req {index}: system prompt changed"
    );
    assert_eq!(
        next.tools, prev.tools,
        "PREFIX BROKEN at req {index}: advertised tools changed"
    );
    assert!(
        next.messages.len() >= prev.messages.len(),
        "PREFIX BROKEN at req {index}: next request dropped history ({} < {})",
        next.messages.len(),
        prev.messages.len()
    );
    for (msg_index, (older, newer)) in prev.messages.iter().zip(next.messages.iter()).enumerate() {
        assert_eq!(
            message_content_json(older),
            message_content_json(newer),
            "PREFIX BROKEN at req {index}: message {msg_index} was rewritten"
        );
    }
}

fn hit_curve(requests: &[LlmRequest]) -> Vec<u32> {
    assert!(
        requests.len() >= 2,
        "curve needs at least two provider requests, got {}",
        requests.len()
    );
    let mut rates = Vec::with_capacity(requests.len());
    rates.push(0);
    for i in 1..requests.len() {
        assert_request_replays_prior(&requests[i - 1], &requests[i], i);
        let prev = payload_bytes(&requests[i - 1]);
        let next = payload_bytes(&requests[i]);
        assert!(
            next >= prev,
            "PREFIX BROKEN at req {i}: payload shrank ({next} < {prev})"
        );
        rates.push(((prev as u64) * 100 / next.max(1) as u64) as u32);
    }
    rates
}

fn assert_tail_hit(rates: &[u32], label: &str) {
    let window = TAIL_WINDOW.min(rates.len().saturating_sub(1));
    assert!(
        window > 0,
        "{label}: not enough requests for a tail window: {rates:?}"
    );
    let tail = &rates[rates.len() - window..];
    let avg = tail.iter().sum::<u32>() / window as u32;
    assert!(
        avg >= MIN_TAIL_HIT_PCT,
        "{label}: tail-{window} mean cache hit {avg}% < {MIN_TAIL_HIT_PCT}% (curve {rates:?})"
    );
}

fn head_bytes(request: &LlmRequest) -> usize {
    let mut head = request.clone();
    head.messages.clear();
    payload_bytes(&head)
}

fn assert_engine_reports_gateway_usage(engine: &AgentEngine, requests: &[LlmRequest], label: &str) {
    let reuse = engine
        .cache_reuse()
        .unwrap_or_else(|| panic!("{label}: engine recorded no provider usage"));
    let last = requests.len() - 1;
    assert_eq!(
        reuse.last_round_input_tokens,
        payload_bytes(&requests[last]) as u64,
        "{label}: engine must report the gateway's prompt size for the last round"
    );
    assert_eq!(
        reuse.last_round_cache_read_tokens,
        payload_bytes(&requests[last - 1]) as u64,
        "{label}: last-round cache read must equal the previous request in full"
    );
    let warm_pct = reuse.warm_cache_read_tokens * 100 / reuse.warm_input_tokens.max(1);
    assert!(
        warm_pct >= u64::from(MIN_TAIL_HIT_PCT),
        "{label}: warm cache reuse {warm_pct}% < {MIN_TAIL_HIT_PCT}%"
    );
}

async fn run_dialogue(provider: &Arc<CurveProvider>) -> AgentEngine {
    let mut engine = AgentEngine::new_with_provider(
        provider.clone(),
        curve_config(),
        ToolRegistry::new(),
        silent_output(),
        std::env::temp_dir(),
    );
    for turn in 0..DIALOGUE_TURNS {
        engine
            .execute_turn(&user_payload(turn), &format!("msg-{turn}"))
            .await
            .unwrap_or_else(|err| panic!("dialogue turn {turn} failed: {err:?}"));
    }
    engine
}

#[tokio::test]
async fn head_only_gateway_caps_reported_reuse_at_the_head_size() {
    let probe = CurveProvider::dialogue();
    run_dialogue(&probe).await;
    let head = head_bytes(&probe.recorded()[0]);
    assert!(head > 0, "the invariant head (system + tools) must not be empty");

    let provider = CurveProvider::head_capped_dialogue(head);
    let engine = run_dialogue(&provider).await;

    let requests = provider.recorded();
    let reuse = engine.cache_reuse().expect("engine recorded usage");
    assert_eq!(
        reuse.last_round_cache_read_tokens, head as u64,
        "a head-only gateway must cache exactly the head and nothing of the conversation"
    );
    assert!(
        reuse.last_round_input_tokens > reuse.last_round_cache_read_tokens,
        "the conversation body must show up as uncached prompt"
    );
    assert!(
        requests.len() >= 2 && reuse.warm_cache_read_tokens <= head as u64 * (requests.len() as u64 - 1),
        "reported reuse can never exceed the head size per warm round"
    );
}

#[tokio::test]
async fn plain_dialogue_prefix_equals_prior_request() {
    let provider = CurveProvider::dialogue();
    let engine = run_dialogue(&provider).await;

    let requests = provider.recorded();
    assert_eq!(requests.len(), DIALOGUE_TURNS);
    let rates = hit_curve(&requests);
    assert_tail_hit(&rates, "plain-dialogue");
    assert_engine_reports_gateway_usage(&engine, &requests, "plain-dialogue");
}

#[tokio::test]
async fn tool_loop_prefix_equals_prior_request() {
    let provider = CurveProvider::tool_loop(TOOL_ROUNDS);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(MockTool::new("echo", "echoed", false)));
    let mut engine = AgentEngine::new_with_provider(
        provider.clone(),
        curve_config(),
        registry,
        silent_output(),
        std::env::temp_dir(),
    );

    engine
        .execute_turn(&user_payload(0), "msg-tool-loop")
        .await
        .expect("tool-loop turn should finish");

    let requests = provider.recorded();
    assert_eq!(
        requests.len(),
        TOOL_ROUNDS + 1,
        "one request per tool round plus the closing reply"
    );
    assert!(
        requests
            .iter()
            .all(|request| request.tools.iter().any(|tool| tool.name == "echo")),
        "echo must stay advertised on every request"
    );
    let rates = hit_curve(&requests);
    assert_tail_hit(&rates, "tool-loop");
    assert_engine_reports_gateway_usage(&engine, &requests, "tool-loop");
}
