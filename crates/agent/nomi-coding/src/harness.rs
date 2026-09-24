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
    CODING_EXPLORE_NUDGE, CODING_PLAN_HARD_STOP, CODING_PLAN_TIMEOUT_NUDGE, CODING_STALE_PLAN_NUDGE,
    CODING_VERIFY_NUDGE, PLAN_STALE_MUTATION_THRESHOLD, CodingProgressAction, CodingProgressGuard,
    ProgressObserveParams, explore_budget_nudge_text, explore_hard_stop_text, is_recon_tool,
};
use crate::read_repeat::{
    CODING_READ_REPEAT_HARD_STOP, CODING_READ_REPEAT_NUDGE, CODING_UNCHANGED_STUB_NUDGE,
    DEFAULT_READ_REPEAT_HARD, DEFAULT_READ_REPEAT_SOFT, ReadRepeatAction, ReadRepeatTracker,
};
use crate::todo_continuation::{
    parse_plan_update_content, PlanSnapshot, TodoContinuationMode, TodoContinuationTracker,
};
use crate::tools::advertise_tool;
use crate::verify::{is_mutating_tool, looks_like_verification_command};

use crate::failure::failure_nudge_if_useful;
use crate::metrics::HarnessKpi;
use crate::plan_artifact::PlanArtifact;
use crate::working_set::WorkingSet;
use serde::{Deserialize, Serialize};

/// How strictly coding mode enforces post-edit verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMode {
    /// After file mutations, optionally soft-hint (see `soft_verify_hint`).
    SoftHint,
    /// Block the first natural EndTurn after mutations until verify-like success
    /// (consumes the system continuation budget). Default for coding (EvidenceRequired).
    #[default]
    HardGate,
    Off,
}

/// Tunables for the coding harness. Defaults bias toward finishing work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodingConfig {
    #[serde(default)]
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
    #[serde(default = "default_recon_lifetime_budget")]
    pub recon_lifetime_budget: usize,
    #[serde(default = "default_recon_lifetime_hard_stop")]
    pub recon_lifetime_hard_stop: usize,
    #[serde(default = "default_serial_recon_budget")]
    pub serial_recon_budget: usize,
    #[serde(default = "default_serial_recon_hard_stop")]
    pub serial_recon_hard_stop: usize,
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
    /// Hard stop after this many successful Reads of the same path+range.
    #[serde(default = "default_read_repeat_hard")]
    pub read_repeat_hard: usize,
    /// Cap on failed verify-like Bash/exec retries after a mutation (Codex: 3).
    #[serde(default = "default_verify_retry_cap")]
    pub verify_retry_cap: usize,
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

fn default_recon_lifetime_budget() -> usize {
    CodingProgressGuard::DEFAULT_RECON_LIFETIME_BUDGET
}

fn default_recon_lifetime_hard_stop() -> usize {
    CodingProgressGuard::DEFAULT_RECON_LIFETIME_HARD_STOP
}

fn default_serial_recon_budget() -> usize {
    CodingProgressGuard::DEFAULT_SERIAL_RECON_BUDGET
}

