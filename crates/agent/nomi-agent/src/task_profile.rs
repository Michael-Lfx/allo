//! Compatibility re-exports for coding-mode types now owned by `nomi-coding`.
//!
//! Prefer `nomi_coding::...` / `CodingHarness` in new code; this module keeps
//! existing `crate::task_profile` / `nomi_agent::TaskProfile` call sites working.

pub use nomi_coding::{
    CODING_EDIT_CONVERGE_NUDGE, CODING_EXPLORE_BUDGET_NUDGE, CODING_EXPLORE_NUDGE,
    CODING_VERIFY_NUDGE, CodingConfig, CodingEnvContext, CodingHarness, CodingProgressAction,
    CodingProgressGuard, CompactPolicyOverrides, FinishDecision, TaskProfile, ToolCallOutcome,
    ToolSuccessFlags, ToolTurnNudge, VerificationMode, advertise_tool, coding_overlay_instructions,
    coding_turn_tail, format_env_context, is_explore_tool, is_mutating_tool,
    looks_like_verification_command,
};
