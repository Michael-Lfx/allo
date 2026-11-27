//! Stateful worker capability extension seam for `flowy-agent`.
//!
//! Extension crates implement [`ToolProvider`] instead of patching core harness
//! turn assembly. Providers are mounted only inside full-capability Worker
//! (`ReadWrite` / `Standard`) turns, never inside `Router` or `ReadOnly`.

use std::path::Path;

use crate::loop_runner::PreToolDecision;
use crate::session::{SessionOrigin, WorkAccessMode};
use crate::tools::ToolRegistry;

/// Per-turn context supplied to [`ToolProvider::register_tools`] and
/// [`ToolProvider::worker_prompt_hint`].
#[derive(Clone, Debug)]
pub struct ToolProviderContext<'a> {
    pub session_id: &'a str,
    pub cwd: &'a Path,
    pub origin: &'a SessionOrigin,
    pub access_mode: WorkAccessMode,
}

/// Per-call context supplied to [`ToolProvider::evaluate_pre_tool`].
#[derive(Clone, Debug)]
pub struct ToolPolicyContext<'a> {
    pub session_id: &'a str,
    pub cwd: &'a Path,
    pub origin: &'a SessionOrigin,
}

/// Extension interface for registering domain, automation, MCP, or skill tools
/// into full-capability Worker turns without editing host routing internals.
pub trait ToolProvider: Send + Sync {
    /// Unique stable identifier for this provider (used for idempotent replacement,
    /// removal, source tagging, and scoped `evaluate_pre_tool` dispatch).
    fn id(&self) -> &str;

    /// Register tools into the Worker turn's staged [`ToolRegistry`].
    ///
    /// Called only for `Worker (ReadWrite)` and legacy `Standard` sessions.
    /// Implementations must remain fast and side-effect-free (no blocking IO,
    /// network calls, or subprocess spawns during registration).
    fn register_tools(
        &self,
        ctx: &ToolProviderContext<'_>,
        registry: &ToolRegistry,
    );

    /// Optional pre-execution policy hook for tools owned by this provider.
    ///
    /// Host policy dispatch invokes this only for tools tagged with this
    /// provider's [`ToolProvider::id`]. Returning [`PreToolDecision::Ask`]
    /// triggers the host's interactive `PendingCard::ToolApproval` flow using
    /// the host's canonical SHA-256 `args_hash`.
    fn evaluate_pre_tool(
        &self,
        _tool_name: &str,
        _args: &serde_json::Value,
        _ctx: &ToolPolicyContext<'_>,
    ) -> Option<PreToolDecision> {
        None
    }

    /// Optional concise summary surfaced in `Router` system prompts so the
    /// coordinator knows which extension capabilities `ReadWrite` Workers own.
    fn router_capability_hint(&self) -> Option<String> {
        None
    }

    /// Optional concise guidance appended to `Worker` system prompts when this
    /// provider is eligible for the turn.
    fn worker_prompt_hint(
        &self,
        _ctx: &ToolProviderContext<'_>,
    ) -> Option<String> {
        None
    }
}
