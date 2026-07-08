use nomi_config::compact::CompactConfig;

/// Fraction of the context window at which to emit a soft-compaction notice
/// (growing context warning) without actually compacting — keeping the
/// cache-stable prefix intact. Mirrors Reasonix's `defaultSoftCompactRatio`.
pub const SOFT_COMPACT_RATIO: f64 = 0.5;

/// After this many consecutive compactions, auto-compaction is paused to
/// prevent a re-fire loop (the kept tail alone exceeds the trigger).
/// Mirrors Reasonix's `consecutiveCompacts >= 2` check.
pub const CONSECUTIVE_COMPACT_PAUSE: u32 = 2;

/// Runtime state for the compaction circuit breaker and cache-aware
/// compaction scheduling.
///
/// Tracks consecutive autocompact failures so we can stop retrying
/// after `config.max_failures` consecutive failures.
///
/// Also tracks soft-compaction notices and consecutive-compaction stalls
/// to keep DeepSeek prefix-cache hits high (mirrors Reasonix's
/// `softCompactNoticed`, `consecutiveCompacts`, and `compactStuck`).
#[derive(Debug, Clone)]
pub struct CompactState {
    /// Number of consecutive autocompact failures.
    pub consecutive_failures: u32,
    /// Input token count from the last API call (used as the watermark).
    pub last_input_tokens: u64,
    /// Whether the soft-compaction notice (50% of window) has already been
    /// emitted for the current growth cycle. Cleared when tokens drop below
    /// the soft threshold so a new cycle can notify again.
    pub soft_compact_noticed: bool,
    /// How many consecutive turns auto-compaction has fired. A healthy
    /// compaction drops the prompt below the trigger, so the next turn
    /// won't compact. Reset to 0 when a turn sits under the trigger.
    pub consecutive_compacts: u32,
    /// When true, auto-compaction is paused because `consecutive_compacts`
    /// reached [`CONSECUTIVE_COMPACT_PAUSE`]. The system prompt plus one
    /// verbatim turn already exceeds the trigger, so re-firing every turn
    /// is the loop users hit. Cleared when tokens drop below the trigger.
    pub compact_stuck: bool,
}

impl CompactState {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_input_tokens: 0,
            soft_compact_noticed: false,
            consecutive_compacts: 0,
            compact_stuck: false,
        }
    }

    /// Check whether the circuit breaker has tripped.
    pub fn is_circuit_broken(&self, config: &CompactConfig) -> bool {
        self.consecutive_failures >= config.max_failures
    }

    /// Record a successful autocompact — resets the failure counter.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Record a failed autocompact — increments the failure counter.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    /// Check if the token watermark has crossed the soft-compaction threshold
    /// (50% of the context window). Returns true once per growth cycle.
    pub fn check_soft_compact(&mut self, config: &CompactConfig) -> bool {
        let soft_threshold =
            (config.context_window as f64 * SOFT_COMPACT_RATIO) as u64;
        if self.last_input_tokens >= soft_threshold && !self.soft_compact_noticed {
            self.soft_compact_noticed = true;
            true
        } else {
            false
        }
    }

    /// Called when a turn sits under the hard trigger — the breathing room a
    /// healthy compaction buys. Clears the stuck latch and the run counter so
    /// a future growth cycle starts fresh.
    pub fn clear_compact_stall(&mut self) {
        self.consecutive_compacts = 0;
        self.compact_stuck = false;
    }

    /// Called after a successful auto-compaction. Increments the consecutive
    /// counter and, if it reaches [`CONSECUTIVE_COMPACT_PAUSE`], sets the
    /// stuck flag. Returns true if the stuck state was just entered.
    pub fn record_consecutive_compact(&mut self) -> bool {
        self.consecutive_compacts += 1;
        if self.consecutive_compacts >= CONSECUTIVE_COMPACT_PAUSE && !self.compact_stuck {
            self.compact_stuck = true;
            true
        } else {
            false
        }
    }

    /// Whether auto-compaction is paused due to consecutive-compaction stall.
    pub fn is_compact_stuck(&self) -> bool {
        self.compact_stuck
    }

    /// Reset the soft-compaction notice when tokens drop below the soft
    /// threshold, so a new growth cycle can notify again.
    pub fn maybe_reset_soft_notice(&mut self, config: &CompactConfig) {
        let soft_threshold =
            (config.context_window as f64 * SOFT_COMPACT_RATIO) as u64;
        if self.last_input_tokens < soft_threshold {
            self.soft_compact_noticed = false;
        }
    }
}

