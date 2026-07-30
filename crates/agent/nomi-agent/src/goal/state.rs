//! In-memory goal state for one Agent engine session. P0: not persisted, no SQLite.
//!
//! Everything here derives serde so the host can carry snapshots across layers
//! (engine → backend → UI) and persist them later. Field names are the wire
//! contract — do not rename without bumping the backend consumers.

use serde::{Deserialize, Serialize};

/// Consecutive judge *parse* failures (empty output / non-JSON) that trip the
/// auto-pause circuit breaker. Mirrors hermes
/// `DEFAULT_MAX_CONSECUTIVE_PARSE_FAILURES` — guards against weak judge models
/// that cannot follow the strict JSON reply contract burning the whole budget.
pub const MAX_CONSECUTIVE_PARSE_FAILURES: u32 = 3;

/// Consecutive judge *transport* failures (auth 401, timeout, DNS) that trip
/// the auto-pause circuit breaker. Mirrors hermes
/// `DEFAULT_MAX_CONSECUTIVE_TRANSPORT_FAILURES` — a permanently broken judge
/// endpoint must not burn every continuation slot on an unreachable API.
pub const MAX_CONSECUTIVE_TRANSPORT_FAILURES: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Complete,
    Blocked,
    /// Auto-paused (circuit breaker) or user-paused; `paused_reason` says why.
    Paused,
    /// Parked on a wait barrier: a time deadline (`waiting_until`), a
    /// process (`waiting_on_pid`), or a session (`waiting_on_session`).
    Waiting,
    /// Explicitly cleared by the user (`/goal clear`). Terminal.
    Cleared,
}

/// The judge's three-way verdict, plus `Skipped` when the judge could not run
/// at all (empty goal). A `Wait` verdict always carries a concrete directive
/// (seconds / pid / session) that parks the loop on a wait barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerdict {
    Done,
    Continue,
    Wait,
    Skipped,
}

/// Optional structured completion contract (hermes `GoalContract`). When
/// present (non-empty), the continuation prompt targets the verification
/// surface and the judge decides DONE strictly against the Verification
/// criterion, refusing completion when a Constraint was violated. Empty
/// fields are omitted everywhere — a goal with no contract behaves exactly
/// like the original free-form goal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoalContract {
    /// The single end state that must be true when done.
    pub outcome: String,
    /// The specific test / command / artifact that PROVES the outcome.
    pub verification: String,
    /// What must not be broken / violated along the way.
    pub constraints: String,
    /// What is in scope (and implicitly, what is not).
    pub boundaries: String,
    /// The condition under which the agent should stop and ask the user.
    pub stop_when: String,
}

impl GoalContract {
    pub fn is_empty(&self) -> bool {
        self.labelled_fields().iter().all(|(_, f)| f.trim().is_empty())
    }