fn default_serial_recon_hard_stop() -> usize {
    CodingProgressGuard::DEFAULT_SERIAL_RECON_HARD_STOP
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

fn default_verify_retry_cap() -> usize {
    3
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
            verification: VerificationMode::HardGate,
            soft_verify_hint: false,
            micro_keep_recent: 16,
            protect_read_results: true,
            constitution_every_tool_turn: false,
            explore_budget: default_explore_budget(),
            explore_hard_stop: default_explore_hard_stop(),
            recon_lifetime_budget: default_recon_lifetime_budget(),
            recon_lifetime_hard_stop: default_recon_lifetime_hard_stop(),
            serial_recon_budget: default_serial_recon_budget(),
            serial_recon_hard_stop: default_serial_recon_hard_stop(),
            edit_fail_converge: default_edit_fail_converge(),
            edit_fail_hard_extra: default_edit_fail_hard_extra(),
            max_system_continuations: default_max_system_continuations(),
            disable_goal_auto_continue: true,
            todo_continuation: TodoContinuationMode::Unlocked,
            plan_mode_budget: default_plan_mode_budget(),
            plan_mode_hard_stop: default_plan_mode_hard_stop(),
            read_repeat_soft: default_read_repeat_soft(),
            read_repeat_hard: default_read_repeat_hard(),
            verify_retry_cap: default_verify_retry_cap(),
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

/// Policy stop after an explore/edit/plan-mode hard-stop.
#[derive(Debug, Clone)]
enum ForcedFinalize {
    /// Last chance to declare plan progress. Only `update_plan` is advertised.
    SyncPlan { reason: String },
    /// No tools; write the user-facing reply.
    Reply { reason: String },
}

impl ForcedFinalize {
    fn reason(&self) -> &str {
        match self {
            Self::SyncPlan { reason } | Self::Reply { reason } => reason,
        }
    }
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
    /// run forced finalize: `SyncPlan` (keep `update_plan`) then `Reply` (no
    /// tools), ending as a normal `EndTurn` — never as
    /// [`nomi_agent::AgentError::Stagnation`].
    pub hard_stop: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCallOutcome {
    pub name: String,
    pub success: bool,
    pub command: Option<String>,
    pub file_path: Option<String>,
    pub error_content: Option<String>,
    /// Full tool result body (used for `update_plan` snapshot parsing).
    pub result_content: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
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
    working_set: WorkingSet,
    kpi: HarnessKpi,
    plan_artifact: Option<PlanArtifact>,
    verify_fail_streak: usize,
    trivial_mutation: bool,
    constitution_sent_this_request: bool,
    /// When set, the next provider turn is a policy stop. `SyncPlan` still
    /// advertises `update_plan`; `Reply` advertises no tools.
    forced_finalize: Option<ForcedFinalize>,
    /// Successful file mutations since the last accepted `update_plan`.
    mutations_since_plan: usize,
    /// One-shot latch so a stale checklist nudges once until the model snapshots again.
    stale_plan_nudge_sent: bool,
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
            working_set: WorkingSet::default(),
            kpi: HarnessKpi::default(),
            plan_artifact: None,
            verify_fail_streak: 0,
            trivial_mutation: false,
            constitution_sent_this_request: false,
            forced_finalize: None,
            mutations_since_plan: 0,
            stale_plan_nudge_sent: false,
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
        self.working_set.reset();
        self.kpi.reset_for_user_request();
        self.plan_artifact = None;
        self.verify_fail_streak = 0;
        self.trivial_mutation = false;
        self.continuations = ContinuationBudget::new(self.config.max_system_continuations);
        self.constitution_sent_this_request = false;
        self.forced_finalize = None;
        self.mutations_since_plan = 0;
        self.stale_plan_nudge_sent = false;
    }

    pub fn reset_progress(&mut self) {
        self.progress.reset();
        self.edit_failures.reset();
        self.read_repeat.reset();
        self.working_set.reset();
        self.verify_fail_streak = 0;
        self.trivial_mutation = false;
    }

    /// Schedule a graceful finish.
    ///
    /// If the last accepted plan is still incomplete, the first pass keeps
    /// `update_plan` so the model can declare what actually happened. The
    /// host never invents completions.
    pub fn begin_forced_finalize(&mut self, reason: String) {
        if self.forced_finalize.is_some() {
            return;
        }
        self.forced_finalize = Some(if self.remaining_plan().is_some() {
            ForcedFinalize::SyncPlan { reason }
        } else {
            ForcedFinalize::Reply { reason }
        });
    }

    pub fn is_forced_finalize(&self) -> bool {
        self.forced_finalize.is_some()
    }

    /// Whether this finalize pass still advertises `update_plan`.
    pub fn allows_update_plan_on_finalize(&self) -> bool {
        matches!(self.forced_finalize, Some(ForcedFinalize::SyncPlan { .. }))
    }

    /// Drop every tool name except `update_plan` during the plan-sync pass.
    pub fn ignores_finalize_tool(&self, name: &str) -> bool {
        match &self.forced_finalize {
            None => false,
            Some(ForcedFinalize::Reply { .. }) => true,
            Some(ForcedFinalize::SyncPlan { .. }) => !name.eq_ignore_ascii_case("update_plan"),
        }
    }

    /// Plan-sync pass is done (the model called `update_plan`, or it was not
    /// advertised). Later provider turns advertise no tools.
    pub fn advance_after_plan_sync(&mut self) {
        if let Some(ForcedFinalize::SyncPlan { reason }) = self.forced_finalize.take() {
            self.forced_finalize = Some(ForcedFinalize::Reply { reason });
        }
    }

    pub fn forced_finalize_reason(&self) -> Option<&str> {
        self.forced_finalize.as_ref().map(ForcedFinalize::reason)
    }

    pub fn take_forced_finalize(&mut self) -> Option<String> {
        match self.forced_finalize.take() {
            Some(ForcedFinalize::Reply { reason }) => Some(reason),
            other => {
                // SyncPlan must still execute `update_plan`; consuming it here
                // would skip the snapshot and leave the UI checklist stale.
                self.forced_finalize = other;
                None
            }
        }
    }

    /// Last accepted plan snapshot if it still has uncompleted steps.
    ///
    /// Injected into the closing instruction. The SyncPlan pass still lets the
    /// model declare progress; Reply must not invent completions.
    pub fn remaining_plan(&self) -> Option<PlanSnapshot> {
        let latest = self.todo.latest();
        if latest.is_empty() || latest.all_completed() {
            None
        } else {
            Some(latest.clone())
        }
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
        // Protect WorkingSet index + last Edit anchors via post_compact_reinject,
        // not by keeping unbounded Read dumps out of microcompact.
        CompactPolicyOverrides {
            micro_keep_recent: self.config.micro_keep_recent.max(1),
            exclude_compactable: Vec::new(),
        }
    }

    /// Reinject after autocompact — preserve discipline without forcing a full re-tour.
    pub fn post_compact_reinject(&self) -> String {
        let mut out = String::from(
            "[Coding context restored after compaction]\n\
             Prior tool transcripts were summarized. Prefer facts already in the summary and the WorkingSet. \
             Re-Read **only uncovered ranges** when about to Edit. Do not restart a workspace-wide tour.\n\n",
        );
        out.push_str(&self.working_set.index_block());
        out.push('\n');
        if let Some(plan) = &self.plan_artifact {
            out.push('\n');
            out.push_str(&plan.summary_block());
        }
        if let Some(block) = self.env.as_ref().and_then(crate::env::format_env_context) {
            out.push_str("\n\n");
            out.push_str(&block);
        }
        out
    }

    /// Env + WorkingSet index — constitution lives on the cache-stable system prompt.
    pub fn turn_tail(&mut self, last_user_has_text: bool) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if self.config.constitution_every_tool_turn {
            parts.push(crate::prompt::coding_overlay_instructions().to_string());
        }
        if last_user_has_text && !self.constitution_sent_this_request {
            self.constitution_sent_this_request = true;
        }
        if let Some(block) = self.env.as_ref().and_then(crate::env::format_env_context) {
            parts.push(block);
        }
        if !self.working_set.is_empty() {
            parts.push(self.working_set.index_block());
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
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

    pub fn kpi(&self) -> &HarnessKpi {
        &self.kpi
    }

    pub fn kpi_mut(&mut self) -> &mut HarnessKpi {
        &mut self.kpi
    }

    pub fn working_set(&self) -> &WorkingSet {
        &self.working_set
    }

    pub fn plan_artifact(&self) -> Option<&PlanArtifact> {
        self.plan_artifact.as_ref()
    }

    /// Natural EndTurn policy: verify gate → todo continuation → budget.
    pub fn on_natural_end(&mut self) -> FinishDecision {
        if self.progress.force_allow_finish() {
            self.progress.clear_force_allow_finish();
            return FinishDecision::Allow;
        }

        // Narrow gate: single trivial text/config file, model already explained skip.
        if self.trivial_mutation && self.progress.needs_verification_before_finish() {
            self.kpi.verify_before_end = false;
            return FinishDecision::Allow;
        }

        // EvidenceRequired (HardGate) verify (one continuation if budget remains).
        if matches!(self.config.verification, VerificationMode::HardGate)
            && self.progress.needs_verification_before_finish()
        {
            if self.continuations.try_consume() {
                self.progress.mark_verification_nudge_sent();
                return FinishDecision::ContinueWithNudge {
                    nudge: CODING_VERIFY_NUDGE.to_string(),
                };
            }
            // Budget exhausted — do not loop; EndTurn is incomplete (KPI flag).
            self.kpi.verify_before_end = false;
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
        let mut had_successful_verification = false;
        let mut had_file_mutation = false;
        let mut had_any_successful_result = false;
        let mut had_plan_update = false;
        let mut mutation_count = 0usize;
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
                mutation_count = mutation_count.saturating_add(1);
            }
            if flags.verification {
                had_successful_verification = true;
            }

            if o.name.eq_ignore_ascii_case("update_plan") && o.success {
                if let Some(content) = o.result_content.as_deref() {
                    if let Some(snap) = parse_plan_update_content(content) {
                        self.todo.observe_plan(snap);
                        had_plan_update = true;
                    }
                }
            }

            if o.name.eq_ignore_ascii_case("ExitPlanMode") && o.success {
                if let Some(content) = o.result_content.as_deref() {
                    self.plan_artifact = Some(PlanArtifact::from_exit_content(content));
                }
            }

            if !o.success {
                if let Some(err) = o.error_content.as_deref()
                    && let Some(nudge) = failure_nudge_if_useful(&o.name, err)
                {
                    texts.push(nudge);
                }
                if matches!(o.name.as_str(), "Bash" | "exec_command")
                    && o.command.as_deref().is_some_and(looks_like_verification_command)
                {
                    self.verify_fail_streak = self.verify_fail_streak.saturating_add(1);
                    if self.verify_fail_streak >= self.config.verify_retry_cap {
                        hard_stop = Some(format!(
                            "Coding verify-retry cap ({}): stop retrying the same test/format command. Report the failure.",
                            self.config.verify_retry_cap
                        ));
                        self.progress.mark_force_allow_finish();
                    }
                }
            }

            if o.name.eq_ignore_ascii_case("Read") && o.success {
                if let Some(path) = o.file_path.as_deref() {
                    let offset = o.offset.unwrap_or(0);
                    let limit = o.limit.unwrap_or(usize::MAX / 4);
                    self.working_set.record_read(path, offset, limit.min(10_000), 10_000, None);
                    self.kpi.observe_read_key(&format!(
                        "{}#{}:{}",
                        path,
                        offset,
                        o.limit.map(|n| n.to_string()).unwrap_or_else(|| "end".into())
                    ));
                }
                match self.read_repeat.observe_read_range(
                    o.file_path.as_deref(),
                    o.result_content.as_deref(),
                    o.offset,
                    o.limit,
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
                if o.success {
                    self.kpi.observe_edit();
                    if let Some(path) = o.file_path.as_deref() {
                        self.working_set.record_edit(path, None);
                    }
                }
            }
        }

        self.kpi.observe_assistant_tools(outcomes.len());
        let (recon_only, parent_tool_count) = recon_turn_facts(outcomes);
        if recon_only {
            self.kpi.observe_recon_turn(parent_tool_count == 1);
        }
        if had_successful_verification {
            self.verify_fail_streak = 0;
            self.kpi.verify_before_end = true;
        }
        self.trivial_mutation = had_file_mutation && is_trivial_mutation_batch(outcomes);

        if had_file_mutation {
            // A successful edit means prior Reads served their purpose — reset
            // the repeat window so a later re-Read for a different file is fine.
            self.read_repeat.reset();
        }

        let action = self.progress.observe_tool_turn(
            &tool_names,
            had_successful_verification,
            had_file_mutation,
            had_any_successful_result,
            recon_only,
            parent_tool_count,
            ProgressObserveParams {
                failed_nudge_threshold: CodingProgressGuard::FAILED_NUDGE_THRESHOLD,
                explore_budget: self.config.explore_budget,
                explore_hard_stop: self.config.explore_hard_stop,
                recon_lifetime_budget: self.config.recon_lifetime_budget,
                recon_lifetime_hard_stop: self.config.recon_lifetime_hard_stop,
                serial_recon_budget: self.config.serial_recon_budget,
                serial_recon_hard_stop: self.config.serial_recon_hard_stop,
            },
        );

        match action {
            CodingProgressAction::Continue | CodingProgressAction::NudgeVerify => {}
            CodingProgressAction::NudgeExplore => texts.push(CODING_EXPLORE_NUDGE.to_string()),
            CodingProgressAction::NudgeExploreBudget(kind) => {
                texts.push(explore_budget_nudge_text(kind).to_string())
            }
            CodingProgressAction::HardStopExplore(kind) => {
                let text = explore_hard_stop_text(kind).to_string();
                texts.push(text.clone());
                hard_stop = Some(text);
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

        if had_plan_update {
            self.mutations_since_plan = 0;
            self.stale_plan_nudge_sent = false;
        } else if mutation_count > 0 {
            self.mutations_since_plan = self
                .mutations_since_plan
                .saturating_add(mutation_count);
            let latest = self.todo.latest();
            if !self.stale_plan_nudge_sent
                && !latest.is_empty()
                && !latest.all_completed()
                && self.mutations_since_plan >= PLAN_STALE_MUTATION_THRESHOLD
            {
                self.stale_plan_nudge_sent = true;
                texts.push(CODING_STALE_PLAN_NUDGE.to_string());
            }
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
        // Bash/exec can change the world. Only recon-like shell (ls/git status)
        // keeps the tour streak; install/run/probe reset it via is_recon_tool.
        if is_mutating_tool(name) && !matches!(name, "Bash" | "exec_command") {
            flags.mutating = true;
        }
        if matches!(name, "Bash" | "exec_command")
            && command.is_some_and(looks_like_verification_command)
        {
            flags.verification = true;
        }
        if name.eq_ignore_ascii_case("verify_change") {
            flags.verification = true;
        }
        flags
    }
}

/// Parent-level recon accounting: ignore isolated subagents; Bash/exec that
/// is only information-gathering counts as recon.
fn recon_turn_facts(outcomes: &[ToolCallOutcome]) -> (bool, usize) {
    let parent: Vec<&ToolCallOutcome> = outcomes
        .iter()
        .filter(|o| !crate::verify::is_isolated_subagent_tool(&o.name))
        .collect();
    if parent.is_empty() {
        return (false, 0);
    }
    let recon_only = parent
        .iter()
        .all(|o| is_recon_tool(&o.name, o.command.as_deref()));
    (recon_only, parent.len())
}

fn is_trivial_mutation_batch(outcomes: &[ToolCallOutcome]) -> bool {
    let mut edited = Vec::new();
    for o in outcomes {
        if o.success && matches!(o.name.as_str(), "Edit" | "Write") {
            if let Some(path) = o.file_path.as_deref() {
                edited.push(path);
            }
        }
        if o.success && o.name == "ApplyPatch" {
            return false;
        }
    }
    if edited.len() != 1 {
        return false;
    }
    let path = edited[0].to_ascii_lowercase();
    matches!(
        path.rsplit_once('.').map(|(_, ext)| ext),
        Some("md" | "txt" | "json" | "toml" | "yml" | "yaml" | "lock" | "svg" | "css" | "html" | "xml")
    )
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
    use crate::progress::PLAN_STALE_MUTATION_THRESHOLD;

    fn edit_ok() -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Edit".into(),
            success: true,
            file_path: Some("a.rs".into()),
            ..Default::default()
        }
    }

    fn read_ok() -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some("a.rs".into()),
            ..Default::default()
        }
    }

    fn edit_fail(path: &str, err: &str) -> ToolCallOutcome {
        ToolCallOutcome {
            name: "Edit".into(),
            success: false,
            file_path: Some(path.into()),
            error_content: Some(err.into()),
            ..Default::default()
        }
    }

    fn plan_ok(json: &str) -> ToolCallOutcome {
        ToolCallOutcome {
            name: "update_plan".into(),
            success: true,
            result_content: Some(json.into()),
            ..Default::default()
        }
    }

    #[test]
    fn evidence_required_default_continues_after_edit() {
        let mut h = CodingHarness::with_defaults(None);
        let nudge = h.after_tool_turn(&[edit_ok()]);
        assert!(!nudge.texts.iter().any(|t| t.contains("verification gate")));
        assert!(matches!(
            h.on_natural_end(),
            FinishDecision::ContinueWithNudge { .. }
        ));
    }

    #[test]
    fn read_repeat_hard_stops() {
        let mut h = CodingHarness::with_defaults(None);
        let read = |path: &str| ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some(path.into()),
            result_content: Some("1:abcd→body".into()),
            ..Default::default()
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
            file_path: Some("a.rs".into()),
            result_content: Some(
                "File unchanged since last read. The content from the earlier Read".into(),
            ),
            ..Default::default()
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
                serial_recon_budget: 20,
                serial_recon_hard_stop: 20,
                recon_lifetime_budget: 20,
                recon_lifetime_hard_stop: 20,
                read_repeat_soft: 20,
                read_repeat_hard: 20,
                ..Default::default()
            },
        );
        let _ = h.after_tool_turn(&[read_ok()]);
        let _ = h.after_tool_turn(&[read_ok()]);
        let last = h.after_tool_turn(&[read_ok()]);
        assert!(last.hard_stop.is_some());
    }

    #[test]
    fn non_verify_bash_counts_as_recon_not_progress() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 2,
                explore_hard_stop: 3,
                serial_recon_budget: 20,
                serial_recon_hard_stop: 20,
                recon_lifetime_budget: 20,
                recon_lifetime_hard_stop: 20,
                ..Default::default()
            },
        );
        let bash = ToolCallOutcome {
            name: "Bash".into(),
            success: true,
            command: Some("git status".into()),
            ..Default::default()
        };
        let _ = h.after_tool_turn(&[bash.clone()]);
        let _ = h.after_tool_turn(&[bash.clone()]);
        let last = h.after_tool_turn(&[bash]);
        assert!(
            last.hard_stop.is_some(),
            "successful non-verify Bash must not reset the recon budget"
        );
    }

    #[test]
    fn serial_one_tool_recon_hard_stops() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 20,
                explore_hard_stop: 20,
                serial_recon_budget: 2,
                serial_recon_hard_stop: 3,
                recon_lifetime_budget: 20,
                recon_lifetime_hard_stop: 20,
                read_repeat_soft: 20,
                read_repeat_hard: 20,
                ..Default::default()
            },
        );
        let read = |i: usize| ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some(format!("src/f{i}.rs")),
            result_content: Some(format!("{i}:abcd→body")),
            ..Default::default()
        };
        let _ = h.after_tool_turn(&[read(0)]);
        let soft = h.after_tool_turn(&[read(1)]);
        assert!(
            soft.texts.iter().any(|t| t.contains("round-trip tax")),
            "got texts: {:?}",
            soft.texts
        );
        let hard = h.after_tool_turn(&[read(2)]);
        assert!(
            hard.hard_stop
                .as_deref()
                .is_some_and(|t| t.contains("round-trip hard-stop")),
            "got hard_stop: {:?}",
            hard.hard_stop
        );
    }

    #[test]
    fn lifetime_recon_hard_stop_before_any_edit() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 20,
                explore_hard_stop: 20,
                serial_recon_budget: 20,
                serial_recon_hard_stop: 20,
                recon_lifetime_budget: 3,
                recon_lifetime_hard_stop: 4,
                read_repeat_soft: 20,
                read_repeat_hard: 20,
                ..Default::default()
            },
        );
        let read = |i: usize| ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some(format!("src/g{i}.rs")),
            result_content: Some(format!("{i}:abcd→body")),
            ..Default::default()
        };
        let _ = h.after_tool_turn(&[read(0)]);
        let _ = h.after_tool_turn(&[read(1)]);
        let soft = h.after_tool_turn(&[read(2)]);
        assert!(
            soft.texts.iter().any(|t| t.contains("recon lifetime")),
            "got texts: {:?}",
            soft.texts
        );
        let hard = h.after_tool_turn(&[read(3)]);
        assert!(
            hard.hard_stop
                .as_deref()
                .is_some_and(|t| t.contains("lifetime hard-stop")),
            "got hard_stop: {:?}",
            hard.hard_stop
        );
    }

    #[test]
    fn lifetime_recon_does_not_abort_after_edits() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 20,
                explore_hard_stop: 20,
                serial_recon_budget: 20,
                serial_recon_hard_stop: 20,
                recon_lifetime_budget: 3,
                recon_lifetime_hard_stop: 4,
                read_repeat_soft: 20,
                read_repeat_hard: 20,
                ..Default::default()
            },
        );
        let read = |i: usize| ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some(format!("src/g{i}.rs")),
            result_content: Some(format!("{i}:abcd→body")),
            ..Default::default()
        };
        let _ = h.after_tool_turn(&[read(0)]);
        let _ = h.after_tool_turn(&[edit_ok()]);
        for i in 1..8 {
            let n = h.after_tool_turn(&[read(i)]);
            assert!(
                n.hard_stop.is_none(),
                "lifetime must not abort after a file mutation, got {:?}",
                n.hard_stop
            );
            assert!(
                n.texts.iter().all(|t| !t.contains("recon lifetime")),
                "lifetime nudge must retire after a file mutation, got {:?}",
                n.texts
            );
        }
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
    fn stale_plan_nudges_once_after_enough_mutations_without_a_new_snapshot() {
        let mut h = CodingHarness::with_defaults(None);
        let _ = h.after_tool_turn(&[plan_ok(
            r#"{"kind":"plan_update","entries":[{"content":"A","status":"in_progress"},{"content":"B","status":"pending"}]}"#,
        )]);
        for _ in 0..(PLAN_STALE_MUTATION_THRESHOLD - 1) {
            let n = h.after_tool_turn(&[edit_ok()]);
            assert!(
                n.texts.iter().all(|t| !t.contains("plan snapshot is stale")),
                "must not tax every file mutation, got {:?}",
                n.texts
            );
        }
        let nudged = h.after_tool_turn(&[edit_ok()]);
        assert!(
            nudged
                .texts
                .iter()
                .any(|t| t.contains("plan snapshot is stale")),
            "missed-milestone floor must fire, got {:?}",
            nudged.texts
        );
        let again = h.after_tool_turn(&[edit_ok()]);
        assert!(
            again.texts.iter().all(|t| !t.contains("plan snapshot is stale")),
            "stale-plan nudge is one-shot until the next snapshot, got {:?}",
            again.texts
        );
        let _ = h.after_tool_turn(&[plan_ok(
            r#"{"kind":"plan_update","entries":[{"content":"A","status":"completed"},{"content":"B","status":"in_progress"}]}"#,
        )]);
        for _ in 0..(PLAN_STALE_MUTATION_THRESHOLD - 1) {
            let n = h.after_tool_turn(&[edit_ok()]);
            assert!(n.texts.iter().all(|t| !t.contains("plan snapshot is stale")));
        }
        let after_refresh = h.after_tool_turn(&[edit_ok()]);
        assert!(
            after_refresh
                .texts
                .iter()
                .any(|t| t.contains("plan snapshot is stale")),
            "a new snapshot must re-arm the missed-milestone floor, got {:?}",
            after_refresh.texts
        );
    }

    #[test]
    fn stale_plan_does_not_nudge_when_there_is_no_plan() {
        let mut h = CodingHarness::with_defaults(None);
        for _ in 0..(PLAN_STALE_MUTATION_THRESHOLD + 1) {
            let n = h.after_tool_turn(&[edit_ok()]);
            assert!(n.texts.iter().all(|t| !t.contains("plan snapshot is stale")));
        }
    }

    #[test]
    fn forced_finalize_keeps_incomplete_plan_for_closing_instruction() {
        let mut h = CodingHarness::with_defaults(None);
        let _ = h.after_tool_turn(&[plan_ok(
            r#"{"kind":"plan_update","entries":[{"content":"A","status":"in_progress"},{"content":"B","status":"pending"}]}"#,
        )]);
        h.begin_forced_finalize("Coding lifetime recon hard-stop".into());
        assert!(
            h.allows_update_plan_on_finalize(),
            "an open plan must keep update_plan on the first finalize pass"
        );
        assert!(!h.ignores_finalize_tool("update_plan"));
        assert!(h.ignores_finalize_tool("Read"));
        assert!(
            h.take_forced_finalize().is_none(),
            "SyncPlan must not be consumed before the model can snapshot"
        );
        assert!(h.allows_update_plan_on_finalize());
        let remaining = h.remaining_plan().expect("open plan must survive hard-stop");
        assert_eq!(remaining.pending().len(), 2);
        let text = crate::finalize::forced_finalize_plan_sync_instruction(
            h.forced_finalize_reason().expect("finalize scheduled"),
            Some(&remaining),
        );
        assert!(text.contains("A"));
        assert!(text.contains("B"));
        assert!(text.contains("update_plan"));
        h.advance_after_plan_sync();
        assert!(!h.allows_update_plan_on_finalize());
        assert!(h.is_forced_finalize());
        assert!(h.ignores_finalize_tool("update_plan"));
        let reply = crate::finalize::forced_finalize_instruction_for_plan(
            h.forced_finalize_reason().expect("reply pass"),
            Some(&remaining),
        );
        assert!(reply.contains("Do not tell the user the task is finished"));
    }

    #[test]
    fn forced_finalize_skips_plan_sync_when_checklist_is_already_closed() {
        let mut h = CodingHarness::with_defaults(None);
        let _ = h.after_tool_turn(&[plan_ok(
            r#"{"kind":"plan_update","entries":[{"content":"A","status":"completed"}]}"#,
        )]);
        h.begin_forced_finalize("Coding explore hard-stop".into());
        assert!(!h.allows_update_plan_on_finalize());
        assert!(h.ignores_finalize_tool("update_plan"));
        assert!(
            h.take_forced_finalize().is_some(),
            "Reply pass is consumable once the checklist is already closed"
        );
        assert!(!h.is_forced_finalize());
    }

    fn exec_go_run() -> ToolCallOutcome {
        ToolCallOutcome {
            name: "exec_command".into(),
            success: true,
            command: Some("go run ./cmd/server".into()),
            ..Default::default()
        }
    }

    #[test]
    fn starting_a_server_resets_the_explore_streak() {
        let mut h = CodingHarness::new(
            None,
            CodingConfig {
                explore_budget: 2,
                explore_hard_stop: 4,
                serial_recon_budget: 20,
                serial_recon_hard_stop: 20,
                recon_lifetime_budget: 20,
                recon_lifetime_hard_stop: 20,
                read_repeat_soft: 20,
                read_repeat_hard: 20,
                ..Default::default()
            },
        );
        let read = || ToolCallOutcome {
            name: "Read".into(),
            success: true,
            file_path: Some("README.md".into()),
            result_content: Some("1:abcd→body".into()),
            ..Default::default()
        };
        let _ = h.after_tool_turn(&[read()]);
        let _ = h.after_tool_turn(&[read()]);
        let _ = h.after_tool_turn(&[exec_go_run()]);
        for _ in 0..3 {
            let n = h.after_tool_turn(&[read()]);
            assert!(
                n.hard_stop.is_none(),
                "go run is progress, not a file tour, got {:?}",
                n.hard_stop
            );
        }
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
    fn env_only_on_turn_tail_without_constitution_flag() {
        let mut h = CodingHarness::with_defaults(None);
        assert!(h.turn_tail(true).is_none());
        assert!(h.turn_tail(false).is_none());
    }

    #[test]
    fn compact_reinject_includes_workingset() {
        let h = CodingHarness::with_defaults(None);
        let text = h.post_compact_reinject();
        assert!(text.contains("WorkingSet"));
        assert!(text.contains("uncovered ranges") || text.contains("Do not restart"));
    }

    #[test]
    fn trivial_markdown_edit_skips_verify_gate() {
        let mut h = CodingHarness::with_defaults(None);
        let _ = h.after_tool_turn(&[ToolCallOutcome {
            name: "Edit".into(),
            success: true,
            file_path: Some("README.md".into()),
            ..Default::default()
        }]);
        assert_eq!(h.on_natural_end(), FinishDecision::Allow);
    }

    #[test]
    fn bash_success_stdout_with_exit_1_does_not_inject_failure_nudge() {
        let mut h = CodingHarness::with_defaults(None);
        let nudge = h.after_tool_turn(&[ToolCallOutcome {
            name: "Bash".into(),
            success: false,
            error_content: Some(
                "Exit code: 1\nSTDOUT:\n快照已更新：C:\\tmp\\modelsdev.json\n  minimax=7\n\nSTDERR:\n\n"
                    .into(),
            ),
            ..Default::default()
        }]);
        assert!(
            nudge
                .texts
                .iter()
                .all(|t| !t.contains("Coding tool-failure")),
            "got texts: {:?}",
            nudge.texts
        );
    }
}
