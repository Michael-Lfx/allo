//! Coding harness — completion policy layer for Allo coding sessions.
//!
//! Lives on the shared `nomi-agent` turn engine (no parallel runtime). Owns:
//! tool surface, prompts, verification, explore/plan hard stops, edit converge,
//! todo continuation, system-continuation budget, and compact preferences.

use crate::continuation::{ContinuationBudget, DEFAULT_MAX_SYSTEM_CONTINUATIONS};
use crate::edit_converge::{
    CODING_EDIT_CONVERGE_NUDGE, CODING_EDIT_HARD_STOP, EditConvergeAction, EditFailureTracker,
};
use crate::edit_hints::infer_edit_failure_kind;
use crate::env::CodingEnvContext;
use crate::profile::TaskProfile;
use crate::progress::{
    CODING_EXPLORE_BUDGET_NUDGE, CODING_EXPLORE_HARD_STOP, CODING_EXPLORE_NUDGE,
    CODING_PLAN_HARD_STOP, CODING_PLAN_TIMEOUT_NUDGE, CODING_VERIFY_NUDGE, CodingProgressAction,
    CodingProgressGuard, ProgressObserveParams,
};
use crate::prompt::coding_turn_tail;
use crate::read_repeat::{
    CODING_READ_REPEAT_HARD_STOP, CODING_READ_REPEAT_NUDGE, CODING_UNCHANGED_STUB_NUDGE,
    DEFAULT_READ_REPEAT_HARD, DEFAULT_READ_REPEAT_SOFT, ReadRepeatAction, ReadRepeatTracker,
};
use crate::todo_continuation::{
    parse_plan_update_content, TodoContinuationMode, TodoContinuationTracker,
};
use crate::tools::advertise_tool;
use crate::verify::{is_mutating_tool, looks_like_verification_command};

use serde::{Deserialize, Serialize};

/// How strictly coding mode enforces post-edit verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMode {
    /// After file mutations, optionally soft-hint (see `soft_verify_hint`).
    #[default]
    SoftHint,
    /// Block the first natural EndTurn after mutations until verify-like success
    /// (consumes the system continuation budget).
    HardGate,
    Off,
}

/// Tunables for the coding harness. Defaults bias toward finishing work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodingConfig {
    pub verification: VerificationMode,
    #[serde(default)]
    pub soft_verify_hint: bool,
    pub micro_keep_recent: usize,
    pub protect_read_results: bool,
    pub constitution_every_tool_turn: bool,
    #[serde(default = "default_explore_budget")]
    pub explore_budget: usize,
    #[serde(default = "default_explore_hard_stop")]
    pub explore_hard_stop: usize,
    #[serde(default = "default_edit_fail_converge")]
    pub edit_fail_converge: usize,
    /// Extra failures after soft converge before hard stop.
    #[serde(default = "default_edit_fail_hard_extra")]
    pub edit_fail_hard_extra: usize,
    /// Cap on system-driven continuations per root user request.
    #[serde(default = "default_max_system_continuations")]
    pub max_system_continuations: usize,
    /// When true (default), coding sessions skip fail-open goal auto-continue.
    #[serde(default = "default_true")]
    pub disable_goal_auto_continue: bool,
    #[serde(default)]
    pub todo_continuation: TodoContinuationMode,
    #[serde(default = "default_plan_mode_budget")]
    pub plan_mode_budget: usize,
    #[serde(default = "default_plan_mode_hard_stop")]
    pub plan_mode_hard_stop: usize,
    /// Soft nudge after this many successful Reads of the same path.
    #[serde(default = "default_read_repeat_soft")]
    pub read_repeat_soft: usize,
    /// Hard stop after this many successful Reads of the same path.
    #[serde(default = "default_read_repeat_hard")]
    pub read_repeat_hard: usize,
}

fn default_true() -> bool {
    true
}

fn default_explore_budget() -> usize {
    CodingProgressGuard::DEFAULT_EXPLORE_BUDGET
}

fn default_explore_hard_stop() -> usize {
    CodingProgressGuard::DEFAULT_EXPLORE_HARD_STOP
}

fn default_edit_fail_converge() -> usize {
    3
}

fn default_edit_fail_hard_extra() -> usize {
    2
}

