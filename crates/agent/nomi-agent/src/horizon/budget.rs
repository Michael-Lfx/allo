//! Continuation budget that is **not** reset when a Goal round starts.
//!
//! The engine's `turn` counter is the net provider-loop budget (default 200).
//! This type additionally caps Goal auto-continuations and optional wall time
//! so a fail-open judge cannot reopen a fresh 200-turn window forever.

use std::time::Instant;

/// Free-form goals (no Verification contract) cannot burn the full default 8.
pub const FREEFORM_AUTO_CONTINUE_CAP: usize = 3;

/// Wall-clock ceiling for one Goal auto-continue session. Generous on purpose:
/// long verified work must finish; overnight idle loops must not.
pub const DEFAULT_WALL_LIMIT_SECS: u64 = 2 * 60 * 60;

#[derive(Debug, Clone)]
pub struct HorizonBudget {
    started_at: Instant,
    continuations: usize,
    max_continuations: usize,
    wall_limit_secs: u64,
    input_tokens: u64,
    output_tokens: u64,
}

impl Default for HorizonBudget {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            continuations: 0,
            max_continuations: 0,
            wall_limit_secs: DEFAULT_WALL_LIMIT_SECS,
            input_tokens: 0,
            output_tokens: 0,
        }
    }
}

impl HorizonBudget {
    pub fn configure(&mut self, max_continuations: usize) {
        self.max_continuations = max_continuations;
        if self.continuations == 0 {
            self.started_at = Instant::now();
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn remaining(&self) -> usize {
        self.max_continuations.saturating_sub(self.continuations)
    }

    pub fn continuations(&self) -> usize {
        self.continuations
    }

    pub fn record_continuation(&mut self) {
        self.continuations = self.continuations.saturating_add(1);
    }

    pub fn record_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
    }

    pub fn wall_exceeded(&self) -> bool {
        self.max_continuations > 0
            && self.started_at.elapsed().as_secs() >= self.wall_limit_secs
    }

    pub fn exhausted(&self) -> bool {
        self.max_continuations > 0 && self.continuations >= self.max_continuations
    }

    pub fn token_totals(&self) -> (u64, u64) {
        (self.input_tokens, self.output_tokens)
    }
}

/// Effective Goal auto-continue cap: requested budget, clamped for free-form.
pub fn effective_auto_continue_cap(requested: usize, has_verification: bool) -> usize {
    if has_verification {
        requested
    } else {
        requested.min(FREEFORM_AUTO_CONTINUE_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeform_clamps_to_three() {
        assert_eq!(effective_auto_continue_cap(8, false), 3);
        assert_eq!(effective_auto_continue_cap(1, false), 1);
        assert_eq!(effective_auto_continue_cap(8, true), 8);
    }

    #[test]
    fn remaining_tracks_continuations() {
        let mut b = HorizonBudget::default();
        b.configure(3);
        assert_eq!(b.remaining(), 3);
        b.record_continuation();
        b.record_continuation();
        assert_eq!(b.remaining(), 1);
        assert!(!b.exhausted());
        b.record_continuation();
        assert!(b.exhausted());
        assert_eq!(b.remaining(), 0);
    }
}
