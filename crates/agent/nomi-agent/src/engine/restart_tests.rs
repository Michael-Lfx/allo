//! The resumable-round predicate has exactly one definition; these pin its truth
//! table so a future edit to one restart path cannot diverge from the other.

use super::StopReason;
use crate::round::{LedgerCutoff, MAX_ROUND_ATTEMPTS, RoundState};
use nomi_types::message::ContentBlock;

fn round_with_a_cutoff_call() -> RoundState {
    let mut round = RoundState::new(vec![ContentBlock::Text {
        text: "do the thing".into(),
    }]);
    round.ledger.set_cutoff(vec![LedgerCutoff {
        tool: "Write".into(),
        argument_bytes: 37,
        state_changing: true,
    }]);
    round
}

/// Every condition is load-bearing. Restarting without a recorded cutoff would
/// burn the remaining attempts re-generating prose against the same ceiling, and
/// restarting past the attempt cap would turn a bounded round into a loop.
#[test]
fn resumable_round_predicate_requires_every_condition() {
    let max_tokens = StopReason::MaxTokens;
    let mut round = round_with_a_cutoff_call();

    assert!(
        super::should_restart_round(max_tokens, &round, true),
        "a truncated tool call with attempts left is resumable"
    );
    assert!(
        !super::should_restart_round(max_tokens, &round, false),
        "a pass that advertised no tools has nothing to resume against"
    );
    assert!(
        !super::should_restart_round(StopReason::EndTurn, &round, true),
        "a clean stop is never resumed"
    );
    assert!(
        !super::should_restart_round(StopReason::ToolUse, &round, true),
        "a pass that produced tool calls is not a truncated round"
    );

    // Exhausting the attempt cap is a terminal outcome, not another restart.
    for _ in 1..MAX_ROUND_ATTEMPTS {
        round.begin_attempt();
    }
    assert!(
        !super::should_restart_round(max_tokens, &round, true),
        "the cap bounds the round even with a cutoff on the books"
    );
}

/// A prose-only truncation has nothing in flight, so it must not restart: the
/// cutoff window is the only evidence that continuing is provably useful.
#[test]
fn a_round_without_a_cutoff_is_never_resumable() {
    let round = RoundState::new(vec![ContentBlock::Text {
        text: "explain something".into(),
    }]);

    assert!(round.ledger.cutoff.is_empty());
    assert!(!super::should_restart_round(StopReason::MaxTokens, &round, true));
}
