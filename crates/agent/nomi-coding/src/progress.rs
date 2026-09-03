//! Progress / exploration / verification state for coding mode.
//!
//! Wall clock of a coding request is dominated by **provider round-trips**, not
//! tool wall time. A turn that only reads, greps, or runs non-verify shell is a
//! recon round even when those tools succeed. Soft nudges still exist, but
//! recon and edit-failure streaks escalate to **hard stops** so the agent
//! cannot tour forever.

use std::collections::HashSet;

use crate::verify::looks_like_verification_command;

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
            | "Lsp"
            | "SemanticSearch"
            | "web_search"
            | "web_extract"
    )
}

/// Recon for progress accounting: explore tools, plus Bash/exec that is **not**
/// a verification command. Isolated subagents are excluded (parent budget).
///
/// Successful `ls` / `git status` / `cat` is recon, not mutating progress.
/// File Edit/Write and verify-like shell are the only things that clear a
/// consecutive tour streak.
pub fn is_recon_tool(name: &str, command: Option<&str>) -> bool {
    if crate::verify::is_isolated_subagent_tool(name) {
        return false;
    }
    if is_explore_tool(name) {
        return true;
    }
    if matches!(name, "Bash" | "exec_command") {
        return !command.is_some_and(looks_like_verification_command);
    }
    false
}

/// Which recon budget fired. Used so harness copy matches the cost model:
/// consecutive tour, serial 1-tool round-trips, or request-lifetime recon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploreBudgetKind {
    ConsecutiveTour,
    SerialRoundTrip,
    LifetimeRecon,
}

