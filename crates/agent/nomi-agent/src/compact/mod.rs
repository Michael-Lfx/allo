//! Multi-level context compaction for long conversations.
//!
//! Three levels, from lightest to heaviest:
//! - **Microcompact**: clears old tool result content (no LLM call)
//! - **Autocompact**: watermark-triggered LLM summarization
//! - **Emergency**: blocks API calls when near the context window limit

pub mod auto;
pub mod emergency;
pub mod estimate;
pub mod micro;
pub mod prompt;
pub mod state;

/// Why the engine is running the compaction pipeline.
///
/// Normal autocompact waits until the user turn has produced a final
/// `EndTurn`. Mid-turn provider passes only compact when the next request
/// is at the emergency limit or the provider reported a context overflow.
/// The exception is the first provider pass of a send: if occupancy already
/// exceeds the current window or autocompact threshold (for example after
/// switching to a smaller model), compact before that call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactReason {
    /// Final assistant `EndTurn` — microcompact then autocompact.
    TurnEnd,
    /// First provider pass of a user send when occupancy already exceeds
    /// the current window or autocompact threshold.
    TurnStart,
    /// Request is at the emergency limit, or the provider returned a
    /// recoverable context overflow before any visible content.
    EmergencyRecovery,
}
