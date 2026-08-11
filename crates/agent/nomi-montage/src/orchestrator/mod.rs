//! Executive Producer orchestration: the LLM↔tool loop, HITL approvals, and preflight.

pub mod approvals;
pub mod ep;
pub mod preflight;
pub mod prompts;
pub mod stage_runner;

pub use approvals::{ApprovalDecision, ApprovalRequest, apply_approval};
pub use ep::{EpRunParams, run_project};
pub use preflight::{ProviderMenu, ToolAvailability, build_provider_menu, missing_tools_for_pipeline};
pub use stage_runner::StageOutcome;
