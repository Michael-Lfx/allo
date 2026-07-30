//! Engine-internal goal-driven continuation (ported from codex `ext/goal`).
//!
//! Opt-in, default off. When a session is started with a goal, the engine's
//! natural-termination point injects a continuation prompt (with a completion
//! audit) until the model proves completion via `update_goal`, hits the
//! auto-continuation cap, or runs into `max_turns`.

pub mod judge;
pub mod runtime;
pub mod state;
pub mod tool;

pub use judge::{
    BackgroundProcessInfo, ContractDraftError, GoalJudgeClient, JudgeOutcome, ProviderJudgeClient,
    WaitDirective, draft_contract,
};
pub use runtime::{GoalRuntime, GoalSpec, GoalWaitProbe};
pub use state::{GoalContract, GoalState, GoalStatus, GoalVerdict};
