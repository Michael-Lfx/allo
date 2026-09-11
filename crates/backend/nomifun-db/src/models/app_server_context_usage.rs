use serde::{Deserialize, Serialize};

/// Durable per-conversation context occupancy snapshot for App Server chats.
///
/// `context_tokens` is the last measured provider prompt occupancy
/// (a gauge) and `window_tokens` is the effective engine context window that
/// occupancy was measured against. Both are server-measured observability
/// values; clients can read them through the App Server projection but never
/// write them. A `window_tokens` of zero means the provider/engine did not
/// report a window, so the projection reports "unknown" instead of a fake
/// percentage.
///
/// `last_turn_input_tokens` / `last_turn_output_tokens` carry the runtime's own
/// per-turn report for the **most recent** completed turn, so the WebUI can
/// still render last turn's tokens (and the catalog-derived cost) after a
/// reload — the gauge above cannot express what one turn cost. Both are
/// nullable: `None` means the runtime reported nothing for that turn and stays
/// distinguishable from a reported `0`. They are overwritten every turn and no
/// amount is stored (cost is derived client-side from catalog rates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppServerContextUsageRow {
    pub id: i64,
    pub conversation_id: String,
    pub context_tokens: i64,
    pub window_tokens: i64,
    /// Most recent turn's prompt (input) tokens; `None` = not reported.
    pub last_turn_input_tokens: Option<i64>,
    /// Most recent turn's completion (output) tokens; `None` = not reported.
    pub last_turn_output_tokens: Option<i64>,
    pub updated_at: i64,
}