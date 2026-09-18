//! Production [`KnowledgeCompleter`]: resolves a default provider/model and
//! runs a one-shot completion. Same layering as the companion learner's
//! `LiveCompanionCompleter` and the IDMM sidecar's `LiveCompleter` — the knowledge
//! crate holds only the trait, this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `KnowledgeService::set_completer`.
//!
//! Unlike companion/IDMM there is no per-feature model setting (yet): knowledge
//! autogen is a background curation task, so the default is the first
//! enabled provider (registry creation order) and its first enabled model.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nomifun_api_types::ModelTask;
use nomifun_common::AppError;
use nomifun_db::{
    FLOWY_CATALOG_FAMILY_PARAM, IProviderModelRepository, IProviderRepository, ProviderModelRow,
    models::Provider,
};
use nomifun_knowledge::KnowledgeCompleter;

use crate::factory::provider_config::{one_shot_completion, resolve_provider_config, user_message};

/// READMEs can be sizeable; keep enough room that the strict-JSON overview
/// reply (description + full readme_markdown) never gets cut mid-object —
/// a truncated reply is guaranteed-unparseable. The prompt side also bounds
/// the README length (see `autogen::OVERVIEW_SYSTEM`).
const KNOWLEDGE_MAX_TOKENS: u32 = 8192;

/// Provider-backed completer for knowledge autogen / snapshot compression.
pub struct LiveKnowledgeCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl LiveKnowledgeCompleter {
    /// First enabled provider (creation order) + its first enabled model.
    async fn resolve_default_model(&self) -> Result<(String, String), AppError> {
        resolve_default_model(&self.provider_repo, &self.provider_model_repo)
            .await
            .ok_or_else(|| {
                AppError::Conflict(
                    "knowledge autogen unavailable: no enabled provider/model is configured"
                        .into(),
                )
            })
    }

    /// Resolve the given `(provider_id, model)` into a provider config and run
    /// the one-shot completion. Shared by [`KnowledgeCompleter::complete`]
    /// (which feeds it the default pick) and
    /// [`KnowledgeCompleter::complete_with`] (which feeds it the caller's
    /// explicit pick), so the resolve→complete tail is identical regardless
    /// of how the model was chosen.
    async fn complete_for_model(
        &self,
        system: &str,
        user: &str,
        provider_id: &str,
        model: &str,
    ) -> Result<String, AppError> {
        let cfg = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            provider_id,
            model,
            &self.workspace,
        )
        .await?;
        one_shot_completion(&cfg, system, vec![user_message(user)], KNOWLEDGE_MAX_TOKENS).await
    }
}

#[async_trait::async_trait]
impl KnowledgeCompleter for LiveKnowledgeCompleter {
    async fn complete(&self, system: &str, user: &str) -> Result<String, AppError> {
        let (provider_id, model) = self.resolve_default_model().await?;
        self.complete_for_model(system, user, &provider_id, &model).await
    }

    /// Honor the caller's explicit `(provider_id, model)`, skipping the
    /// default-model resolution entirely — the knowledge UI uses this to let
    /// the user pick which model generates/regenerates a base.
    async fn complete_with(
        &self,
        system: &str,
        user: &str,
        provider_id: &str,
        model: &str,
    ) -> Result<String, AppError> {
        self.complete_for_model(system, user, provider_id, model).await
    }
}

/// First entry of a provider's `provider_models` rows (already ordered by
/// `sort_order`) that is enabled, with a trimmed non-empty model id — the row
/// replacement for the retired `models` array + `model_enabled` map columns.
pub(crate) fn first_enabled_model<'a, I>(rows: I) -> Option<String>
where
    I: IntoIterator<Item = &'a ProviderModelRow>,
{
    rows.into_iter()
        .filter(|row| row.enabled)
        .map(|row| row.model.trim().to_owned())
        .find(|model| !model.is_empty())
}

/// Whether a provider-model row was tagged by the Flowy catalog as one of the
/// automatic routing models. Auto models are valid for an explicit chat choice
/// but should not become an implicit background default.
pub(crate) fn is_auto_catalog_model(row: &ProviderModelRow) -> bool {
    serde_json::from_str::<serde_json::Value>(&row.params)
        .ok()
        .and_then(|value| {
            value
                .get(FLOWY_CATALOG_FAMILY_PARAM)
                .and_then(serde_json::Value::as_str)
                .map(|family| family == "auto")
        })
        .unwrap_or(false)
}

