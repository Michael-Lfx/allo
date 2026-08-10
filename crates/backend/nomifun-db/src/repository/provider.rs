use crate::error::DbError;
use crate::models::Provider;
use crate::repository::provider_model::IProviderModelRepository;

/// Reserved `provider_models.params` key for a server-authoritative Flowy
/// catalog output limit. It is maintained during catalog sync and is not a
/// user-authored model parameter.
pub const FLOWY_CATALOG_MAX_TOKENS_PARAM: &str = "_flowy_catalog_max_tokens";

/// Reserved `provider_models.params` key for server-advertised
/// `reasoning_effort` levels (Flowy catalog `extra.reasoning_effort`).
/// Maintained during catalog sync; not a user-authored model parameter.
pub const FLOWY_CATALOG_REASONING_EFFORT_PARAM: &str = "_flowy_catalog_reasoning_effort";

/// Inferred capability data for a catalog model.
///
/// Cloud catalog reconciliation uses this small owned value so the provider
/// row and the model-profile rows can be committed by one repository
/// operation. User-authored profile fields remain authoritative; catalog-owned
/// metadata is merged through its reserved parameter keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelProfileSeed {
    pub model: String,
    pub tasks: String,
    pub traits: String,
    /// Cloud catalog output limit. `None` removes the managed key while
    /// preserving all other model parameters.
    pub catalog_max_tokens: Option<u32>,
    /// Cloud-advertised reasoning effort levels when the model supports
    /// deep thinking. `None` removes the managed key.
    pub catalog_reasoning_effort: Option<Vec<String>>,
    /// Whether the authoritative catalog declares image input. `None` means
    /// this seed has no catalog-owned vision capability.
    pub catalog_vision: Option<bool>,
}

pub(crate) fn initial_catalog_params(
    max_tokens: Option<u32>,
    reasoning_effort: Option<&[String]>,
) -> String {
    let mut object = serde_json::Map::new();
    if let Some(max_tokens) = max_tokens {
        object.insert(
            FLOWY_CATALOG_MAX_TOKENS_PARAM.to_string(),
            serde_json::json!(max_tokens),
        );
    }
    if let Some(levels) = reasoning_effort {
        object.insert(
            FLOWY_CATALOG_REASONING_EFFORT_PARAM.to_string(),
            serde_json::json!(levels),
        );
    }
    serde_json::Value::Object(object).to_string()
}

/// Merge catalog-owned metadata into an existing user-extensible parameter
/// object. Invalid/non-object JSON is left untouched rather than replacing
/// user data during a background sync.
///
/// Returns `None` when the merged object is identical to the input (no write).
pub(crate) fn merge_catalog_params(
    params: &str,
    max_tokens: Option<u32>,
    reasoning_effort: Option<&[String]>,
) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(params).ok()?;
    let object = value.as_object_mut()?;
    let mut changed = false;

    match max_tokens {
        Some(max_tokens) => {
            if object
                .get(FLOWY_CATALOG_MAX_TOKENS_PARAM)
                .and_then(serde_json::Value::as_u64)
                != Some(u64::from(max_tokens))
            {
                object.insert(
                    FLOWY_CATALOG_MAX_TOKENS_PARAM.to_string(),
                    serde_json::json!(max_tokens),
                );
                changed = true;
            }
        }
        None => {
            if object.remove(FLOWY_CATALOG_MAX_TOKENS_PARAM).is_some() {
                changed = true;
            }
        }
    }

    match reasoning_effort {
        Some(levels) => {
            let next = serde_json::json!(levels);
            if object.get(FLOWY_CATALOG_REASONING_EFFORT_PARAM) != Some(&next) {
                object.insert(FLOWY_CATALOG_REASONING_EFFORT_PARAM.to_string(), next);
                changed = true;
            }
        }
        None => {
            if object
                .remove(FLOWY_CATALOG_REASONING_EFFORT_PARAM)
                .is_some()
            {
                changed = true;
            }
        }
    }

    if !changed {
        return None;
    }
    serde_json::to_string(&value).ok()
}

