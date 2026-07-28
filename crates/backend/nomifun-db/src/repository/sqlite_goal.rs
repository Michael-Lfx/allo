use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::GoalRow;
use crate::repository::goal::{IGoalRepository, UpsertGoalParams};

const GOAL_COLUMNS: &str = "id, session_id, objective, status, turns_used, max_turns, \
     created_at, last_turn_at, last_verdict, last_reason, paused_reason, \
     consecutive_parse_failures, consecutive_transport_failures, \
     subgoals_json, contract_json, waiting_on_pid, waiting_on_session, \
     waiting_until, waiting_reason, updated_at, version";

#[derive(Clone, Debug)]
pub struct SqliteGoalRepository {
    pool: SqlitePool,
}

impl SqliteGoalRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IGoalRepository for SqliteGoalRepository {
    async fn upsert(&self, params: &UpsertGoalParams) -> Result<GoalRow, DbError> {
        let sql = format!(
            "INSERT INTO goals (\
                session_id, objective, status, turns_used, max_turns, \
                created_at, last_turn_at, last_verdict, last_reason, paused_reason, \
                consecutive_parse_failures, consecutive_transport_failures, \
                subgoals_json, contract_json, waiting_on_pid, waiting_on_session, \
                waiting_until, waiting_reason, updated_at, version\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0) \
            ON CONFLICT(session_id) DO UPDATE SET \
                objective = excluded.objective, \
                status = excluded.status, \
                turns_used = excluded.turns_used, \
                max_turns = excluded.max_turns, \
                created_at = excluded.created_at, \
                last_turn_at = excluded.last_turn_at, \
                last_verdict = excluded.last_verdict, \
                last_reason = excluded.last_reason, \
                paused_reason = excluded.paused_reason, \
                consecutive_parse_failures = excluded.consecutive_parse_failures, \
                consecutive_transport_failures = excluded.consecutive_transport_failures, \
                subgoals_json = excluded.subgoals_json, \
                contract_json = excluded.contract_json, \
                waiting_on_pid = excluded.waiting_on_pid, \
                waiting_on_session = excluded.waiting_on_session, \
                waiting_until = excluded.waiting_until, \
                waiting_reason = excluded.waiting_reason, \
                updated_at = excluded.updated_at, \
                version = goals.version + 1 \
            RETURNING {GOAL_COLUMNS}"
        );
        let row = sqlx::query_as::<_, GoalRow>(&sql)
            .bind(&params.session_id)
            .bind(&params.objective)
            .bind(&params.status)
            .bind(params.turns_used)
            .bind(params.max_turns)
            .bind(params.created_at)
            .bind(params.last_turn_at)
            .bind(&params.last_verdict)
            .bind(&params.last_reason)
            .bind(&params.paused_reason)
            .bind(params.consecutive_parse_failures)
            .bind(params.consecutive_transport_failures)
            .bind(&params.subgoals_json)
            .bind(&params.contract_json)
            .bind(params.waiting_on_pid)
            .bind(&params.waiting_on_session)
            .bind(params.waiting_until)
            .bind(&params.waiting_reason)
            .bind(params.updated_at)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn load_by_session(&self, session_id: &str) -> Result<Option<GoalRow>, DbError> {
        let sql = format!("SELECT {GOAL_COLUMNS} FROM goals WHERE session_id = ?");
        let row = sqlx::query_as::<_, GoalRow>(&sql)
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    async fn clear(&self, session_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM goals WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn migrate_session_id(
        &self,
        old_session_id: &str,
        new_session_id: &str,
    ) -> Result<bool, DbError> {
        if old_session_id == new_session_id {
            return Ok(true);
        }
        let mut transaction = self.pool.begin().await?;
        // The new identity must win: a stale row already parked under the
        // new session id would otherwise fail the UNIQUE(session_id) rebind.
        sqlx::query("DELETE FROM goals WHERE session_id = ?")
            .bind(new_session_id)
            .execute(&mut *transaction)
            .await?;
        let moved = sqlx::query("UPDATE goals SET session_id = ? WHERE session_id = ?")
            .bind(new_session_id)
            .bind(old_session_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(moved.rows_affected() > 0)
    }
}
