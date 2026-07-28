use std::sync::{Arc, Mutex};

use nomi_types::message::{ContentBlock, Message, Role};

use crate::goal::judge::{self, GoalJudgeClient, WaitDirective};
use crate::goal::state::{
    GoalState, GoalStatus, GoalVerdict, MAX_CONSECUTIVE_PARSE_FAILURES,
    MAX_CONSECUTIVE_TRANSPORT_FAILURES, epoch_ms, render_subgoals_block,
};

const CONTINUATION_TEMPLATE: &str = include_str!("templates/continuation.md");
/// Variant rendered when the user added `/subgoal` criteria. A separate file
/// (not conditional blocks in one template) so both shapes stay byte-stable:
/// within one goal session with unchanged subgoals the rendered prompt is
/// byte-identical across turns — prompt-cache friendly.
const CONTINUATION_SUBGOALS_TEMPLATE: &str = include_str!("templates/continuation_subgoals.md");

/// What a caller supplies to start a goal-driven session.
#[derive(Debug, Clone)]
pub struct GoalSpec {
    pub objective: String,
    pub max_auto_continuations: usize,
}

impl GoalSpec {
    pub fn new(objective: impl Into<String>, max_auto_continuations: usize) -> Self {
        Self {
            objective: objective.into(),
            max_auto_continuations,
        }
    }
}

/// Engine-side goal runtime: holds the shared state (also held by
/// `UpdateGoalTool`), runs the judge at each natural-termination point, and
/// renders the continuation prompt.
///
/// `Clone` duplicates the *handle*, not the state: clones share the same
/// `Arc<Mutex<GoalState>>`, so a host-side clone (taken via
/// `AgentEngine::goal_runtime_handle`) can pause/resume/clear a goal while
/// the engine itself is busy inside `execute_turn`.
#[derive(Clone)]
pub struct GoalRuntime {
    state: Arc<Mutex<GoalState>>,
}

