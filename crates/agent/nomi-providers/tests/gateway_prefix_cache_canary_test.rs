//! Live gateway canary for prompt-prefix caching.
//!
//! Run manually against a real gateway:
//!
//! ```text
//! NOMI_CANARY_BASE_URL=https://gateway.example/v1 \
//! NOMI_CANARY_API_KEY=... \
//! NOMI_CANARY_MODEL=deepseek-chat \
//! cargo test -p nomi-providers --test gateway_prefix_cache_canary_test -- --ignored --nocapture
//! ```
//!
//! The same prefix is sent twice, then once more with an appended turn. A
//! gateway that caches the whole prefix reports `cache_read_tokens` close to
//! the first request's full prompt on both follow-ups. A gateway that can only
//! cache a fixed head (system + tools) fails here, which is the only
//! experiment that pins the responsibility on the gateway rather than the client.
//!
//! The tools variant answers a separate question: JSON key order in the request
//! body does not decide where the server renders tools, so only a run that
//! passes without tools and fails with tools proves the gateway renders them
//! after the messages.

use std::time::Duration;

use nomi_config::compat::ProviderCompat;
use nomi_providers::LlmProvider;
use nomi_providers::openai::OpenAIProvider;
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::message::{ContentBlock, Message, Role, TokenUsage};
use nomi_types::tool::ToolDef;
use serde_json::json;

const MIN_REUSE_PCT: u64 = 90;
const SETTLE: Duration = Duration::from_secs(3);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the gateway canary"))
}

fn text(role: Role, body: String) -> Message {
    Message::new(role, vec![ContentBlock::Text { text: body }])
}

fn stable_system_prompt() -> String {
    (0..400)
        .map(|index| format!("Rule {index}: keep answers short, factual and deterministic."))
        .collect::<Vec<_>>()
        .join("\n")
}

fn canary_tools() -> Vec<ToolDef> {
    (0..8)
        .map(|index| ToolDef {
            name: format!("canary_tool_{index}"),
            description: format!("Canary tool number {index}; never call it."),
            input_schema: json!({
                "type": "object",
                "properties": { "value": { "type": "string", "description": "unused" } },
                "required": ["value"]
            }),
            deferred: false,
        })
        .collect()
}

fn request(model: &str, messages: Vec<Message>, tools: Vec<ToolDef>) -> LlmRequest {
    LlmRequest {
        model: model.to_owned(),
        system: stable_system_prompt(),
        messages,
        tools,
        max_tokens: Some(16),
        thinking: None,
        reasoning_effort: None,
        temperature: Some(0.0),
        retain_provider_round: false,
        isolate_malformed_tool_calls: false,
    }
}

async fn usage_of(provider: &OpenAIProvider, request: &LlmRequest) -> TokenUsage {
    let mut rx = provider
        .stream(request)
        .await
        .expect("gateway rejected the canary request");
    let mut usage = None;
    while let Some(event) = rx.recv().await {
        if let LlmEvent::Done { usage: done, .. } = event {
            usage = Some(done);
        }
    }
    usage.expect("gateway returned no terminal usage")
}

fn assert_reuses_prefix(label: &str, first: &TokenUsage, next: &TokenUsage) {
    let reuse_pct = next.cache_read_tokens * 100 / first.input_tokens.max(1);
    assert!(
        reuse_pct >= MIN_REUSE_PCT,
        "{label}: gateway reused {} of the {} prompt tokens it had just seen ({reuse_pct}% < {MIN_REUSE_PCT}%). \
         The cache is not covering the whole prefix, so this is a gateway limit, not a client rewrite.",
        next.cache_read_tokens,
        first.input_tokens,
    );
}

async fn run_canary(label: &str, tools: Vec<ToolDef>) {
    let provider = OpenAIProvider::new(
        &env("NOMI_CANARY_API_KEY"),
        &env("NOMI_CANARY_BASE_URL"),
        ProviderCompat::openai_defaults(),
    );
    let model = env("NOMI_CANARY_MODEL");

    let base = vec![text(Role::User, "Reply with the single word: ok".to_owned())];
    let first = usage_of(&provider, &request(&model, base.clone(), tools.clone())).await;
    println!("{label} first request: {first:?}");
    assert!(
        first.input_tokens > 1_000,
        "{label}: canary prompt too small to observe prefix caching: {first:?}"
    );

    tokio::time::sleep(SETTLE).await;
    let identical = usage_of(&provider, &request(&model, base.clone(), tools.clone())).await;
    println!("{label} identical replay: {identical:?}");
    assert_reuses_prefix(&format!("{label} identical replay"), &first, &identical);

    let mut extended = base;
    extended.push(text(Role::Assistant, "ok".to_owned()));
    extended.push(text(Role::User, "Reply with the single word: again".to_owned()));
    tokio::time::sleep(SETTLE).await;
    let appended = usage_of(&provider, &request(&model, extended, tools)).await;
    println!("{label} appended turn: {appended:?}");
    assert_reuses_prefix(&format!("{label} appended turn"), &first, &appended);
}

#[tokio::test]
#[ignore = "needs a live gateway: set NOMI_CANARY_BASE_URL, NOMI_CANARY_API_KEY, NOMI_CANARY_MODEL"]
async fn gateway_caches_the_whole_prefix_without_tools() {
    run_canary("no-tools", vec![]).await;
}

#[tokio::test]
#[ignore = "needs a live gateway: set NOMI_CANARY_BASE_URL, NOMI_CANARY_API_KEY, NOMI_CANARY_MODEL"]
async fn gateway_caches_the_whole_prefix_with_tools() {
    run_canary("with-tools", canary_tools()).await;
}