fn default_max_system_continuations() -> usize {
    DEFAULT_MAX_SYSTEM_CONTINUATIONS
}

fn default_plan_mode_budget() -> usize {
    CodingProgressGuard::DEFAULT_PLAN_MODE_BUDGET
}

fn default_plan_mode_hard_stop() -> usize {
    CodingProgressGuard::DEFAULT_PLAN_MODE_HARD_STOP
}

fn default_read_repeat_soft() -> usize {
    DEFAULT_READ_REPEAT_SOFT
}

fn default_read_repeat_hard() -> usize {
    DEFAULT_READ_REPEAT_HARD
}

impl VerificationMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("hard_gate") | Some("hard") | Some("evidence") => Self::HardGate,
            Some("off") | Some("none") | Some("disabled") => Self::Off,
            Some("soft_hint") | Some("soft") | Some("hint") | None => Self::SoftHint,
            _ => Self::SoftHint,
        }
    }
}

impl Default for CodingConfig {
    fn default() -> Self {
        Self {
            verification: VerificationMode::SoftHint,
            soft_verify_hint: false,
            micro_keep_recent: 16,
            protect_read_results: true,
            constitution_every_tool_turn: false,
            explore_budget: default_explore_budget(),
            explore_hard_stop: default_explore_hard_stop(),
            edit_fail_converge: default_edit_fail_converge(),
            edit_fail_hard_extra: default_edit_fail_hard_extra(),
            max_system_continuations: default_max_system_continuations(),
            disable_goal_auto_continue: true,
            todo_continuation: TodoContinuationMode::Unlocked,
            plan_mode_budget: default_plan_mode_budget(),
            plan_mode_hard_stop: default_plan_mode_hard_stop(),
            read_repeat_soft: default_read_repeat_soft(),
            read_repeat_hard: default_read_repeat_hard(),
        }
    }
}

impl CodingConfig {
    pub fn from_host_extra(
        verification: Option<&str>,
        micro_keep_recent: Option<usize>,
        protect_read_results: Option<bool>,
        constitution_every_tool_turn: Option<bool>,
    ) -> Self {
        let mut cfg = Self::default();
        if verification.is_some() {
            cfg.verification = VerificationMode::parse(verification);
        }
        if let Some(n) = micro_keep_recent.filter(|n| *n > 0) {
            cfg.micro_keep_recent = n;
        }
        if let Some(v) = protect_read_results {
            cfg.protect_read_results = v;
        }
        if let Some(v) = constitution_every_tool_turn {
            cfg.constitution_every_tool_turn = v;
        }
        cfg
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactPolicyOverrides {
    pub micro_keep_recent: usize,
    pub exclude_compactable: Vec<&'static str>,
}

/// Decision at a natural `EndTurn` (no tool calls).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishDecision {
    Allow,
    /// Inject `nudge` and run another provider pass (counts toward budget).
    ContinueWithNudge { nudge: String },
}

/// Policy after a completed tool batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolTurnNudge {
    pub texts: Vec<String>,
    /// When set, the engine must stop the tool loop after appending texts and
    /// run one forced finalize provider turn (no tools) that ends as a normal
    /// `EndTurn` — never as [`nomi_agent::AgentError::Stagnation`].
    pub hard_stop: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolCallOutcome {
    pub name: String,
    pub success: bool,
    pub command: Option<String>,
    pub file_path: Option<String>,
    pub error_content: Option<String>,
    /// Full tool result body (used for `update_plan` snapshot parsing).
    pub result_content: Option<String>,
}

/// Stateful coding harness installed on an agent session.
#[derive(Debug)]
pub struct CodingHarness {
    config: CodingConfig,
    env: Option<CodingEnvContext>,
    progress: CodingProgressGuard,
    edit_failures: EditFailureTracker,
    todo: TodoContinuationTracker,
    continuations: ContinuationBudget,
    read_repeat: ReadRepeatTracker,
    constitution_sent_this_request: bool,
    /// When set, the next provider turn advertises no tools and must EndTurn.
    forced_finalize: Option<String>,
}

impl CodingHarness {
    pub fn new(env: Option<CodingEnvContext>, config: CodingConfig) -> Self {
        let continuations = ContinuationBudget::new(config.max_system_continuations);
        let todo = TodoContinuationTracker::new(config.todo_continuation);
        Self {
            config,
            env,
            progress: CodingProgressGuard::default(),
            edit_failures: EditFailureTracker::default(),
            todo,
            continuations,
            read_repeat: ReadRepeatTracker::default(),
            constitution_sent_this_request: false,
            forced_finalize: None,
        }
    }

