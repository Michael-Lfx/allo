//! De-identified insights contribution configuration (client → ops server).

use serde::{Deserialize, Serialize};

/// Controls opt-in upload of anonymized domain work packages for ops analytics.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InsightsConfig {
    #[serde(default)]
    pub contribution: InsightsContributionConfig,
}

/// Per-feature contribution toggles and REST transport settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsightsContributionConfig {
    /// Master switch — default off (opt-in).
    #[serde(default)]
    pub enabled: bool,

    /// REST batch ingest URL (e.g. `https://ops.example.com/v1/insights/batch`).
    #[serde(default)]
    pub endpoint: String,

    /// Legacy v2 toggle — kept for config.yaml / `hermes config` compat; v3 uses work packages.
    #[serde(default = "default_upload_interests")]
    pub upload_interests: bool,

    /// Legacy v2 toggle — kept for config.yaml / `hermes config` compat.
    #[serde(default = "default_upload_skills")]
    pub upload_skills: bool,

    /// Enqueue work packages at session end.
    #[serde(default = "default_on_session_end")]
    pub on_session_end: bool,

    /// Include sanitized SKILL.md body in work packages.
    #[serde(default = "default_redacted_body")]
    pub redacted_body: bool,

    /// Minimum resolution evidence tier to upload (`A`..`D`).
    #[serde(default = "default_min_evidence_tier")]
    pub min_evidence_tier: String,

    /// Verdicts excluded from upload (e.g. `abandoned`, `indeterminate`).
    #[serde(default = "default_exclude_verdicts")]
    pub exclude_verdicts: Vec<String>,

    /// Require at least one session skill binding to upload.
    #[serde(default = "default_require_skill_binding")]
    pub require_skill_binding: bool,

    /// Minimum user turns before a work session is eligible.
    #[serde(default = "default_min_work_turns")]
    pub min_work_turns: u32,

    /// Auto-build work packages when conversation thresholds are met.
    #[serde(default = "default_auto_extract_enabled")]
    pub auto_extract_enabled: bool,

    /// Idle seconds before the background scanner triggers insights extraction.
    #[serde(default = "default_auto_extract_idle_secs")]
    pub auto_extract_idle_secs: u64,

    /// Resolution labeling: `hybrid` (default), `llm`, or `rules`.
    #[serde(default = "default_resolution_mode")]
    pub resolution_mode: String,

    /// Run resolution LLM at session end when mode is `llm` or `hybrid`.
    #[serde(default = "default_resolution_llm_on_session_end")]
    pub resolution_llm_on_session_end: bool,

    /// Legacy v2 skill age gate — kept for config.yaml / `hermes config` compat.
    #[serde(default = "default_skill_min_age_hours")]
    pub skill_min_age_hours: u32,

    /// Refresh pending outbox payloads from disk before upload.
    #[serde(default = "default_upload_skills_refresh")]
    pub upload_skills_refresh: bool,

    /// `Authorization: Bearer` credential for the ops server.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "installation_token"
    )]
    pub auth_token: Option<String>,

    /// Opt-in (default off): mine tool-call patterns from normal conversations at
    /// session end and broadcast skill suggestions (optimization 1). When enabled,
    /// the session-end pipeline extracts repeated tool-call sequences and emits
    /// them as skill candidates, bridging the normal-dialog → evolution gap.
    #[serde(default)]
    pub skill_mining_enabled: bool,
}

fn default_upload_interests() -> bool {
    true
}

fn default_upload_skills() -> bool {
    true
}

fn default_on_session_end() -> bool {
    true
}

fn default_skill_min_age_hours() -> u32 {
    24
}

fn default_redacted_body() -> bool {
    true
}

fn default_min_evidence_tier() -> String {
    "C".to_string()
}

fn default_exclude_verdicts() -> Vec<String> {
    vec!["abandoned".to_string(), "indeterminate".to_string()]
}

fn default_require_skill_binding() -> bool {
    true
}

fn default_min_work_turns() -> u32 {
    2
}

fn default_auto_extract_enabled() -> bool {
    true
}

fn default_auto_extract_idle_secs() -> u64 {
    300
}

fn default_resolution_mode() -> String {
    "hybrid".to_string()
}

fn default_resolution_llm_on_session_end() -> bool {
    true
}

fn default_upload_skills_refresh() -> bool {
    true
}

