//! Coding harness for Allo: completion policy on the shared agent engine.
//!
//! Owns coding-session **policy**: profile, constitution, tool advertising,
//! progress/verify/explore hard stops, todo continuation, edit converge,
//! compact preferences, and Edit/Bash boundary hints. The generic turn loop
//! stays in `nomi-agent`; install a [`CodingHarness`] when `task_profile=coding`.
//!
//! Completion model is Vetta-inspired (continuation budget + todo nudge +
//! no-progress hard stops). Edit UX prefers anchor `line:hash` edits over
//! Codex ApplyPatch (which is hidden in coding mode).

pub mod bash_hints;
pub mod continuation;
pub mod edit_converge;
pub mod edit_hints;
pub mod env;
pub mod finalize;
pub mod harness;
pub mod patch;
pub mod profile;
pub mod progress;
pub mod prompt;
pub mod read_repeat;
pub mod todo_continuation;
pub mod tools;
pub mod verify;

pub use bash_hints::{BOUNDARY_PREFIX, format_write_root_rejection, wrap_boundary_error};
pub use continuation::{ContinuationBudget, DEFAULT_MAX_SYSTEM_CONTINUATIONS};
pub use edit_converge::{
    CODING_EDIT_CONVERGE_NUDGE, CODING_EDIT_HARD_STOP, EditConvergeAction, EditFailureTracker,
};
pub use edit_hints::{EditFailureKind, append_edit_recovery_hint, infer_edit_failure_kind};
pub use env::{CodingEnvContext, format_env_context};
pub use finalize::{
    FRIENDLY_FINALIZE_FALLBACK, finalize_reply_or_fallback, forced_finalize_instruction,
    sanitize_user_facing_reply,
};
pub use harness::{
    CodingConfig, CodingHarness, CompactPolicyOverrides, FinishDecision, ToolCallOutcome,
    ToolSuccessFlags, ToolTurnNudge, VerificationMode,
};
pub use patch::{
    PatchFileOp, PatchHunk, PatchParseError, ParsedPatch, looks_like_freeform_patch,
    parse_freeform_patch,
};
pub use profile::TaskProfile;
pub use progress::{
    CODING_EXPLORE_BUDGET_NUDGE, CODING_EXPLORE_HARD_STOP, CODING_EXPLORE_NUDGE,
    CODING_PLAN_HARD_STOP, CODING_PLAN_TIMEOUT_NUDGE, CODING_VERIFY_NUDGE, CodingProgressAction,
    CodingProgressGuard, ProgressObserveParams, is_explore_tool,
};
pub use prompt::{coding_overlay_instructions, coding_turn_tail};
pub use read_repeat::{
    CODING_READ_REPEAT_HARD_STOP, CODING_READ_REPEAT_NUDGE, CODING_UNCHANGED_STUB_NUDGE,
    ReadRepeatAction, ReadRepeatTracker, normalize_read_path,
};
pub use todo_continuation::{
    PlanSnapshot, PlanStepView, TodoContinuationMode, TodoContinuationTracker,
    parse_plan_update_content,
};
pub use tools::advertise_tool;
pub use verify::{is_mutating_tool, looks_like_verification_command};
