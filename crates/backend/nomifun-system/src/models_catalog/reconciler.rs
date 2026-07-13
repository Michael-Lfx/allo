//! Reconcile models.dev catalog capabilities into `model_profiles`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nomifun_api_types::{ProfileSource, build_catalog_params, catalog_to_tasks_traits};
use nomifun_db::{IModelProfileRepository, UpsertModelProfileParams};
use nomifun_models_dev::{MergePolicy, ModelsDevClient, merge_policy, resolve_catalog_capabilities};
use tracing::warn;

use crate::model_profile::source_from_str;

pub struct CatalogReconciler {
    client: Arc<ModelsDevClient>,
    profile_repo: Arc<dyn IModelProfileRepository>,
}

impl CatalogReconciler {
    pub fn new(client: Arc<ModelsDevClient>, profile_repo: Arc<dyn IModelProfileRepository>) -> Self {
        Self {
            client,
            profile_repo,
        }
    }

    /// Upsert catalog profiles for each `(provider_id, platform, model)`.
    ///
    /// - [`MergePolicy::Never`] → skip
    /// - missing registry entry → skip
    /// - existing `source = user` → never overwrite
    /// - missing / inferred / catalog → upsert with `source = catalog`
    pub async fn reconcile_models(
        &self,
        provider_id: &str,
        platform: &str,
        models: &[String],
    ) -> usize {
        if merge_policy(platform) == MergePolicy::Never {
            return 0;
        }

        let synced_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let mut upserted = 0usize;
        for model in models {
            let Some(caps) = resolve_catalog_capabilities(self.client.as_ref(), platform, model)
            else {
                continue;
            };

            match self.profile_repo.get(provider_id, model).await {
                Ok(Some(row)) => {
                    if source_from_str(&row.source) == ProfileSource::User {
                        continue;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        provider_id,
                        model,
                        error = %e,
                        "models-dev reconcile: failed to load profile"
                    );
                    continue;
                }
            }

            let (tasks, traits) = catalog_to_tasks_traits(
                caps.supports_tools,
                caps.supports_vision,
                caps.supports_reasoning,
            );
            let tasks_json = match serde_json::to_string(&tasks) {
                Ok(s) => s,
                Err(e) => {
                    warn!(provider_id, model, error = %e, "models-dev reconcile: serialize tasks");
                    continue;
                }
            };
            let traits_json = match serde_json::to_string(&traits) {
                Ok(s) => s,
                Err(e) => {
                    warn!(provider_id, model, error = %e, "models-dev reconcile: serialize traits");
                    continue;
                }
            };
            let params = build_catalog_params(
                caps.context_window,
                caps.max_output_tokens,
                caps.cost_input,
                caps.cost_output,
                caps.family.as_deref(),
                &caps.status,
                &caps.models_dev_provider,
                synced_at,
            );
            let params_json = match serde_json::to_string(&params) {
                Ok(s) => s,
                Err(e) => {
                    warn!(provider_id, model, error = %e, "models-dev reconcile: serialize params");
                    continue;
                }
            };

            match self
                .profile_repo
                .upsert(&UpsertModelProfileParams {
                    provider_id,
                    model,
                    tasks: &tasks_json,
                    traits: &traits_json,
                    params: &params_json,
                    source: "catalog",
                })
                .await
            {
                Ok(_) => {
                    upserted += 1;
                }
                Err(e) => {
                    warn!(
                        provider_id,
                        model,
                        error = %e,
                        "models-dev reconcile: upsert failed"
                    );
                }
            }
        }
        upserted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nomifun_db::{DbError, ModelProfileRow};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MemProfileRepo {
        rows: Mutex<HashMap<(String, String), ModelProfileRow>>,
    }

    impl MemProfileRepo {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl IModelProfileRepository for MemProfileRepo {
        async fn list(&self) -> Result<Vec<ModelProfileRow>, DbError> {
            Ok(self.rows.lock().unwrap().values().cloned().collect())
        }
        async fn list_for_provider(&self, provider_id: &str) -> Result<Vec<ModelProfileRow>, DbError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .values()
                .filter(|r| r.provider_id == provider_id)
                .cloned()
                .collect())
        }
        async fn get(&self, provider_id: &str, model: &str) -> Result<Option<ModelProfileRow>, DbError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(provider_id.to_string(), model.to_string()))
                .cloned())
        }
        async fn upsert(&self, params: &UpsertModelProfileParams<'_>) -> Result<ModelProfileRow, DbError> {
            let row = ModelProfileRow {
                provider_id: params.provider_id.to_string(),
                model: params.model.to_string(),
                tasks: params.tasks.to_string(),
                traits: params.traits.to_string(),
                params: params.params.to_string(),
                source: params.source.to_string(),
                updated_at: 1,
            };
            self.rows
                .lock()
                .unwrap()
                .insert((row.provider_id.clone(), row.model.clone()), row.clone());
            Ok(row)
        }
        async fn delete(&self, provider_id: &str, model: &str) -> Result<bool, DbError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .remove(&(provider_id.to_string(), model.to_string()))
                .is_some())
        }
    }

    fn seeded_client() -> Arc<ModelsDevClient> {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new(
            "http://invalid.invalid/api.json",
            dir.path().join("cache.json"),
            None,
        );
        c.seed_cache(json!({
            "anthropic": {
                "models": {
                    "claude-sonnet-4-5": {
                        "tool_call": true,
                        "attachment": true,
                        "reasoning": true,
                        "family": "claude",
                        "limit": {"context": 200000, "output": 8192},
                        "cost": {"input": 3.0, "output": 15.0}
                    }
                }
            }
        }));
        std::mem::forget(dir);
        Arc::new(c)
    }

    #[tokio::test]
    async fn reconcile_inserts_catalog_and_skips_user() {
        let repo = Arc::new(MemProfileRepo::new());
        // Pre-seed a user override for a different model, and nothing for sonnet.
        repo.upsert(&UpsertModelProfileParams {
            provider_id: "p1",
            model: "user-model",
            tasks: r#"["chat"]"#,
            traits: "[]",
            params: "{}",
            source: "user",
        })
        .await
        .unwrap();

        let reconciler = CatalogReconciler::new(seeded_client(), repo.clone());
        let n = reconciler
            .reconcile_models(
                "p1",
                "anthropic",
                &["claude-sonnet-4-5".into(), "user-model".into(), "missing".into()],
            )
            .await;
        assert_eq!(n, 1);

        let sonnet = repo.get("p1", "claude-sonnet-4-5").await.unwrap().unwrap();
        assert_eq!(sonnet.source, "catalog");
        assert!(sonnet.traits.contains("vision_input"));
        assert!(sonnet.params.contains("context_window"));

        let user = repo.get("p1", "user-model").await.unwrap().unwrap();
        assert_eq!(user.source, "user");
    }

    #[tokio::test]
    async fn reconcile_upgrades_inferred_to_catalog() {
        let repo = Arc::new(MemProfileRepo::new());
        repo.upsert(&UpsertModelProfileParams {
            provider_id: "p1",
            model: "claude-sonnet-4-5",
            tasks: r#"["chat"]"#,
            traits: "[]",
            params: "{}",
            source: "inferred",
        })
        .await
        .unwrap();

        let reconciler = CatalogReconciler::new(seeded_client(), repo.clone());
        let n = reconciler
            .reconcile_models("p1", "anthropic", &["claude-sonnet-4-5".into()])
            .await;
        assert_eq!(n, 1);
        let row = repo.get("p1", "claude-sonnet-4-5").await.unwrap().unwrap();
        assert_eq!(row.source, "catalog");
        assert!(row.traits.contains("function_calling"));
    }

    #[tokio::test]
    async fn reconcile_skips_never_platforms() {
        let repo = Arc::new(MemProfileRepo::new());
        let reconciler = CatalogReconciler::new(seeded_client(), repo.clone());
        let n = reconciler
            .reconcile_models("p1", "bedrock", &["claude-sonnet-4-5".into()])
            .await;
        assert_eq!(n, 0);
        assert!(repo.list().await.unwrap().is_empty());
    }
}
