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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppServerContextUsageRow {
    pub id: i64,
    pub conversation_id: String,
    pub context_tokens: i64,
    pub window_tokens: i64,
    pub updated_at: i64,
}