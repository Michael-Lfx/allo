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
    /// Reserved for phase 2: parked on a wait barrier (pid / session / time).
    Waiting,
    /// Reserved for phase 2: explicitly cleared by the user (`/goal clear`).
    Cleared,
}

/// The judge's three-way verdict, plus `Skipped` when the judge could not run
/// at all (empty goal). `Wait` is parsed in phase 1 but handled as `Continue`
/// by the runtime — the wait barrier lands in phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerdict {
    Done,
    Continue,
    Wait,
    Skipped,
}

/// Optional structured completion contract (hermes `GoalContract`). Reserved
/// in phase 1: the fields exist for serialization/persistence, but neither the
/// continuation prompt nor the judge prompt renders them yet (phase 2/3).
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
        [
            &self.outcome,
            &self.verification,
            &self.constraints,
            &self.boundaries,
            &self.stop_when,
        ]
        .iter()
        .all(|f| f.trim().is_empty())
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
    /// Reserved for phase 2: user-added criteria (`/subgoal`).
    pub subgoals: Vec<String>,
    /// Reserved for phase 2/3: structured completion contract.
    pub contract: Option<GoalContract>,
    /// Reserved for phase 2: time-based wait barrier deadline (epoch ms).
    pub waiting_until: Option<u64>,
    /// Reserved for phase 2: pid-based wait barrier (releases on exit).
    pub waiting_on_pid: Option<u32>,
    /// Reserved for phase 2: session-based wait barrier (exit OR watch trigger).
    pub waiting_on_session: Option<String>,
    /// Reserved for phase 2: human-readable reason for the wait barrier.
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
}
