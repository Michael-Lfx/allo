//! Goal judge: asks an LLM whether the standing goal is satisfied.
//!
//! Port of hermes `goals.py` `judge_goal()` / `_parse_judge_response()`.
//! The judge is a one-shot side request — it never touches the engine's
//! system prompt or conversation history, so the main prompt cache stays
//! intact. Deliberately fail-open: anything unusable degrades to `Continue`
//! (never a false `Done`), with parse/transport failures tracked separately
//! so the runtime can trip its circuit breakers.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nomi_providers::LlmProvider;
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role};

use crate::goal::state::GoalVerdict;

/// Judge output budget. Reasoning models burn tokens on hidden reasoning
/// before the visible one-line JSON verdict; tight caps truncate the JSON
/// mid-string and trip the parse-failure breaker (hermes learned this the
/// hard way — see its `DEFAULT_JUDGE_MAX_TOKENS` comment).
const JUDGE_MAX_TOKENS: u32 = 4096;

/// Wall-clock cap for one judge call (stream open + collection).
const JUDGE_TIMEOUT_SECS: u64 = 30;

/// Cap how much of the goal / last response we send to the judge.
const JUDGE_GOAL_SNIPPET_CHARS: usize = 2000;
const JUDGE_RESPONSE_SNIPPET_CHARS: usize = 4000;

/// Three-verdict judge contract (port of hermes `JUDGE_SYSTEM_PROMPT`).
/// Phase 1 parses all three verdicts but the runtime downgrades WAIT to
/// CONTINUE — the wait barrier (pid/session/time parking) lands in phase 2.
const JUDGE_SYSTEM_PROMPT: &str = "\
You are a strict judge evaluating whether an autonomous agent has achieved a \
user's stated goal. You receive the goal text and the agent's most recent \
response. Decide one of three verdicts.\n\n\
DONE — the goal is fully satisfied:\n\
- The response explicitly confirms the goal was completed, OR\n\
- The response clearly shows the final deliverable was produced, OR\n\
- The response explains the goal is unachievable / blocked / needs user \
input (treat this as DONE with reason describing the block).\n\n\
WAIT — the goal is NOT done, but the next step is to wait for async work to \
finish rather than act again. Choose this ONLY when the agent's progress is \
genuinely gated on something running on its own (a build, CI, a long test \
run) and re-poking now would be pure busy-work.\n\n\
CONTINUE — not done, and there is a concrete next step the agent can take \
right now. This is the default when in doubt.\n\n\
Reply ONLY with a single JSON object on one line. Shapes:\n\
{\"verdict\": \"done\", \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"continue\", \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"wait\", \"reason\": \"<one sentence>\"}\n\
The legacy shape {\"done\": <true|false>, \"reason\": \"...\"} is still \
accepted (true=done, false=continue).";

/// Port of hermes `JUDGE_USER_PROMPT_TEMPLATE` (no background block /
/// subgoals / contract in phase 1 — those render additively in phase 2/3).
fn render_judge_user_prompt(goal: &str, response: &str) -> String {
    format!(
        "Goal:\n{goal}\n\nAgent's most recent response:\n{response}\n\n\
         Current time: {now}\n\n\
         Is the goal satisfied — done, continue, or wait?",
        goal = truncate_chars(goal, JUDGE_GOAL_SNIPPET_CHARS),
        response = truncate_chars(response, JUDGE_RESPONSE_SNIPPET_CHARS),
        now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z"),
    )
}

/// Char-boundary-safe truncation (the transcript may contain CJK text).
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let cut: String = s.chars().take(max_chars).collect();
    format!("{cut}…")
}

/// One judge evaluation, with the failure kind split out so the runtime can
/// feed the right circuit breaker (see hermes `judge_goal` return contract).
#[derive(Debug, Clone)]
pub struct JudgeOutcome {
    pub verdict: GoalVerdict,
    pub reason: String,
    /// The judge replied but its output was unusable (empty / non-JSON).
    pub parse_failed: bool,
    /// The judge API was unreachable (auth, timeout, DNS, stream error).
    pub transport_failed: bool,
}

