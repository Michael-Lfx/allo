//! Resolve Nomifun platform + model into catalog capability fields.
//!
//! Types are kept local (no `nomifun-api-types` dependency) to avoid cycles.

use super::client::ModelsDevClient;
use super::mapping::{self, MergePolicy};

/// Catalog capabilities resolved from models.dev for a Nomifun platform + model.
#[derive(Debug, Clone)]
pub struct CatalogCapabilities {
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    /// `None` when the registry reports 0 / unknown.
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub cost_input: Option<f64>,
    pub cost_output: Option<f64>,
    pub family: Option<String>,
    pub status: String,
    pub models_dev_provider: String,
}

/// Resolve vision support from the in-memory registry without a platform hint.
///
/// Finds an exact (case-insensitive) model id via [`ModelsDevClient::search`],
/// then reads the `attachment` / modalities flags. Returns `None` when the
/// registry is empty or the model is unknown — callers should fall back to
/// name heuristics.
pub fn catalog_vision_hint(client: &ModelsDevClient, model: &str) -> Option<bool> {
    let hits = client.search(model, None, 8);
    let hit = hits
        .iter()
        .find(|h| h.model_id.eq_ignore_ascii_case(model))?;
    let caps = super::parse::parse_model_capabilities(&hit.entry);
    // Prefer full ModelInfo vision (attachment OR image modality) when possible.
    if let Some(info) = client.model_info(&hit.provider, &hit.model_id) {
        return Some(info.supports_vision());
    }
    Some(caps.supports_vision)
}

/// Look up catalog capabilities for a Nomifun platform + model.
///
/// Returns `None` when the platform's [`MergePolicy`] is [`MergePolicy::Never`],
/// when there is no models.dev mapping, or when the model is not in the registry.
pub fn resolve_catalog_capabilities(
    client: &ModelsDevClient,
    platform: &str,
    model: &str,
) -> Option<CatalogCapabilities> {
    if mapping::merge_policy(platform) == MergePolicy::Never {
        return None;
    }
    let mdev = mapping::to_models_dev(platform)?;
    let info = client.model_info(platform, model)?;

    let context_window = if info.context_window > 0 {
        Some(info.context_window)
    } else {
        None
    };
    let max_output_tokens = if info.max_output > 0 {
        Some(info.max_output)
    } else {
        None
    };
    let cost_input = if info.cost_input > 0.0 {
        Some(info.cost_input)
    } else {
        None
    };
    let cost_output = if info.cost_output > 0.0 {
        Some(info.cost_output)
    } else {
        None
    };
    let family = if info.family.is_empty() {
        None
    } else {
        Some(info.family.clone())
    };

    Some(CatalogCapabilities {
        supports_tools: info.tool_call,
        supports_vision: info.supports_vision(),
        supports_reasoning: info.reasoning,
        context_window,
        max_output_tokens,
        cost_input,
        cost_output,
        family,
        status: info.status.clone(),
        models_dev_provider: mdev.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn client_with_fixture() -> ModelsDevClient {
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
                        "name": "Claude Sonnet 4.5",
                        "family": "claude",
                        "tool_call": true,
                        "attachment": true,
                        "reasoning": true,
                        "limit": {"context": 200000, "output": 8192},
                        "cost": {"input": 3.0, "output": 15.0},
                        "status": ""
                    },
                    "zero-ctx": {
                        "tool_call": false,
                        "limit": {"context": 0, "output": 0}
                    }
                }
            },
            "google": {
                "models": {
                    "gemini-2.5-pro": {
                        "tool_call": true,
                        "attachment": true,
                        "family": "gemini",
                        "limit": {"context": 1048576, "output": 65536}
                    }
                }
            }
        }));
        std::mem::forget(dir);
        c
    }

    #[test]
    fn resolve_returns_caps_for_mapped_platform() {
        let c = client_with_fixture();
        let caps = resolve_catalog_capabilities(&c, "anthropic", "claude-sonnet-4-5").unwrap();
        assert!(caps.supports_tools);
        assert!(caps.supports_vision);
        assert!(caps.supports_reasoning);
        assert_eq!(caps.context_window, Some(200_000));
        assert_eq!(caps.max_output_tokens, Some(8_192));
        assert_eq!(caps.cost_input, Some(3.0));
        assert_eq!(caps.cost_output, Some(15.0));
        assert_eq!(caps.family.as_deref(), Some("claude"));
        assert_eq!(caps.models_dev_provider, "anthropic");
    }

    #[test]
    fn resolve_follows_platform_alias() {
        let c = client_with_fixture();
        let caps = resolve_catalog_capabilities(&c, "claude", "claude-sonnet-4-5").unwrap();
        assert_eq!(caps.models_dev_provider, "anthropic");

        let caps = resolve_catalog_capabilities(&c, "gemini", "gemini-2.5-pro").unwrap();
        assert_eq!(caps.models_dev_provider, "google");
        assert_eq!(caps.context_window, Some(1_048_576));
    }

    #[test]
    fn resolve_maps_zero_context_to_none() {
        let c = client_with_fixture();
        let caps = resolve_catalog_capabilities(&c, "anthropic", "zero-ctx").unwrap();
        assert_eq!(caps.context_window, None);
        assert_eq!(caps.max_output_tokens, None);
        assert!(!caps.supports_tools);
    }

    #[test]
    fn resolve_returns_none_for_never_platforms() {
        let c = client_with_fixture();
        assert!(resolve_catalog_capabilities(&c, "bedrock", "any").is_none());
        assert!(resolve_catalog_capabilities(&c, "custom", "any").is_none());
        assert!(resolve_catalog_capabilities(&c, "ark-cn", "any").is_none());
        assert!(resolve_catalog_capabilities(&c, "siliconflow", "any").is_none());
    }

    #[test]
    fn resolve_returns_none_for_unknown_model() {
        let c = client_with_fixture();
        assert!(resolve_catalog_capabilities(&c, "anthropic", "nope").is_none());
    }

    #[test]
    fn catalog_vision_hint_finds_exact_model() {
        let c = client_with_fixture();
        assert_eq!(catalog_vision_hint(&c, "claude-sonnet-4-5"), Some(true));
        assert_eq!(catalog_vision_hint(&c, "zero-ctx"), Some(false));
        assert_eq!(catalog_vision_hint(&c, "totally-unknown"), None);
    }
}