impl Default for CompactState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CompactConfig {
        CompactConfig {
            max_failures: 3,
            ..Default::default()
        }
    }

    #[test]
    fn new_state_not_circuit_broken() {
        let state = CompactState::new();
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.last_input_tokens, 0);
        assert!(!state.is_circuit_broken(&test_config()));
    }

    #[test]
    fn circuit_breaker_trips_at_max_failures() {
        let config = test_config();
        let mut state = CompactState::new();

        state.record_failure();
        assert!(!state.is_circuit_broken(&config));
        state.record_failure();
        assert!(!state.is_circuit_broken(&config));
        state.record_failure();
        assert!(state.is_circuit_broken(&config));
    }

    #[test]
    fn success_resets_failure_counter() {
        let config = test_config();
        let mut state = CompactState::new();

        state.record_failure();
        state.record_failure();
        assert_eq!(state.consecutive_failures, 2);

        state.record_success();
        assert_eq!(state.consecutive_failures, 0);
        assert!(!state.is_circuit_broken(&config));
    }

    #[test]
    fn circuit_breaker_with_max_failures_one() {
        let config = CompactConfig {
            max_failures: 1,
            ..Default::default()
        };
        let mut state = CompactState::new();

        assert!(!state.is_circuit_broken(&config));
        state.record_failure();
        assert!(state.is_circuit_broken(&config));
    }

    #[test]
    fn default_impl_matches_new() {
        let a = CompactState::new();
        let b = CompactState::default();
        assert_eq!(a.consecutive_failures, b.consecutive_failures);
        assert_eq!(a.last_input_tokens, b.last_input_tokens);
        assert_eq!(a.soft_compact_noticed, b.soft_compact_noticed);
        assert_eq!(a.consecutive_compacts, b.consecutive_compacts);
        assert_eq!(a.compact_stuck, b.compact_stuck);
    }

    // ── Soft compaction notice ──────────────────────────────────────────

    #[test]
    fn soft_compact_notices_once() {
        let config = CompactConfig {
            context_window: 200_000,
            ..Default::default()
        };
        let mut state = CompactState::new();
        state.last_input_tokens = 100_000; // 50% of 200k
        assert!(state.check_soft_compact(&config));
        // Second call should not re-notify
        assert!(!state.check_soft_compact(&config));
    }

    #[test]
    fn soft_compact_skips_below_threshold() {
        let config = CompactConfig {
            context_window: 200_000,
            ..Default::default()
        };
        let mut state = CompactState::new();
        state.last_input_tokens = 99_999; // just under 50%
        assert!(!state.check_soft_compact(&config));
    }

    #[test]
    fn soft_compact_resets_when_tokens_drop() {
        let config = CompactConfig {
            context_window: 200_000,
            ..Default::default()
        };
        let mut state = CompactState::new();
        state.last_input_tokens = 100_000;
        assert!(state.check_soft_compact(&config));
        // Tokens drop below 50%
        state.last_input_tokens = 50_000;
        state.maybe_reset_soft_notice(&config);
        // Should be able to notify again
        state.last_input_tokens = 100_000;
        assert!(state.check_soft_compact(&config));
    }

    // ── Consecutive compaction stall ────────────────────────────────────

    #[test]
    fn consecutive_compact_pauses_after_two() {
        let mut state = CompactState::new();
        // First compaction
        assert!(!state.record_consecutive_compact());
        assert!(!state.is_compact_stuck());
        // Second compaction → stuck
        assert!(state.record_consecutive_compact());
        assert!(state.is_compact_stuck());
    }

    #[test]
    fn clear_stall_resets_counter() {
        let mut state = CompactState::new();
        state.record_consecutive_compact();
        state.record_consecutive_compact();
        assert!(state.is_compact_stuck());
        state.clear_compact_stall();
        assert!(!state.is_compact_stuck());
        assert_eq!(state.consecutive_compacts, 0);
    }
}