impl Default for InsightsContributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            upload_interests: default_upload_interests(),
            upload_skills: default_upload_skills(),
            on_session_end: default_on_session_end(),
            redacted_body: default_redacted_body(),
            min_evidence_tier: default_min_evidence_tier(),
            exclude_verdicts: default_exclude_verdicts(),
            require_skill_binding: default_require_skill_binding(),
            min_work_turns: default_min_work_turns(),
            auto_extract_enabled: default_auto_extract_enabled(),
            auto_extract_idle_secs: default_auto_extract_idle_secs(),
            resolution_mode: default_resolution_mode(),
            resolution_llm_on_session_end: default_resolution_llm_on_session_end(),
            skill_min_age_hours: default_skill_min_age_hours(),
            upload_skills_refresh: default_upload_skills_refresh(),
            auth_token: None,
            skill_mining_enabled: false,
        }
    }
}

impl InsightsContributionConfig {
    pub fn effective_token(&self) -> Option<String> {
        if let Ok(env) = std::env::var("NOMIFUN_INSIGHTS_TOKEN") {
            let trimmed = env.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        self.auth_token
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    pub fn upload_ready(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty() && self.effective_token().is_some()
    }

    /// Whether proactive threshold-based work package extraction is active.
    pub fn proactive_extraction_enabled(&self) -> bool {
        self.enabled && self.auto_extract_enabled
    }

    fn tier_rank(tier: &str) -> u8 {
        match tier.trim().to_ascii_uppercase().as_str() {
            "A" => 4,
            "B" => 3,
            "C" => 2,
            "D" => 1,
            _ => 0,
        }
    }

    pub fn evidence_tier_meets_min(&self, tier: &str) -> bool {
        Self::tier_rank(tier) >= Self::tier_rank(&self.min_evidence_tier)
    }

    pub fn verdict_excluded(&self, verdict: &str) -> bool {
        self.exclude_verdicts.iter().any(|v| v == verdict)
    }

    pub fn resolution_mode_normalized(&self) -> String {
        self.resolution_mode.trim().to_ascii_lowercase()
    }

    pub fn resolution_uses_llm(&self) -> bool {
        matches!(self.resolution_mode_normalized().as_str(), "llm" | "hybrid")
    }

    pub fn resolution_uses_rules(&self) -> bool {
        matches!(
            self.resolution_mode_normalized().as_str(),
            "rules" | "hybrid"
        )
    }

    /// Session-end resolution LLM when mode is `llm` or `hybrid`.
    pub fn session_end_resolution_llm_enabled(&self) -> bool {
        if !self.enabled || !self.resolution_uses_llm() {
            return false;
        }
        self.resolution_llm_on_session_end
            || crate::gateway::env_var_enabled("NOMIFUN_INSIGHTS_RESOLUTION_LLM")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_opt_in_off() {
        let cfg = InsightsContributionConfig::default();
        assert!(!cfg.enabled);
        assert!(!cfg.upload_ready());
    }

    #[test]
    fn evidence_tier_gate() {
        let cfg = InsightsContributionConfig::default();
        assert!(cfg.evidence_tier_meets_min("C"));
        assert!(!cfg.evidence_tier_meets_min("D"));
    }

    #[test]
    fn legacy_v2_defaults_preserved() {
        let cfg = InsightsContributionConfig::default();
        assert!(cfg.upload_interests);
        assert!(cfg.upload_skills);
        assert_eq!(cfg.skill_min_age_hours, 24);
    }

    #[test]
    fn upload_ready_requires_bearer_token() {
        let mut cfg = InsightsContributionConfig {
            enabled: true,
            endpoint: "https://ops.example.com/v1/insights/batch".to_string(),
            ..Default::default()
        };
        assert!(!cfg.upload_ready());
        cfg.auth_token = Some("eyJhbGciOiJIUzI1NiJ9.test".to_string());
        assert!(cfg.upload_ready());
    }

    #[test]
    fn hybrid_resolution_defaults() {
        let cfg = InsightsContributionConfig::default();
        assert_eq!(cfg.resolution_mode, "hybrid");
        assert!(cfg.resolution_llm_on_session_end);
        assert!(!cfg.session_end_resolution_llm_enabled());
        let mut enabled = cfg.clone();
        enabled.enabled = true;
        assert!(enabled.session_end_resolution_llm_enabled());
    }

    #[test]
    fn auth_token_yaml_alias() {
        let yaml = r#"
enabled: true
endpoint: "https://ops.example.com/v1/insights/batch"
auth_token: "flowy-sk-test"
"#;
        let cfg: InsightsContributionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.effective_token().as_deref(), Some("flowy-sk-test"));
    }
}
