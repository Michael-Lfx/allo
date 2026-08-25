//! Late-wire meeting notes (N3) LLM + conversation sink into MeetingSessionService.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use nomifun_ai_agent::{
    one_shot_completion, resolve_default_model, resolve_provider_config, user_message,
};
use nomifun_audio::{MeetingNotesCompleter, MeetingNotesConversationSink};
use nomifun_db::{
    IConversationRepository, IProviderModelRepository, IProviderRepository, models::MessageRow,
};
use uuid::Uuid;

const NOTES_MAX_TOKENS: u32 = 4096;

pub struct LiveMeetingNotesCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

#[async_trait]
impl MeetingNotesCompleter for LiveMeetingNotesCompleter {
    async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let (provider_id, model) =
            resolve_default_model(&self.provider_repo, &self.provider_model_repo)
                .await
                .ok_or_else(|| {
                    "meeting notes LLM unavailable: no enabled provider/model".to_string()
                })?;
        let cfg = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            &provider_id,
            &model,
            &self.workspace,
        )
        .await
        .map_err(|e| e.to_string())?;
        one_shot_completion(&cfg, system, vec![user_message(user)], NOTES_MAX_TOKENS)
            .await
            .map_err(|e| e.to_string())
    }
}

pub struct MeetingNotesConversationSinkImpl {
    pub conversation_repo: Arc<dyn IConversationRepository>,
}

#[async_trait]
impl MeetingNotesConversationSink for MeetingNotesConversationSinkImpl {
    async fn post_notes(&self, conversation_id: &str, markdown: &str) -> Result<(), String> {
        let id = Uuid::now_v7().to_string();
        let row = MessageRow {
            id: 0,
            message_id: id.clone(),
            conversation_id: conversation_id.to_string(),
            msg_id: Some(id),
            r#type: "text".into(),
            content: serde_json::json!({
                "content": markdown,
                "origin": "meeting_notes",
            })
            .to_string(),
            position: Some("left".into()),
            status: Some("finish".into()),
            hidden: false,
            created_at: nomifun_common::now_ms(),
        };
        self.conversation_repo
            .insert_message(&row)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
