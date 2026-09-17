//! Horizon controller: unique owner of Goal auto-continue and office Plan overlay.
//!
//! Coding sessions keep `nomi_coding`'s todo/verify `on_natural_end` path.
//! Horizon never resets the engine `turn` counter.

mod budget;
mod delta;
mod ledger;

pub use budget::{effective_auto_continue_cap, HorizonBudget, FREEFORM_AUTO_CONTINUE_CAP};
pub use delta::{render_continuation_delta, ContinuationDelta};
pub use ledger::{
    ProgressLedger, ProgressSnapshot, ToolObservation, NO_PROGRESS_STOP_STREAK,
};

use std::path::Path;

use crate::goal::state::GoalContract;

/// Office Plan Mode soft nudge (provider turns spent planning).
pub const OFFICE_PLAN_SOFT: usize = 8;
/// Office Plan Mode hard stop instruction (still leaves ExitPlanMode available).
pub const OFFICE_PLAN_HARD: usize = 12;

const OFFICE_PLAN_NUDGE: &str = "\
Plan mode has run long enough. Stop exploring. Call ExitPlanMode now with a \
complete plan that includes a Verification command the user can run.";

const OFFICE_PLAN_HARD_STOP: &str = "\
Plan mode hard stop: do not read more files. Submit the plan via ExitPlanMode \
in this turn, including Context, Files to modify, and a concrete Verification \
command. Implementation tools stay locked until the user sends the next message.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizonStopKind {
    Continue,
    IdleStreak,
    BudgetExhausted,
    WallClock,
    PlanActive,
    AwaitingPlanApproval,
}

