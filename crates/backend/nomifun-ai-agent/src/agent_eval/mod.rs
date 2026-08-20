//! Isolated live-agent evaluation lab.
//!
//! Runs production `AgentEngine` (Office profile or CodingHarness) against an
//! eval corpus without registering in `AgentRuntimeRegistry`. Each case is
//! bound to Session Observation via a dedicated `conversation_id` (and an
//! optional conversation shell from [`EvalSessionBridge`]).

mod capture;
mod lab;
mod live;
mod session_bridge;

pub use lab::EvalLab;
pub use live::{LiveEvalTrace, LiveNomiHarness};
pub use session_bridge::{
    eval_run_workspace_label, suite_business_label, EvalCaseTurnUsage, EvalSessionBridge,
    OpenEvalCaseSession, RecordEvalCaseTurn,
};
