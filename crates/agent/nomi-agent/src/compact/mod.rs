//! Multi-level context compaction for long conversations.
//!
//! Cheap layers first (Claude-style lazy degradation):
//! - **Budget reduction**: oversized tool results become `[content_ref …]` locators
//! - **Snip**: drop old plain user/assistant turns without splitting tool pairs
//! - **Microcompact**: stub old tool result bodies (no LLM call)
//! - **Context collapse**: a structured snip notice in the transcript
//! - **Autocompact**: watermark-triggered LLM summarization (last)
//! - **Emergency**: blocks API calls when near the context window limit
//!
//! Folded history is archived under `{cwd}/.flowy/context-archive/` so the
//! model can Read/Grep what was dropped.

pub mod archive;
pub mod auto;
pub mod emergency;
pub mod estimate;
pub mod micro;
pub mod prompt;
pub mod snip;
pub mod state;

/// Why the engine is running the compaction pipeline.
///
/// Normal autocompact waits until the user turn has produced a final
/// `EndTurn`. Mid-turn provider passes only compact when the next request
/// is at the emergency limit or the provider reported a context overflow.
/// The exception is the first provider pass of a send: if occupancy already
/// exceeds the current window or autocompact threshold (for example after
/// switching to a smaller model), compact before that call.
/// After a long idle gap the provider prefix cache is presumed expired, so
/// cheap layers run even below the watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactReason {
    /// Final assistant `EndTurn` — microcompact then autocompact.
    TurnEnd,
    /// First provider pass of a user send when occupancy already exceeds
    /// the current window or autocompact threshold.
    TurnStart,
    /// Previous turn ended long enough ago that prefix cache is cold.
    /// Snip + microcompact always; LLM summarizer only when occupancy
    /// justifies the fold.
    IdleCacheExpired,
    /// Request is at the emergency limit, or the provider returned a
    /// recoverable context overflow before any visible content.
    EmergencyRecovery,
}