    pub fn with_defaults(env: Option<CodingEnvContext>) -> Self {
        Self::new(env, CodingConfig::default())
    }

    pub fn env(&self) -> Option<&CodingEnvContext> {
        self.env.as_ref()
    }

    pub fn config(&self) -> &CodingConfig {
        &self.config
    }

    pub fn profile(&self) -> TaskProfile {
        TaskProfile::Coding
    }

    pub fn reset_for_user_request(&mut self) {
        self.progress.reset();
        self.edit_failures.reset();
        self.todo.reset();
        self.read_repeat.reset();
        self.continuations = ContinuationBudget::new(self.config.max_system_continuations);
        self.constitution_sent_this_request = false;
        self.forced_finalize = None;
    }

    pub fn reset_progress(&mut self) {
        self.progress.reset();
        self.edit_failures.reset();
        self.read_repeat.reset();
    }

    /// Schedule a graceful finish: one more provider pass with no tools.
    pub fn begin_forced_finalize(&mut self, reason: String) {
        if self.forced_finalize.is_none() {
            self.forced_finalize = Some(reason);
        }
    }

    pub fn is_forced_finalize(&self) -> bool {
        self.forced_finalize.is_some()
    }

    pub fn forced_finalize_reason(&self) -> Option<&str> {
        self.forced_finalize.as_deref()
    }

    pub fn take_forced_finalize(&mut self) -> Option<String> {
        self.forced_finalize.take()
    }

    pub fn set_env(&mut self, env: Option<CodingEnvContext>) {
        self.env = env;
    }

    pub fn advertise_tool(&self, tool_name: &str) -> bool {
        advertise_tool(TaskProfile::Coding, tool_name)
    }

    pub fn prefers_relaxed_error_cascade(&self) -> bool {
        true
    }

    /// Coding sessions should not use fail-open goal auto-continue by default.
    pub fn disables_goal_auto_continue(&self) -> bool {
        self.config.disable_goal_auto_continue
    }

    pub fn compact_overrides(&self) -> CompactPolicyOverrides {
        let mut exclude = Vec::new();
        if self.config.protect_read_results {
            exclude.push("Read");
        }
        CompactPolicyOverrides {
            micro_keep_recent: self.config.micro_keep_recent.max(1),
            exclude_compactable: exclude,
        }
    }

    /// Reinject after autocompact — preserve discipline without forcing a full re-tour.
    pub fn post_compact_reinject(&self) -> String {
        let mut out = String::from(
            "[Coding context restored after compaction]\n\
             Prior tool transcripts were summarized. Prefer facts already in the summary. \
             Re-Read a file **only when about to Edit** that file (to obtain fresh line:hash \
             anchors). Do not restart a workspace-wide Read/Grep tour.\n\n",
        );
        out.push_str(&coding_turn_tail(self.env.as_ref()));
        out
    }

    pub fn turn_tail(&mut self, last_user_has_text: bool) -> Option<String> {
        let inject = if self.config.constitution_every_tool_turn {
            true
        } else if last_user_has_text {
            !self.constitution_sent_this_request
        } else {
            false
        };
        if !inject {
            return None;
        }
        self.constitution_sent_this_request = true;
        Some(coding_turn_tail(self.env.as_ref()))
    }

    /// Observe whether plan mode is active this provider turn (before tools).
    pub fn before_provider_turn(&mut self, plan_mode_active: bool) -> Option<String> {
        match self.progress.observe_plan_mode_turn(
            plan_mode_active,
            self.config.plan_mode_budget,
            self.config.plan_mode_hard_stop,
        ) {
            CodingProgressAction::NudgePlanTimeout => Some(CODING_PLAN_TIMEOUT_NUDGE.to_string()),
            CodingProgressAction::HardStopPlanTimeout => Some(CODING_PLAN_HARD_STOP.to_string()),
            _ => None,
        }
    }