/// First enabled non-Auto model in catalog order.
pub(crate) fn first_enabled_cloud_model<'a, I>(rows: I) -> Option<String>
where
    I: IntoIterator<Item = &'a ProviderModelRow>,
{
    rows.into_iter()
        .filter(|row| !is_auto_catalog_model(row))
        .find_map(|row| {
            row.enabled
                .then(|| row.model.trim().to_owned())
                .filter(|model| !model.is_empty())
        })
}

/// Resolve the app's DEFAULT `(provider_id, model)`: the first enabled provider
/// (creation order) and its first enabled model (row `sort_order` order).
/// `None` when no enabled provider/model is configured. The shared "what model
/// would the app use by default" resolution — reused wherever a caller has no
/// explicit model.
pub async fn resolve_default_model(
    provider_repo: &std::sync::Arc<dyn IProviderRepository>,
    provider_model_repo: &std::sync::Arc<dyn IProviderModelRepository>,
) -> Option<(String, String)> {
    let providers = provider_repo.list().await.ok()?;
    // Rows come back ordered by (provider_id, sort_order, model), so each
    // provider's group preserves its catalog order.
    let rows = provider_model_repo.list().await.ok()?;
    let mut grouped: HashMap<&str, Vec<&ProviderModelRow>> = HashMap::new();
    for row in &rows {
        grouped.entry(row.provider_id.as_str()).or_default().push(row);
    }
    let eligible = |provider: &&Provider| {
        provider.enabled && !nomifun_common::managed_free_models_disabled(&provider.platform)
    };
    let select = |provider: &Provider| {
        let provider_rows = grouped.get(provider.provider_id.as_str())?;
        let model = if provider.provider_id == nomifun_common::FLOWY_BUILTIN_PROVIDER_ID {
            first_enabled_cloud_model(provider_rows.iter().copied())
        } else {
            first_enabled_model(provider_rows.iter().copied())
        }?;
        Some((provider.provider_id.clone(), model))
    };

    // Flowy Cloud is the product default when it is configured; retain the
    // historical first-enabled-provider fallback for local/test deployments
    // that do not have a Cloud catalog yet.
    providers
        .iter()
        .filter(eligible)
        .find(|provider| provider.provider_id == nomifun_common::FLOWY_BUILTIN_PROVIDER_ID)
        .and_then(select)
        .or_else(|| providers.iter().filter(eligible).find_map(select))
}

