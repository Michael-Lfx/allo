use std::sync::{Arc, Mutex};

use nomi_types::message::{ContentBlock, Message, Role};

use crate::goal::judge::{self, GoalJudgeClient};
use crate::goal::state::{
    GoalState, GoalStatus, GoalVerdict, MAX_CONSECUTIVE_PARSE_FAILURES,
    MAX_CONSECUTIVE_TRANSPORT_FAILURES, epoch_ms,
};

const CONTINUATION_TEMPLATE: &str = include_str!("templates/continuation.md");

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
    /// - auto-continuation budget exhausted → None
    /// - judge says done → status becomes `Complete`, None
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
        let objective = {
            let g = self.state.lock().unwrap();
            if !g.should_continue() {
                return None;
            }
            g.objective.clone()
        };

        let outcome = judge::judge_goal(&objective, last_response, judge_client).await;

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

        // TODO(phase 2): a Wait verdict should park the loop on a wait
        // barrier (waiting_on_pid / waiting_on_session / waiting_until)
        // instead of continuing. Phase 1 treats it as Continue.
        // Skipped (judge couldn't run) also fails open to Continue.

        g.turns_used += 1;
        g.auto_continuations += 1;
        let prompt = render_continuation(&g.objective, g.blocked_threshold);
        Some(Message::now(
            Role::User,
            vec![ContentBlock::Text { text: prompt }],
        ))
    }

    /// Pause the goal (user action or host-side breaker). No-op on a goal
    /// that is already terminal (Complete/Blocked/Cleared).
    pub fn pause(&self, reason: &str) {
        let mut g = self.state.lock().unwrap();
        if matches!(g.status, GoalStatus::Active | GoalStatus::Waiting) {
            g.status = GoalStatus::Paused;
            g.paused_reason = Some(reason.to_string());
        }
    }

    /// Resume a paused goal. Mirrors hermes `/goal resume`: the turn budget
    /// and both circuit-breaker counters start fresh.
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
    }

    /// Clear the goal entirely (terminal; nothing continues afterwards).
    pub fn clear(&self) {
        let mut g = self.state.lock().unwrap();
        g.status = GoalStatus::Cleared;
    }
}

fn render_continuation(objective: &str, blocked_threshold: usize) -> String {
    CONTINUATION_TEMPLATE
        .replace("{{objective}}", objective)
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
    async fn wait_verdict_treated_as_continue_in_phase_1() {
        let rt = GoalRuntime::new("ship the feature".into(), 8);
        let judge = MockJudgeClient::new(vec![Ok(
            r#"{"verdict": "wait", "reason": "CI running"}"#.to_string()
        )]);
        // Phase 1: wait downgrades to a normal continuation.
        assert!(rt.evaluate_and_continue("waiting on CI", &judge).await.is_some());
        let s = rt.snapshot();
        assert_eq!(s.status, GoalStatus::Active);
        assert_eq!(s.last_verdict, Some(GoalVerdict::Wait));
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
}