impl GoalRuntime {
    pub fn new(objective: String, max_auto_continuations: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(GoalState::new(objective, max_auto_continuations))),
        }
    }

    /// Rebuild a runtime from a complete state snapshot (restore semantics:
    /// every field — turns_used/status/counters/created_at — is taken as-is).
    /// Counterpart of `new()`, which starts a fresh goal.
    pub fn from_state(state: GoalState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Replace the entire state in place. Mutates *inside* the shared Arc so
    /// an already-registered `UpdateGoalTool` keeps observing the same slot.
    pub fn restore(&self, state: GoalState) {
        *self.state.lock().unwrap() = state;
    }

    /// Clone the shared handle for injection into `UpdateGoalTool`.
    pub fn shared_state(&self) -> Arc<Mutex<GoalState>> {
        Arc::clone(&self.state)
    }

    /// Serializable snapshot of the current goal state, for the host to
    /// carry out alongside `AgentResult` / output events.
    pub fn snapshot(&self) -> GoalState {
        self.state.lock().unwrap().clone()
    }

    /// Called at the engine's natural-termination point. Runs the judge on
    /// the assistant's last response and returns `Some(message)` to inject a
    /// continuation (verdict = continue), or `None` to stop:
    ///
    /// - non-Active status (incl. a terminal state the model already declared
    ///   via `update_goal` — first terminal state wins, no judge call) → None
    /// - parked on a live time barrier (`Waiting`, `waiting_until` in the
    ///   future) → None without a judge call or budget burn; once the
    ///   deadline has passed the barrier lazily clears and this same call
    ///   resumes normal judging (hermes lazy auto-clear)
    /// - auto-continuation budget exhausted → None
    /// - judge says done → status becomes `Complete`, None
    /// - judge says wait with a time directive → status becomes `Waiting`,
    ///   `waiting_until`/`waiting_reason` set, None — no budget consumed
    /// - circuit breaker tripped (parse/transport failures) → `Paused`, None
    ///
    /// The judge call is a one-shot side request that never touches the main
    /// conversation history or system prompt.
    pub async fn evaluate_and_continue(
        &self,
        last_response: &str,
        judge_client: &dyn GoalJudgeClient,
    ) -> Option<Message> {
        // Snapshot what the judge needs, then release the lock — it must not
        // be held across the await below.
        let (objective, subgoals) = {
            let mut g = self.state.lock().unwrap();
            if g.status == GoalStatus::Waiting {
                if g.waiting_until.is_some_and(|until| epoch_ms() < until) {
                    // Parked on a live time barrier: quiesce — no judge call,
                    // no budget burn. The engine stops naturally this turn.
                    return None;
                }
                // Deadline passed — or a barrier kind this phase cannot check
                // (pid/session restored via set_goal_state; those land in
                // phase 3). Fail open: lazily clear the barrier, go back to
                // Active and fall through to normal judging right here.
                // TODO(phase 3): honor waiting_on_pid / waiting_on_session
                // via the process supervisor instead of failing open.
                g.status = GoalStatus::Active;
                g.clear_wait_barrier();
            }
            if !g.should_continue() {
                return None;
            }
            (g.objective.clone(), g.subgoals.clone())
        };

        let outcome = judge::judge_goal(&objective, &subgoals, last_response, judge_client).await;

        let mut g = self.state.lock().unwrap();
        // Re-check: a terminal state declared via `update_goal` mid-flight
        // wins over the judge verdict.
        if !g.should_continue() {
            return None;
        }

        g.last_verdict = Some(outcome.verdict);
        g.last_reason = Some(outcome.reason.clone());
        g.last_turn_at = Some(epoch_ms());

        // Each breaker resets independently on its own success dimension
        // (hermes semantics): a flaky network must not trip the parse breaker
        // meant for weak judge models, and vice versa.
        if outcome.parse_failed {
            g.consecutive_parse_failures += 1;
        } else {
            g.consecutive_parse_failures = 0;
        }
        if outcome.transport_failed {
            g.consecutive_transport_failures += 1;
        } else {
            g.consecutive_transport_failures = 0;
        }

        if outcome.verdict == GoalVerdict::Done {
            g.status = GoalStatus::Complete;
            return None;
        }

        // Circuit breakers: a permanently broken judge must not burn the
        // remaining continuation budget.
        if g.consecutive_transport_failures >= MAX_CONSECUTIVE_TRANSPORT_FAILURES {
            g.status = GoalStatus::Paused;
            g.paused_reason = Some(format!(
                "judge API unreachable {} turns in a row",
                g.consecutive_transport_failures
            ));
            return None;
        }
        if g.consecutive_parse_failures >= MAX_CONSECUTIVE_PARSE_FAILURES {
            g.status = GoalStatus::Paused;
            g.paused_reason = Some(format!(
                "judge model returned unparseable output {} turns in a row",
                g.consecutive_parse_failures
            ));
            return None;
        }

        // TODO(phase 3): Pid/Session directives need the process supervisor;
        // until then a wait verdict without a time target downgrades to a
        // normal continuation. Skipped (judge couldn't run) also fails open
        // to Continue.
        if outcome.verdict == GoalVerdict::Wait
            && let Some(WaitDirective::Seconds(secs)) = outcome.wait_directive
        {
            // Park on the time barrier. Deliberately does NOT consume
            // turns_used / auto_continuations — waiting is not progress, and
            // the budget should measure real agent work (task contract;
            // hermes parks without burning a continuation either).
            g.status = GoalStatus::Waiting;
            g.waiting_until = Some(epoch_ms() + secs.saturating_mul(1000));
            g.waiting_reason = Some(outcome.reason.clone());
            return None;
        }

        g.turns_used += 1;
        g.auto_continuations += 1;
        let prompt = render_continuation(&g.objective, g.blocked_threshold, &g.subgoals);
        Some(Message::now(
            Role::User,
            vec![ContentBlock::Text { text: prompt }],
        ))
    }

    /// Pause the goal (user action or host-side breaker). No-op on a goal
    /// that is already terminal (Complete/Blocked/Cleared). Also drops any
    /// wait barrier — a barrier is meaningless once paused (hermes).
    /// Subgoals are preserved.
    pub fn pause(&self, reason: &str) {
        let mut g = self.state.lock().unwrap();
        if matches!(g.status, GoalStatus::Active | GoalStatus::Waiting) {
            g.status = GoalStatus::Paused;
            g.paused_reason = Some(reason.to_string());
            g.clear_wait_barrier();
        }
    }

    /// Resume a paused goal. Mirrors hermes `/goal resume`: the turn budget
    /// and both circuit-breaker counters start fresh, any stale wait barrier
    /// is dropped, and subgoals are preserved.
    pub fn resume(&self) {
        let mut g = self.state.lock().unwrap();
        if g.status != GoalStatus::Paused {
            return;
        }
        g.status = GoalStatus::Active;
        g.paused_reason = None;
        g.consecutive_parse_failures = 0;
        g.consecutive_transport_failures = 0;
        g.auto_continuations = 0;
        g.turns_used = 0;
        g.clear_wait_barrier();
    }

    /// Clear the goal entirely (terminal; nothing continues afterwards).
    /// Hermes `/goal clear` wipes everything: subgoals and any wait barrier
    /// go with it.
    pub fn clear(&self) {
        let mut g = self.state.lock().unwrap();
        g.status = GoalStatus::Cleared;
        g.subgoals.clear();
        g.clear_wait_barrier();
    }

    /// Append a user-added criterion (`/subgoal add`). Whitespace-trimmed;
    /// an empty text is ignored.
    pub fn add_subgoal(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.state.lock().unwrap().subgoals.push(text.to_string());
    }

    /// Remove the 1-based `index`-th criterion (`/subgoal remove N`).
    /// Returns `false` when the index is out of range (incl. 0).
    pub fn remove_subgoal(&self, index_1based: usize) -> bool {
        let mut g = self.state.lock().unwrap();
        if index_1based == 0 || index_1based > g.subgoals.len() {
            return false;
        }
        g.subgoals.remove(index_1based - 1);
        true
    }
}

