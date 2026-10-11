//! Host-side implementation of the local model → provider publish seam.
//!
//! The local model runtime never touches the `providers` table directly. This
//! adapter owns that bridge using the same [`ProviderService`] the system
//! router uses (including its deletion coordinator), so a running model shows
//! up in the chat model picker and uninstalling a still-referenced provider
//! degrades to a disabled row instead of a hard failure.

use std::collections::HashMap;

use async_trait::async_trait;
use nomifun_api_types::{CreateProviderRequest, UpdateProviderRequest};
use nomifun_common::AppError;
use nomifun_local_models::{
    LocalModelProviderPublisher, LocalModelProviderSpec, ProviderRemoveOutcome,
};
use nomifun_system::ProviderService;

/// Publishes local model processes as OpenAI-compatible providers.
pub struct AppLocalModelProviderPublisher {
    service: ProviderService,
}

impl AppLocalModelProviderPublisher {
    pub fn new(service: ProviderService) -> Self {
        Self { service }
    }
}

#[async_trait]
impl LocalModelProviderPublisher for AppLocalModelProviderPublisher {
    async fn publish(&self, spec: &LocalModelProviderSpec) -> Result<(), String> {
        let mut limits = HashMap::new();
        limits.insert(spec.model.clone(), spec.context_size as i64);
        let models = vec![spec.model.clone()];

        let existing = self
            .service
            .list()
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .any(|provider| provider.provider_id == spec.provider_id);

        if existing {
            self.service
                .update(
                    &spec.provider_id,
                    UpdateProviderRequest {
                        platform: Some("openai".to_owned()),
                        name: Some(spec.name.clone()),
                        base_url: Some(spec.base_url.clone()),
                        api_key: Some(spec.api_key.clone()),
                        models: Some(models),
                        enabled: Some(true),
                        model_context_limits: Some(limits),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
        } else {
            self.service
                .create(CreateProviderRequest {
                    provider_id: Some(spec.provider_id.clone()),
                    platform: "openai".to_owned(),
                    name: spec.name.clone(),
                    base_url: spec.base_url.clone(),
                    api_key: spec.api_key.clone(),
                    models,
                    enabled: true,
                    capabilities: Vec::new(),
                    model_context_limits: Some(limits),
                    model_protocols: None,
                    model_descriptions: None,
                    model_enabled: None,
                    model_health: None,
                    bedrock_config: None,
                    is_full_url: false,
                    sort_order: None,
                })
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    async fn disable(&self, provider_id: &str) -> Result<(), String> {
        self.service
            .update(
                provider_id,
                UpdateProviderRequest {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn remove(&self, provider_id: &str) -> Result<ProviderRemoveOutcome, String> {
        match self.service.delete(provider_id).await {
            Ok(()) => Ok(ProviderRemoveOutcome::Deleted),
            Err(AppError::ProviderInUse(_)) => {
                self.disable(provider_id).await?;
                Ok(ProviderRemoveOutcome::Disabled)
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