/// Tracks coding-turn productivity for both soft nudges and hard finish gates.
#[derive(Debug, Default)]
pub struct CodingProgressGuard {
    /// Consecutive turns where every tool call failed (or nothing ran).
    failed_turns: usize,
    /// Consecutive recon-only turns (resets on file mutation, verify, or a
    /// non-recon parent tool). Bash without verify counts as recon.
    explore_only_turns: usize,
    /// Recon-only turns this user request. Does **not** reset on Edit.
    recon_turns_total: usize,
    /// Consecutive recon-only turns that issued exactly one parent tool.
    serial_recon_turns: usize,
    /// True once this request mutated files via Edit/Write/ApplyPatch.
    mutated_files: bool,
    /// True once a verification-like command succeeded after a mutation.
    verified_after_mutation: bool,
    /// True after we already injected a verify nudge for the current mutation.
    verification_nudge_sent: bool,
    /// True after we already injected a consecutive-tour soft nudge this streak.
    explore_budget_nudge_sent: bool,
    /// True once consecutive-tour hard-stop has been requested this streak.
    explore_hard_stop_sent: bool,
    serial_nudge_sent: bool,
    serial_hard_stop_sent: bool,
    lifetime_nudge_sent: bool,
    lifetime_hard_stop_sent: bool,
    /// Turns spent in plan mode without exiting (coding sessions).
    plan_mode_turns: usize,
    plan_budget_nudge_sent: bool,
    /// When set, the next natural EndTurn must allow finish (no continuations).
    force_allow_finish: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingProgressAction {
    Continue,
    /// Soft hint only (failed tool streak).
    NudgeExplore,
    /// A recon budget is exhausted — act, batch, verify, or stop.
    NudgeExploreBudget(ExploreBudgetKind),
    /// A recon hard budget is exhausted — engine must stop after this turn.
    HardStopExplore(ExploreBudgetKind),
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
    /// Soft consecutive recon-only nudge.
    pub explore_budget: usize,
    /// Hard stop after this many consecutive recon-only turns.
    pub explore_hard_stop: usize,
    /// Soft request-lifetime recon-round nudge (does not reset on Edit).
    pub recon_lifetime_budget: usize,
    /// Hard request-lifetime recon-round stop.
    pub recon_lifetime_hard_stop: usize,
    /// Soft consecutive 1-tool recon nudge (provider round-trip tax).
    pub serial_recon_budget: usize,
    /// Hard stop after this many consecutive 1-tool recon turns.
    pub serial_recon_hard_stop: usize,
}

impl Default for ProgressObserveParams {
    fn default() -> Self {
        Self {
            failed_nudge_threshold: CodingProgressGuard::FAILED_NUDGE_THRESHOLD,
            explore_budget: CodingProgressGuard::DEFAULT_EXPLORE_BUDGET,
            explore_hard_stop: CodingProgressGuard::DEFAULT_EXPLORE_HARD_STOP,
            recon_lifetime_budget: CodingProgressGuard::DEFAULT_RECON_LIFETIME_BUDGET,
            recon_lifetime_hard_stop: CodingProgressGuard::DEFAULT_RECON_LIFETIME_HARD_STOP,
            serial_recon_budget: CodingProgressGuard::DEFAULT_SERIAL_RECON_BUDGET,
            serial_recon_hard_stop: CodingProgressGuard::DEFAULT_SERIAL_RECON_HARD_STOP,
        }
    }
}

impl CodingProgressGuard {
    pub const FAILED_NUDGE_THRESHOLD: usize = 8;
    /// Soft consecutive recon nudge — keep low so simple tasks cannot tour forever.
    pub const DEFAULT_EXPLORE_BUDGET: usize = 6;
    /// Hard stop shortly after consecutive soft budget (then forced finalize).
    pub const DEFAULT_EXPLORE_HARD_STOP: usize = 10;
    /// Soft request-lifetime recon rounds (survives Edit; resets on new user request).
    pub const DEFAULT_RECON_LIFETIME_BUDGET: usize = 10;
    pub const DEFAULT_RECON_LIFETIME_HARD_STOP: usize = 16;
    /// Soft 1-tool recon streak. Each such turn is a full provider RTT.
    pub const DEFAULT_SERIAL_RECON_BUDGET: usize = 3;
    pub const DEFAULT_SERIAL_RECON_HARD_STOP: usize = 6;
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
        had_successful_verification: bool,
        had_file_mutation: bool,
        had_any_successful_result: bool,
        recon_only: bool,
        tool_count: usize,
        params: ProgressObserveParams,
    ) -> CodingProgressAction {
        if had_file_mutation {
            self.mutated_files = true;
            self.verified_after_mutation = false;
            self.verification_nudge_sent = false;
            self.clear_consecutive_recon_streak();
            self.force_allow_finish = false;
        }
        if had_successful_verification {
            self.verified_after_mutation = true;
            self.verification_nudge_sent = false;
            self.failed_turns = 0;
            self.clear_consecutive_recon_streak();
            self.force_allow_finish = false;
            return CodingProgressAction::Continue;
        }

        if had_any_successful_result {
            self.failed_turns = 0;
            // Isolated explore/verify/research loops do not count toward the
            // parent tour budget and must not reset an in-progress tour streak.
            if !tool_names.is_empty()
                && tool_names
                    .iter()
                    .all(|n| crate::verify::is_isolated_subagent_tool(n))
            {
                return CodingProgressAction::Continue;
            }
            if recon_only {
                return self.observe_recon_only_turn(tool_count, params);
            }
            self.clear_consecutive_recon_streak();
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

    fn clear_consecutive_recon_streak(&mut self) {
        self.explore_only_turns = 0;
        self.serial_recon_turns = 0;
        self.explore_budget_nudge_sent = false;
        self.explore_hard_stop_sent = false;
        self.serial_nudge_sent = false;
        self.serial_hard_stop_sent = false;
    }

    fn observe_recon_only_turn(
        &mut self,
        tool_count: usize,
        params: ProgressObserveParams,
    ) -> CodingProgressAction {
        self.explore_only_turns = self.explore_only_turns.saturating_add(1);
        self.recon_turns_total = self.recon_turns_total.saturating_add(1);
        if tool_count == 1 {
            self.serial_recon_turns = self.serial_recon_turns.saturating_add(1);
        } else {
            self.serial_recon_turns = 0;
            self.serial_nudge_sent = false;
            self.serial_hard_stop_sent = false;
        }

        let consecutive_soft = params.explore_budget.max(1);
        let consecutive_hard = params.explore_hard_stop.max(consecutive_soft);
        let lifetime_soft = params.recon_lifetime_budget.max(1);
        let lifetime_hard = params.recon_lifetime_hard_stop.max(lifetime_soft);
        let serial_soft = params.serial_recon_budget.max(1);
        let serial_hard = params.serial_recon_hard_stop.max(serial_soft);

        // Hard stops first. Serial is the round-trip tax; consecutive is a
        // no-edit tour; lifetime survives interleaved Edit+Read.
        if self.serial_recon_turns >= serial_hard && !self.serial_hard_stop_sent {
            self.serial_hard_stop_sent = true;
            self.force_allow_finish = true;
            return CodingProgressAction::HardStopExplore(ExploreBudgetKind::SerialRoundTrip);
        }
        if self.explore_only_turns >= consecutive_hard && !self.explore_hard_stop_sent {
            self.explore_hard_stop_sent = true;
            self.force_allow_finish = true;
            return CodingProgressAction::HardStopExplore(ExploreBudgetKind::ConsecutiveTour);
        }
        if self.recon_turns_total >= lifetime_hard && !self.lifetime_hard_stop_sent {
            self.lifetime_hard_stop_sent = true;
            self.force_allow_finish = true;
            return CodingProgressAction::HardStopExplore(ExploreBudgetKind::LifetimeRecon);
        }

        if self.serial_recon_turns >= serial_soft && !self.serial_nudge_sent {
            self.serial_nudge_sent = true;
            return CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::SerialRoundTrip);
        }
        if self.explore_only_turns >= consecutive_soft && !self.explore_budget_nudge_sent {
            self.explore_budget_nudge_sent = true;
            return CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::ConsecutiveTour);
        }
        if self.recon_turns_total >= lifetime_soft && !self.lifetime_nudge_sent {
            self.lifetime_nudge_sent = true;
            return CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::LifetimeRecon);
        }
        CodingProgressAction::Continue
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
        self.clear_consecutive_recon_streak();
    }

    pub fn explore_only_turns(&self) -> usize {
        self.explore_only_turns
    }

    pub fn recon_turns_total(&self) -> usize {
        self.recon_turns_total
    }

    pub fn serial_recon_turns(&self) -> usize {
        self.serial_recon_turns
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

pub const CODING_SERIAL_RECON_NUDGE: &str = "Coding round-trip tax: several consecutive assistant \
messages issued only one recon tool (Read/Grep/Glob/DirTree or non-verify Bash). Independent \
lookups belong in the **same** assistant message. Batch the remaining reads now, or Edit, or stop. \
Do not keep paying a full model round-trip per file.";

pub const CODING_SERIAL_RECON_HARD_STOP: &str = "Coding round-trip hard-stop: too many consecutive \
one-tool recon turns. Stop now. Summarize what you already know. If more files are needed, the \
user can ask again — do not continue a serial Read/Grep/Bash tour.";

pub const CODING_LIFETIME_RECON_NUDGE: &str = "Coding recon lifetime: this user request has already \
spent many provider rounds on Read/Grep/Glob/non-verify Bash (edits do not reset this budget). \
Make the remaining change, run one verify command, or stop with status. Do not start another tour.";

pub const CODING_LIFETIME_RECON_HARD_STOP: &str = "Coding recon lifetime hard-stop: recon round \
budget for this request is exhausted. Stop now. Summarize findings and any remaining blocker. \
Do not call more Read/Grep/Glob/DirTree or non-verify Bash unless the user asks again.";

pub const CODING_VERIFY_NUDGE: &str = "Coding verification gate: you changed files but have not \
yet run a build/test/lint command that exercises those changes. Run the narrowest verification \
now before claiming the task is done. If verification is impossible, say why and stop.";

pub const CODING_PLAN_TIMEOUT_NUDGE: &str = "Coding plan-mode budget: you have spent many turns \
exploring in plan mode. Finish the plan text and call ExitPlanMode now, or stop and ask the user \
a clarifying question. Do not keep touring the tree.";

pub const CODING_PLAN_HARD_STOP: &str = "Coding plan-mode hard-stop: plan mode ran too long without \
ExitPlanMode. Stop exploring. Either call ExitPlanMode with the best plan you have, or end with \
questions for the user. Do not continue a read-only tour.";

pub fn explore_budget_nudge_text(kind: ExploreBudgetKind) -> &'static str {
    match kind {
        ExploreBudgetKind::ConsecutiveTour => CODING_EXPLORE_BUDGET_NUDGE,
        ExploreBudgetKind::SerialRoundTrip => CODING_SERIAL_RECON_NUDGE,
        ExploreBudgetKind::LifetimeRecon => CODING_LIFETIME_RECON_NUDGE,
    }
}

