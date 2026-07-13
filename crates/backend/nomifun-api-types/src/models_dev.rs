//! models.dev HTTP DTOs and catalog params helpers.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model_task::{ModelProfile, ModelTask, ModelTrait, ProfileSource, derive_tasks_and_traits};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevStatusResponse {
    pub populated: bool,
    pub cache_age_secs: Option<u64>,
    pub last_error: Option<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub cache_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevRefreshRequest {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevLookupQuery {
    pub platform: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevLookupResponse {
    pub found: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub cost_input: Option<f64>,
    pub cost_output: Option<f64>,
    pub family: Option<String>,
    pub status: String,
    pub models_dev_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevSearchQuery {
    pub q: String,
    pub platform: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevSearchHit {
    pub platform: String,
    pub model_id: String,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevSearchResponse {
    pub hits: Vec<ModelsDevSearchHit>,
}

/// Read `params.catalog.context_window` if present and > 0.
pub fn catalog_context_from_params(params: &Value) -> Option<u64> {
    params
        .get("catalog")?
        .get("context_window")?
        .as_u64()
        .filter(|v| *v > 0)
}

/// Build the `params.catalog` object to store on a catalog profile.
pub fn build_catalog_params(
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    cost_input: Option<f64>,
    cost_output: Option<f64>,
    family: Option<&str>,
    status: &str,
    models_dev_provider: &str,
    synced_at: i64,
) -> Value {
    let mut catalog = serde_json::Map::new();
    if let Some(v) = context_window {
        catalog.insert("context_window".into(), json!(v));
    }
    if let Some(v) = max_output_tokens {
        catalog.insert("max_output_tokens".into(), json!(v));
    }
    if let Some(v) = cost_input {
        catalog.insert("cost_input".into(), json!(v));
    }
    if let Some(v) = cost_output {
        catalog.insert("cost_output".into(), json!(v));
    }
    if let Some(f) = family {
        if !f.is_empty() {
            catalog.insert("family".into(), json!(f));
        }
    }
    catalog.insert("status".into(), json!(status));
    catalog.insert("models_dev_provider".into(), json!(models_dev_provider));
    catalog.insert("synced_at".into(), json!(synced_at));
    json!({ "catalog": catalog })
}

/// Map catalog capability flags → (tasks, traits). Chat by default; add traits.
pub fn catalog_to_tasks_traits(
    supports_tools: bool,
    supports_vision: bool,
    supports_reasoning: bool,
) -> (Vec<ModelTask>, Vec<ModelTrait>) {
    let tasks = vec![ModelTask::Chat];
    let mut traits = Vec::new();
    if supports_tools {
        traits.push(ModelTrait::FunctionCalling);
    }
    if supports_vision {
        traits.push(ModelTrait::VisionInput);
    }
    if supports_reasoning {
        traits.push(ModelTrait::Reasoning);
    }
    (tasks, traits)
}

/// Resolve modalities for router: profile VisionInput trait wins, else name heuristic.
pub fn resolve_model_modalities(model: &str, profile: Option<&ModelProfile>) -> Vec<String> {
    if let Some(p) = profile {
        if p.traits.contains(&ModelTrait::VisionInput) {
            return vec!["vision".to_string()];
        }
        // Explicit user/catalog profile without vision → trust it (no heuristic upgrade)
        if matches!(p.source, ProfileSource::User | ProfileSource::Catalog) {
            return Vec::new();
        }
    }
    crate::infer_model_modalities(model)
}

/// Effective tasks/traits: stored profile as-is; else derive.
pub fn resolve_effective_tasks_traits(
    platform: &str,
    model: &str,
    profile: Option<&ModelProfile>,
) -> (Vec<ModelTask>, Vec<ModelTrait>) {
    match profile {
        Some(p) => (p.tasks.clone(), p.traits.clone()),
        None => derive_tasks_and_traits(platform, model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_task::ModelProfile;

    #[test]
    fn catalog_context_from_params_reads_positive() {
        let params = json!({ "catalog": { "context_window": 200000 } });
        assert_eq!(catalog_context_from_params(&params), Some(200_000));
    }

    #[test]
    fn catalog_context_from_params_rejects_zero_and_missing() {
        assert_eq!(
            catalog_context_from_params(&json!({ "catalog": { "context_window": 0 } })),
            None
        );
        assert_eq!(catalog_context_from_params(&json!({})), None);
        assert_eq!(catalog_context_from_params(&json!({ "catalog": {} })), None);
    }

    #[test]
    fn resolve_model_modalities_vision_trait_wins() {
        let profile = ModelProfile {
            provider_id: "p".into(),
            model: "plain-name".into(),
            tasks: vec![ModelTask::Chat],
            traits: vec![ModelTrait::VisionInput],
            params: json!({}),
            source: ProfileSource::Catalog,
            updated_at: 0,
        };
        assert_eq!(
            resolve_model_modalities("plain-name", Some(&profile)),
            vec!["vision".to_string()]
        );
    }

    #[test]
    fn resolve_model_modalities_user_catalog_without_vision_blocks_heuristic() {
        let profile = ModelProfile {
            provider_id: "p".into(),
            model: "gpt-4o".into(),
            tasks: vec![ModelTask::Chat],
            traits: vec![],
            params: json!({}),
            source: ProfileSource::User,
            updated_at: 0,
        };
        assert!(resolve_model_modalities("gpt-4o", Some(&profile)).is_empty());

        let catalog = ModelProfile {
            source: ProfileSource::Catalog,
            ..profile.clone()
        };
        assert!(resolve_model_modalities("gpt-4o", Some(&catalog)).is_empty());
    }

    #[test]
    fn resolve_model_modalities_inferred_falls_back_to_heuristic() {
        let profile = ModelProfile {
            provider_id: "p".into(),
            model: "gpt-4o".into(),
            tasks: vec![ModelTask::Chat],
            traits: vec![],
            params: json!({}),
            source: ProfileSource::Inferred,
            updated_at: 0,
        };
        let modalities = resolve_model_modalities("gpt-4o", Some(&profile));
        assert!(modalities.contains(&"vision".to_string()));
        assert_eq!(
            resolve_model_modalities("gpt-4o", None),
            crate::infer_model_modalities("gpt-4o")
        );
    }

    #[test]
    fn catalog_to_tasks_traits_maps_flags() {
        let (tasks, traits) = catalog_to_tasks_traits(true, true, true);
        assert_eq!(tasks, vec![ModelTask::Chat]);
        assert!(traits.contains(&ModelTrait::FunctionCalling));
        assert!(traits.contains(&ModelTrait::VisionInput));
        assert!(traits.contains(&ModelTrait::Reasoning));

        let (tasks, traits) = catalog_to_tasks_traits(false, false, false);
        assert_eq!(tasks, vec![ModelTask::Chat]);
        assert!(traits.is_empty());
    }
}