/// Merge only the catalog-owned `vision_input` trait. Other model traits are
/// user-extensible and deliberately remain untouched during a catalog sync.
pub(crate) fn merge_catalog_vision_trait(traits: &str, supports_vision: bool) -> Option<String> {
    let mut traits: Vec<String> = serde_json::from_str(traits).ok()?;
    let has_vision = traits.iter().any(|trait_name| trait_name == "vision_input");
    if supports_vision == has_vision {
        return None;
    }
    if supports_vision {
        traits.push("vision_input".to_owned());
    } else {
        traits.retain(|trait_name| trait_name != "vision_input");
    }
    serde_json::to_string(&traits).ok()
}

async fn reconcile_inferred_model_profiles(
    provider_id: &str,
    profiles: &[ProviderModelProfileSeed],
    model_repo: &dyn IProviderModelRepository,
) -> Result<(), DbError> {
    if profiles.is_empty() {
        return Ok(());
    }

    let existing = model_repo.list_for_provider(provider_id).await?;
    let mut known = existing
        .iter()
        .map(|row| row.model.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut next_sort = existing
        .iter()
        .map(|row| row.sort_order)
        .max()
        .map_or(0, |max| max + 1);

    for seed in profiles {
        if seed.model.trim().is_empty() || known.contains(seed.model.as_str()) {
            continue;
        }
        let params = initial_catalog_params(
            seed.catalog_max_tokens,
            seed.catalog_reasoning_effort.as_deref(),
        );
        let inserted = model_repo
            .insert_if_absent(
                provider_id,
                &crate::models::NewProviderModel {
                    model: &seed.model,
                    enabled: true,
                    sort_order: next_sort,
                    tasks: &seed.tasks,
                    traits: &seed.traits,
                    params: &params,
                    source: if seed.catalog_vision.is_some() {
                        "catalog"
                    } else {
                        "inferred"
                    },
                    ..Default::default()
                },
            )
            .await?;
        if inserted {
            known.insert(seed.model.as_str());
            next_sort += 1;
        }
    }

    let seeds = profiles
        .iter()
        .map(|seed| (seed.model.as_str(), seed))
        .collect::<std::collections::HashMap<_, _>>();
    for row in model_repo.list_for_provider(provider_id).await? {
        let Some(seed) = seeds.get(row.model.as_str()) else {
            continue;
        };
        let fill_inferred_profile = row.tasks.trim() == "[]" && row.source == "inferred";
        let params = merge_catalog_params(
            &row.params,
            seed.catalog_max_tokens,
            seed.catalog_reasoning_effort.as_deref(),
        );
        let promote_catalog_source = seed.catalog_vision.is_some() && row.source == "inferred";
        let traits = seed
            .catalog_vision
            .and_then(|supports_vision| merge_catalog_vision_trait(&row.traits, supports_vision));
        if !fill_inferred_profile && !promote_catalog_source && params.is_none() && traits.is_none() {
            continue;
        }
        model_repo
            .update(
                provider_id,
                &row.model,
                &crate::models::ProviderModelUpdate {
                    tasks: fill_inferred_profile.then_some(seed.tasks.as_str()),
                    traits: if fill_inferred_profile {
                        Some(seed.traits.as_str())
                    } else {
                        traits.as_deref()
                    },
                    params: params.as_deref(),
                    source: promote_catalog_source.then_some("catalog"),
                    ..Default::default()
                },
            )
            .await?;
    }

    Ok(())
}

/// Model provider data access abstraction.
///
/// Provides CRUD operations on the `providers` table.
/// API keys are stored encrypted; callers handle encryption/decryption.
#[async_trait::async_trait]
pub trait IProviderRepository: Send + Sync {
    /// Returns all providers, ordered by creation time ascending.
    async fn list(&self) -> Result<Vec<Provider>, DbError>;

    /// Finds a provider by ID, or `None` if not found.
    async fn find_by_id(&self, id: &str) -> Result<Option<Provider>, DbError>;

    /// Creates a new provider and returns the inserted row.
    async fn create(&self, params: CreateProviderParams<'_>) -> Result<Provider, DbError>;

    /// Updates an existing provider. Returns `DbError::NotFound` if the ID doesn't exist.
    async fn update(&self, id: &str, params: UpdateProviderParams<'_>) -> Result<Provider, DbError>;

    /// Deletes a provider by ID. Returns `DbError::NotFound` if the ID doesn't exist.
    async fn delete(&self, id: &str) -> Result<(), DbError>;

    /// Update a provider and reconcile inferred model profiles.
    ///
    /// SQLite overrides this with one transaction.  The default keeps custom
    /// repository implementations source-compatible; those implementations
    /// use the same idempotent reconciliation semantics but cannot promise
    /// cross-table atomicity unless they provide their own override.
    async fn update_with_model_profiles(
        &self,
        id: &str,
        params: UpdateProviderParams<'_>,
        profiles: &[ProviderModelProfileSeed],
        model_repo: &dyn IProviderModelRepository,
    ) -> Result<Provider, DbError> {
        let provider = self.update(id, params).await?;
        reconcile_inferred_model_profiles(id, profiles, model_repo).await?;
        Ok(provider)
    }

    /// Create a provider and reconcile inferred model profiles.
    ///
    /// See [`Self::update_with_model_profiles`] for the compatibility fallback
    /// and the SQLite implementation's atomic guarantee.
    async fn create_with_model_profiles(
        &self,
        params: CreateProviderParams<'_>,
        profiles: &[ProviderModelProfileSeed],
        model_repo: &dyn IProviderModelRepository,
    ) -> Result<Provider, DbError> {
        let provider = self.create(params).await?;
        reconcile_inferred_model_profiles(&provider.provider_id, profiles, model_repo).await?;
        Ok(provider)
    }
}

/// Parameters for creating a new provider.
///
/// `models` and the four per-model map params (`model_context_limits`,
/// `model_protocols`, `model_descriptions`, `model_enabled`) are wire-compat
/// INPUTS only: migration 022 dropped the matching providers columns, so
/// these params feed exclusively the `provider_models` row sync
/// (`sync_provider_models_tx`) — one row per `models` entry, mirrored columns
/// seeded from the maps. They are never persisted on the providers row.
///
/// There is deliberately no `model_health` param: since P3 the server-side
/// health probe (`IProviderModelRepository::set_health`) is the only health
/// writer, and no `capabilities` param: migration 023 dropped the column.
#[derive(Debug)]
pub struct CreateProviderParams<'a> {
    /// Optional caller-supplied stable business ID.
    pub provider_id: Option<&'a str>,
    pub platform: &'a str,
    pub name: &'a str,
    pub base_url: &'a str,
    pub api_key_encrypted: &'a str,
    pub models: &'a str,
    pub enabled: bool,
    pub model_context_limits: Option<&'a str>,
    pub model_protocols: Option<&'a str>,
    pub model_descriptions: Option<&'a str>,
    pub model_enabled: Option<&'a str>,
    pub bedrock_config: Option<&'a str>,
    pub is_full_url: bool,
    /// Optional explicit provider priority. Omitted means append after current max.
    pub sort_order: Option<i64>,
}

/// Parameters for updating an existing provider.
///
/// All fields are optional; `None` means "keep the current value".
///
/// Like [`CreateProviderParams`], `models` and the four per-model map params
/// are wire-compat INPUTS that only drive the `provider_models` row sync:
/// `models: Some` replaces membership (insert new rows, delete removed,
/// re-index survivors); a map param `Some(...)` is a whole-map replacement of
/// that mirrored column across ALL rows (`Some(None)` = empty map → column
/// defaults); a map param `None` leaves existing rows untouched. Health is
/// intentionally not updatable here — `set_health` is the only write path.
#[derive(Debug, Default)]
pub struct UpdateProviderParams<'a> {
    pub platform: Option<&'a str>,
    pub name: Option<&'a str>,
    pub base_url: Option<&'a str>,
    pub api_key_encrypted: Option<&'a str>,
    pub models: Option<&'a str>,
    pub enabled: Option<bool>,
    pub model_context_limits: Option<Option<&'a str>>,
    pub model_protocols: Option<Option<&'a str>>,
    pub model_descriptions: Option<Option<&'a str>>,
    pub model_enabled: Option<Option<&'a str>>,
    pub bedrock_config: Option<Option<&'a str>>,
    pub is_full_url: Option<bool>,
    pub sort_order: Option<i64>,
}
