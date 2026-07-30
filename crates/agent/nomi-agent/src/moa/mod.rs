//! Mixture of Agents (MoA) engine core.
//!
//! Before an aggregator (session model) call, the engine may fan out the
//! conversation to a set of advisory "reference" models. References never
//! execute tools — they only analyze the presented state and hand advice to
//! the acting model. Their advice rides the turn tail of the request copy
//! (see `context_contributor`), so persisted history stays byte-stable.
//!
//! The module is self-contained: the engine only touches [`MoaState`],
//! [`runner::MoaRunner`] and [`guidance::format_guidance`]. Hosts resolve
//! each configured slot into a full provider `Config` and inject the state
//! via `AgentEngine::set_moa_state` — the engine never reads global config.

pub mod advisory_view;
pub mod guidance;
pub mod prompts;
pub mod redact;
pub mod runner;
pub mod trace;

use nomi_config::config::{Config, MoaConfig};

/// One advisory response produced by a reference model.
#[derive(Debug, Clone, PartialEq)]
pub struct MoaAdvice {
    /// Human-readable slot label, `"provider/model"`.
    pub label: String,
    /// Advice text, or a `"[failed: …]"` sentinel when the slot errored out.
    pub text: String,
}

/// Advisor token usage attributed to one reference slot within one user turn.
///
/// Accumulated across every real fan-out of the turn (cache-hit rounds add
/// nothing); failed slots contribute zero-usage entries so the slot list
/// stays aligned with the configured references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoaSlotTurnUsage {
    /// Slot label, `"provider/model"`.
    pub label: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// How often the reference fan-out runs within one user turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutCadence {
    /// Advise once per user turn; later tool iterations reuse the cached
    /// advice (signature HIT). Cheapest, the default.
    UserTurn,
    /// Re-advise on every model iteration of the turn.
    PerIteration,
    /// Advise on iteration 1, then on every Nth iteration; non-hit
    /// iterations reuse the last cached advice.
    EveryN(u32),
}

impl FanoutCadence {
    /// Parse the `moa.fanout` config string. Unknown or malformed values fall
    /// back to [`FanoutCadence::UserTurn`] — config typos must never disable
    /// the agent loop.
    pub fn from_str(raw: &str) -> Self {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("user_turn") {
            return Self::UserTurn;
        }
        if raw.eq_ignore_ascii_case("per_iteration") {
            return Self::PerIteration;
        }
        if let Some(n) = raw
            .to_ascii_lowercase()
            .strip_prefix("every_n:")
            .and_then(|n| n.trim().parse::<u32>().ok())
        {
            return match n {
                0 => Self::UserTurn,
                1 => Self::PerIteration,
                n => Self::EveryN(n),
            };
        }
        Self::UserTurn
    }
}

/// A reference slot resolved by the host into a ready-to-use provider config.
#[derive(Debug, Clone)]
pub struct MoaResolvedSlot {
    /// Full provider config for this reference (api key, base url, model…).
    pub config: Config,
    /// Display label, typically `"provider_id/model"`.
    pub label: String,
    /// Per-slot output token ceiling; `None` → `MoaConfig::reference_max_tokens`.
    pub max_tokens: Option<u32>,
    /// Sampling temperature forwarded to the advisor request; `None` leaves
    /// the provider default in effect.
    pub temperature: Option<f32>,
    /// Advisor context window in tokens, when the host knows it. `None` →
    /// a conservative default is used when trimming the advisory view.
    pub context_window_tokens: Option<u64>,
}

