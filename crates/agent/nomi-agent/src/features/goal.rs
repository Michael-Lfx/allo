//! Goal-driven continuation as a self-registering feature.
//!
//! The feature owns the horizon controller (progress ledger + auto-continue
//! budget), the goal runtime, and the host liveness probe. It contributes the
//! `update_goal` tool, its per-turn awareness reminder, and the natural-end
//! hook that runs the judge and decides whether the turn continues.
//!
//! The hardening gates are unchanged and stay inside this feature: the
//! horizon's continuation budget, the no-progress idle-streak veto, judge
//! fail-closed on parse/transport failure, and the "judge cannot declare done
//! without mechanical evidence" completion gate.

use nomi_tools::registry::ToolRegistry;

use super::{Feature, FeatureHooks};

/// Goal-driven continuation as an engine feature.
///
/// The feature is registered unconditionally; whether a goal is *active* is a
/// host decision (`AgentBootstrap::goal` / `AgentEngine::set_goal`), so a
/// goal-less session contributes a no-op hook set and renders no reminder.
pub struct GoalFeature {
    // Populated in the extraction phase with the horizon controller, the goal
    // runtime handle and the host liveness probe.
    _private: (),
}

impl Default for GoalFeature {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalFeature {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Feature for GoalFeature {
    fn name(&self) -> &'static str {
        "goal"
    }

    fn register_tools(&self, _registry: &mut ToolRegistry) {
        // `update_goal` is registered by `set_goal` / `set_goal_state`, which
        // own the runtime handle; the feature re-homes that registration when
        // the state moves here.
    }

    fn hooks(&self) -> FeatureHooks {
        FeatureHooks::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_name_is_stable() {
        assert_eq!(GoalFeature::new().name(), "goal");
    }

    #[test]
    fn a_goal_less_session_contributes_no_hooks() {
        // Registering the feature must not change a goal-less session's turn:
        // every fold iterates an empty hook set and returns its input.
        let hooks = GoalFeature::new().hooks();
        assert!(hooks.on_user_request.is_empty());
        assert!(hooks.dispatch_gate.is_empty());
        assert!(hooks.apply_modifier.is_empty());
        assert!(hooks.request_gates.is_empty());
        assert!(hooks.on_tool_turn.is_empty());
        assert!(hooks.on_natural_end.is_empty());
    }
}
