//! Noise / hidden-model filters for models.dev catalog listings.

use std::sync::OnceLock;

use regex::Regex;

/// Google models excluded from provider catalogs.
///
/// Verbatim port of `_GOOGLE_HIDDEN_MODELS` in `agent/models_dev.py`.
/// Low-TPM Gemma models and stale/retired Gemini slugs that still surface
/// through models.dev but either 404 on current Google endpoints or trip
/// quota walls under agent traffic.
pub const GOOGLE_HIDDEN_MODELS: &[&str] = &[
    "gemma-4-31b-it",
    "gemma-4-26b-it",
    "gemma-4-26b-a4b-it",
    "gemma-3-1b",
    "gemma-3-1b-it",
    "gemma-3-2b",
    "gemma-3-2b-it",
    "gemma-3-4b",
    "gemma-3-4b-it",
    "gemma-3-12b",
    "gemma-3-12b-it",
    "gemma-3-27b",
    "gemma-3-27b-it",
    "gemini-1.5-flash",
    "gemini-1.5-pro",
    "gemini-1.5-flash-8b",
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
];

/// Returns true when a model should be excluded from provider catalogs.
///
/// Currently only applies to Google/Gemini (low-TPM Gemma variants and
/// retired Gemini slugs). The provider argument may be either a Nomifun
/// platform ID (e.g. `"gemini"`) or a models.dev ID (`"google"`).
pub fn should_hide(provider: &str, model_id: &str) -> bool {
    let is_google = matches!(
        provider,
        "gemini" | "google" | "gemini-vertex-ai" | "vertex-ai"
    );
    is_google
        && GOOGLE_HIDDEN_MODELS
            .iter()
            .any(|h| h.eq_ignore_ascii_case(model_id))
}

/// Substring/regex patterns that indicate non-agentic / noise models —
/// TTS, embedding, dated previews, image-only, etc.
pub fn noise_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)-tts\b|embedding|live-|-(preview|exp)-\d{2,4}[-_]|-image\b|-image-preview\b|-customtools\b",
        )
        .expect("noise regex compiles")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_hide_is_case_insensitive() {
        assert!(should_hide("google", "Gemini-2.0-Flash"));
        assert!(should_hide("gemini", "GEMINI-1.5-PRO"));
        assert!(should_hide("vertex-ai", "gemini-2.0-flash"));
        assert!(!should_hide("anthropic", "gemini-2.0-flash"));
    }

    #[test]
    fn noise_re_matches_known_patterns() {
        assert!(noise_re().is_match("claude-3-tts"));
        assert!(noise_re().is_match("embedding-001"));
        assert!(noise_re().is_match("gemini-live-001"));
        assert!(!noise_re().is_match("claude-sonnet-4-5"));
    }
}
