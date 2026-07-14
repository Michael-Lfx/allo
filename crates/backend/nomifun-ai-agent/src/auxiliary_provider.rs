//! Provider-backed [`AuxiliaryClient`] for POI / insights background LLM tasks.
//!
//! Background auxiliary LLM (POI extraction, insights resolution) is restricted
//! to the built-in **`flowy-cloud`** provider. Transient upstream faults are
//! retried with bounded backoff.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nomi_auxiliary::{AuxiliaryClient, AuxiliaryClientBuilder, ChatLlmProvider};
use nomi_config::InterestConfig;
use nomi_types::message::{Message, Role};
use nomifun_common::AppError;
use nomifun_db::IProviderRepository;
use tracing::{debug, warn};

use crate::factory::provider_config::{one_shot_completion_no_thinking, resolve_provider_config, user_message};
use crate::knowledge_completer::first_enabled_model;

/// Built-in Flowy cloud provider — the only LLM source for auxiliary tasks.
pub const FLOWY_CLOUD_PROVIDER_ID: &str = "flowy-cloud";

const AUXILIARY_MAX_TOKENS: u32 = 4096;
const AUXILIARY_MAX_ATTEMPTS: u32 = 3;
const AUXILIARY_RETRY_BASE_DELAY_MS: u64 = 1_000;

