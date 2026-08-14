//! Shared policy for the optional NomiFun-managed free-model supply.
//!
//! The policy lives in the dependency-light common crate so every request
//! path (application startup, Nomi, and the multimodal invoke layer) applies
//! the same default-off gate without introducing a crate dependency cycle.

pub const FREE_MODEL_PLATFORM: &str = "nomifun-free-model";
pub const FREE_MODELS_ENV: &str = "NOMIFUN_ENABLE_FREE_MODELS";
pub const MANAGED_FREE_MODELS_DISABLED_CODE: &str = "MANAGED_FREE_MODELS_DISABLED";

static FREE_MODELS_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Parse the accepted truthy spellings for a feature environment variable.
pub fn parse_enabled_value(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// Whether the optional managed free-model supply is enabled for this process.
///
/// The feature is deliberately default-off. It is an operator/runtime switch,
/// not a user preference, and is read once per process so changing the
/// environment after startup cannot create a half-enabled service graph.
pub fn free_models_enabled() -> bool {
    *FREE_MODELS_ENABLED.get_or_init(|| {
        std::env::var(FREE_MODELS_ENV)
            .map(|value| parse_enabled_value(&value))
            .unwrap_or(false)
    })
}

pub fn is_free_model_platform(platform: &str) -> bool {
    platform.trim().eq_ignore_ascii_case(FREE_MODEL_PLATFORM)
}

/// Whether a provider platform is the reserved managed supply and that supply
/// is unavailable for this process. Keeping this predicate beside the feature
/// flag prevents request, health, preset, and runtime wiring from drifting.
pub fn managed_free_models_disabled(platform: &str) -> bool {
    !free_models_enabled() && is_free_model_platform(platform)
}

/// Match the stable disabled-supply error after an async execution boundary
/// has flattened a typed error into a log/run-history string.
pub fn is_managed_free_models_disabled_message(message: &str) -> bool {
    message.contains(MANAGED_FREE_MODELS_DISABLED_CODE)
        || message
            .to_ascii_lowercase()
            .contains("managed free models are disabled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_values_enable_the_feature() {
        for value in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(parse_enabled_value(value), "{value}");
        }
    }

    #[test]
    fn other_values_are_disabled() {
        for value in ["", "0", "false", "no", "off", "random"] {
            assert!(!parse_enabled_value(value), "{value}");
        }
    }

    #[test]
    fn platform_matching_is_exact_and_case_insensitive() {
        assert!(is_free_model_platform("nomifun-free-model"));
        assert!(is_free_model_platform(" NOMIFUN-FREE-MODEL "));
        assert!(!is_free_model_platform("opencode"));
        assert!(!is_free_model_platform("openai"));
    }

    #[test]
    fn disabled_predicate_is_scoped_to_the_reserved_platform() {
        assert!(!managed_free_models_disabled("opencode"));
        assert!(!managed_free_models_disabled("openai"));
    }
}
