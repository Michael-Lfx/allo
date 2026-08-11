//! Cost estimation / reservation / reconciliation against a project's credit budget.
//!
//! Maps 1:1 to Flowy credits (no separate currency): `estimate` before a tool
//! call, `reserve` to soft-lock budget, `reconcile` after the call actually
//! bills. Never silently exceeds `budget_credits` — a hard cap raises
//! [`crate::error::MontageError::GovernanceBlocked`] so the orchestrator can
//! route to `awaiting_human`.

use serde::{Deserialize, Serialize};

use crate::error::{MontageError, MontageResult};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CostDelta {
    pub credits: i64,
}

impl CostDelta {
    pub fn zero() -> Self {
        Self { credits: 0 }
    }

    pub fn of(credits: i64) -> Self {
        Self { credits }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub tool: String,
    pub estimated_credits: u64,
    pub basis: String,
}

/// Running ledger for one project. Not thread-safe by itself — callers hold it
/// behind the project's single-writer lock (see `orchestrator::ep`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostLedger {
    pub budget_credits: u64,
    pub reserved_credits: u64,
    pub spent_credits: u64,
    /// Soft warning threshold as a fraction of budget (default 0.8 = 80%).
    #[serde(default = "default_warn_fraction")]
    pub warn_fraction: f32,
}

fn default_warn_fraction() -> f32 {
    0.8
}

impl CostLedger {
    pub fn new(budget_credits: u64) -> Self {
        Self {
            budget_credits,
            reserved_credits: 0,
            spent_credits: 0,
            warn_fraction: default_warn_fraction(),
        }
    }

    pub fn committed(&self) -> u64 {
        self.reserved_credits + self.spent_credits
    }

    pub fn remaining(&self) -> i64 {
        self.budget_credits as i64 - self.committed() as i64
    }

    pub fn is_over_warn_threshold(&self) -> bool {
        self.budget_credits > 0
            && (self.committed() as f32) >= (self.budget_credits as f32 * self.warn_fraction)
    }

    /// Reserve `credits` ahead of a tool call. Errors (hard cap) instead of
    /// silently overspending; caller should route to `awaiting_human` /
    /// `single_action_approval`.
    pub fn reserve(&mut self, credits: u64) -> MontageResult<()> {
        if self.committed() + credits > self.budget_credits {
            return Err(MontageError::GovernanceBlocked(format!(
                "reserving {credits} credits would exceed budget ({}/{} already committed)",
                self.committed(),
                self.budget_credits
            )));
        }
        self.reserved_credits += credits;
        Ok(())
    }

    /// Reconcile a reservation against the actual spend (may be less or more
    /// than reserved; over-spend beyond budget still errors).
    pub fn reconcile(&mut self, reserved: u64, actual: u64) -> MontageResult<()> {
        self.reserved_credits = self.reserved_credits.saturating_sub(reserved);
        if self.spent_credits + actual > self.budget_credits {
            self.spent_credits += actual;
            return Err(MontageError::GovernanceBlocked(format!(
                "actual spend {actual} pushed total spend to {} over budget {}",
                self.spent_credits, self.budget_credits
            )));
        }
        self.spent_credits += actual;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_blocks_when_over_budget() {
        let mut ledger = CostLedger::new(100);
        ledger.reserve(80).unwrap();
        let err = ledger.reserve(30).unwrap_err();
        assert!(matches!(err, MontageError::GovernanceBlocked(_)));
    }

    #[test]
    fn reconcile_moves_reserved_to_spent() {
        let mut ledger = CostLedger::new(100);
        ledger.reserve(50).unwrap();
        ledger.reconcile(50, 40).unwrap();
        assert_eq!(ledger.spent_credits, 40);
        assert_eq!(ledger.reserved_credits, 0);
        assert_eq!(ledger.remaining(), 60);
    }

    #[test]
    fn warn_threshold_trips_at_80_percent() {
        let mut ledger = CostLedger::new(100);
        ledger.reserve(79).unwrap();
        assert!(!ledger.is_over_warn_threshold());
        ledger.reserve(1).unwrap();
        assert!(ledger.is_over_warn_threshold());
    }
}
