//! models.dev catalog service — status / refresh / lookup / search / reconcile.

use std::sync::Arc;

use nomifun_api_types::{
    ModelsDevLookupResponse, ModelsDevSearchHit, ModelsDevSearchResponse, ModelsDevStatusResponse,
};
use nomifun_db::{IModelProfileRepository, IProviderRepository};
use nomifun_models_dev::{ModelsDevClient, resolve_catalog_capabilities};
use nomifun_models_dev::parse;
use tracing::warn;

use super::reconciler::CatalogReconciler;

pub struct ModelsCatalogService {
    client: Arc<ModelsDevClient>,
    reconciler: CatalogReconciler,
    provider_repo: Arc<dyn IProviderRepository>,
}

impl ModelsCatalogService {
    pub fn new(
        client: Arc<ModelsDevClient>,
        profile_repo: Arc<dyn IModelProfileRepository>,
        provider_repo: Arc<dyn IProviderRepository>,
    ) -> Self {
        let reconciler = CatalogReconciler::new(client.clone(), profile_repo);
        Self {
            client,
            reconciler,
            provider_repo,
        }
    }

    pub fn client(&self) -> &ModelsDevClient {
        self.client.as_ref()
    }

    pub fn status(&self) -> ModelsDevStatusResponse {
        let s = self.client.status();
        ModelsDevStatusResponse {
            populated: s.populated,
            cache_age_secs: s.cache_age_secs,
            last_error: s.last_error,
            provider_count: s.provider_count,
            model_count: s.model_count,
            cache_path: s.cache_path.display().to_string(),
        }
    }

    pub async fn refresh(&self, force: bool) -> ModelsDevStatusResponse {
        let _ = self.client.fetch(force).await;
        self.status()
    }

    pub fn lookup(&self, platform: &str, model: &str) -> ModelsDevLookupResponse {
        match resolve_catalog_capabilities(self.client.as_ref(), platform, model) {
            Some(caps) => ModelsDevLookupResponse {
                found: true,
                supports_tools: caps.supports_tools,
                supports_vision: caps.supports_vision,
                supports_reasoning: caps.supports_reasoning,
                context_window: caps.context_window,
                max_output_tokens: caps.max_output_tokens,
                cost_input: caps.cost_input,
                cost_output: caps.cost_output,
                family: caps.family,
                status: caps.status,
                models_dev_provider: Some(caps.models_dev_provider),
            },
            None => ModelsDevLookupResponse {
                found: false,
                supports_tools: false,
                supports_vision: false,
                supports_reasoning: false,
                context_window: None,
                max_output_tokens: None,
                cost_input: None,
                cost_output: None,
                family: None,
                status: String::new(),
                models_dev_provider: None,
            },
        }
    }

    pub fn search(&self, q: &str, platform: Option<&str>, limit: usize) -> ModelsDevSearchResponse {
        let hits = self
            .client
            .search(q, platform, limit)
            .into_iter()
            .map(|hit| {
                let caps = parse::parse_model_capabilities(&hit.entry);
                ModelsDevSearchHit {
                    platform: hit.provider,
                    model_id: hit.model_id,
                    supports_tools: caps.supports_tools,
                    supports_vision: caps.supports_vision,
                    context_window: if caps.context_window > 0 {
                        Some(caps.context_window)
                    } else {
                        None
                    },
                }
            })
            .collect();
        ModelsDevSearchResponse { hits }
    }

    /// Reconcile catalog profiles for every provider model. Returns upsert count.
    pub async fn reconcile_all(&self) -> usize {
        let providers = match self.provider_repo.list().await {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "models-dev reconcile_all: failed to list providers");
                return 0;
            }
        };
        let mut total = 0usize;
        for provider in &providers {
            let models: Vec<String> = serde_json::from_str(&provider.models).unwrap_or_default();
            total += self
                .reconciler
                .reconcile_models(&provider.id, &provider.platform, &models)
                .await;
        }
        total
    }

    /// Reconcile catalog profiles for one provider's model list.
    pub async fn reconcile_provider_models(
        &self,
        provider_id: &str,
        platform: &str,
        models: &[String],
    ) -> usize {
        self.reconciler
            .reconcile_models(provider_id, platform, models)
            .await
    }
}