impl JudgeOutcome {
    fn ok(verdict: GoalVerdict, reason: impl Into<String>) -> Self {
        Self {
            verdict,
            reason: reason.into(),
            parse_failed: false,
            transport_failed: false,
        }
    }
}

/// Narrow judge-side LLM interface: one blocking text completion. The
/// production impl rides the engine's main provider; tests use a mock.
#[async_trait]
pub trait GoalJudgeClient: Send + Sync {
    /// Returns the raw assistant text, or `Err(description)` on any
    /// transport-level failure (connect, auth, timeout, stream error).
    async fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Production judge client: a one-shot, tool-less, non-thinking request on
/// the engine's main provider/model. Mirrors the side-request pattern of
/// `compact::auto::summarize_with_retry` (independent `LlmRequest`, stream
/// collected to text, hard timeout).
pub struct ProviderJudgeClient {
    provider: Arc<dyn LlmProvider>,
    model: String,
}

impl ProviderJudgeClient {
    pub fn new(provider: Arc<dyn LlmProvider>, model: String) -> Self {
        Self { provider, model }
    }
}

#[async_trait]
impl GoalJudgeClient for ProviderJudgeClient {
    async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let request = LlmRequest {
            model: self.model.clone(),
            system: system.to_string(),
            messages: vec![Message::now(
                Role::User,
                vec![ContentBlock::Text {
                    text: user.to_string(),
                }],
            )],
            tools: vec![],
            max_tokens: JUDGE_MAX_TOKENS,
            thinking: Some(ThinkingConfig::Disabled),
            reasoning_effort: None,
            temperature: Some(0.0),
        };

        let collected = tokio::time::timeout(Duration::from_secs(JUDGE_TIMEOUT_SECS), async {
            let mut rx = self
                .provider
                .stream(&request)
                .await
                .map_err(|e| format!("judge request failed: {e}"))?;
            let mut text = String::new();
            while let Some(event) = rx.recv().await {
                match event {
                    LlmEvent::TextDelta(delta) => text.push_str(&delta),
                    LlmEvent::Done { .. } => return Ok(text),
                    LlmEvent::Error(e) => return Err(format!("judge stream error: {e}")),
                    // Thinking deltas / tool calls are irrelevant here.
                    _ => {}
                }
            }
            // Channel closed without Done — treat as transport failure.
            Err("judge stream closed without completion".to_string())
        })
        .await;

        match collected {
            Ok(result) => result,
            Err(_) => Err(format!("judge timed out after {JUDGE_TIMEOUT_SECS}s")),
        }
    }
}

/// Ask the judge whether `objective` is satisfied by `last_response`.
/// Never errors — every failure mode maps onto a conservative
/// [`JudgeOutcome`] (fail-open, never a false `Done`).
pub async fn judge_goal(
    objective: &str,
    last_response: &str,
    client: &dyn GoalJudgeClient,
) -> JudgeOutcome {
    if objective.trim().is_empty() {
        return JudgeOutcome::ok(GoalVerdict::Skipped, "empty goal");
    }
    if last_response.trim().is_empty() {
        // No substantive reply this turn — almost certainly not done yet.
        return JudgeOutcome::ok(GoalVerdict::Continue, "empty response (nothing to evaluate)");
    }

    let user_prompt = render_judge_user_prompt(objective, last_response);
    let raw = match client.complete(JUDGE_SYSTEM_PROMPT, &user_prompt).await {
        Ok(raw) => raw,
        Err(e) => {
            tracing::info!(target: "nomi_agent", error = %e, "goal judge: transport failure — falling through to continue");
            return JudgeOutcome {
                verdict: GoalVerdict::Continue,
                reason: format!("judge error: {e}"),
                parse_failed: false,
                transport_failed: true,
            };
        }
    };

    let (verdict, reason, parse_failed) = parse_judge_response(&raw);
    tracing::info!(
        target: "nomi_agent",
        ?verdict,
        reason = %truncate_chars(&reason, 120),
        parse_failed,
        "goal judge verdict"
    );
    JudgeOutcome {
        verdict,
        reason,
        parse_failed,
        transport_failed: false,
    }
}

