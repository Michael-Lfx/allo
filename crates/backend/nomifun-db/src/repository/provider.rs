use crate::error::DbError;
use crate::models::Provider;
use crate::repository::provider_model::IProviderModelRepository;

/// Inferred capability data for a catalog model.
///
/// Cloud catalog reconciliation uses this small owned value so the provider
/// row and the model-profile rows can be committed by one repository
/// operation.  User-authored profile rows are never represented here and are
/// therefore never overwritten by catalog sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelProfileSeed {
    pub model: String,
    pub tasks: String,
    pub traits: String,
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
        let inserted = model_repo
            .insert_if_absent(
                provider_id,
                &crate::models::NewProviderModel {
                    model: &seed.model,
                    enabled: true,
                    sort_order: next_sort,
                    tasks: &seed.tasks,
                    traits: &seed.traits,
                    params: "{}",
                    source: "inferred",
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
        if row.tasks.trim() != "[]" || row.source != "inferred" {
            continue;
        }
        let Some(seed) = seeds.get(row.model.as_str()) else {
            continue;
        };
        model_repo
            .update(
                provider_id,
                &row.model,
                &crate::models::ProviderModelUpdate {
                    tasks: Some(&seed.tasks),
                    traits: Some(&seed.traits),
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
