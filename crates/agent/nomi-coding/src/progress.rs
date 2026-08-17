//! Progress / exploration / verification state for coding mode.
//!
//! Soft nudges still exist, but exploration and edit-failure streaks can escalate
//! to **hard stops** so the agent cannot "keep working" forever without finishing.

use std::collections::HashSet;

/// Tools that count as exploration (not file mutation / verify).
pub fn is_explore_tool(name: &str) -> bool {
    matches!(
        name,
        "Read"
            | "Grep"
            | "Glob"
            | "DirTree"
            | "dir_tree"
            | "ToolSearch"
            | "LS"
            | "SemanticSearch"
            | "web_search"
            | "web_extract"
    )
}

/// Tracks coding-turn productivity for both soft nudges and hard finish gates.
#[derive(Debug, Default)]
pub struct CodingProgressGuard {
    /// Consecutive turns where every tool call failed (or nothing ran).
    failed_turns: usize,
    /// Consecutive turns that only ran successful explore tools (no mutation).
    explore_only_turns: usize,
    /// True once this request mutated files via Edit/Write/ApplyPatch.
    mutated_files: bool,
    /// True once a verification-like command succeeded after a mutation.
    verified_after_mutation: bool,
    /// True after we already injected a verify nudge for the current mutation.
    verification_nudge_sent: bool,
    /// True after we already injected an explore-budget soft nudge this streak.
    explore_budget_nudge_sent: bool,
    /// True once explore hard-stop has been requested this streak.
    explore_hard_stop_sent: bool,
    /// Turns spent in plan mode without exiting (coding sessions).
    plan_mode_turns: usize,
    plan_budget_nudge_sent: bool,
    /// When set, the next natural EndTurn must allow finish (no continuations).
    force_allow_finish: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingProgressAction {
    Continue,
    /// Soft hint only.
    NudgeExplore,
    /// Exploration soft budget exhausted — act, verify, or stop.
    NudgeExploreBudget,
    /// Exploration hard budget exhausted — engine must stop after this turn.
    HardStopExplore,
    /// Plan mode lingered too long without ExitPlanMode.
    NudgePlanTimeout,
    /// Plan mode hard stop.
    HardStopPlanTimeout,
    NudgeVerify,
}

/// Tunables consumed by [`CodingProgressGuard::observe_tool_turn`].
#[derive(Debug, Clone, Copy)]
pub struct ProgressObserveParams {
    pub failed_nudge_threshold: usize,
    /// Soft explore nudge threshold.
    pub explore_budget: usize,
    /// Hard stop after this many explore-only turns (must be >= explore_budget).
    pub explore_hard_stop: usize,
}

impl Default for ProgressObserveParams {
    fn default() -> Self {
        Self {
            failed_nudge_threshold: CodingProgressGuard::FAILED_NUDGE_THRESHOLD,
            explore_budget: CodingProgressGuard::DEFAULT_EXPLORE_BUDGET,
            explore_hard_stop: CodingProgressGuard::DEFAULT_EXPLORE_HARD_STOP,
        }
    }
}

impl CodingProgressGuard {
    pub const FAILED_NUDGE_THRESHOLD: usize = 8;
    /// Soft explore nudge — keep low so simple tasks cannot tour forever.
    pub const DEFAULT_EXPLORE_BUDGET: usize = 6;
    /// Hard stop shortly after soft budget (then forced finalize EndTurn).
    pub const DEFAULT_EXPLORE_HARD_STOP: usize = 10;
    pub const DEFAULT_PLAN_MODE_BUDGET: usize = 10;
    pub const DEFAULT_PLAN_MODE_HARD_STOP: usize = 14;

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn force_allow_finish(&self) -> bool {
        self.force_allow_finish
    }

    pub fn mark_force_allow_finish(&mut self) {
        self.force_allow_finish = true;
    }

    pub fn clear_force_allow_finish(&mut self) {
        self.force_allow_finish = false;
    }