/// Render the continuation prompt. Template substitution is deterministic:
/// same objective/threshold/subgoals → byte-identical output (prompt-cache
/// stability within one goal session).
fn render_continuation(objective: &str, blocked_threshold: usize, subgoals: &[String]) -> String {
    let template = if subgoals.is_empty() {
        CONTINUATION_TEMPLATE
    } else {
        CONTINUATION_SUBGOALS_TEMPLATE
    };
    template
        .replace("{{objective}}", objective)
        .replace("{{subgoals}}", &render_subgoals_block(subgoals))
        .replace("{{blocked_threshold}}", &blocked_threshold.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::judge::tests::MockJudgeClient;

    fn continue_reply() -> Result<String, String> {
        Ok(r#"{"verdict": "continue", "reason": "keep going"}"#.to_string())
    }

    fn done_reply() -> Result<String, String> {
        Ok(r#"{"verdict": "done", "reason": "all verified"}"#.to_string())
    }

    #[tokio::test]
    async fn continuation_injects_until_cap() {
        let rt = GoalRuntime::new("ship the feature".into(), 2);
        let judge = MockJudgeClient::new(vec![continue_reply(), continue_reply()]);
        // First two fire, third stops at cap — without calling the judge.
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_none());
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let s = rt.snapshot();
        assert_eq!(s.turns_used, 2);
        assert_eq!(s.auto_continuations, 2);
        assert_eq!(s.last_verdict, Some(GoalVerdict::Continue));
        assert_eq!(s.last_reason.as_deref(), Some("keep going"));
        assert!(s.last_turn_at.is_some());
    }

    #[tokio::test]
    async fn done_verdict_completes_and_stops() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let judge = MockJudgeClient::new(vec![done_reply()]);
        assert!(rt.evaluate_and_continue("shipped", &judge).await.is_none());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Complete);
        assert_eq!(s.last_verdict, Some(GoalVerdict::Done));
        // No continuation was burned on the done verdict.
        assert_eq!(s.auto_continuations, 0);
    }

    #[tokio::test]
    async fn targetless_wait_verdict_downgrades_to_continue() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let judge = MockJudgeClient::new(vec![Ok(
            r#"{"verdict": "wait", "reason": "CI running"}"#.to_string()
        )]);
        // A wait verdict without a concrete target can't park — the parser
        // downgrades it to a normal continuation.
        assert!(rt.evaluate_and_continue("waiting on CI", &judge).await.is_some());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Active);
        assert_eq!(s.last_verdict, Some(GoalVerdict::Continue));
    }

    #[tokio::test]
    async fn wait_for_seconds_parks_without_burning_budget() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let judge = MockJudgeClient::new(vec![Ok(
            r#"{"verdict": "wait", "wait_for_seconds": 3600, "reason": "rate limited"}"#
                .to_string(),
        )]);
        let before = epoch_ms();
        assert!(rt.evaluate_and_continue("backing off", &judge).await.is_none());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Waiting);
        assert_eq!(s.last_verdict, Some(GoalVerdict::Wait));
        assert!(s.waiting_until.unwrap() >= before + 3_600_000);
        assert_eq!(s.waiting_reason.as_deref(), Some("rate limited"));
        // Parking consumes no budget — waiting is not progress.
        assert_eq!(s.turns_used, 0);
        assert_eq!(s.auto_continuations, 0);

        // While parked: quiesce — no continuation, and no judge call either.
        assert!(rt.evaluate_and_continue("still waiting", &judge).await.is_none());
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(rt.snapshot().status, GoalStatus::Waiting);
    }

    #[tokio::test]
    async fn expired_wait_barrier_resumes_judging() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let shared = rt.shared_state();
        {
            let mut g = shared.lock().unwrap();
            g.status = GoalStatus::Waiting;
            g.waiting_until = Some(epoch_ms().saturating_sub(1)); // already past
            g.waiting_reason = Some("stale".into());
        }
        let judge = MockJudgeClient::new(vec![continue_reply()]);
        // Lazy auto-clear: the expired barrier drops and the same call runs
        // the judge and injects a continuation.
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Active);
        assert!(s.waiting_until.is_none());
        assert!(s.waiting_reason.is_none());
        assert_eq!(s.turns_used, 1);
    }

    #[tokio::test]
    async fn pid_wait_directive_downgrades_to_continue_in_phase_2() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let judge = MockJudgeClient::new(vec![Ok(
            r#"{"verdict": "wait", "wait_on_pid": 4242, "reason": "build running"}"#.to_string(),
        )]);
        // TODO(phase 3): once the process barrier lands this parks instead.
        assert!(rt.evaluate_and_continue("building", &judge).await.is_some());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Active);
        assert!(s.waiting_on_pid.is_none());
        assert_eq!(s.turns_used, 1);
    }

    #[tokio::test]
    async fn pause_from_waiting_drops_the_barrier() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let shared = rt.shared_state();
        {
            let mut g = shared.lock().unwrap();
            g.status = GoalStatus::Waiting;
            g.waiting_until = Some(epoch_ms() + 60_000);
            g.waiting_reason = Some("cooldown".into());
        }
        rt.pause("user asked");
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Paused);
        // A barrier is meaningless once paused (hermes) — dropped.
        assert!(s.waiting_until.is_none());
        assert!(s.waiting_reason.is_none());
    }

    #[tokio::test]
    async fn tool_declared_terminal_state_skips_judge() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        // The model already declared completion via update_goal.
        rt.shared_state().lock().unwrap().status = GoalStatus::Complete;
        let judge = MockJudgeClient::new(vec![continue_reply()]);
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_none());
        // The judge was never consulted — first terminal state wins.
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(rt.snapshot().status, GoalStatus::Complete);
    }

    #[tokio::test]
    async fn parse_failures_trip_breaker_into_paused() {
        let rt = GoalRuntime::new("ship the feature".into(), 20);
        let judge = MockJudgeClient::new(vec![
            Ok("prose, not json".to_string()),
            Ok("still prose".to_string()),
            Ok("and again".to_string()),
        ]);
        // Failures 1 and 2 fail open (continuation still fires)…
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        // …failure 3 trips the breaker: Paused, no continuation.
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_none());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Paused);
        assert_eq!(s.consecutive_parse_failures, 3);
        assert!(s.paused_reason.as_deref().unwrap().contains("unparseable"));
    }

    #[tokio::test]
    async fn transport_failures_trip_breaker_into_paused() {
        let rt = GoalRuntime::new("ship the feature".into(), 20);
        let judge = MockJudgeClient::new(vec![
            Err("401".to_string()),
            Err("401".to_string()),
            Err("401".to_string()),
            Err("401".to_string()),
            Err("401".to_string()),
        ]);
        for _ in 0..4 {
            assert!(rt.evaluate_and_continue("progress", &judge).await.is_some());
        }
        assert!(rt.evaluate_and_continue("progress", &judge).await.is_none());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Paused);
        assert_eq!(s.consecutive_transport_failures, 5);
        assert!(s.paused_reason.as_deref().unwrap().contains("unreachable"));
    }

    #[tokio::test]
    async fn successful_judgement_resets_both_breakers() {
        let rt = GoalRuntime::new("ship the feature".into(), 20);
        let judge = MockJudgeClient::new(vec![
            Ok("prose".to_string()),      // parse failure #1
            Err("timeout".to_string()),   // transport failure #1, parse resets
            continue_reply(),             // clean → both reset
        ]);
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
        let s = rt.snapshot();
        assert_eq!(s.consecutive_parse_failures, 0);
        assert_eq!(s.consecutive_transport_failures, 0);
        assert_eq!(s.status, GoalStatus::Active);
    }

    #[tokio::test]
    async fn resume_resets_counters_and_budget() {
        let rt = GoalRuntime::new("ship the feature".into(), 2);
        let judge = MockJudgeClient::new(vec![continue_reply(), continue_reply(), continue_reply()]);
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
        rt.pause("user asked");
        assert_eq!(rt.snapshot().status, GoalStatus::Paused);

        rt.resume();
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Active);
        assert!(s.paused_reason.is_none());
        assert_eq!(s.auto_continuations, 0);
        assert_eq!(s.turns_used, 0);
        // Budget is fresh — continuation fires again.
        assert!(rt.evaluate_and_continue("p", &judge).await.is_some());
    }

    #[tokio::test]
    async fn resume_is_noop_on_non_paused_goal() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        rt.shared_state().lock().unwrap().status = GoalStatus::Complete;
        rt.resume();
        assert_eq!(rt.snapshot().status, GoalStatus::Complete);
    }

    #[tokio::test]
    async fn pause_does_not_override_terminal_state() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        rt.shared_state().lock().unwrap().status = GoalStatus::Blocked;
        rt.pause("too late");
        assert_eq!(rt.snapshot().status, GoalStatus::Blocked);
    }

    #[tokio::test]
    async fn clear_stops_everything() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        rt.clear();
        assert_eq!(rt.snapshot().status, GoalStatus::Cleared);
        let judge = MockJudgeClient::new(vec![continue_reply()]);
        assert!(rt.evaluate_and_continue("p", &judge).await.is_none());
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn from_state_and_restore_preserve_all_fields() {
        let mut snapshot = GoalState::new("restore me".into(), 8);
        snapshot.status = GoalStatus::Paused;
        snapshot.paused_reason = Some("budget".into());
        snapshot.turns_used = 5;
        snapshot.auto_continuations = 5;
        snapshot.consecutive_parse_failures = 2;
        snapshot.created_at = 42;

        let rt = GoalRuntime::from_state(snapshot.clone());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Paused);
        assert_eq!(s.turns_used, 5);
        assert_eq!(s.consecutive_parse_failures, 2);
        assert_eq!(s.created_at, 42);

        // restore() swaps in place: a shared handle taken *before* the swap
        // (e.g. by UpdateGoalTool) observes the new state afterwards.
        let shared = rt.shared_state();
        snapshot.status = GoalStatus::Active;
        snapshot.turns_used = 6;
        rt.restore(snapshot);
        assert_eq!(shared.lock().unwrap().status, GoalStatus::Active);
        assert_eq!(shared.lock().unwrap().turns_used, 6);
    }

    #[tokio::test]
    async fn continuation_renders_objective_and_threshold() {
        let rt = GoalRuntime::new("migrate the database".into(), 8);
        let judge = MockJudgeClient::new(vec![continue_reply()]);
        let msg = rt.evaluate_and_continue("progress", &judge).await.unwrap();
        let text = match &msg.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text block"),
        };
        assert!(text.contains("migrate the database"));
        assert!(text.contains("连续 3 个目标轮次"));
        assert!(!text.contains("{{")); // all placeholders substituted
    }

    // ── subgoals ────────────────────────────────────────────────

    #[test]
    fn add_and_remove_subgoal_with_bounds() {
        let rt = GoalRuntime::new("ship it".into(), 8);
        rt.add_subgoal("  tests added  ");
        rt.add_subgoal("docs updated");
        rt.add_subgoal("   "); // empty after trim — ignored
        assert_eq!(rt.snapshot().subgoals, vec!["tests added", "docs updated"]);

        // 1-based; 0 and past-the-end are out of range.
        assert!(!rt.remove_subgoal(0));
        assert!(!rt.remove_subgoal(3));
        assert!(rt.remove_subgoal(1));
        assert_eq!(rt.snapshot().subgoals, vec!["docs updated"]);
        assert!(!rt.remove_subgoal(2));
    }

    #[tokio::test]
    async fn continuation_with_subgoals_renders_criteria_block() {
        let rt = GoalRuntime::new("ship it".into(), 8);
        rt.add_subgoal("tests added");
        rt.add_subgoal("docs updated");
        let judge = MockJudgeClient::new(vec![continue_reply()]);
        let msg = rt.evaluate_and_continue("progress", &judge).await.unwrap();
        let text = match &msg.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text block"),
        };
        assert!(text.contains("- 1. tests added"));
        assert!(text.contains("- 2. docs updated"));
        assert!(text.contains("额外准则"));
        assert!(!text.contains("{{")); // all placeholders substituted
    }

    #[test]
    fn continuation_render_is_byte_stable() {
        // Prompt-cache guard: same inputs → byte-identical output, for both
        // template variants.
        let subgoals = vec!["tests added".to_string(), "docs updated".to_string()];
        assert_eq!(
            render_continuation("ship it", 3, &[]),
            render_continuation("ship it", 3, &[]),
        );
        assert_eq!(
            render_continuation("ship it", 3, &subgoals),
            render_continuation("ship it", 3, &subgoals),
        );
        // And the two variants differ only by the presence of the block.
        assert!(!render_continuation("ship it", 3, &[]).contains("<subgoals>"));
        assert!(render_continuation("ship it", 3, &subgoals).contains("<subgoals>"));
    }

    #[tokio::test]
    async fn resume_preserves_subgoals_and_clear_wipes_them() {
        let rt = GoalRuntime::new("ship it".into(), 8);
        rt.add_subgoal("tests added");

        rt.pause("user asked");
        rt.resume();
        // Hermes: resume starts the budget fresh but keeps the criteria.
        assert_eq!(rt.snapshot().subgoals, vec!["tests added"]);

        rt.clear();
        let s = rt.snapshot();
        // Hermes: clear wipes everything.
        assert_eq!(s.status, GoalStatus::Cleared);
        assert!(s.subgoals.is_empty());
    }
}