/// Parse the judge's reply. Fail-open on unusable output (port of hermes
/// `_parse_judge_response`). Returns `(verdict, reason, parse_failed)`.
///
/// Accepts both the new `{"verdict": ...}` shape and the legacy
/// `{"done": <bool>}` shape; strips markdown code fences; extracts the first
/// JSON object embedded in prose. A `wait` verdict is returned as-is —
/// phase 1 callers treat it as continue (wait barrier is phase 2).
pub(crate) fn parse_judge_response(raw: &str) -> (GoalVerdict, String, bool) {
    if raw.trim().is_empty() {
        return (
            GoalVerdict::Continue,
            "judge returned empty response".to_string(),
            true,
        );
    }

    let mut text = raw.trim().to_string();

    // Strip markdown code fences the model may wrap JSON in.
    if text.starts_with("```") {
        text = text.trim_matches('`').to_string();
        // Peel off a leading json/JSON/etc language tag line.
        if let Some(nl) = text.find('\n') {
            text = text[nl + 1..].to_string();
        }
    }

    // First try: parse the whole blob. Second try: pull the first {...} out.
    let data: Option<serde_json::Value> = serde_json::from_str(&text).ok().or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        if end <= start {
            return None;
        }
        serde_json::from_str(&text[start..=end]).ok()
    });

    let Some(data) = data.filter(|d| d.is_object()) else {
        return (
            GoalVerdict::Continue,
            format!("judge reply was not JSON: {}", truncate_chars(raw, 200)),
            true,
        );
    };

    let reason = data
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("no reason provided")
        .to_string();

    // Determine verdict — prefer the explicit "verdict" field, fall back to
    // the legacy "done" boolean (also accepting a few string spellings).
    let verdict = match data.get("verdict").and_then(|v| v.as_str()) {
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "done" => GoalVerdict::Done,
            "wait" => GoalVerdict::Wait,
            // Unknown verdict strings fall back to the conservative default.
            _ => GoalVerdict::Continue,
        },
        None => {
            let done = match data.get("done") {
                Some(serde_json::Value::Bool(b)) => *b,
                Some(serde_json::Value::String(s)) => {
                    matches!(
                        s.trim().to_ascii_lowercase().as_str(),
                        "true" | "yes" | "1" | "done"
                    )
                }
                Some(v) => v.as_i64().is_some_and(|n| n != 0),
                None => false,
            };
            if done {
                GoalVerdict::Done
            } else {
                GoalVerdict::Continue
            }
        }
    };

    // TODO(phase 2): extract wait directives (wait_on_pid / wait_on_session /
    // wait_for_seconds) so the runtime can park the loop on a real barrier.
    (verdict, reason, false)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Scripted mock: returns queued replies in order (transport errors as Err).
    pub(crate) struct MockJudgeClient {
        replies: std::sync::Mutex<std::collections::VecDeque<Result<String, String>>>,
        pub calls: std::sync::atomic::AtomicUsize,
    }

    impl MockJudgeClient {
        pub(crate) fn new(replies: Vec<Result<String, String>>) -> Self {
            Self {
                replies: std::sync::Mutex::new(replies.into()),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl GoalJudgeClient for MockJudgeClient {
        async fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("mock exhausted".to_string()))
        }
    }

    // ── parse_judge_response ────────────────────────────────────────────

    #[test]
    fn parses_clean_json_done() {
        let (v, r, failed) = parse_judge_response(r#"{"verdict": "done", "reason": "all shipped"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "all shipped");
        assert!(!failed);
    }

    #[test]
    fn parses_clean_json_continue() {
        let (v, _, failed) =
            parse_judge_response(r#"{"verdict": "continue", "reason": "tests missing"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn parses_wait_verdict() {
        let (v, r, failed) = parse_judge_response(r#"{"verdict": "wait", "reason": "CI running"}"#);
        assert_eq!(v, GoalVerdict::Wait);
        assert_eq!(r, "CI running");
        assert!(!failed);
    }

    #[test]
    fn parses_verdict_case_insensitively() {
        let (v, _, failed) = parse_judge_response(r#"{"verdict": " DONE ", "reason": "ok"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert!(!failed);
    }

    #[test]
    fn parses_legacy_done_true() {
        let (v, r, failed) = parse_judge_response(r#"{"done": true, "reason": "finished"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "finished");
        assert!(!failed);
    }

    #[test]
    fn parses_legacy_done_false() {
        let (v, _, failed) = parse_judge_response(r#"{"done": false, "reason": "not yet"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn parses_legacy_done_string_values() {
        for (s, expected) in [
            ("true", GoalVerdict::Done),
            ("yes", GoalVerdict::Done),
            ("1", GoalVerdict::Done),
            ("done", GoalVerdict::Done),
            ("false", GoalVerdict::Continue),
            ("no", GoalVerdict::Continue),
        ] {
            let (v, _, failed) =
                parse_judge_response(&format!(r#"{{"done": "{s}", "reason": "x"}}"#));
            assert_eq!(v, expected, "done={s}");
            assert!(!failed);
        }
    }

    #[test]
    fn strips_markdown_fence() {
        let raw = "```json\n{\"verdict\": \"done\", \"reason\": \"ok\"}\n```";
        let (v, r, failed) = parse_judge_response(raw);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "ok");
        assert!(!failed);
    }

    #[test]
    fn extracts_json_embedded_in_prose() {
        let raw = "Sure! Here is my verdict: {\"verdict\": \"continue\", \"reason\": \"more to do\"} hope that helps";
        let (v, _, failed) = parse_judge_response(raw);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn unknown_verdict_falls_back_to_continue() {
        let (v, _, failed) = parse_judge_response(r#"{"verdict": "maybe", "reason": "hmm"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn malformed_json_fails_open_to_continue() {
        let (v, _, failed) = parse_judge_response("the goal seems mostly finished");
        assert_eq!(v, GoalVerdict::Continue);
        assert!(failed);
    }

    #[test]
    fn empty_response_is_parse_failure() {
        let (v, _, failed) = parse_judge_response("   ");
        assert_eq!(v, GoalVerdict::Continue);
        assert!(failed);
    }

    #[test]
    fn missing_reason_gets_placeholder() {
        let (_, r, _) = parse_judge_response(r#"{"verdict": "done"}"#);
        assert_eq!(r, "no reason provided");
    }

    // ── judge_goal ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_goal_is_skipped_without_calling_client() {
        let client = MockJudgeClient::new(vec![]);
        let out = judge_goal("  ", "some response", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Skipped);
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn empty_response_continues_without_calling_client() {
        let client = MockJudgeClient::new(vec![]);
        let out = judge_goal("ship it", "", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(!out.parse_failed);
        assert!(!out.transport_failed);
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn transport_error_flags_transport_failed() {
        let client = MockJudgeClient::new(vec![Err("401 unauthorized".to_string())]);
        let out = judge_goal("ship it", "did stuff", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(out.transport_failed);
        assert!(!out.parse_failed);
    }

    #[tokio::test]
    async fn parse_error_flags_parse_failed() {
        let client = MockJudgeClient::new(vec![Ok("not json at all".to_string())]);
        let out = judge_goal("ship it", "did stuff", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(out.parse_failed);
        assert!(!out.transport_failed);
    }

    #[tokio::test]
    async fn done_reply_yields_done() {
        let client =
            MockJudgeClient::new(vec![Ok(r#"{"verdict": "done", "reason": "verified"}"#.into())]);
        let out = judge_goal("ship it", "shipped and tested", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Done);
        assert_eq!(out.reason, "verified");
        assert!(!out.parse_failed);
        assert!(!out.transport_failed);
    }

    #[test]
    fn truncation_is_char_boundary_safe() {
        let s = "目标".repeat(3000);
        let t = truncate_chars(&s, 100);
        assert!(t.chars().count() <= 101);
    }
}
