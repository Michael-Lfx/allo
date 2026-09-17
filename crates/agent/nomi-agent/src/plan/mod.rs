// Plan Mode: state management, tool implementations, prompts, and file I/O.
//
// Plan Mode restricts the agent to read-only tools while composing an
// implementation plan. A verifiable ExitPlanMode submits the plan; write
// tools stay locked until the next user message (approval).

pub mod file;
pub mod prompt;
pub mod state;
pub mod tools;
