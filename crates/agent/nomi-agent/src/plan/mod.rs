// Plan Mode: state management, tool implementations, prompts, and file I/O.
//
// Plan Mode restricts the agent to read-only tools while composing an
// implementation plan. A verifiable ExitPlanMode submits the plan; write
// tools stay locked until the next user message (approval).
//
// The implementation lives in `crate::features::plan` — plan mode is a Feature
// (see `docs/architecture/plan-goal-feature-seam.zh.md`). This module is the
// compatibility façade that keeps `nomi_agent::plan::*` resolving for existing
// callers: a few hosts and integration tests reach the plan tools, state and
// prompt through this path.
//
// NOTE: delete this façade once those callers move to
// `nomi_agent::features::plan::*`. Do not add new items here.

pub use crate::features::plan::*;
