//! Provider-backed [`AuxiliaryClient`] for POI / insights background LLM tasks.
//!
//! Mirrors the layering of [`crate::knowledge_completer::LiveKnowledgeCompleter`]:
//! resolve a default provider/model, then run one-shot completions.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use nomi_auxiliary::{AuxiliaryClient, AuxiliaryClientBuilder, ChatLlmProvider};
use nomi_types::message::{Message, Role};
use nomifun_common::AppError;
use nomifun_db::IProviderRepository;
use tracing::debug;

use crate::factory::provider_config::{one_shot_completion_no_thinking, resolve_provider_config, user_message};
use crate::knowledge_completer::resolve_default_model;

const AUXILIARY_MAX_TOKENS: u32 = 4096;

/// Builds an [`AuxiliaryClient`] from the first enabled provider/model.
#[derive(Clone)]
pub struct AuxiliaryClientFactory {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl AuxiliaryClientFactory {
    pub fn new(
        provider_repo: Arc<dyn IProviderRepository>,
        encryption_key: [u8; 32],
        workspace: PathBuf,
    ) -> Self {
        Self {
            provider_repo,
            encryption_key,
            workspace,
        }
    }

    /// Resolve the default provider/model and build an auxiliary client.
    pub async fn build_client(&self) -> Result<Arc<AuxiliaryClient>, AppError> {
        let (provider_id, model) = resolve_default_model(&self.provider_repo).await.ok_or_else(|| {
            AppError::Conflict(
                "auxiliary LLM unavailable: no enabled provider/model is configured".into(),
            )
        })?;
        let provider = Arc::new(ProviderBackedAuxiliaryLlm {
            factory: self.clone(),
            provider_id: provider_id.clone(),
            model: model.clone(),
        });
        let client = AuxiliaryClientBuilder::new()
            .provider(provider)
            .label(&provider_id)
            .default_model(&model)
            .build()
            .map_err(|e| AppError::Internal(format!("auxiliary client build failed: {e}")))?;
        Ok(Arc::new(client))
    }
}

#[derive(Clone)]
struct ProviderBackedAuxiliaryLlm {
    factory: AuxiliaryClientFactory,
    provider_id: String,
    model: String,
}

#[async_trait]
impl ChatLlmProvider for ProviderBackedAuxiliaryLlm {
    async fn chat_completion(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
        _temperature: Option<f64>,
        model: Option<&str>,
    ) -> Result<String, String> {
        let (system, user) = split_system_user(messages)?;
        let model = model.filter(|m| !m.is_empty()).unwrap_or(self.model.as_str());
        let cfg = resolve_provider_config(
            &self.factory.provider_repo,
            &self.factory.encryption_key,
            &self.provider_id,
            model,
            &self.factory.workspace,
        )
        .await
        .map_err(|e| e.to_string())?;
        let max = max_tokens.unwrap_or(AUXILIARY_MAX_TOKENS);
        one_shot_completion_no_thinking(&cfg, &system, vec![user_message(&user)], max)
            .await
            .map_err(|e| e.to_string())
    }
}

fn split_system_user(messages: &[Message]) -> Result<(String, String), String> {
    let mut system_parts = Vec::new();
    let mut user_parts = Vec::new();
    for msg in messages {
        let text = message_text(msg);
        if text.is_empty() {
            continue;
        }
        match msg.role {
            Role::System => system_parts.push(text),
            Role::User => user_parts.push(text),
            Role::Assistant => user_parts.push(text),
            Role::Tool => {}
        }
    }
    if user_parts.is_empty() {
        return Err("auxiliary chat requires at least one user message".into());
    }
    let system = if system_parts.is_empty() {
        "You are a helpful assistant.".to_string()
    } else {
        system_parts.join("\n\n")
    };
    Ok((system, user_parts.join("\n\n")))
}

fn message_text(msg: &Message) -> String {
    msg.content
        .iter()
        .filter_map(|block| match block {
            nomi_types::message::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Best-effort auxiliary client for background pipelines (POI / insights).
pub async fn try_build_auxiliary_client(
    factory: &AuxiliaryClientFactory,
) -> Option<Arc<AuxiliaryClient>> {
    match factory.build_client().await {
        Ok(client) => Some(client),
        Err(err) => {
            debug!(error = %err, "auxiliary client unavailable for session extraction");
            None
        }
    }
}
