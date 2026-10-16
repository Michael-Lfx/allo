use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{
    ConversationSkillPin, NewSkillSelectionEvent, SkillInvocationOverride, SkillSelectionEvent,
};
use crate::repository::skill_discovery::ISkillDiscoveryRepository;

#[derive(Clone, Debug)]
pub struct SqliteSkillDiscoveryRepository {
    pool: SqlitePool,
}

impl SqliteSkillDiscoveryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ISkillDiscoveryRepository for SqliteSkillDiscoveryRepository {
    async fn insert_selection_event(&self, event: &NewSkillSelectionEvent) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO skill_selection_event \
                (conversation_id, turn, query_hash, candidate_ids, model_loaded_ids, \
                 method, listing_truncated, applied_to_l1, latency_ms, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event.conversation_id)
        .bind(event.turn)
        .bind(&event.query_hash)
        .bind(&event.candidate_ids)
        .bind(&event.model_loaded_ids)
        .bind(&event.method)
        .bind(i64::from(event.listing_truncated))
        .bind(i64::from(event.applied_to_l1))
        .bind(event.latency_ms)
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_selection_events(
        &self,
        conversation_id: &str,
        limit: i64,
    ) -> Result<Vec<SkillSelectionEvent>, DbError> {
        Ok(sqlx::query_as::<_, SkillSelectionEvent>(
            "SELECT conversation_id, turn, query_hash, candidate_ids, model_loaded_ids, \
                    method, listing_truncated, applied_to_l1, latency_ms, created_at \
             FROM skill_selection_event \
             WHERE conversation_id = ? \
             ORDER BY created_at DESC, id DESC \
             LIMIT ?",
        )
        .bind(conversation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn list_pins(&self, conversation_id: &str) -> Result<Vec<ConversationSkillPin>, DbError> {
        Ok(sqlx::query_as::<_, ConversationSkillPin>(
            "SELECT conversation_id, skill_id, pinned_at \
             FROM conversation_skill_pins \
             WHERE conversation_id = ? \
             ORDER BY pinned_at ASC, id ASC",
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn pin_skill(
        &self,
        conversation_id: &str,
        skill_id: &str,
        pinned_at: i64,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO conversation_skill_pins (conversation_id, skill_id, pinned_at) \
             VALUES (?, ?, ?) \
             ON CONFLICT(conversation_id, skill_id) DO UPDATE SET pinned_at = excluded.pinned_at",
        )
        .bind(conversation_id)
        .bind(skill_id)
        .bind(pinned_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unpin_skill(&self, conversation_id: &str, skill_id: &str) -> Result<(), DbError> {
        sqlx::query(
            "DELETE FROM conversation_skill_pins WHERE conversation_id = ? AND skill_id = ?",
        )
        .bind(conversation_id)
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_invocation_overrides(&self) -> Result<Vec<SkillInvocationOverride>, DbError> {
        Ok(sqlx::query_as::<_, SkillInvocationOverride>(
            "SELECT skill_id, allow_model_auto, updated_at FROM skill_invocation_overrides \
             ORDER BY skill_id ASC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn set_invocation_override(
        &self,
        skill_id: &str,
        allow_model_auto: bool,
        updated_at: i64,
    ) -> Result<SkillInvocationOverride, DbError> {
        sqlx::query(
            "INSERT INTO skill_invocation_overrides (skill_id, allow_model_auto, updated_at) \
             VALUES (?, ?, ?) \
             ON CONFLICT(skill_id) DO UPDATE SET \
                allow_model_auto = excluded.allow_model_auto, \
                updated_at = excluded.updated_at",
        )
        .bind(skill_id)
        .bind(i64::from(allow_model_auto))
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(SkillInvocationOverride {
            skill_id: skill_id.to_owned(),
            allow_model_auto: i64::from(allow_model_auto),
            updated_at,
        })
    }
}
