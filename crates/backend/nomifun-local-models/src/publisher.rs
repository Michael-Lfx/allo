//! The provider-publish seam.
//!
//! `nomifun-local-models` never touches the provider table directly. The host
//! (`nomifun-app`) injects an implementation that upserts/disables/removes the
//! OpenAI-compatible provider row backing each running model.

use async_trait::async_trait;

/// Everything the host needs to upsert the provider row for a running model.
#[derive(Debug, Clone)]
pub struct LocalModelProviderSpec {
    /// Stable UUIDv7 recorded per model so repeated starts update one row.
    pub provider_id: String,
    /// Display name shown in the chat model picker.
    pub name: String,
    /// OpenAI-compatible base URL, e.g. `http://127.0.0.1:54321/v1`.
    pub base_url: String,
    /// Locally generated API key; also passed to `llama-server --api-key`.
    pub api_key: String,
    /// Model alias (= catalog id) served by this process.
    pub model: String,
    /// Context window to persist in `model_context_limits`.
    pub context_size: u64,
}

/// Outcome of a remove attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRemoveOutcome {
    /// The provider row was deleted.
    Deleted,
    /// The provider is still referenced; the row was disabled instead.
    Disabled,
}

/// Host-provided provider lifecycle bridge.
#[async_trait]
pub trait LocalModelProviderPublisher: Send + Sync {
    /// Create or update the provider row for a running model.
    async fn publish(&self, spec: &LocalModelProviderSpec) -> Result<(), String>;

    /// Disable the provider row so the chat resolver stops listing it.
    async fn disable(&self, provider_id: &str) -> Result<(), String>;

    /// Try to delete the provider row; disable it when still in use.
    async fn remove(&self, provider_id: &str) -> Result<ProviderRemoveOutcome, String>;
}
