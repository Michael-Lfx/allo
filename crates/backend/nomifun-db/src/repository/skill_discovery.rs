use crate::error::DbError;
use crate::models::{
    ConversationSkillPin, NewSkillSelectionEvent, SkillInvocationOverride, SkillSelectionEvent,
};

#[async_trait::async_trait]
pub trait ISkillDiscoveryRepository: Send + Sync {
    async fn insert_selection_event(&self, event: &NewSkillSelectionEvent) -> Result<(), DbError>;

    async fn list_selection_events(
        &self,
        conversation_id: &str,
        limit: i64,
    ) -> Result<Vec<SkillSelectionEvent>, DbError>;

    async fn list_pins(&self, conversation_id: &str) -> Result<Vec<ConversationSkillPin>, DbError>;

    async fn pin_skill(
        &self,
        conversation_id: &str,
        skill_id: &str,
        pinned_at: i64,
    ) -> Result<(), DbError>;

    async fn unpin_skill(&self, conversation_id: &str, skill_id: &str) -> Result<(), DbError>;

    async fn list_invocation_overrides(&self) -> Result<Vec<SkillInvocationOverride>, DbError>;

    async fn set_invocation_override(
        &self,
        skill_id: &str,
        allow_model_auto: bool,
        updated_at: i64,
    ) -> Result<SkillInvocationOverride, DbError>;
}
