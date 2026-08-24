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
use nomi_agent_trace::ObservationScope;
use nomi_providers::LlmProvider;
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role};

use crate::observation::ObservationSession;

use crate::goal::state::{
    GoalContract, GoalVerdict, render_contract_block, render_subgoals_block,
};

/// Judge output budget. Reasoning models burn tokens on hidden reasoning
/// before the visible one-line JSON verdict; tight caps truncate the JSON
/// mid-string and trip the parse-failure breaker (hermes learned this the
/// hard way — see its `DEFAULT_JUDGE_MAX_TOKENS` comment).
const JUDGE_MAX_TOKENS: u32 = 4096;

/// Wall-clock cap for one judge call (stream open + collection).
const JUDGE_TIMEOUT_SECS: u64 = 30;

/// Cap how much of the goal / contract / last response we send to the judge.
const JUDGE_GOAL_SNIPPET_CHARS: usize = 2000;
const JUDGE_CONTRACT_SNIPPET_CHARS: usize = 2500;
const JUDGE_RESPONSE_SNIPPET_CHARS: usize = 4000;

/// Three-verdict judge contract (port of hermes `JUDGE_SYSTEM_PROMPT`).
/// Advertises all three wait directives — the runtime honors pid/session
/// barriers through the host-injected [`GoalWaitProbe`]
/// (`crate::goal::runtime::GoalWaitProbe`) and the time barrier natively.
const JUDGE_SYSTEM_PROMPT: &str = "\
You are a strict judge evaluating whether an autonomous agent has achieved a \
user's stated goal. You receive the goal text, the agent's most recent \
response, and — when present — a list of background processes the agent has \
running. Decide one of three verdicts.\n\n\
DONE — the goal is fully satisfied:\n\
- The response explicitly confirms the goal was completed AND shows how \
that was verified, OR\n\
- The response clearly shows the final deliverable was produced AND \
contains concrete evidence it is real and checked (a command result, file \
contents excerpt, test output — not just a fluent description of it), OR\n\
- The response explains the goal is unachievable / blocked / needs user \
input (treat this as DONE with reason describing the block).\n\
Do NOT pick DONE merely because the response reads like a complete, \
well-formed final answer. A first-pass answer that has not verified its \
own claims against the real workspace or system state is CONTINUE, not \
DONE. When the evidence is missing, weak, or indirect, prefer CONTINUE.\n\n\
WAIT — the goal is NOT done, but the next step is to wait for async work to \
finish rather than act again. Choose this ONLY when the agent's progress is \
genuinely gated on something running on its own:\n\
- A background process listed below is still running AND the response shows \
the agent is waiting on its result (e.g. a CI poller, build, test run, \
deploy). If the process has a session id, return it in ``wait_on_session`` \
— that releases when the process exits OR its watch_patterns trigger fires \
(use this for a long-lived watcher that signals mid-run and may never \
exit). Otherwise return its pid in ``wait_on_pid`` (releases on exit only).\n\
- The agent says it is rate-limited / backing off / must wait a fixed \
period — return seconds in ``wait_for_seconds``.\n\
Picking WAIT parks the loop without burning a turn; it resumes \
automatically when the pid exits or the time elapses. Do NOT pick WAIT just \
because work remains — only when re-poking now would be pure busy-work \
because the agent can't progress until the async thing finishes.\n\n\
CONTINUE — not done, and there is a concrete next step the agent can take \
right now. This is the default when in doubt.\n\n\
Reply ONLY with a single JSON object on one line. Shapes:\n\
{\"verdict\": \"done\", \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"continue\", \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"wait\", \"wait_on_session\": \"<id>\", \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"wait\", \"wait_on_pid\": <int>, \"reason\": \"<one sentence>\"}\n\
{\"verdict\": \"wait\", \"wait_for_seconds\": <int>, \"reason\": \"<one sentence>\"}\n\
The legacy shape {\"done\": <true|false>, \"reason\": \"...\"} is still \
accepted (true=done, false=continue).";

/// System prompt for contract drafting (port of hermes
/// `DRAFT_CONTRACT_SYSTEM_PROMPT`) — turns a plain-language objective into a
/// structured completion contract the user can review before activating.
const DRAFT_CONTRACT_SYSTEM_PROMPT: &str = "\
You turn a user's plain-language objective into a structured completion \
contract for an autonomous coding agent. The contract has five fields:\n\
- outcome: the single end state that must be true when done\n\
- verification: the specific test / command / artifact that PROVES the \
outcome (must be concrete and checkable)\n\
- constraints: what must NOT change or regress\n\
- boundaries: which files, dirs, tools, or systems are in scope\n\
- stop_when: the condition under which the agent should stop and ask for \
human input instead of pushing on\n\n\
Infer sensible, specific values from the objective and any project context \
implied by it. Prefer concrete verification (a named test command, a build, \
a benchmark) over vague phrases. Keep each field to one or two sentences. \
If a field genuinely cannot be inferred, use an empty string for it.\n\n\
Reply ONLY with a single JSON object on one line:\n\
{\"outcome\": \"...\", \"verification\": \"...\", \"constraints\": \"...\", \
\"boundaries\": \"...\", \"stop_when\": \"...\"}";