/// Per-engine MoA state: resolved slots plus turn-scoped cadence bookkeeping
/// and the cross-iteration advice cache.
#[derive(Debug)]
pub struct MoaState {
    pub config: MoaConfig,
    pub slots: Vec<MoaResolvedSlot>,
    /// 1-based count of fan-out opportunities within the current user turn.
    pub(crate) iteration_count: u32,
    /// Signature of the last fan-out that actually ran (see
    /// [`runner::turn_signature`]). Kept across turns: a new turn produces a
    /// new signature and naturally MISSes.
    pub(crate) last_run_signature: Option<u64>,
    /// Advice from the last fan-out that ran, reused on cache hits.
    pub(crate) cached_advices: Option<Vec<MoaAdvice>>,
    /// Per-slot advisor usage accumulated over the current user turn
    /// (real fan-outs only; cleared by [`MoaState::reset_turn`]).
    pub(crate) turn_slot_usage: Vec<MoaSlotTurnUsage>,
}

impl MoaState {
    pub fn new(config: MoaConfig, slots: Vec<MoaResolvedSlot>) -> Self {
        Self {
            config,
            slots,
            iteration_count: 0,
            last_run_signature: None,
            cached_advices: None,
            turn_slot_usage: Vec::new(),
        }
    }

    /// Whether the fan-out can run at all.
    pub fn is_active(&self) -> bool {
        self.config.enabled && !self.slots.is_empty()
    }

    /// Parsed fan-out cadence.
    pub fn cadence(&self) -> FanoutCadence {
        FanoutCadence::from_str(&self.config.fanout)
    }

    /// Per-slot advisor usage accumulated over the current user turn.
    pub fn turn_slot_usage(&self) -> &[MoaSlotTurnUsage] {
        &self.turn_slot_usage
    }

    /// Reset turn-scoped state at the start of a new user turn. The signature
    /// cache is intentionally kept: the new turn's signature differs, so the
    /// first iteration MISSes and re-runs while stale advice is never reused.
    pub fn reset_turn(&mut self) {
        self.iteration_count = 0;
        self.turn_slot_usage.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_parses_known_values() {
        assert_eq!(FanoutCadence::from_str("user_turn"), FanoutCadence::UserTurn);
        assert_eq!(
            FanoutCadence::from_str("per_iteration"),
            FanoutCadence::PerIteration
        );
        assert_eq!(FanoutCadence::from_str("every_n:3"), FanoutCadence::EveryN(3));
    }

    #[test]
    fn cadence_degenerate_every_n_values_normalize() {
        // every_n:1 is per-iteration by definition; every_n:0 is meaningless.
        assert_eq!(
            FanoutCadence::from_str("every_n:1"),
            FanoutCadence::PerIteration
        );
        assert_eq!(FanoutCadence::from_str("every_n:0"), FanoutCadence::UserTurn);
    }

    #[test]
    fn cadence_invalid_values_fall_back_to_user_turn() {
        for raw in ["", "always", "every_n", "every_n:", "every_n:x", "42"] {
            assert_eq!(FanoutCadence::from_str(raw), FanoutCadence::UserTurn, "{raw}");
        }
    }

    #[test]
    fn state_reset_turn_clears_iterations_but_keeps_cache() {
        let mut state = MoaState::new(MoaConfig::default(), Vec::new());
        state.iteration_count = 4;
        state.last_run_signature = Some(99);
        state.cached_advices = Some(vec![MoaAdvice {
            label: "p/m".into(),
            text: "advice".into(),
        }]);
        state.turn_slot_usage.push(MoaSlotTurnUsage {
            label: "p/m".into(),
            input_tokens: 10,
            output_tokens: 5,
        });
        state.reset_turn();
        assert_eq!(state.iteration_count, 0);
        assert_eq!(state.last_run_signature, Some(99));
        assert!(state.cached_advices.is_some());
        assert!(
            state.turn_slot_usage().is_empty(),
            "per-slot usage is turn-scoped and must reset"
        );
    }

    #[test]
    fn state_inactive_without_slots_or_switch() {
        let disabled = MoaState::new(MoaConfig::default(), Vec::new());
        assert!(!disabled.is_active());
        let enabled_no_slots = MoaState::new(
            MoaConfig {
                enabled: true,
                ..MoaConfig::default()
            },
            Vec::new(),
        );
        assert!(!enabled_no_slots.is_active());
    }
}