    /// When plan-mode hard-stop already fired, refuse further provider turns.
    pub fn abort_before_provider(&self) -> Option<&'static str> {
        if self.progress.force_allow_finish() && self.progress.plan_mode_turns() > 0 {
            // Distinguish explore hard-stop (also sets force_allow_finish) by
            // requiring an active plan-mode streak; explore hard-stop clears
            // plan turns when inactive.
            if self.progress.plan_mode_turns() >= self.config.plan_mode_hard_stop {
                return Some(CODING_PLAN_HARD_STOP);
            }
        }
        None
    }

    /// Natural EndTurn policy: verify gate → todo continuation → budget.
    pub fn on_natural_end(&mut self) -> FinishDecision {
        if self.progress.force_allow_finish() {
            self.progress.clear_force_allow_finish();
            return FinishDecision::Allow;
        }

        // HardGate verify (one continuation if budget remains).
        if matches!(self.config.verification, VerificationMode::HardGate)
            && self.progress.needs_verification_before_finish()
        {
            if self.continuations.try_consume() {
                self.progress.mark_verification_nudge_sent();
                return FinishDecision::ContinueWithNudge {
                    nudge: CODING_VERIFY_NUDGE.to_string(),
                };
            }
            // Budget exhausted — allow stop rather than looping forever.
            return FinishDecision::Allow;
        }

        // Todo / plan continuation (unlocked: one nudge per signature).
        if let Some(nudge) = self.todo.continuation_nudge() {
            if self.continuations.try_consume() {
                return FinishDecision::ContinueWithNudge { nudge };
            }
            // Budget exhausted — allow EndTurn (pending plan stays in last tool result).
            return FinishDecision::Allow;
        }

        FinishDecision::Allow
    }