/// Resolve the first enabled Flowy Cloud chat-capable catalog row.
///
/// This is intentionally narrower than [`resolve_default_model`]: it is used
/// when a stale reference points at the reserved managed free supply, so the
/// fallback must never select an arbitrary local provider or an image/audio
/// model merely because it happens to be first.
pub async fn resolve_flowy_cloud_model(
    provider_repo: &std::sync::Arc<dyn IProviderRepository>,
    provider_model_repo: &std::sync::Arc<dyn IProviderModelRepository>,
) -> Option<(String, String)> {
    let provider = provider_repo
        .find_by_id(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .ok()??;
    if !provider.enabled {
        return None;
    }
    let rows = provider_model_repo
        .list_for_provider(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .ok()?;
    rows.into_iter().find_map(|row| {
        if !row.enabled || row.model.trim().is_empty() {
            return None;
        }
        if is_auto_catalog_model(&row) {
            return None;
        }
        let tasks = serde_json::from_str::<Vec<ModelTask>>(&row.tasks).unwrap_or_default();
        let tasks = if tasks.is_empty() {
            // Catalog rows from older syncs may not have materialized task
            // metadata yet. Derive it from the Cloud provider/model pair
            // before admitting a fallback; an arbitrary first row could be
            // vision, image, audio, or embedding-only.
            nomifun_api_types::derive_tasks_and_traits(&provider.platform, &row.model).0
        } else {
            tasks
        };
        if !tasks.contains(&ModelTask::Chat) {
            return None;
        }
        Some((provider.provider_id.clone(), row.model.trim().to_owned()))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nomifun_db::models::Provider;
    use nomifun_db::{
        CreateProviderParams, DbError, NewProviderModel, ProviderModelUpdate,
        UpdateProviderParams,
    };

    pub(crate) fn provider(id: &str, enabled: bool) -> Provider {
        Provider {
            id: 0,
            provider_id: id.into(),
            platform: "openai".into(),
            name: id.into(),
            base_url: String::new(),
            api_key_encrypted: String::new(),
            enabled,
            bedrock_config: None,
            is_full_url: false,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    pub(crate) fn model_row(
        provider_id: &str,
        model: &str,
        enabled: bool,
        sort_order: i64,
    ) -> ProviderModelRow {
        ProviderModelRow {
            id: 0,
            provider_id: provider_id.into(),
            model: model.into(),
            enabled,
            sort_order,
            tasks: "[]".into(),
            traits: "[]".into(),
            protocol: None,
            connection_role: None,
            params: "{}".into(),
            context_limit: None,
            output_limit: None,
            description: None,
            source: "inferred".into(),
            health: None,
            health_checked_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    pub(crate) struct ListOnlyRepo(pub(crate) Vec<Provider>);

    #[async_trait::async_trait]
    impl IProviderRepository for ListOnlyRepo {
        async fn list(&self) -> Result<Vec<Provider>, DbError> {
            Ok(self.0.clone())
        }
        async fn find_by_id(&self, id: &str) -> Result<Option<Provider>, DbError> {
            Ok(self.0.iter().find(|p| p.provider_id == id).cloned())
        }
        async fn create(&self, _params: CreateProviderParams<'_>) -> Result<Provider, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn update(&self, _id: &str, _params: UpdateProviderParams<'_>) -> Result<Provider, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn delete(&self, _id: &str) -> Result<(), DbError> {
            unimplemented!("not used by these tests")
        }
    }

    /// Row-list stub mirroring the ordering contract of the SQLite
    /// implementation (`provider_id, sort_order, model`).
    pub(crate) struct ListOnlyModelRepo(pub Vec<ProviderModelRow>);

    #[async_trait::async_trait]
    impl IProviderModelRepository for ListOnlyModelRepo {
        async fn list(&self) -> Result<Vec<ProviderModelRow>, DbError> {
            let mut rows = self.0.clone();
            rows.sort_by(|a, b| {
                (&a.provider_id, a.sort_order, &a.model)
                    .cmp(&(&b.provider_id, b.sort_order, &b.model))
            });
            Ok(rows)
        }
        async fn list_for_provider(
            &self,
            provider_id: &str,
        ) -> Result<Vec<ProviderModelRow>, DbError> {
            let mut rows: Vec<ProviderModelRow> = self
                .0
                .iter()
                .filter(|row| row.provider_id == provider_id)
                .cloned()
                .collect();
            rows.sort_by(|a, b| (a.sort_order, &a.model).cmp(&(b.sort_order, &b.model)));
            Ok(rows)
        }
        async fn get(
            &self,
            provider_id: &str,
            model: &str,
        ) -> Result<Option<ProviderModelRow>, DbError> {
            Ok(self
                .0
                .iter()
                .find(|row| row.provider_id == provider_id && row.model == model)
                .cloned())
        }
        async fn create(
            &self,
            _provider_id: &str,
            _row: &NewProviderModel<'_>,
        ) -> Result<ProviderModelRow, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn insert_if_absent(
            &self,
            _provider_id: &str,
            _row: &NewProviderModel<'_>,
        ) -> Result<bool, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn update(
            &self,
            _provider_id: &str,
            _model: &str,
            _update: &ProviderModelUpdate<'_>,
        ) -> Result<ProviderModelRow, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn set_health(
            &self,
            _provider_id: &str,
            _model: &str,
            _health_json: Option<&str>,
        ) -> Result<bool, DbError> {
            unimplemented!("not used by these tests")
        }
        async fn delete(&self, _provider_id: &str, _model: &str) -> Result<bool, DbError> {
            unimplemented!("not used by these tests")
        }
    }

    fn completer(
        providers: Vec<Provider>,
        rows: Vec<ProviderModelRow>,
    ) -> LiveKnowledgeCompleter {
        LiveKnowledgeCompleter {
            provider_repo: Arc::new(ListOnlyRepo(providers)),
            provider_model_repo: Arc::new(ListOnlyModelRepo(rows)),
            encryption_key: [0u8; 32],
            workspace: std::env::temp_dir(),
        }
    }

    #[test]
    fn first_enabled_model_honors_row_enabled_flag() {
        let rows = vec![
            model_row("p", "m1", true, 0),
            model_row("p", "m2", true, 1),
        ];
        assert_eq!(first_enabled_model(&rows).as_deref(), Some("m1"));

        let rows = vec![
            model_row("p", "m1", false, 0),
            model_row("p", "m2", true, 1),
        ];
        assert_eq!(first_enabled_model(&rows).as_deref(), Some("m2"));

        let rows = vec![model_row("p", "m1", false, 0)];
        assert_eq!(first_enabled_model(&rows), None);

        let rows = vec![model_row("p", "  ", true, 0)];
        assert_eq!(first_enabled_model(&rows), None);
    }

    #[test]
    fn first_enabled_cloud_model_skips_implicit_auto_catalog_rows() {
        let mut auto = model_row("flowy", "AIPC-auto-balance", true, 0);
        auto.params = r#"{"_flowy_catalog_family":"auto","_flowy_catalog_auto_tier":"balance"}"#.into();
        let cloud = model_row("flowy", "AIPC-glm-5", true, 1);
        assert_eq!(
            first_enabled_cloud_model([&auto, &cloud].into_iter()).as_deref(),
            Some("AIPC-glm-5")
        );
        assert!(is_auto_catalog_model(&auto));
    }

    #[tokio::test]
    async fn default_model_skips_disabled_providers_and_models() {
        // Disabled provider first, then an enabled one whose first model row
        // is disabled — the pick must be (p2, m2).
        let c = completer(
            vec![provider("p1", false), provider("p2", true)],
            vec![
                model_row("p1", "m0", true, 0),
                model_row("p2", "m1", false, 0),
                model_row("p2", "m2", true, 1),
            ],
        );
        let (provider_id, model) = c.resolve_default_model().await.unwrap();
        assert_eq!((provider_id.as_str(), model.as_str()), ("p2", "m2"));
    }

    #[tokio::test]
    async fn default_model_skips_disabled_managed_free_provider() {
        if nomifun_common::free_models_enabled() {
            return;
        }

        let mut free = provider("free", true);
        free.platform = nomifun_common::FREE_MODEL_PLATFORM.into();
        let cloud = provider("cloud", true);
        let c = completer(
            vec![free, cloud],
            vec![model_row("free", "free-model", true, 0), model_row("cloud", "cloud-model", true, 0)],
        );

        let selected = c.resolve_default_model().await.unwrap();
        assert_eq!(selected, ("cloud".to_owned(), "cloud-model".to_owned()));
    }

    #[tokio::test]
    async fn resolve_default_model_free_fn_picks_first_enabled_else_none() {
        let repo: Arc<dyn IProviderRepository> =
            Arc::new(ListOnlyRepo(vec![provider("p1", false), provider("p2", true)]));
        let model_repo: Arc<dyn IProviderModelRepository> = Arc::new(ListOnlyModelRepo(vec![
            model_row("p1", "m0", true, 0),
            model_row("p2", "m1", false, 0),
            model_row("p2", "m2", true, 1),
        ]));
        assert_eq!(
            resolve_default_model(&repo, &model_repo).await,
            Some(("p2".to_owned(), "m2".to_owned()))
        );
        // No enabled provider/model → None (the caller then truthfully reports
        // no model rather than pretending one exists).
        let none: Arc<dyn IProviderRepository> =
            Arc::new(ListOnlyRepo(vec![provider("p", false)]));
        let none_rows: Arc<dyn IProviderModelRepository> =
            Arc::new(ListOnlyModelRepo(vec![model_row("p", "m", true, 0)]));
        assert_eq!(resolve_default_model(&none, &none_rows).await, None);
    }

    #[tokio::test]
    async fn flowy_default_model_does_not_promote_auto_to_background_default() {
        let cloud = provider(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID, true);
        let mut auto = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "AIPC-auto-balance",
            true,
            0,
        );
        auto.tasks = r#"["chat"]"#.into();
        auto.params = r#"{"_flowy_catalog_family":"auto","_flowy_catalog_auto_tier":"balance"}"#.into();
        let mut cloud_model = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "AIPC-glm-5",
            true,
            1,
        );
        cloud_model.tasks = r#"["chat"]"#.into();

        let provider_repo: Arc<dyn IProviderRepository> = Arc::new(ListOnlyRepo(vec![cloud]));
        let model_repo: Arc<dyn IProviderModelRepository> =
            Arc::new(ListOnlyModelRepo(vec![auto, cloud_model]));
        assert_eq!(
            resolve_default_model(&provider_repo, &model_repo).await,
            Some((
                nomifun_common::FLOWY_BUILTIN_PROVIDER_ID.to_owned(),
                "AIPC-glm-5".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn flowy_cloud_fallback_requires_a_chat_capable_row() {
        let cloud = provider(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID, true);
        let mut image = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "dall-e-3",
            true,
            0,
        );
        image.tasks = "[\"image_generation\"]".into();
        let mut chat = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "gpt-4o-mini",
            true,
            1,
        );
        chat.tasks = "[\"chat\"]".into();
        let provider_repo: Arc<dyn IProviderRepository> = Arc::new(ListOnlyRepo(vec![cloud]));
        let model_repo: Arc<dyn IProviderModelRepository> =
            Arc::new(ListOnlyModelRepo(vec![image, chat]));

        assert_eq!(
            resolve_flowy_cloud_model(&provider_repo, &model_repo).await,
            Some((
                nomifun_common::FLOWY_BUILTIN_PROVIDER_ID.to_owned(),
                "gpt-4o-mini".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn flowy_cloud_fallback_derives_missing_task_metadata() {
        let cloud = provider(nomifun_common::FLOWY_BUILTIN_PROVIDER_ID, true);
        let image = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "dall-e-3",
            true,
            0,
        );
        let chat = model_row(
            nomifun_common::FLOWY_BUILTIN_PROVIDER_ID,
            "gpt-4o-mini",
            true,
            1,
        );
        let provider_repo: Arc<dyn IProviderRepository> = Arc::new(ListOnlyRepo(vec![cloud]));
        let model_repo: Arc<dyn IProviderModelRepository> =
            Arc::new(ListOnlyModelRepo(vec![image, chat]));

        assert_eq!(
            resolve_flowy_cloud_model(&provider_repo, &model_repo).await,
            Some((
                nomifun_common::FLOWY_BUILTIN_PROVIDER_ID.to_owned(),
                "gpt-4o-mini".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn default_model_errors_with_clear_message_when_unconfigured() {
        let c = completer(vec![provider("p1", false)], vec![model_row("p1", "m0", true, 0)]);
        let err = c.resolve_default_model().await.unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
        assert!(err.to_string().contains("no enabled provider"), "{err}");
    }

    /// `complete_with` must resolve the EXPLICIT `(provider_id, model)` the
    /// caller passes, never the default. Pinned at the network-free
    /// resolution boundary: a provider id that the repo cannot find yields
    /// `BadRequest("Provider '…' not found")`. Passing the explicit `px`
    /// surfaces *that* id in the error — proving the override (not the
    /// enabled default `p1`) drove resolution.
    #[tokio::test]
    async fn complete_with_resolves_the_explicit_provider_not_the_default() {
        // `p1` is enabled, so the default path would pick it; `px` is absent.
        let c = completer(vec![provider("p1", true)], vec![model_row("p1", "m1", true, 0)]);
        let err = c
            .complete_with("sys", "usr", "px", "model-z")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err:?}");
        assert!(err.to_string().contains("Provider 'px' not found"), "{err}");
    }

    /// `complete` keeps using `resolve_default_model`: with the enabled
    /// default `p1` present in the repo it gets PAST the (network-free)
    /// provider-resolution stage — so the failure is NOT "Provider not
    /// found". This is the contrast to the override test above and confirms
    /// the default path is unchanged.
    #[tokio::test]
    async fn complete_uses_the_default_provider() {
        let c = completer(vec![provider("p1", true)], vec![model_row("p1", "m1", true, 0)]);
        // No network in tests: this will fail building/calling the provider,
        // but it must have resolved the existing default `p1` first — so the
        // error must not be a missing-provider BadRequest.
        let err = c.complete("sys", "usr").await.unwrap_err();
        assert!(
            !err.to_string().contains("not found"),
            "default path must resolve the existing provider p1, got: {err}"
        );
    }
}
