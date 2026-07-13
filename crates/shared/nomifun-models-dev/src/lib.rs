//! models.dev registry integration for Nomifun.
//!
//! Fetches the community registry at <https://models.dev/api.json> with an
//! in-memory + disk cache, and exposes typed helpers for context lookup,
//! capability resolution, agentic-model filtering and fuzzy search.
//!
//! # Architecture
//!
//! - [`types`]    — `ModelInfo`, `ProviderInfo`, `ModelCapabilities`
//! - [`mapping`]  — Nomifun platform ↔ models.dev provider ID + merge policy
//! - [`cache`]    — atomic disk-cache load/save
//! - [`parse`]    — defensive JSON → typed-struct converters
//! - [`noise`]    — Google hidden models + noise regex
//! - [`client`]   — `ModelsDevClient` (HTTP + cache + queries)
//! - [`resolve`]  — catalog capability resolution for Nomifun platforms
//!
//! # Quick start
//!
//! ```ignore
//! use nomifun_models_dev::default_client;
//!
//! # async fn run() {
//! let client = default_client();
//! let _ = client.fetch(false).await;
//! let ctx = client.lookup_context("anthropic", "claude-sonnet-4-5");
//! # }
//! ```
//!
//! For tests that need to avoid the network, construct a custom
//! [`ModelsDevClient`] and call [`ModelsDevClient::seed_cache`].

pub mod cache;
pub mod client;
pub mod mapping;
pub mod noise;
pub mod parse;
pub mod resolve;
pub mod types;

pub use client::{MODELS_DEV_URL, ModelsDevClient, RegistryStatus, SearchHit};
pub use mapping::{
    MergePolicy, PlatformMapEntry, all_mapped_platforms, forward_map, merge_policy,
    resolve_models_dev_id, reverse_map, to_models_dev, to_nomifun,
};
pub use noise::{GOOGLE_HIDDEN_MODELS, noise_re, should_hide};
pub use resolve::{CatalogCapabilities, catalog_vision_hint, resolve_catalog_capabilities};
pub use types::{InterleavedFlag, ModelCapabilities, ModelInfo, ProviderInfo};

use std::sync::{Arc, OnceLock};

fn client_cell() -> &'static Arc<ModelsDevClient> {
    static CLIENT: OnceLock<Arc<ModelsDevClient>> = OnceLock::new();
    CLIENT.get_or_init(|| Arc::new(ModelsDevClient::default_production()))
}

/// Process-wide shared [`ModelsDevClient`] (`Arc` for DI).
///
/// Same underlying instance as [`default_client`]. Prefer this when storing the
/// client in `AppState` / services.
pub fn shared_client() -> Arc<ModelsDevClient> {
    client_cell().clone()
}

/// Process-wide default [`ModelsDevClient`] reference.
///
/// Lazily initialised on first call. Uses the production endpoint and
/// [`cache::default_cache_path`].
pub fn default_client() -> &'static ModelsDevClient {
    client_cell().as_ref()
}