/// Builds an [`AuxiliaryClient`] backed exclusively by `flowy-cloud`.
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

    /// Resolve the enabled `flowy-cloud` model and build an auxiliary client.
    pub async fn build_client(&self) -> Result<Arc<AuxiliaryClient>, AppError> {
        self.build_client_with_model(None).await
    }

    /// Build an auxiliary client using an explicit flowy-cloud model when provided.
    pub async fn build_client_with_model(
        &self,
        model_override: Option<&str>,
    ) -> Result<Arc<AuxiliaryClient>, AppError> {
        let (provider_id, model) = resolve_auxiliary_model(&self.provider_repo, model_override)
            .await
            .ok_or_else(|| {
                AppError::Conflict(
                    "auxiliary LLM unavailable: flowy-cloud provider is not enabled or has no model"
                        .into(),
                )
            })?;
        let provider = Arc::new(FlowyCloudAuxiliaryLlm {
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
struct FlowyCloudAuxiliaryLlm {
    factory: AuxiliaryClientFactory,
    provider_id: String,
    model: String,
}

#[async_trait]
impl ChatLlmProvider for FlowyCloudAuxiliaryLlm {
    async fn chat_completion(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
        _temperature: Option<f64>,
        model: Option<&str>,
    ) -> Result<String, String> {
        let (system, user) = split_system_user(messages)?;
        let model = model
            .filter(|m| !m.is_empty())
            .unwrap_or(self.model.as_str());
        let max = max_tokens.unwrap_or(AUXILIARY_MAX_TOKENS);

        let mut last_err = String::new();
        for attempt in 0..AUXILIARY_MAX_ATTEMPTS {
            match completion_for_provider(
                &self.factory,
                &system,
                &user,
                &self.provider_id,
                model,
                max,
            )
            .await
            {
                Ok(text) if !text.trim().is_empty() => {
                    if attempt > 0 {
                        debug!(
                            provider = %self.provider_id,
                            model = %model,
                            attempt = attempt + 1,
                            "auxiliary LLM succeeded after retry"
                        );
                    }
                    return Ok(text);
                }
                Ok(_) => {
                    last_err = format!("{}/{}: empty response", self.provider_id, model);
                }
                Err(err) => {
                    last_err = err.clone();
                }
            }

            let retryable = is_retryable_auxiliary_err(&last_err);
            let has_more = attempt + 1 < AUXILIARY_MAX_ATTEMPTS;
            if !retryable || !has_more {
                break;
            }
            let delay_ms = AUXILIARY_RETRY_BASE_DELAY_MS.saturating_mul(attempt as u64 + 1);
            warn!(
                provider = %self.provider_id,
                model = %model,
                attempt = attempt + 1,
                max_attempts = AUXILIARY_MAX_ATTEMPTS,
                delay_ms,
                error = %last_err,
                "auxiliary LLM attempt failed — retrying"
            );
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        Err(format!(
            "LLM error on provider {}: {}",
            self.provider_id, last_err
        ))
    }
}

async fn resolve_flowy_cloud_model(
    provider_repo: &Arc<dyn IProviderRepository>,
) -> Option<(String, String)> {
    resolve_auxiliary_model(provider_repo, None).await
}

async fn resolve_auxiliary_model(
    provider_repo: &Arc<dyn IProviderRepository>,
    model_override: Option<&str>,
) -> Option<(String, String)> {
    let row = provider_repo
        .find_by_id(FLOWY_CLOUD_PROVIDER_ID)
        .await
        .ok()??;
    if !row.enabled {
        return None;
    }
    if let Some(model) = model_override.map(str::trim).filter(|m| !m.is_empty()) {
        return Some((FLOWY_CLOUD_PROVIDER_ID.to_string(), model.to_string()));
    }
    first_enabled_model(&row.models, row.model_enabled.as_deref())
        .map(|model| (FLOWY_CLOUD_PROVIDER_ID.to_string(), model))
}

/// Sentinel stored in `InterestConfig.llm_model` for “follow active session”.
pub const POI_LLM_MODEL_FOLLOW_SESSION: &str = "__session__";

/// Resolve the flowy-cloud model for POI LLM extraction / starter generation.
///
/// - Explicit model id → that model  
/// - `__session__` → active conversation model, else first enabled cloud model  
/// - unset / empty → **first enabled cloud model** (product default), else session
pub async fn resolve_poi_llm_model(
    interest_cfg: &InterestConfig,
    session_model: Option<&str>,
    provider_repo: &Arc<dyn IProviderRepository>,
) -> Option<String> {
    let configured = interest_cfg
        .llm_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(model) = configured {
        if model == POI_LLM_MODEL_FOLLOW_SESSION {
            if let Some(session) = session_model.map(str::trim).filter(|s| !s.is_empty()) {
                return Some(session.to_string());
            }
            return resolve_flowy_cloud_model(provider_repo)
                .await
                .map(|(_, model)| model);
        }
        return Some(model.to_string());
    }

    if let Some(model) = resolve_flowy_cloud_model(provider_repo)
        .await
        .map(|(_, model)| model)
    {
        return Some(model);
    }
    session_model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn is_retryable_auxiliary_err(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("bad gateway")
        || lower.contains("all channel models failed")
        || lower.contains("rate limit")
        || lower.contains("timeout")
        || lower.contains("temporarily unavailable")
        || lower.contains("upstream")
}

async fn completion_for_provider(
    factory: &AuxiliaryClientFactory,
    system: &str,
    user: &str,
    provider_id: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let cfg = resolve_provider_config(
        &factory.provider_repo,
        &factory.encryption_key,
        provider_id,
        model,
        &factory.workspace,
    )
    .await
    .map_err(|e| e.to_string())?;
    one_shot_completion_no_thinking(&cfg, system, vec![user_message(user)], max_tokens)
        .await
        .map_err(|e| e.to_string())
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

/// Best-effort auxiliary client for POI extraction with resolved model priority.
pub async fn try_build_auxiliary_client_for_poi(
    factory: &AuxiliaryClientFactory,
    interest_cfg: &InterestConfig,
    session_model: Option<&str>,
) -> Option<Arc<AuxiliaryClient>> {
    let model = resolve_poi_llm_model(interest_cfg, session_model, &factory.provider_repo).await;
    match factory.build_client_with_model(model.as_deref()).await {
        Ok(client) => Some(client),
        Err(err) => {
            debug!(
                error = %err,
                model = model.as_deref().unwrap_or("default"),
                "auxiliary client unavailable for POI extraction"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_errors_include_cloud_gateway_failures() {
        assert!(is_retryable_auxiliary_err(
            "Bad gateway: LLM provider error: API error 500: All channel models failed"
        ));
        assert!(is_retryable_auxiliary_err("HTTP 502 Bad Gateway"));
        assert!(!is_retryable_auxiliary_err("Provider 'openai' not found"));
        assert!(!is_retryable_auxiliary_err("invalid api key"));
    }
}
