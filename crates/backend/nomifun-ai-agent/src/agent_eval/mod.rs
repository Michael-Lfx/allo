//! Isolated live-agent evaluation lab.
//!
//! Runs production `AgentEngine` (Office profile or CodingHarness) against an
//! eval corpus without registering in `AgentRuntimeRegistry` or writing user
//! sessions.

mod capture;
mod lab;
mod live;

pub use lab::EvalLab;
pub use live::{LiveEvalTrace, LiveNomiHarness};