    /// The five fields with their human labels, in hermes display order.
    fn labelled_fields(&self) -> [(&'static str, &str); 5] {
        [
            ("Outcome", self.outcome.as_str()),
            ("Verification", self.verification.as_str()),
            ("Constraints", self.constraints.as_str()),
            ("Boundaries", self.boundaries.as_str()),
            ("Stop when blocked", self.stop_when.as_str()),
        ]
    }

    /// Render the non-empty fields as a labelled `- Label: value` block (port
    /// of hermes `GoalContract.render_block`). Empty contract → empty string;
    /// callers must then skip the contract section entirely.
    pub fn render_block(&self) -> String {
        self.labelled_fields()
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(label, v)| format!("- {label}: {}", v.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Current wall-clock time in epoch milliseconds.
pub(crate) fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Per-session goal state. Lives in engine memory for the lifetime of one
/// `engine.execute_turn()`; lost on restart (degrades to a plain session, no
/// data loss). Snapshots are handed to the host via `AgentEngine::goal_state`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GoalState {
    /// The objective text (provided at session start).
    pub objective: String,
    /// Current status. Only `Active` continues.
    pub status: GoalStatus,
    /// Cap on automatic continuations (anti-runaway). Default 8.
    pub max_auto_continuations: usize,
    /// How many automatic continuations have fired so far.
    pub auto_continuations: usize,
    /// Threshold (in goal turns) before `blocked` is allowed. Rendered into the
    /// continuation prompt; P0 only constrains the model via the prompt.
    pub blocked_threshold: usize,
    /// Goal-turn counter (each judged-then-continued round), independent of
    /// the engine's own turn counter. Reset by `resume()`.
    pub turns_used: usize,
    /// The judge's most recent verdict, if any evaluation ran.
    pub last_verdict: Option<GoalVerdict>,
    /// The judge's one-line reason for `last_verdict`.
    pub last_reason: Option<String>,
    /// Why the goal is `Paused` (circuit breaker / user action).
    pub paused_reason: Option<String>,
    /// Judge parse failures in a row (see `MAX_CONSECUTIVE_PARSE_FAILURES`).
    pub consecutive_parse_failures: u32,
    /// Judge transport failures in a row (see `MAX_CONSECUTIVE_TRANSPORT_FAILURES`).
    pub consecutive_transport_failures: u32,
    /// When this goal was created (epoch ms).
    pub created_at: u64,
    /// When the last goal evaluation finished (epoch ms).
    pub last_turn_at: Option<u64>,
    /// User-added criteria appended mid-loop (`/subgoal`). When non-empty the
    /// judge prompt and continuation prompt both render them so the agent
    /// works toward them and the judge factors them into the verdict.
    pub subgoals: Vec<String>,
    /// Optional structured completion contract. When set (non-empty) the
    /// judge decides DONE against its Verification criterion and the
    /// continuation prompt renders the full contract block.
    pub contract: Option<GoalContract>,
    /// Time-based wait barrier deadline (epoch ms). While `status` is
    /// `Waiting` and this deadline lies in the future, the continuation hook
    /// quiesces — no judge call, no budget burn. Lazily auto-cleared once the
    /// deadline passes (next evaluation resumes normal judging).
    pub waiting_until: Option<u64>,
    /// Pid-based wait barrier: park while the process is alive (releases on
    /// exit). Checked via the host-injected `GoalWaitProbe`; without a probe
    /// the barrier fails open at the next evaluation point.
    pub waiting_on_pid: Option<u32>,
    /// Session-based wait barrier: park while the session's own trigger has
    /// not fired (it exits OR its watch pattern matches). Checked via the
    /// host-injected `GoalWaitProbe`; fails open without one.
    pub waiting_on_session: Option<String>,
    /// Human-readable reason for the wait barrier.
    pub waiting_reason: Option<String>,
}

impl Default for GoalState {
    fn default() -> Self {
        Self::new(String::new(), 8)
    }
}

impl GoalState {
    pub fn new(objective: String, max_auto_continuations: usize) -> Self {
        Self {
            objective,
            status: GoalStatus::Active,
            max_auto_continuations,
            auto_continuations: 0,
            blocked_threshold: 3,
            turns_used: 0,
            last_verdict: None,
            last_reason: None,
            paused_reason: None,
            consecutive_parse_failures: 0,
            consecutive_transport_failures: 0,
            created_at: epoch_ms(),
            last_turn_at: None,
            subgoals: Vec::new(),
            contract: None,
            waiting_until: None,
            waiting_on_pid: None,
            waiting_on_session: None,
            waiting_reason: None,
        }
    }

    /// Whether continuation should still fire: Active and under the cap.
    pub fn should_continue(&self) -> bool {
        self.status == GoalStatus::Active && self.auto_continuations < self.max_auto_continuations
    }

    /// Drop every wait-barrier field. Hermes semantics: a barrier is
    /// meaningless once the goal is paused/resumed/cleared, and it is lazily
    /// cleared when its deadline passes.
    pub(crate) fn clear_wait_barrier(&mut self) {
        self.waiting_until = None;
        self.waiting_on_pid = None;
        self.waiting_on_session = None;
        self.waiting_reason = None;
    }
}

/// Render user-added subgoals as a numbered `- N. text` block (port of hermes
/// `render_subgoals_block`). Empty string when there are none — callers must
/// then keep the prompt byte-identical to the no-subgoals shape.
pub(crate) fn render_subgoals_block(subgoals: &[String]) -> String {
    subgoals
        .iter()
        .enumerate()
        .map(|(i, text)| format!("- {}. {}", i + 1, text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Contract block with any subgoals folded in as extra criteria (hermes:
/// when a contract and subgoals coexist, the subgoals are appended into the
/// contract block so the judge and the agent see a single source of truth).
/// Deterministic: same contract + subgoals → byte-identical output.
pub(crate) fn render_contract_block(contract: &GoalContract, subgoals: &[String]) -> String {
    let block = contract.render_block();
    if subgoals.is_empty() {
        return block;
    }
    let extra = subgoals
        .iter()
        .enumerate()
        .map(|(i, text)| format!("- Extra criterion {}: {}", i + 1, text))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{block}\n{extra}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_under_cap_continues() {
        let g = GoalState::new("do X".into(), 8);
        assert!(g.should_continue());
    }

    #[test]
    fn completed_does_not_continue() {
        let mut g = GoalState::new("do X".into(), 8);
        g.status = GoalStatus::Complete;
        assert!(!g.should_continue());
    }

    #[test]
    fn blocked_does_not_continue() {
        let mut g = GoalState::new("do X".into(), 8);
        g.status = GoalStatus::Blocked;
        assert!(!g.should_continue());
    }

    #[test]
    fn paused_does_not_continue() {
        let mut g = GoalState::new("do X".into(), 8);
        g.status = GoalStatus::Paused;
        assert!(!g.should_continue());
    }

    #[test]
    fn cap_stops_continuation() {
        let mut g = GoalState::new("do X".into(), 2);
        g.auto_continuations = 2;
        assert!(!g.should_continue());
    }

    #[test]
    fn serializes_wire_contract_field_names() {
        // The backend (task #2) consumes these exact names — lock them down.
        let mut g = GoalState::new("do X".into(), 8);
        g.last_verdict = Some(GoalVerdict::Continue);
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(v["status"], "active");
        assert_eq!(v["last_verdict"], "continue");
        assert_eq!(v["turns_used"], 0);
        assert_eq!(v["consecutive_parse_failures"], 0);
        assert_eq!(v["consecutive_transport_failures"], 0);
        assert!(v.get("created_at").is_some());
        assert!(v.get("subgoals").is_some());
        assert!(v.get("contract").is_some());
        assert!(v.get("waiting_on_pid").is_some());
    }

    #[test]
    fn deserializes_with_missing_fields() {
        // Forward-compat: an old/partial payload still round-trips via defaults.
        let g: GoalState =
            serde_json::from_str(r#"{"objective":"do X","status":"paused"}"#).unwrap();
        assert_eq!(g.objective, "do X");
        assert_eq!(g.status, GoalStatus::Paused);
        assert_eq!(g.turns_used, 0);
        assert!(g.last_verdict.is_none());
    }

    #[test]
    fn status_serializes_snake_case() {
        for (status, expected) in [
            (GoalStatus::Active, "\"active\""),
            (GoalStatus::Complete, "\"complete\""),
            (GoalStatus::Blocked, "\"blocked\""),
            (GoalStatus::Paused, "\"paused\""),
            (GoalStatus::Waiting, "\"waiting\""),
            (GoalStatus::Cleared, "\"cleared\""),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
        }
    }

    #[test]
    fn contract_is_empty_ignores_whitespace() {
        let mut c = GoalContract::default();
        assert!(c.is_empty());
        c.outcome = "  ".into();
        assert!(c.is_empty());
        c.verification = "tests pass".into();
        assert!(!c.is_empty());
    }

    #[test]
    fn contract_render_block_lists_only_non_empty_fields() {
        let c = GoalContract {
            outcome: "auth migrated to JWT".into(),
            verification: " the auth test suite passes ".into(),
            constraints: String::new(),
            boundaries: "only services/auth".into(),
            stop_when: "schema change needs sign-off".into(),
        };
        assert_eq!(
            c.render_block(),
            "- Outcome: auth migrated to JWT\n\
             - Verification: the auth test suite passes\n\
             - Boundaries: only services/auth\n\
             - Stop when blocked: schema change needs sign-off"
        );
        assert_eq!(GoalContract::default().render_block(), "");
    }

    #[test]
    fn contract_block_folds_subgoals_in_as_extra_criteria() {
        let c = GoalContract {
            outcome: "ship it".into(),
            ..Default::default()
        };
        let subgoals = vec!["tests added".to_string(), "docs updated".to_string()];
        assert_eq!(
            render_contract_block(&c, &subgoals),
            "- Outcome: ship it\n\
             - Extra criterion 1: tests added\n\
             - Extra criterion 2: docs updated"
        );
        // Without subgoals the block is exactly the contract's own block.
        assert_eq!(render_contract_block(&c, &[]), c.render_block());
    }
}