    /// Observe a completed tool batch.
    pub fn after_tool_turn(&mut self, outcomes: &[ToolCallOutcome]) -> ToolTurnNudge {
        let mut tool_names = std::collections::HashSet::new();
        let mut had_successful_mutating_result = false;
        let mut had_successful_verification = false;
        let mut had_file_mutation = false;
        let mut had_any_successful_result = false;
        let mut edit_action = EditConvergeAction::None;
        let mut hard_stop: Option<String> = None;
        let mut texts: Vec<String> = Vec::new();

        for o in outcomes {
            tool_names.insert(o.name.clone());
            let flags = Self::classify_tool_success(&o.name, o.command.as_deref(), o.success);
            if flags.any {
                had_any_successful_result = true;
            }
            if flags.file_mutation {
                had_file_mutation = true;
            }
            if flags.mutating {
                had_successful_mutating_result = true;
            }
            if flags.verification {
                had_successful_verification = true;
            }

            if o.name.eq_ignore_ascii_case("update_plan") && o.success {
                if let Some(content) = o.result_content.as_deref() {
                    if let Some(snap) = parse_plan_update_content(content) {
                        self.todo.observe_plan(snap);
                    }
                }
            }

            if o.name.eq_ignore_ascii_case("Read") && o.success {
                match self.read_repeat.observe_read(
                    o.file_path.as_deref(),
                    o.result_content.as_deref(),
                    self.config.read_repeat_soft,
                    self.config.read_repeat_hard,
                ) {
                    ReadRepeatAction::None => {}
                    ReadRepeatAction::SoftNudge => {
                        texts.push(CODING_READ_REPEAT_NUDGE.to_string());
                    }
                    ReadRepeatAction::UnchangedStubNudge => {
                        texts.push(CODING_UNCHANGED_STUB_NUDGE.to_string());
                    }
                    ReadRepeatAction::HardStop => {
                        texts.push(CODING_READ_REPEAT_HARD_STOP.to_string());
                        hard_stop = Some(CODING_READ_REPEAT_HARD_STOP.to_string());
                        self.progress.mark_force_allow_finish();
                    }
                }
            }

            if matches!(o.name.as_str(), "Edit" | "Write" | "ApplyPatch") {
                let kind = if o.success {
                    None
                } else {
                    o.error_content
                        .as_deref()
                        .map(infer_edit_failure_kind)
                };
                let action = self.edit_failures.observe(
                    o.file_path.as_deref(),
                    o.success,
                    kind,
                    self.config.edit_fail_converge,
                    self.config.edit_fail_hard_extra,
                );
                if action != EditConvergeAction::None {
                    edit_action = action;
                }
            }
        }

        if had_file_mutation {
            // A successful edit means prior Reads served their purpose — reset
            // the repeat window so a later re-Read for a different file is fine.
            self.read_repeat.reset();
        }

        let action = self.progress.observe_tool_turn(
            &tool_names,
            had_successful_mutating_result,
            had_successful_verification,
            had_file_mutation,
            had_any_successful_result,
            ProgressObserveParams {
                failed_nudge_threshold: CodingProgressGuard::FAILED_NUDGE_THRESHOLD,
                explore_budget: self.config.explore_budget,
                explore_hard_stop: self.config.explore_hard_stop,
            },
        );

        match action {
            CodingProgressAction::Continue | CodingProgressAction::NudgeVerify => {}
            CodingProgressAction::NudgeExplore => texts.push(CODING_EXPLORE_NUDGE.to_string()),
            CodingProgressAction::NudgeExploreBudget => {
                texts.push(CODING_EXPLORE_BUDGET_NUDGE.to_string())
            }
            CodingProgressAction::HardStopExplore => {
                texts.push(CODING_EXPLORE_HARD_STOP.to_string());
                hard_stop = Some(CODING_EXPLORE_HARD_STOP.to_string());
            }
            CodingProgressAction::NudgePlanTimeout => {
                texts.push(CODING_PLAN_TIMEOUT_NUDGE.to_string())
            }
            CodingProgressAction::HardStopPlanTimeout => {
                texts.push(CODING_PLAN_HARD_STOP.to_string());
                hard_stop = Some(CODING_PLAN_HARD_STOP.to_string());
            }
        }

        match edit_action {
            EditConvergeAction::None => {}
            EditConvergeAction::SoftNudge => texts.push(CODING_EDIT_CONVERGE_NUDGE.to_string()),
            EditConvergeAction::HardStop => {
                texts.push(CODING_EDIT_HARD_STOP.to_string());
                hard_stop = Some(CODING_EDIT_HARD_STOP.to_string());
                self.progress.mark_force_allow_finish();
            }
        }

        if matches!(self.config.verification, VerificationMode::SoftHint)
            && self.config.soft_verify_hint
            && had_file_mutation
            && !had_successful_verification
            && self.progress.needs_verification_before_finish()
        {
            self.progress.mark_verification_nudge_sent();
            texts.push(CODING_VERIFY_NUDGE.to_string());
        }

        // Plan-mode hard stop may have been set in before_provider_turn; surface it.
        if hard_stop.is_none() && self.progress.force_allow_finish() {
            // Explore/edit already set hard_stop when escalating; plan timeout
            // from before_provider_turn only set the flag — check texts.
        }

        ToolTurnNudge { texts, hard_stop }
    }

    pub fn classify_tool_success(
        name: &str,
        command: Option<&str>,
        success: bool,
    ) -> ToolSuccessFlags {
        if !success {
            return ToolSuccessFlags::default();
        }
        let mut flags = ToolSuccessFlags {
            any: true,
            ..Default::default()
        };
        if matches!(name, "Edit" | "Write" | "ApplyPatch") {
            flags.file_mutation = true;
        }
        if is_mutating_tool(name) {
            flags.mutating = true;
        }
        if matches!(name, "Bash" | "exec_command")
            && command.is_some_and(looks_like_verification_command)
        {
            flags.verification = true;
        }
        flags
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolSuccessFlags {
    pub any: bool,
    pub mutating: bool,
    pub file_mutation: bool,
    pub verification: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_ok() -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Edit".into(),
            success: true,
            command: None,
            file_path: Some("a.rs".into()),
            error_content: None,
            result_content: None,
        }
    }