/// One live background process the host supplies for the judge prompt
/// (mirrors a hermes `process_registry.list_sessions()` entry). The host
/// gathers these — nomi-agent never scans processes itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BackgroundProcessInfo {
    pub pid: u32,
    /// Process-runtime session id, when the process runs under one. Lets the
    /// judge return `wait_on_session` (releases on the session's own trigger).
    pub session_id: Option<String>,
    pub command: String,
    pub uptime_seconds: Option<u64>,
    /// Tail of the process's recent output.
    pub output_preview: Option<String>,
    /// Watch patterns the session was started with (trigger fires mid-run).
    pub watch_patterns: Vec<String>,
    /// Whether a watch pattern already matched.
    pub watch_hit: bool,
    pub notify_on_complete: bool,
    /// Exited processes are nothing to wait on — skipped in the prompt.
    pub exited: bool,
}

/// Render the live background-process list for the judge prompt (port of
/// hermes `_render_background_block`). Returns an empty string when nothing
/// is running, so the prompt stays byte-identical to the no-background case.
fn render_background_block(processes: &[BackgroundProcessInfo]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for p in processes {
        if p.exited || p.pid == 0 {
            continue;
        }
        let cmd = truncate_chars(p.command.replace('\n', " ").trim(), 120);
        let mut line = format!("- pid {}", p.pid);
        if let Some(sid) = p.session_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            line.push_str(&format!(" / session {sid}"));
        }
        line.push_str(&format!(": {cmd}"));
        if let Some(uptime) = p.uptime_seconds {
            line.push_str(&format!(" (running {uptime}s)"));
        }
        // Surface the process's own trigger so the judge can wait on a
        // mid-run signal (watch-pattern) or completion, not just exit.
        if !p.watch_patterns.is_empty() {
            let hit = if p.watch_hit { " [already matched]" } else { "" };
            line.push_str(&format!(" | watch_patterns={:?}{hit}", p.watch_patterns));
        } else if p.notify_on_complete {
            line.push_str(" | notify_on_complete");
        }
        let tail = truncate_chars(
            p.output_preview.as_deref().unwrap_or("").replace('\n', " ").trim(),
            120,
        );
        if !tail.is_empty() {
            line.push_str(&format!(" | recent output: {tail}"));
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "Background processes the agent currently has running (it may be \
         waiting on one of these):\n{}\n\n",
        lines.join("\n")
    )
}

/// Port of the hermes judge user-prompt family. Template priority mirrors
/// hermes `judge_goal`: contract > subgoals > plain. With a contract present
/// the Verification criterion is the sole authoritative definition of done
/// and any subgoals fold into the contract block as extra criteria; with
/// subgoals only, the judge must find concrete evidence for EVERY numbered
/// criterion. Without either, the prompt is byte-identical to the phase-1
/// shape. The background block renders additively in all three shapes (empty
/// → byte-identical to the no-background case).
fn render_judge_user_prompt(
    goal: &str,
    subgoals: &[String],
    contract: Option<&GoalContract>,
    background: &[BackgroundProcessInfo],
    response: &str,
) -> String {
    let goal = truncate_chars(goal, JUDGE_GOAL_SNIPPET_CHARS);
    let response = truncate_chars(response, JUDGE_RESPONSE_SNIPPET_CHARS);
    let background_block = render_background_block(background);
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    if let Some(contract) = contract.filter(|c| !c.is_empty()) {
        let contract_block = truncate_chars(
            &render_contract_block(contract, subgoals),
            JUDGE_CONTRACT_SNIPPET_CHARS,
        );
        format!(
            "Goal:\n{goal}\n\n\
             Completion contract (the authoritative definition of done):\n\
             {contract_block}\n\n\
             Agent's most recent response:\n{response}\n\n\
             {background_block}\
             Current time: {now}\n\n\
             Decision rules:\n\
             - The goal is DONE only when the Verification criterion is \
             satisfied AND the response shows concrete evidence of it (a \
             command result, file contents excerpt, test/benchmark output) \
             — not a claim like 'done' or 'all tests pass' without \
             evidence.\n\
             - If any stated Constraint was violated, the goal is NOT done \
             — CONTINUE.\n\
             - If the response shows the agent is waiting on a listed \
             background process to satisfy the Verification criterion (e.g. \
             CI is the verification and it's still running), return WAIT on \
             that process instead of re-poking — re-poking now would be \
             pure busy-work.\n\
             - If the response explains the work is blocked / unachievable \
             / needs user input (e.g. the stated Stop condition was hit), \
             treat it as DONE with the reason describing the block.\n\
             - Otherwise the goal is NOT done — CONTINUE.\n\n\
             Is the goal satisfied per its completion contract — done, \
             continue, or wait?",
        )
    } else if subgoals.is_empty() {
        format!(
            "Goal:\n{goal}\n\nAgent's most recent response:\n{response}\n\n\
             {background_block}\
             Current time: {now}\n\n\
             Decision rules:\n\
             - DONE requires concrete evidence in the response that the \
             goal's end state is real and verified (a command result, file \
             contents excerpt, test or build output) — a fluent, \
             complete-looking answer alone is NOT enough.\n\
             - If the response is a first attempt that has not verified its \
             own claims, or any part of the goal's scope is unaddressed, \
             the goal is NOT done — CONTINUE.\n\n\
             Is the goal satisfied — done, continue, or wait?",
        )
    } else {
        format!(
            "Goal:\n{goal}\n\n\
             Additional criteria the user added mid-loop (all must also be \
             satisfied for the goal to be DONE):\n{subgoals_block}\n\n\
             Agent's most recent response:\n{response}\n\n\
             {background_block}\
             Current time: {now}\n\n\
             Decision: For each numbered criterion above, find concrete \
             evidence in the agent's response that the criterion is \
             satisfied. Do not accept generic phrases like 'all requirements \
             met' or 'implying it was done' — require specific evidence (a \
             file contents excerpt, an output line, a command result). If \
             ANY criterion lacks specific evidence in the response, the goal \
             is NOT done — return CONTINUE (or WAIT if genuinely gated on \
             async work).\n\n\
             Is the goal AND every additional criterion satisfied?",
            subgoals_block = render_subgoals_block(subgoals),
        )
    }
}

