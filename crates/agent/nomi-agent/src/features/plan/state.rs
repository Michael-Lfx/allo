//! Runtime state for Plan Mode.
//!
//! Tracks whether the agent is currently in plan mode and the tool allow-list
//! that was active before plan mode was entered (for restoration on approval).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanPhase {
    /// Not planning.
    #[default]
    Idle,
    /// Read-only exploration / drafting.
    Exploring,
    /// A verifiable plan was submitted. Writes stay locked until the next
    /// user message (the Build click analogue).
    AwaitingApproval,
}

#[derive(Debug, Clone, Default)]
pub struct PlanState {
    /// Whether plan mode is currently active (exploring or awaiting approval).
    pub is_active: bool,

    /// The tool allow-list that was in effect before entering plan mode.
    /// Restored when the user approves the plan (next user turn).
    pub pre_plan_allow_list: Vec<String>,

    pub phase: PlanPhase,

    /// Last plan text accepted by ExitPlanMode.
    pub pending_plan: Option<String>,
}

impl PlanState {
    pub fn awaiting_approval(&self) -> bool {
        self.phase == PlanPhase::AwaitingApproval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_inactive() {
        let state = PlanState::default();
        assert!(!state.is_active);
        assert_eq!(state.phase, PlanPhase::Idle);
        assert!(state.pending_plan.is_none());
    }

    #[test]
    fn default_has_empty_allow_list() {
        let state = PlanState::default();
        assert!(state.pre_plan_allow_list.is_empty());
    }

    #[test]
    fn can_set_active_with_allow_list() {
        let state = PlanState {
            is_active: true,
            pre_plan_allow_list: vec!["Read".into(), "Bash".into()],
            phase: PlanPhase::Exploring,
            ..Default::default()
        };
        assert!(state.is_active);
        assert_eq!(state.pre_plan_allow_list, vec!["Read", "Bash"]);
    }

    #[test]
    fn clone_produces_independent_copy() {
        let original = PlanState {
            is_active: true,
            pre_plan_allow_list: vec!["Grep".into()],
            phase: PlanPhase::Exploring,
            pending_plan: Some("plan".into()),
        };
        let mut cloned = original.clone();
        cloned.is_active = false;
        cloned.pre_plan_allow_list.push("Read".into());

        assert!(original.is_active);
        assert_eq!(original.pre_plan_allow_list, vec!["Grep"]);
    }
}
