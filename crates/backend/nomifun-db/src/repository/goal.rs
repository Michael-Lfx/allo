use crate::error::DbError;
use crate::models::GoalRow;

/// Caller-supplied goal snapshot for [`IGoalRepository::upsert`] — everything
/// except the local row identity and the optimistic-lock counter, which the
/// repository owns.
///
/// Field semantics mirror `nomi_agent::goal::state::GoalState` (`max_turns`
/// carries the engine's `max_auto_continuations` budget; the JSON columns are
/// stored verbatim).
#[derive(Debug, Clone)]
pub struct UpsertGoalParams {
    pub session_id: String,
    pub objective: String,
    pub status: String,
    pub turns_used: i64,
    pub max_turns: i64,
    pub created_at: i64,
    pub last_turn_at: Option<i64>,
    pub last_verdict: Option<String>,
    pub last_reason: Option<String>,
    pub paused_reason: Option<String>,
    pub consecutive_parse_failures: i64,
    pub consecutive_transport_failures: i64,
    pub subgoals_json: String,
    pub contract_json: Option<String>,
    pub waiting_on_pid: Option<i64>,
    pub waiting_on_session: Option<String>,
    pub waiting_until: Option<i64>,
    pub waiting_reason: Option<String>,
    pub updated_at: i64,
}

/// Data access abstraction for the `goals` table.
///
/// One goal snapshot per session: `session_id` (the conversation_id for Nomi
/// runtimes) is unique, so persisting an engine snapshot is always an upsert.
#[async_trait::async_trait]
pub trait IGoalRepository: Send + Sync {
    /// Insert or replace the goal snapshot for a session. On conflict every
    /// snapshot column is replaced and `version` is bumped by one. Returns
    /// the persisted row. `version` is an audit-only modification counter,
    /// NOT an optimistic lock — the upsert is unconditional last-writer-wins
    /// (single-writer product model).
    async fn upsert(&self, params: &UpsertGoalParams) -> Result<GoalRow, DbError>;

    /// Return the goal snapshot for a session, or `None`.
    async fn load_by_session(&self, session_id: &str) -> Result<Option<GoalRow>, DbError>;

    /// Delete the goal row for a session. Idempotent — clearing an absent
    /// goal is not an error.
    async fn clear(&self, session_id: &str) -> Result<(), DbError>;

    /// Rebind a goal row to a new session id (session identity rename, e.g.
    /// a hypothetical compaction that rotates ids — Nomi keeps its
    /// conversation_id across compaction, so production rarely hits this).
    /// Returns `false` when the old session has no goal row.
    async fn migrate_session_id(
        &self,
        old_session_id: &str,
        new_session_id: &str,
    ) -> Result<bool, DbError>;
}