/// Char-boundary-safe truncation (the transcript may contain CJK text).
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let cut: String = s.chars().take(max_chars).collect();
    format!("{cut}…")
}

/// A concrete wait target extracted from a `wait` verdict (port of hermes
/// `wait_directive`). `Seconds` parks on the wall clock; `Pid` / `Session`
/// park on the host-injected `GoalWaitProbe` (without a probe they fail open
/// at the next evaluation point).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitDirective {
    /// Park for a fixed number of seconds (rate-limit backoff, cooldown).
    Seconds(u64),
    /// Park until the process exits.
    Pid(u32),
    /// Park until the session exits or its watch trigger fires.
    Session(String),
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
    /// Set only for `verdict == Wait`: the concrete barrier to park on. A
    /// wait verdict with no usable target is downgraded to `Continue` by the
    /// parser (can't park on nothing), so `Wait` always carries `Some`.
    pub wait_directive: Option<WaitDirective>,
}

impl JudgeOutcome {
    fn ok(verdict: GoalVerdict, reason: impl Into<String>) -> Self {
        Self {
            verdict,
            reason: reason.into(),
            parse_failed: false,
            transport_failed: false,
            wait_directive: None,
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
    observation: Option<Arc<ObservationSession>>,
}

impl ProviderJudgeClient {
    pub fn new(provider: Arc<dyn LlmProvider>, model: String) -> Self {
        Self {
            provider,
            model,
            observation: None,
        }
    }

    pub fn with_observation(mut self, session: Arc<ObservationSession>) -> Self {
        self.observation = Some(session);
        self
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
            max_tokens: Some(JUDGE_MAX_TOKENS),
            thinking: Some(ThinkingConfig::Disabled),
            reasoning_effort: None,
            temperature: Some(0.0),
                retain_provider_round: false,
        };

        let collected = tokio::time::timeout(Duration::from_secs(JUDGE_TIMEOUT_SECS), async {
            let mut rx = crate::observation::stream_llm(
                self.provider.as_ref(),
                &request,
                self.observation.clone(),
                "goal_judge",
                ObservationScope::SessionWorkflow,
            )
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

/// Ask the judge whether `objective` (and every subgoal / the contract's
/// Verification criterion, when present) is satisfied by `last_response`.
/// `background` is the host-gathered live process snapshot the judge may
/// return a wait directive against. Never errors — every failure mode maps
/// onto a conservative [`JudgeOutcome`] (fail-open, never a false `Done`).
pub async fn judge_goal(
    objective: &str,
    subgoals: &[String],
    contract: Option<&GoalContract>,
    background: &[BackgroundProcessInfo],
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

    let user_prompt = render_judge_user_prompt(objective, subgoals, contract, background, last_response);
    let raw = match client.complete(JUDGE_SYSTEM_PROMPT, &user_prompt).await {
        Ok(raw) => raw,
        Err(e) => {
            tracing::info!(target: "nomi_agent", error = %e, "goal judge: transport failure — falling through to continue");
            return JudgeOutcome {
                verdict: GoalVerdict::Continue,
                reason: format!("judge error: {e}"),
                parse_failed: false,
                transport_failed: true,
                wait_directive: None,
            };
        }
    };

    let (verdict, reason, parse_failed, wait_directive) = parse_judge_response(&raw);
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
        wait_directive,
    }
}

/// Best-effort JSON recovery from a model reply (port of hermes
/// `_extract_json_object` / the fence-stripping half of
/// `_parse_judge_response`): strips markdown code fences, then parses the
/// whole blob or pulls the first `{...}` out of surrounding prose. `None`
/// when no JSON object can be recovered. Shared by the judge parser and the
/// contract drafter.
fn extract_json_object(raw: &str) -> Option<serde_json::Value> {
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
    data.filter(|d| d.is_object())
}

/// Parse the judge's reply. Fail-open on unusable output (port of hermes
/// `_parse_judge_response`). Returns `(verdict, reason, parse_failed,
/// wait_directive)`.
///
/// Accepts both the new `{"verdict": ...}` shape and the legacy
/// `{"done": <bool>}` shape; strips markdown code fences; extracts the first
/// JSON object embedded in prose. A `wait` verdict must carry a concrete
/// target (session > pid > seconds, hermes priority); wait with no usable
/// target is downgraded to `continue` — can't park on nothing.
pub(crate) fn parse_judge_response(raw: &str) -> (GoalVerdict, String, bool, Option<WaitDirective>) {
    if raw.trim().is_empty() {
        return (
            GoalVerdict::Continue,
            "judge returned empty response".to_string(),
            true,
            None,
        );
    }

    let Some(data) = extract_json_object(raw) else {
        return (
            GoalVerdict::Continue,
            format!("judge reply was not JSON: {}", truncate_chars(raw, 200)),
            true,
            None,
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

    if verdict != GoalVerdict::Wait {
        return (verdict, reason, false, None);
    }

    // Wait verdict: extract a concrete directive. Hermes priority — prefer a
    // session-id (releases on the process's own trigger), then pid (exit
    // only), then seconds. Accept a few key spellings the model might emit.
    let session = ["wait_on_session", "session_id", "wait_session"]
        .iter()
        .find_map(|k| data.get(*k).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(s) = session {
        return (verdict, reason, false, Some(WaitDirective::Session(s.to_string())));
    }
    if let Some(pid) = first_positive_int(&data, &["wait_on_pid", "pid", "wait_pid"]) {
        return (verdict, reason, false, Some(WaitDirective::Pid(pid as u32)));
    }
    if let Some(secs) = first_positive_int(&data, &["wait_for_seconds", "seconds", "wait_seconds"])
    {
        return (verdict, reason, false, Some(WaitDirective::Seconds(secs)));
    }
    // Wait with no usable target — can't park on nothing; treat as continue.
    (
        GoalVerdict::Continue,
        format!("{reason} (wait verdict had no target — continuing)"),
        false,
        None,
    )
}

/// First positive integer found under any of `keys` (hermes `_first_int`).
/// Accepts JSON numbers and numeric strings; zero/negative/garbage is
/// skipped, not an error.
fn first_positive_int(data: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let parsed = match data.get(*key) {
            Some(serde_json::Value::Number(n)) => n.as_i64(),
            Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
            _ => None,
        };
        if let Some(n) = parsed
            && n > 0
        {
            return Some(n as u64);
        }
    }
    None
}

/// Why [`draft_contract`] could not produce a contract. Explicit errors (not
/// a panic, not a silent empty contract) so the backend can surface the right
/// message and fall back to a bare free-form goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractDraftError {
    /// The objective was blank — nothing to draft from.
    EmptyObjective,
    /// The drafting LLM was unreachable (auth, timeout, DNS, stream error).
    Transport(String),
    /// The model replied, but no JSON contract could be recovered. Carries a
    /// truncated excerpt of the raw reply for diagnostics.
    Unparsable(String),
    /// The reply parsed, but every contract field came back empty.
    EmptyContract,
}

impl std::fmt::Display for ContractDraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyObjective => write!(f, "cannot draft a contract from an empty objective"),
            Self::Transport(e) => write!(f, "contract draft request failed: {e}"),
            Self::Unparsable(raw) => write!(f, "contract draft reply was not JSON: {raw}"),
            Self::EmptyContract => write!(f, "contract draft produced no usable fields"),
        }
    }
}

impl std::error::Error for ContractDraftError {}

/// Cap how much of the user's objective we send to the drafting model
/// (hermes truncates at 4000).
const DRAFT_OBJECTIVE_SNIPPET_CHARS: usize = 4000;

/// Expand a plain-language objective into a structured [`GoalContract`]
/// (port of hermes `draft_contract`). One side LLM call on the same narrow
/// [`GoalJudgeClient`] interface the judge uses — never a conversation turn,
/// so the main prompt cache stays intact. Missing fields default to empty
/// strings (hermes `GoalContract.from_dict` semantics); an unusable reply is
/// an explicit [`ContractDraftError`], never a panic. The backend applies
/// the result via `AgentEngine::goal_set_contract` (and may show it to the
/// user for review first).
pub async fn draft_contract(
    objective: &str,
    client: &dyn GoalJudgeClient,
) -> Result<GoalContract, ContractDraftError> {
    let objective = objective.trim();
    if objective.is_empty() {
        return Err(ContractDraftError::EmptyObjective);
    }

    let user_prompt = format!(
        "Objective:\n{}",
        truncate_chars(objective, DRAFT_OBJECTIVE_SNIPPET_CHARS)
    );
    let raw = client
        .complete(DRAFT_CONTRACT_SYSTEM_PROMPT, &user_prompt)
        .await
        .map_err(ContractDraftError::Transport)?;

    let Some(data) = extract_json_object(&raw) else {
        return Err(ContractDraftError::Unparsable(truncate_chars(&raw, 200)));
    };
    let field = |key: &str| -> String {
        match data.get(key) {
            Some(serde_json::Value::String(s)) => s.trim().to_string(),
            // Tolerate a model emitting a non-string scalar; null/missing → empty.
            Some(serde_json::Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    };
    let contract = GoalContract {
        outcome: field("outcome"),
        verification: field("verification"),
        constraints: field("constraints"),
        boundaries: field("boundaries"),
        stop_when: field("stop_when"),
    };
    if contract.is_empty() {
        return Err(ContractDraftError::EmptyContract);
    }
    Ok(contract)
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
        let (v, r, failed, dir) =
            parse_judge_response(r#"{"verdict": "done", "reason": "all shipped"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "all shipped");
        assert!(!failed);
        assert!(dir.is_none());
    }

    #[test]
    fn parses_clean_json_continue() {
        let (v, _, failed, dir) =
            parse_judge_response(r#"{"verdict": "continue", "reason": "tests missing"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
        assert!(dir.is_none());
    }

    #[test]
    fn wait_without_target_downgrades_to_continue() {
        // Hermes: a wait verdict with neither session, pid nor seconds cannot
        // park on anything — it degrades to a normal continue, not a failure.
        let (v, r, failed, dir) =
            parse_judge_response(r#"{"verdict": "wait", "reason": "CI running"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(r.contains("CI running"));
        assert!(r.contains("wait verdict had no target"));
        assert!(!failed);
        assert!(dir.is_none());
    }

    #[test]
    fn parses_wait_for_seconds_directive() {
        let (v, r, failed, dir) = parse_judge_response(
            r#"{"verdict": "wait", "wait_for_seconds": 30, "reason": "rate limited"}"#,
        );
        assert_eq!(v, GoalVerdict::Wait);
        assert_eq!(r, "rate limited");
        assert!(!failed);
        assert_eq!(dir, Some(WaitDirective::Seconds(30)));
    }

    #[test]
    fn wait_seconds_accepts_string_number_and_alternate_keys() {
        for raw in [
            r#"{"verdict": "wait", "wait_for_seconds": "45", "reason": "x"}"#,
            r#"{"verdict": "wait", "seconds": 45, "reason": "x"}"#,
            r#"{"verdict": "wait", "wait_seconds": 45, "reason": "x"}"#,
        ] {
            let (v, _, _, dir) = parse_judge_response(raw);
            assert_eq!(v, GoalVerdict::Wait, "raw={raw}");
            assert_eq!(dir, Some(WaitDirective::Seconds(45)), "raw={raw}");
        }
    }

    #[test]
    fn wait_with_unusable_seconds_downgrades_to_continue() {
        for raw in [
            r#"{"verdict": "wait", "wait_for_seconds": "soon", "reason": "x"}"#,
            r#"{"verdict": "wait", "wait_for_seconds": 0, "reason": "x"}"#,
            r#"{"verdict": "wait", "wait_for_seconds": -5, "reason": "x"}"#,
        ] {
            let (v, r, failed, dir) = parse_judge_response(raw);
            assert_eq!(v, GoalVerdict::Continue, "raw={raw}");
            assert!(r.contains("wait verdict had no target"), "raw={raw}");
            assert!(!failed, "raw={raw}");
            assert!(dir.is_none(), "raw={raw}");
        }
    }

    #[test]
    fn parses_wait_on_pid_directive() {
        let (v, _, _, dir) = parse_judge_response(
            r#"{"verdict": "wait", "wait_on_pid": 4242, "reason": "build running"}"#,
        );
        assert_eq!(v, GoalVerdict::Wait);
        assert_eq!(dir, Some(WaitDirective::Pid(4242)));
    }

    #[test]
    fn parses_wait_on_session_directive() {
        let (v, _, _, dir) = parse_judge_response(
            r#"{"verdict": "wait", "wait_on_session": "sess-1", "reason": "watching CI"}"#,
        );
        assert_eq!(v, GoalVerdict::Wait);
        assert_eq!(dir, Some(WaitDirective::Session("sess-1".to_string())));
    }

    #[test]
    fn wait_directive_priority_is_session_then_pid_then_seconds() {
        let (_, _, _, dir) = parse_judge_response(
            r#"{"verdict": "wait", "wait_on_session": "s", "wait_on_pid": 1, "wait_for_seconds": 9, "reason": "x"}"#,
        );
        assert_eq!(dir, Some(WaitDirective::Session("s".to_string())));
        let (_, _, _, dir) = parse_judge_response(
            r#"{"verdict": "wait", "wait_on_pid": 1, "wait_for_seconds": 9, "reason": "x"}"#,
        );
        assert_eq!(dir, Some(WaitDirective::Pid(1)));
    }

    #[test]
    fn parses_verdict_case_insensitively() {
        let (v, _, failed, _) = parse_judge_response(r#"{"verdict": " DONE ", "reason": "ok"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert!(!failed);
    }

    #[test]
    fn parses_legacy_done_true() {
        let (v, r, failed, _) = parse_judge_response(r#"{"done": true, "reason": "finished"}"#);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "finished");
        assert!(!failed);
    }

    #[test]
    fn parses_legacy_done_false() {
        let (v, _, failed, _) = parse_judge_response(r#"{"done": false, "reason": "not yet"}"#);
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
            let (v, _, failed, _) =
                parse_judge_response(&format!(r#"{{"done": "{s}", "reason": "x"}}"#));
            assert_eq!(v, expected, "done={s}");
            assert!(!failed);
        }
    }

    #[test]
    fn strips_markdown_fence() {
        let raw = "```json\n{\"verdict\": \"done\", \"reason\": \"ok\"}\n```";
        let (v, r, failed, _) = parse_judge_response(raw);
        assert_eq!(v, GoalVerdict::Done);
        assert_eq!(r, "ok");
        assert!(!failed);
    }

    #[test]
    fn extracts_json_embedded_in_prose() {
        let raw = "Sure! Here is my verdict: {\"verdict\": \"continue\", \"reason\": \"more to do\"} hope that helps";
        let (v, _, failed, _) = parse_judge_response(raw);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn unknown_verdict_falls_back_to_continue() {
        let (v, _, failed, _) = parse_judge_response(r#"{"verdict": "maybe", "reason": "hmm"}"#);
        assert_eq!(v, GoalVerdict::Continue);
        assert!(!failed);
    }

    #[test]
    fn malformed_json_fails_open_to_continue() {
        let (v, _, failed, _) = parse_judge_response("the goal seems mostly finished");
        assert_eq!(v, GoalVerdict::Continue);
        assert!(failed);
    }

    #[test]
    fn empty_response_is_parse_failure() {
        let (v, _, failed, _) = parse_judge_response("   ");
        assert_eq!(v, GoalVerdict::Continue);
        assert!(failed);
    }

    #[test]
    fn missing_reason_gets_placeholder() {
        let (_, r, _, _) = parse_judge_response(r#"{"verdict": "done"}"#);
        assert_eq!(r, "no reason provided");
    }

    // ── render_judge_user_prompt ──────────────────────────────────

    fn sample_contract() -> GoalContract {
        GoalContract {
            outcome: "all goal tests pass".to_string(),
            verification: "cargo test -p nomi-agent --lib goal exits 0".to_string(),
            constraints: "no serialization contract changes".to_string(),
            boundaries: "crates/agent/nomi-agent only".to_string(),
            stop_when: "a pre-existing test fails".to_string(),
        }
    }

    #[test]
    fn judge_prompt_without_subgoals_has_no_criteria_block() {
        let p = render_judge_user_prompt("ship it", &[], None, &[], "progress");
        assert!(p.contains("Goal:\nship it"));
        assert!(!p.contains("Additional criteria"));
        assert!(!p.contains("Completion contract"));
        assert!(p.contains("Is the goal satisfied — done, continue, or wait?"));
    }

    #[test]
    fn judge_prompt_with_subgoals_lists_every_criterion() {
        let subgoals = vec!["tests added".to_string(), "docs updated".to_string()];
        let p = render_judge_user_prompt("ship it", &subgoals, None, &[], "progress");
        assert!(p.contains("Additional criteria the user added mid-loop"));
        assert!(p.contains("- 1. tests added"));
        assert!(p.contains("- 2. docs updated"));
        // The subgoals variant demands evidence for ALL criteria before done.
        assert!(p.contains("Is the goal AND every additional criterion satisfied?"));
    }

    #[test]
    fn judge_prompt_with_contract_makes_verification_authoritative() {
        let contract = sample_contract();
        let p = render_judge_user_prompt("ship it", &[], Some(&contract), &[], "progress");
        assert!(p.contains("Completion contract (the authoritative definition of done):"));
        assert!(p.contains("- Verification: cargo test -p nomi-agent --lib goal exits 0"));
        assert!(p.contains("- Stop when blocked: a pre-existing test fails"));
        // Decision rules: verification is authoritative, constraints enforced,
        // a hit stop condition maps to DONE-with-block-reason.
        assert!(p.contains("DONE only when the Verification criterion"));
        assert!(p.contains("If any stated Constraint was violated"));
        assert!(p.contains("the stated Stop condition was hit"));
        assert!(p.contains(
            "Is the goal satisfied per its completion contract — done, continue, or wait?"
        ));
        // Contract shape supersedes both other shapes.
        assert!(!p.contains("Additional criteria"));
    }

    #[test]
    fn judge_prompt_with_contract_and_subgoals_folds_criteria_into_contract_block() {
        // Hermes: with a contract present, subgoals fold into the contract
        // block as extra criteria instead of rendering their own section.
        let contract = sample_contract();
        let subgoals = vec!["tests added".to_string(), "docs updated".to_string()];
        let p = render_judge_user_prompt("ship it", &subgoals, Some(&contract), &[], "progress");
        assert!(p.contains("Completion contract"));
        assert!(p.contains("- Extra criterion 1: tests added"));
        assert!(p.contains("- Extra criterion 2: docs updated"));
        assert!(!p.contains("Additional criteria the user added mid-loop"));
        assert!(!p.contains("Is the goal AND every additional criterion satisfied?"));
    }

    #[test]
    fn empty_contract_falls_back_to_plain_prompt() {
        let p = render_judge_user_prompt(
            "ship it",
            &[],
            Some(&GoalContract::default()),
            &[],
            "progress",
        );
        assert!(!p.contains("Completion contract"));
        assert!(p.contains("Is the goal satisfied — done, continue, or wait?"));
    }

    // ── background-process block ────────────────────────────────────────

    #[test]
    fn background_block_renders_running_processes_only() {
        let procs = vec![
            BackgroundProcessInfo {
                pid: 4242,
                session_id: Some("sess-1".to_string()),
                command: "cargo watch -x test".to_string(),
                uptime_seconds: Some(90),
                output_preview: Some("Compiling nomi-agent".to_string()),
                watch_patterns: vec!["tests passed".to_string()],
                watch_hit: true,
                ..Default::default()
            },
            BackgroundProcessInfo {
                pid: 7,
                command: "already gone".to_string(),
                exited: true,
                ..Default::default()
            },
        ];
        let block = render_background_block(&procs);
        assert!(block.contains("Background processes the agent currently has running"));
        assert!(block.contains("- pid 4242 / session sess-1: cargo watch -x test (running 90s)"));
        assert!(block.contains("watch_patterns=[\"tests passed\"] [already matched]"));
        assert!(block.contains("recent output: Compiling nomi-agent"));
        assert!(!block.contains("already gone"));
    }

    #[test]
    fn background_block_is_empty_for_no_running_processes() {
        assert_eq!(render_background_block(&[]), "");
        // Exited-only lists degrade to the empty string too, keeping the
        // judge prompt byte-identical to the no-background case.
        let exited = vec![BackgroundProcessInfo {
            pid: 1,
            command: "x".to_string(),
            exited: true,
            ..Default::default()
        }];
        assert_eq!(render_background_block(&exited), "");
    }

    #[test]
    fn judge_prompt_renders_background_block_between_response_and_time() {
        let procs = vec![BackgroundProcessInfo {
            pid: 99,
            command: "npm run build".to_string(),
            ..Default::default()
        }];
        let p = render_judge_user_prompt("ship it", &[], None, &procs, "progress");
        let response_at = p.find("Agent's most recent response:").unwrap();
        let block_at = p.find("Background processes").unwrap();
        let time_at = p.find("Current time:").unwrap();
        assert!(response_at < block_at && block_at < time_at);
        // Without background the prompt stays byte-identical in shape.
        let bare = render_judge_user_prompt("ship it", &[], None, &[], "progress");
        assert!(!bare.contains("Background processes"));
    }

    // ── draft_contract ──────────────────────────────────────────────────

    #[tokio::test]
    async fn draft_contract_parses_full_reply() {
        let client = MockJudgeClient::new(vec![Ok(r#"{"outcome": "o", "verification": "v", "constraints": "c", "boundaries": "b", "stop_when": "s"}"#.into())]);
        let c = draft_contract("ship the feature", &client).await.unwrap();
        assert_eq!(c.outcome, "o");
        assert_eq!(c.verification, "v");
        assert_eq!(c.constraints, "c");
        assert_eq!(c.boundaries, "b");
        assert_eq!(c.stop_when, "s");
    }

    #[tokio::test]
    async fn draft_contract_defaults_missing_fields_to_empty() {
        // Hermes from_dict semantics: absent fields become empty strings.
        let client = MockJudgeClient::new(vec![Ok(
            r#"```json
{"outcome": "o", "verification": "v"}
```"#
                .into(),
        )]);
        let c = draft_contract("ship it", &client).await.unwrap();
        assert_eq!(c.outcome, "o");
        assert_eq!(c.verification, "v");
        assert_eq!(c.constraints, "");
        assert_eq!(c.boundaries, "");
        assert_eq!(c.stop_when, "");
    }

    #[tokio::test]
    async fn draft_contract_rejects_unusable_replies() {
        let client = MockJudgeClient::new(vec![Ok("no json here".to_string())]);
        let err = draft_contract("ship it", &client).await.unwrap_err();
        assert!(matches!(err, ContractDraftError::Unparsable(_)));

        let client = MockJudgeClient::new(vec![Err("401".to_string())]);
        let err = draft_contract("ship it", &client).await.unwrap_err();
        assert!(matches!(err, ContractDraftError::Transport(_)));

        // Parsed but all-empty contract is an explicit error, not Ok(empty).
        let client = MockJudgeClient::new(vec![Ok(r#"{"outcome": ""}"#.to_string())]);
        let err = draft_contract("ship it", &client).await.unwrap_err();
        assert_eq!(err, ContractDraftError::EmptyContract);
    }

    #[tokio::test]
    async fn draft_contract_rejects_empty_objective_without_calling_client() {
        let client = MockJudgeClient::new(vec![]);
        let err = draft_contract("   ", &client).await.unwrap_err();
        assert_eq!(err, ContractDraftError::EmptyObjective);
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // ── judge_goal ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_goal_is_skipped_without_calling_client() {
        let client = MockJudgeClient::new(vec![]);
        let out = judge_goal("  ", &[], None, &[], "some response", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Skipped);
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn empty_response_continues_without_calling_client() {
        let client = MockJudgeClient::new(vec![]);
        let out = judge_goal("ship it", &[], None, &[], "", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(!out.parse_failed);
        assert!(!out.transport_failed);
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn transport_error_flags_transport_failed() {
        let client = MockJudgeClient::new(vec![Err("401 unauthorized".to_string())]);
        let out = judge_goal("ship it", &[], None, &[], "did stuff", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(out.transport_failed);
        assert!(!out.parse_failed);
    }

    #[tokio::test]
    async fn parse_error_flags_parse_failed() {
        let client = MockJudgeClient::new(vec![Ok("not json at all".to_string())]);
        let out = judge_goal("ship it", &[], None, &[], "did stuff", &client).await;
        assert_eq!(out.verdict, GoalVerdict::Continue);
        assert!(out.parse_failed);
        assert!(!out.transport_failed);
    }

    #[tokio::test]
    async fn done_reply_yields_done() {
        let client =
            MockJudgeClient::new(vec![Ok(r#"{"verdict": "done", "reason": "verified"}"#.into())]);
        let out = judge_goal("ship it", &[], None, &[], "shipped and tested", &client).await;
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

    struct ScriptedJudgeProvider;

    #[async_trait]
    impl LlmProvider for ScriptedJudgeProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, nomi_providers::ProviderError> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(LlmEvent::TextDelta(
                r#"{"verdict":"continue","reason":"more work"}"#.into(),
            ))
            .await
            .ok();
            tx.send(LlmEvent::Done {
                stop_reason: nomi_types::message::StopReason::EndTurn,
                usage: Default::default(),
            })
            .await
            .ok();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn provider_judge_client_writes_observation_events() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = nomi_agent_trace::ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(nomi_agent_trace::ObservationIds {
            conversation_id: Some("c-judge".into()),
            root_turn_id: Some("t-judge".into()),
            ..Default::default()
        });
        let client = ProviderJudgeClient::new(Arc::new(ScriptedJudgeProvider), "judge-model".into())
            .with_observation(session);
        let text = client.complete("sys", "user").await.unwrap();
        assert!(text.contains("continue"));
        let events = recorder.read_events(Some("c-judge")).unwrap();
        assert!(events.iter().any(|event| {
            event.event_type == nomi_agent_trace::EVENT_LLM_REQUEST
                && event.payload["call_kind"] == "goal_judge"
        }));
        assert!(
            events
                .iter()
                .any(|event| event.event_type == nomi_agent_trace::EVENT_LLM_RESPONSE)
        );
    }
}