#[derive(Debug, Clone)]
pub struct HorizonDecision {
    pub kind: HorizonStopKind,
    pub allow_continue: bool,
    /// Pause the Goal when auto-continue is vetoed for idle/budget/wall.
    /// Plan-mode vetoes do not pause: the Goal stays Active for after approval.
    pub pause_goal: bool,
    pub reason: String,
    pub streak: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficePlanAction {
    None,
    Nudge,
    HardStop,
}

impl OfficePlanAction {
    pub fn text(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Nudge => Some(OFFICE_PLAN_NUDGE),
            Self::HardStop => Some(OFFICE_PLAN_HARD_STOP),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HorizonController {
    ledger: ProgressLedger,
    budget: HorizonBudget,
    office_plan_turns: usize,
    office_plan_nudge_sent: bool,
    office_plan_hard_sent: bool,
}

impl HorizonController {
    pub fn ledger(&self) -> &ProgressLedger {
        &self.ledger
    }

    pub fn progress(&self) -> ProgressSnapshot {
        self.ledger.snapshot()
    }

    pub fn reset(&mut self) {
        self.ledger.reset_all();
        self.budget.reset();
        self.office_plan_turns = 0;
        self.office_plan_nudge_sent = false;
        self.office_plan_hard_sent = false;
    }

    pub fn on_user_request(&mut self) {
        self.ledger.on_user_request();
        self.office_plan_turns = 0;
        self.office_plan_nudge_sent = false;
        self.office_plan_hard_sent = false;
    }

    pub fn configure_goal(&mut self, requested_cap: usize, contract: Option<&GoalContract>) {
        let has_verification = contract.is_some_and(|c| !c.verification.trim().is_empty());
        self.budget
            .configure(effective_auto_continue_cap(requested_cap, has_verification));
    }

    /// `/goal resume` zeroes `auto_continuations`; Horizon's budget must follow
    /// or the previous run's cap would still block the fresh window.
    pub fn align_budget(
        &mut self,
        requested_cap: usize,
        contract: Option<&GoalContract>,
        auto_continuations: usize,
    ) {
        if auto_continuations == 0 && self.budget.continuations() > 0 {
            self.budget.reset();
        }
        self.configure_goal(requested_cap, contract);
    }

    pub fn observe_tools(&mut self, tools: &[ToolObservation]) {
        self.ledger.observe_tools(tools);
    }

    pub fn observe_end_turn(
        &mut self,
        assistant_text: &str,
        cwd: Option<&Path>,
        pending_steps: usize,
    ) -> bool {
        self.ledger
            .observe_end_turn(assistant_text, cwd, pending_steps)
    }

    /// Call after `progress()` has been copied onto GoalState for this EndTurn.
    pub fn consume_turn_scoped(&mut self) {
        self.ledger.consume_turn_scoped();
    }

    pub fn record_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.budget.record_usage(input_tokens, output_tokens);
    }

    pub fn record_continuation(&mut self) {
        self.budget.record_continuation();
    }

    pub fn observe_office_plan_turn(&mut self, plan_active: bool) -> OfficePlanAction {
        if !plan_active {
            self.office_plan_turns = 0;
            self.office_plan_nudge_sent = false;
            self.office_plan_hard_sent = false;
            return OfficePlanAction::None;
        }
        self.office_plan_turns = self.office_plan_turns.saturating_add(1);
        if self.office_plan_turns >= OFFICE_PLAN_HARD {
            self.office_plan_hard_sent = true;
            OfficePlanAction::HardStop
        } else if self.office_plan_turns >= OFFICE_PLAN_SOFT && !self.office_plan_nudge_sent {
            self.office_plan_nudge_sent = true;
            OfficePlanAction::Nudge
        } else {
            OfficePlanAction::None
        }
    }

    pub fn decide(
        &self,
        plan_active: bool,
        awaiting_plan_approval: bool,
    ) -> HorizonDecision {
        let streak = self.ledger.no_progress_streak();
        let remaining = self.budget.remaining();
        if awaiting_plan_approval {
            return HorizonDecision {
                kind: HorizonStopKind::AwaitingPlanApproval,
                allow_continue: false,
                pause_goal: false,
                reason: "plan submitted; waiting for the user to continue".into(),
                streak,
                remaining,
            };
        }
        if plan_active {
            return HorizonDecision {
                kind: HorizonStopKind::PlanActive,
                allow_continue: false,
                pause_goal: false,
                reason: "plan mode is active; Goal auto-continue is suspended".into(),
                streak,
                remaining,
            };
        }
        if self.budget.wall_exceeded() {
            return HorizonDecision {
                kind: HorizonStopKind::WallClock,
                allow_continue: false,
                pause_goal: true,
                reason: "goal wall-clock budget exhausted".into(),
                streak,
                remaining,
            };
        }
        if self.budget.exhausted() {
            return HorizonDecision {
                kind: HorizonStopKind::BudgetExhausted,
                allow_continue: false,
                pause_goal: true,
                reason: "goal auto-continuation budget exhausted".into(),
                streak,
                remaining,
            };
        }
        if streak >= NO_PROGRESS_STOP_STREAK {
            return HorizonDecision {
                kind: HorizonStopKind::IdleStreak,
                allow_continue: false,
                pause_goal: true,
                reason: format!(
                    "no observable progress for {streak} consecutive goal turns"
                ),
                streak,
                remaining,
            };
        }
        HorizonDecision {
            kind: HorizonStopKind::Continue,
            allow_continue: true,
            pause_goal: false,
            reason: self
                .ledger
                .last_idle_reason()
                .unwrap_or("continue toward the goal")
                .to_string(),
            streak,
            remaining,
        }
    }

    pub fn continuation_delta(
        &self,
        objective: &str,
        contract: Option<&GoalContract>,
        subgoal_block: &str,
    ) -> ContinuationDelta {
        let missing = if contract.is_some_and(|c| !c.verification.trim().is_empty())
            && !self.ledger.snapshot().verify_ok
        {
            format!(
                "Missing verification: run `{}` (or the equivalent) and read the output before completing.",
                contract
                    .map(|c| c.verification.trim())
                    .unwrap_or_default()
            )
        } else {
            String::new()
        };
        let mut criteria = String::new();
        if let Some(c) = contract.filter(|c| !c.is_empty()) {
            criteria = format!("\n完成合约：\n{}\n", c.render_block());
        } else if !subgoal_block.is_empty() {
            criteria = format!("\n附加准则：\n{subgoal_block}\n");
        }
        ContinuationDelta {
            objective: objective.to_string(),
            criteria,
            world_delta: self.ledger.idle_brief(),
            remaining: self.budget.remaining(),
            missing_verification: missing,
        }
    }

    pub fn observation_payload(&self, decision: &HorizonDecision) -> serde_json::Value {
        let (input_tokens, output_tokens) = self.budget.token_totals();
        let snap = self.ledger.snapshot();
        serde_json::json!({
            "kind": format!("{:?}", decision.kind),
            "allow_continue": decision.allow_continue,
            "pause_goal": decision.pause_goal,
            "reason": decision.reason,
            "no_progress_streak": decision.streak,
            "remaining": decision.remaining,
            "continuations": self.budget.continuations(),
            "mutated": snap.mutated,
            "verify_ok": snap.verify_ok,
            "workspace_changed": snap.workspace_changed,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon::ledger::ToolObservation;

    #[test]
    fn idle_streak_pauses_after_two_empty_endturns() {
        let mut h = HorizonController::default();
        h.configure_goal(8, None);
        h.observe_end_turn("I will keep going.", None, 0);
        let first = h.decide(false, false);
        assert!(first.allow_continue);
        h.observe_end_turn("I will keep going.", None, 0);
        let second = h.decide(false, false);
        assert_eq!(second.kind, HorizonStopKind::IdleStreak);
        assert!(!second.allow_continue);
        assert!(second.pause_goal);
    }

    #[test]
    fn plan_mode_blocks_continue_without_pausing_goal() {
        let h = HorizonController::default();
        let d = h.decide(true, false);
        assert_eq!(d.kind, HorizonStopKind::PlanActive);
        assert!(!d.allow_continue);
        assert!(!d.pause_goal);
    }

    #[test]
    fn awaiting_approval_blocks_continue_without_pausing_goal() {
        let h = HorizonController::default();
        let d = h.decide(true, true);
        assert_eq!(d.kind, HorizonStopKind::AwaitingPlanApproval);
        assert!(!d.pause_goal);
    }

    #[test]
    fn mutation_keeps_continue_open() {
        let mut h = HorizonController::default();
        h.configure_goal(8, None);
        h.observe_tools(&[ToolObservation {
            name: "Write".into(),
            command: None,
            success: true,
        }]);
        h.observe_end_turn("wrote the file", None, 0);
        h.consume_turn_scoped();
        h.observe_end_turn("I will keep going.", None, 0);
        h.consume_turn_scoped();
        // One idle EndTurn after real work still continues.
        let d = h.decide(false, false);
        assert!(d.allow_continue);
        assert_eq!(d.kind, HorizonStopKind::Continue);
        h.observe_end_turn("I will keep going.", None, 0);
        let paused = h.decide(false, false);
        assert_eq!(paused.kind, HorizonStopKind::IdleStreak);
        assert!(paused.pause_goal);
    }

    #[test]
    fn office_plan_overlay_nudge_then_hard() {
        let mut h = HorizonController::default();
        for _ in 0..(OFFICE_PLAN_SOFT - 1) {
            assert_eq!(h.observe_office_plan_turn(true), OfficePlanAction::None);
        }
        assert_eq!(h.observe_office_plan_turn(true), OfficePlanAction::Nudge);
        for _ in OFFICE_PLAN_SOFT..(OFFICE_PLAN_HARD - 1) {
            assert_eq!(h.observe_office_plan_turn(true), OfficePlanAction::None);
        }
        assert_eq!(h.observe_office_plan_turn(true), OfficePlanAction::HardStop);
    }

    #[test]
    fn unconfigured_budget_does_not_exhaust() {
        let h = HorizonController::default();
        let d = h.decide(false, false);
        assert!(d.allow_continue);
        assert_eq!(d.kind, HorizonStopKind::Continue);
    }

    #[test]
    fn freeform_budget_exhausts_at_three() {
        let mut h = HorizonController::default();
        h.configure_goal(8, None);
        for _ in 0..3 {
            h.record_continuation();
        }
        let d = h.decide(false, false);
        assert_eq!(d.kind, HorizonStopKind::BudgetExhausted);
        assert!(d.pause_goal);
    }
}