pub fn explore_hard_stop_text(kind: ExploreBudgetKind) -> &'static str {
    match kind {
        ExploreBudgetKind::ConsecutiveTour => CODING_EXPLORE_HARD_STOP,
        ExploreBudgetKind::SerialRoundTrip => CODING_SERIAL_RECON_HARD_STOP,
        ExploreBudgetKind::LifetimeRecon => CODING_LIFETIME_RECON_HARD_STOP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ProgressObserveParams {
        ProgressObserveParams::default()
    }

    fn observe(
        guard: &mut CodingProgressGuard,
        names: &HashSet<String>,
        recon_only: bool,
        tool_count: usize,
        p: ProgressObserveParams,
    ) -> CodingProgressAction {
        guard.observe_tool_turn(names, false, false, true, recon_only, tool_count, p)
    }

    #[test]
    fn explore_budget_then_hard_stop() {
        let mut guard = CodingProgressGuard::default();
        let names: HashSet<String> = ["Read"].into_iter().map(str::to_owned).collect();
        let p = ProgressObserveParams {
            explore_budget: 2,
            explore_hard_stop: 4,
            serial_recon_budget: 20,
            serial_recon_hard_stop: 20,
            recon_lifetime_budget: 20,
            recon_lifetime_hard_stop: 20,
            ..params()
        };
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::ConsecutiveTour)
        );
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::HardStopExplore(ExploreBudgetKind::ConsecutiveTour)
        );
        assert!(guard.force_allow_finish());
    }

    #[test]
    fn bash_without_verify_is_recon_not_progress() {
        let mut guard = CodingProgressGuard::default();
        let bash: HashSet<String> = ["Bash"].into_iter().map(str::to_owned).collect();
        let p = ProgressObserveParams {
            explore_budget: 2,
            explore_hard_stop: 3,
            serial_recon_budget: 20,
            serial_recon_hard_stop: 20,
            recon_lifetime_budget: 20,
            recon_lifetime_hard_stop: 20,
            ..params()
        };
        assert_eq!(
            observe(&mut guard, &bash, true, 1, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &bash, true, 1, p),
            CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::ConsecutiveTour)
        );
        assert_eq!(
            observe(&mut guard, &bash, true, 1, p),
            CodingProgressAction::HardStopExplore(ExploreBudgetKind::ConsecutiveTour)
        );
        assert_eq!(guard.explore_only_turns(), 3);
    }

    #[test]
    fn edit_does_not_reset_lifetime_recon() {
        let mut guard = CodingProgressGuard::default();
        let read: HashSet<String> = ["Read"].into_iter().map(str::to_owned).collect();
        let edit: HashSet<String> = ["Edit"].into_iter().map(str::to_owned).collect();
        let p = ProgressObserveParams {
            explore_budget: 20,
            explore_hard_stop: 20,
            serial_recon_budget: 20,
            serial_recon_hard_stop: 20,
            recon_lifetime_budget: 3,
            recon_lifetime_hard_stop: 4,
            ..params()
        };
        assert_eq!(
            observe(&mut guard, &read, true, 1, p),
            CodingProgressAction::Continue
        );
        let _ = guard.observe_tool_turn(&edit, false, true, true, false, 1, p);
        assert_eq!(guard.recon_turns_total(), 1);
        assert_eq!(guard.explore_only_turns(), 0);
        assert_eq!(
            observe(&mut guard, &read, true, 1, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &read, true, 1, p),
            CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::LifetimeRecon)
        );
        assert_eq!(
            observe(&mut guard, &read, true, 1, p),
            CodingProgressAction::HardStopExplore(ExploreBudgetKind::LifetimeRecon)
        );
        assert_eq!(guard.recon_turns_total(), 4);
    }

    #[test]
    fn serial_one_tool_recon_nudge_then_hard_stop() {
        let mut guard = CodingProgressGuard::default();
        let names: HashSet<String> = ["Read"].into_iter().map(str::to_owned).collect();
        let p = ProgressObserveParams {
            explore_budget: 20,
            explore_hard_stop: 20,
            serial_recon_budget: 2,
            serial_recon_hard_stop: 3,
            recon_lifetime_budget: 20,
            recon_lifetime_hard_stop: 20,
            ..params()
        };
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::NudgeExploreBudget(ExploreBudgetKind::SerialRoundTrip)
        );
        assert_eq!(
            observe(&mut guard, &names, true, 1, p),
            CodingProgressAction::HardStopExplore(ExploreBudgetKind::SerialRoundTrip)
        );
    }

    #[test]
    fn batched_recon_resets_serial_streak() {
        let mut guard = CodingProgressGuard::default();
        let names: HashSet<String> = ["Read", "Grep"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let p = ProgressObserveParams {
            explore_budget: 20,
            explore_hard_stop: 20,
            serial_recon_budget: 2,
            serial_recon_hard_stop: 2,
            recon_lifetime_budget: 20,
            recon_lifetime_hard_stop: 20,
            ..params()
        };
        assert_eq!(
            observe(&mut guard, &names, true, 2, p),
            CodingProgressAction::Continue
        );
        assert_eq!(
            observe(&mut guard, &names, true, 2, p),
            CodingProgressAction::Continue
        );
        assert_eq!(guard.serial_recon_turns(), 0);
        assert_eq!(guard.explore_only_turns(), 2);
    }

    #[test]
    fn verify_command_is_not_recon_tool() {
        assert!(is_recon_tool("Bash", Some("ls -la")));
        assert!(is_recon_tool("Bash", Some("git status")));
        assert!(!is_recon_tool("Bash", Some("cargo test -p foo")));
        assert!(!is_recon_tool("Edit", None));
        assert!(is_recon_tool("Lsp", None));
        assert!(!is_recon_tool("explore_code", None));
    }

    #[test]
    fn mutation_clears_force_finish() {
        let mut guard = CodingProgressGuard::default();
        guard.mark_force_allow_finish();
        let edit: HashSet<String> = ["Edit"].into_iter().map(str::to_owned).collect();
        let _ = guard.observe_tool_turn(&edit, false, true, true, false, 1, params());
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
