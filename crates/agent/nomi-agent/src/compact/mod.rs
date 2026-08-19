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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactReason {
    /// Final assistant `EndTurn` — microcompact then autocompact.
    TurnEnd,
    /// Request is at the emergency limit, or the provider returned a
    /// recoverable context overflow before any visible content.
    EmergencyRecovery,
}