    pub fn observe_plan_mode_turn(
        &mut self,
        active: bool,
        soft_budget: usize,
        hard_stop: usize,
    ) -> CodingProgressAction {
        if !active {
            self.plan_mode_turns = 0;
            self.plan_budget_nudge_sent = false;
            return CodingProgressAction::Continue;
        }
        self.plan_mode_turns = self.plan_mode_turns.saturating_add(1);
        let soft = soft_budget.max(1);
        let hard = hard_stop.max(soft);
        if self.plan_mode_turns >= hard {
            self.force_allow_finish = true;
            return CodingProgressAction::HardStopPlanTimeout;
        }
        if self.plan_mode_turns >= soft && !self.plan_budget_nudge_sent {
            self.plan_budget_nudge_sent = true;
            return CodingProgressAction::NudgePlanTimeout;
        }
        CodingProgressAction::Continue
    }

    pub fn observe_tool_turn(
        &mut self,
        tool_names: &HashSet<String>,
        had_successful_mutating_result: bool,
        had_successful_verification: bool,
        had_file_mutation: bool,
        had_any_successful_result: bool,
        params: ProgressObserveParams,
    ) -> CodingProgressAction {
        if had_file_mutation {
            self.mutated_files = true;
            self.verified_after_mutation = false;
            self.verification_nudge_sent = false;
            self.explore_only_turns = 0;
            self.explore_budget_nudge_sent = false;
            self.explore_hard_stop_sent = false;
            self.force_allow_finish = false;
        }
        if had_successful_verification {
            self.verified_after_mutation = true;
            self.verification_nudge_sent = false;
            self.failed_turns = 0;
            self.explore_only_turns = 0;
            self.explore_budget_nudge_sent = false;
            self.explore_hard_stop_sent = false;
            self.force_allow_finish = false;
            return CodingProgressAction::Continue;
        }
        if had_successful_mutating_result {
            self.failed_turns = 0;
            if !had_file_mutation {
                self.explore_only_turns = 0;
                self.explore_budget_nudge_sent = false;
                self.explore_hard_stop_sent = false;
            }
            return CodingProgressAction::Continue;
        }

        if had_any_successful_result {
            self.failed_turns = 0;
            let explore_only =
                !tool_names.is_empty() && tool_names.iter().all(|n| is_explore_tool(n));
            if explore_only {
                self.explore_only_turns = self.explore_only_turns.saturating_add(1);
                let soft = params.explore_budget.max(1);
                let hard = params.explore_hard_stop.max(soft);
                if self.explore_only_turns >= hard && !self.explore_hard_stop_sent {
                    self.explore_hard_stop_sent = true;
                    self.force_allow_finish = true;
                    return CodingProgressAction::HardStopExplore;
                }
                if self.explore_only_turns >= soft && !self.explore_budget_nudge_sent {
                    self.explore_budget_nudge_sent = true;
                    return CodingProgressAction::NudgeExploreBudget;
                }
            } else {
                self.explore_only_turns = 0;
                self.explore_budget_nudge_sent = false;
                self.explore_hard_stop_sent = false;
            }
            return CodingProgressAction::Continue;
        }

        self.failed_turns = self.failed_turns.saturating_add(1);
        let fail_thresh = params.failed_nudge_threshold.max(1);
        if self.failed_turns >= fail_thresh {
            CodingProgressAction::NudgeExplore
        } else {
            CodingProgressAction::Continue
        }
    }

    pub fn needs_verification_before_finish(&self) -> bool {
        self.mutated_files && !self.verified_after_mutation && !self.verification_nudge_sent
    }

    pub fn mark_verification_nudge_sent(&mut self) {
        self.verification_nudge_sent = true;
    }

    pub fn mark_verified(&mut self) {
        self.verified_after_mutation = true;
        self.verification_nudge_sent = false;
        self.failed_turns = 0;
        self.explore_only_turns = 0;
        self.explore_budget_nudge_sent = false;
        self.explore_hard_stop_sent = false;
    }