    fn read_ok() -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Read".into(),
            success: true,
            command: None,
            file_path: Some("a.rs".into()),
            error_content: None,
            result_content: None,
        }
    }

    fn edit_fail(path: &str, err: &str) -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Edit".into(),
            success: false,
            command: None,
            file_path: Some(path.into()),
            error_content: Some(err.into()),
            result_content: None,
        }
    }

    fn plan_ok(json: &str) -> ToolCallOutcome {
        ToolCallOutcome {
            name: "update_plan".into(),
            success: true,
            command: None,
            file_path: None,
            error_content: None,
            result_content: Some(json.into()),
        }
    }

    #[test]
    fn soft_hint_default_does_not_push_verify() {
        let mut h = CodingHarness::with_defaults(None);
        let nudge = h.after_tool_turn(&[edit_ok()]);
        assert!(!nudge.texts.iter().any(|t| t.contains("verification gate")));
        assert_eq!(h.on_natural_end(), FinishDecision::Allow);
    }

    #[test]
    fn read_repeat_hard_stops() {
        let mut h = CodingHarness::with_defaults(None);
        let read = |path: &str| ToolCallOutcome {
            name: "Read".into(),
            success: true,
            command: None,
            file_path: Some(path.into()),
            error_content: None,
            result_content: Some("1:abcd→body".into()),
        };
        let _ = h.after_tool_turn(&[read("src/a.rs")]);
        let soft = h.after_tool_turn(&[read("./src/a.rs")]);
        assert!(soft.texts.iter().any(|t| t.contains("read-repeat")));
        let hard = h.after_tool_turn(&[read("src/a.rs")]);
        assert!(hard.hard_stop.is_some());
    }

    #[test]
    fn unchanged_stub_nudges() {
        let mut h = CodingHarness::with_defaults(None);
        let o = ToolCallOutcome {
            name: "Read".into(),
            success: true,
            command: None,
            file_path: Some("a.rs".into()),
            error_content: None,
            result_content: Some(
                "File unchanged since last read. The content from the earlier Read".into(),
            ),
        };
        let nudge = h.after_tool_turn(&[o]);
        assert!(nudge.texts.iter().any(|t| t.contains("unchanged")));
    }

    #[test]
    fn hard_gate_blocks_once_within_budget() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                verification: VerificationMode::HardGate,
                ..Default::default()
            },
        );
        let _ = h.after_tool_turn(&[edit_ok()]);
        assert!(matches!(
            h.on_natural_end(),
            FinishDecision::ContinueWithNudge { .. }
        ));
        assert_eq!(h.on_natural_end(), FinishDecision::Allow);
    }

    #[test]
    fn explore_hard_stop_fires() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 2,
                explore_hard_stop: 3,
                ..Default::default()
            },
        );
        let _ = h.after_tool_turn(&[read_ok()]);
        let _ = h.after_tool_turn(&[read_ok()]);
        let last = h.after_tool_turn(&[read_ok()]);
        assert!(last.hard_stop.is_some());
    }

    #[test]
    fn todo_unlocked_continues_once() {
        let mut h = CodingHarness::with_defaults(None);
        let _ = h.after_tool_turn(&[plan_ok(
            r#"{"kind":"plan_update","entries":[{"content":"A","status":"pending"}]}"#,
        )]);
        assert!(matches!(
            h.on_natural_end(),
            FinishDecision::ContinueWithNudge { .. }
        ));
        assert_eq!(h.on_natural_end(), FinishDecision::Allow);
    }

    #[test]
    fn disables_goal_by_default() {
        let h = CodingHarness::with_defaults(None);
        assert!(h.disables_goal_auto_continue());
    }

    #[test]
    fn edit_fail_hard_stop() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                edit_fail_converge: 2,
                edit_fail_hard_extra: 1,
                ..Default::default()
            },
        );
        let err = "old_string not found";
        let _ = h.after_tool_turn(&[edit_fail("a.rs", err)]);
        let soft = h.after_tool_turn(&[edit_fail("a.rs", err)]);
        assert!(soft.texts.iter().any(|t| t.contains("edit converge")));
        let hard = h.after_tool_turn(&[edit_fail("a.rs", err)]);
        assert!(hard.hard_stop.is_some());
    }

    #[test]
    fn constitution_skipped_on_pure_tool_rounds() {
        let mut h = CodingHarness::with_defaults(None);
        assert!(h.turn_tail(true).is_some());
        assert!(h.turn_tail(true).is_none());
        assert!(h.turn_tail(false).is_none());
    }

    #[test]
    fn compact_reinject_avoids_full_retours() {
        let h = CodingHarness::with_defaults(None);
        let text = h.post_compact_reinject();
        assert!(text.contains("only when about to Edit"));
        assert!(!text.contains("Re-Read any file before editing; do not rely"));
    }
}
