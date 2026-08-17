//! System-continuation budget for coding (and reusable by the engine).
//!
//! Every *automatic* continuation that fires at a natural stop (no tool calls)
//! — verify hard-gate, todo nudge, explore force-continue — consumes one unit.
//! User steering does **not** consume the budget. When the budget is exhausted,
//! the harness must allow EndTurn so the agent cannot loop forever on soft policy.

use serde::{Deserialize, Serialize};

/// Default cap aligned with Vetta plugin/Stop continuation limits.
pub const DEFAULT_MAX_SYSTEM_CONTINUATIONS: usize = 8;

/// Tracks how many system-driven continuations have been issued this root request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContinuationBudget {
    used: usize,
    max: usize,
}

impl ContinuationBudget {
    pub fn new(max: usize) -> Self {
        Self {
            used: 0,
            max: max.max(1),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_MAX_SYSTEM_CONTINUATIONS)
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn remaining(&self) -> usize {
        self.max.saturating_sub(self.used)
    }

    /// Returns true if another system continuation is still allowed.
    pub fn can_continue(&self) -> bool {
        self.used < self.max
    }

    /// Consume one continuation slot. Returns false if the budget was already exhausted.
    pub fn try_consume(&mut self) -> bool {
        if !self.can_continue() {
            return false;
        }
        self.used = self.used.saturating_add(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausts_at_max() {
        let mut b = ContinuationBudget::new(2);
        assert!(b.try_consume());
        assert!(b.try_consume());
        assert!(!b.try_consume());
        assert!(!b.can_continue());
        assert_eq!(b.used(), 2);
    }

    #[test]
    fn reset_clears_used() {
        let mut b = ContinuationBudget::new(1);
        assert!(b.try_consume());
        b.reset();
        assert!(b.try_consume());
    }
}