    pub fn explore_only_turns(&self) -> usize {
        self.explore_only_turns
    }

    pub fn mutated_files(&self) -> bool {
        self.mutated_files
    }

    pub fn plan_mode_turns(&self) -> usize {
        self.plan_mode_turns
    }
}

pub const CODING_EXPLORE_NUDGE: &str = "Coding progress guard: several recent tool turns failed \
without a successful result. Inspect the errors, change strategy (different path, query, or tool), \
or stop and report what is blocking you. Successful Read/Grep/Glob exploration is fine — keep going \
when those succeed.";

pub const CODING_EXPLORE_BUDGET_NUDGE: &str = "Coding explore budget: you have spent many turns only \
reading/searching without changing files. Either make the smallest Edit/Write that solves the \
task now, or stop with a short status. Do not continue an open-ended file tour — and do not \
re-Read files you already have in context.";

pub const CODING_EXPLORE_HARD_STOP: &str = "Coding explore hard-stop: exploration budget exhausted \
with no file changes. Stop now. Summarize findings and the blocker (or the smallest next edit). \
Do not call more Read/Grep/Glob/DirTree on this request unless the user asks again.";

pub const CODING_VERIFY_NUDGE: &str = "Coding verification gate: you changed files but have not \
yet run a build/test/lint command that exercises those changes. Run the narrowest verification \
now before claiming the task is done. If verification is impossible, say why and stop.";

pub const CODING_PLAN_TIMEOUT_NUDGE: &str = "Coding plan-mode budget: you have spent many turns \
exploring in plan mode. Finish the plan text and call ExitPlanMode now, or stop and ask the user \
a clarifying question. Do not keep touring the tree.";

pub const CODING_PLAN_HARD_STOP: &str = "Coding plan-mode hard-stop: plan mode ran too long without \
ExitPlanMode. Stop exploring. Either call ExitPlanMode with the best plan you have, or end with \
questions for the user. Do not continue a read-only tour.";

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ProgressObserveParams {
        ProgressObserveParams::default()
    }

    #[test]
    fn explore_budget_then_hard_stop() {
        let mut guard = CodingProgressGuard::default();
        let names: HashSet<String> = ["Read"].into_iter().map(str::to_owned).collect();
        let p = ProgressObserveParams {
            explore_budget: 2,
            explore_hard_stop: 4,
            ..params()
        };
        assert_eq!(
            guard.observe_tool_turn(&names, false, false, false, true, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            guard.observe_tool_turn(&names, false, false, false, true, p),
            CodingProgressAction::NudgeExploreBudget
        );
        assert_eq!(
            guard.observe_tool_turn(&names, false, false, false, true, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            guard.observe_tool_turn(&names, false, false, false, true, p),
            CodingProgressAction::HardStopExplore
        );
        assert!(guard.force_allow_finish());
    }

    #[test]
    fn mutation_clears_force_finish() {
        let mut guard = CodingProgressGuard::default();
        guard.mark_force_allow_finish();
        let edit: HashSet<String> = ["Edit"].into_iter().map(str::to_owned).collect();
        let _ = guard.observe_tool_turn(&edit, true, false, true, true, params());
        assert!(!guard.force_allow_finish());
    }

    #[test]
    fn plan_mode_hard_stop() {
        let mut g2 = CodingProgressGuard::default();
        assert_eq!(
            g2.observe_plan_mode_turn(true, 2, 3),
            CodingProgressAction::Continue
        );
        assert_eq!(
            g2.observe_plan_mode_turn(true, 2, 3),
            CodingProgressAction::NudgePlanTimeout
        );
        assert_eq!(
            g2.observe_plan_mode_turn(true, 2, 3),
            CodingProgressAction::HardStopPlanTimeout
        );
    }
}
