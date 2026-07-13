//! Nomifun platform ↔ models.dev provider ID mapping and merge policies.

use std::collections::HashMap;
use std::sync::OnceLock;

/// How catalog data from models.dev should be merged into Nomifun provider state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePolicy {
    /// Do not consult models.dev for this platform.
    Never,
    /// Use models.dev only to enrich fields that Nomifun does not already know.
    EnrichOnly,
    /// Merge model lists from models.dev with Nomifun's own list.
    ListMerge,
}

/// One row in the Nomifun → models.dev platform map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformMapEntry {
    pub platform: &'static str,
    pub models_dev: &'static str,
    pub merge_policy: MergePolicy,
}

/// Platforms that map into models.dev (EnrichOnly / ListMerge).
const MAPPED: &[PlatformMapEntry] = &[
    PlatformMapEntry {
        platform: "anthropic",
        models_dev: "anthropic",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "claude",
        models_dev: "anthropic",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "gemini",
        models_dev: "google",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "gemini-vertex-ai",
        models_dev: "google",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "vertex-ai",
        models_dev: "google",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "deepseek",
        models_dev: "deepseek",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "openrouter",
        models_dev: "openrouter",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "groq",
        models_dev: "groq",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "xai",
        models_dev: "xai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "mistral",
        models_dev: "mistral",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "minimax",
        models_dev: "minimax",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "minimax-code",
        models_dev: "minimax",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "minimax-coding-plan",
        models_dev: "minimax",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "moonshot-cn",
        models_dev: "moonshot",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "moonshot-global",
        models_dev: "moonshot",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "zhipu",
        models_dev: "zhipuai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "glm-coding-plan",
        models_dev: "zhipuai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "dashscope",
        models_dev: "alibaba",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "dashscope-coding",
        models_dev: "alibaba",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "fireworks",
        models_dev: "fireworks-ai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "together",
        models_dev: "togetherai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "togetherai",
        models_dev: "togetherai",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "cohere",
        models_dev: "cohere",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "perplexity",
        models_dev: "perplexity",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "huggingface",
        models_dev: "huggingface",
        merge_policy: MergePolicy::EnrichOnly,
    },
    PlatformMapEntry {
        platform: "nvidia",
        models_dev: "nvidia",
        merge_policy: MergePolicy::EnrichOnly,
    },
];

/// Platforms explicitly excluded from models.dev enrichment.
const NEVER: &[&str] = &[
    "bedrock",
    "custom",
    "new-api",
    "siliconflow",
    "hunyuan",
    "lingyi",
];

/// Prefixes that map to [`MergePolicy::Never`] (e.g. `ark-*`, `stepfun-*`).
const NEVER_PREFIXES: &[&str] = &["ark", "stepfun", "qianfan", "mimo"];

fn entry_for(platform: &str) -> Option<&'static PlatformMapEntry> {
    MAPPED.iter().find(|e| e.platform == platform)
}

fn is_never_platform(platform: &str) -> bool {
    if NEVER.contains(&platform) {
        return true;
    }
    NEVER_PREFIXES.iter().any(|prefix| {
        platform == *prefix
            || platform
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-') || rest.starts_with('_'))
    })
}

/// Look up the models.dev provider ID for a Nomifun platform.
pub fn to_models_dev(platform: &str) -> Option<&'static str> {
    entry_for(platform).map(|e| e.models_dev)
}

/// Merge policy for a Nomifun platform. Unmapped platforms default to [`MergePolicy::Never`].
pub fn merge_policy(platform: &str) -> MergePolicy {
    if let Some(e) = entry_for(platform) {
        return e.merge_policy;
    }
    if is_never_platform(platform) {
        return MergePolicy::Never;
    }
    MergePolicy::Never
}

/// Resolve to a models.dev ID, falling back to the input when unmapped.
pub fn resolve_models_dev_id(platform: &str) -> &str {
    to_models_dev(platform).unwrap_or(platform)
}

/// Forward map: Nomifun platform → models.dev provider ID.
pub fn forward_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| MAPPED.iter().map(|e| (e.platform, e.models_dev)).collect())
}

/// Reverse map: models.dev provider ID → Nomifun platform.
///
/// When multiple Nomifun platforms share one models.dev ID, the **last**
/// entry in [`MAPPED`] wins.
pub fn reverse_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for e in MAPPED {
            m.insert(e.models_dev, e.platform);
        }
        m
    })
}

/// Look up the Nomifun platform for a models.dev provider ID.
pub fn to_nomifun(mdev_id: &str) -> Option<&'static str> {
    reverse_map().get(mdev_id).copied()
}

/// All platforms that have an explicit models.dev mapping (EnrichOnly / ListMerge).
pub fn all_mapped_platforms() -> impl Iterator<Item = &'static PlatformMapEntry> {
    MAPPED.iter()
}

/// Full static table including Never platforms that are named explicitly.
pub fn all_entries() -> impl Iterator<Item = PlatformMapEntry> {
    MAPPED.iter().copied().chain(NEVER.iter().map(|p| PlatformMapEntry {
        platform: p,
        models_dev: "",
        merge_policy: MergePolicy::Never,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_map_resolves_known_platforms() {
        assert_eq!(to_models_dev("anthropic"), Some("anthropic"));
        assert_eq!(to_models_dev("claude"), Some("anthropic"));
        assert_eq!(to_models_dev("gemini"), Some("google"));
        assert_eq!(to_models_dev("gemini-vertex-ai"), Some("google"));
        assert_eq!(to_models_dev("vertex-ai"), Some("google"));
        assert_eq!(to_models_dev("moonshot-cn"), Some("moonshot"));
        assert_eq!(to_models_dev("moonshot-global"), Some("moonshot"));
        assert_eq!(to_models_dev("zhipu"), Some("zhipuai"));
        assert_eq!(to_models_dev("glm-coding-plan"), Some("zhipuai"));
        assert_eq!(to_models_dev("dashscope"), Some("alibaba"));
        assert_eq!(to_models_dev("dashscope-coding"), Some("alibaba"));
        assert_eq!(to_models_dev("fireworks"), Some("fireworks-ai"));
        assert_eq!(to_models_dev("together"), Some("togetherai"));
        assert_eq!(to_models_dev("togetherai"), Some("togetherai"));
        assert_eq!(to_models_dev("minimax-code"), Some("minimax"));
        assert_eq!(to_models_dev("minimax-coding-plan"), Some("minimax"));
        assert_eq!(to_models_dev("nonexistent"), None);
    }

    #[test]
    fn merge_policy_enrich_only_for_mapped() {
        assert_eq!(merge_policy("anthropic"), MergePolicy::EnrichOnly);
        assert_eq!(merge_policy("gemini"), MergePolicy::EnrichOnly);
        assert_eq!(merge_policy("openrouter"), MergePolicy::EnrichOnly);
    }

    #[test]
    fn merge_policy_never_for_excluded() {
        assert_eq!(merge_policy("bedrock"), MergePolicy::Never);
        assert_eq!(merge_policy("custom"), MergePolicy::Never);
        assert_eq!(merge_policy("new-api"), MergePolicy::Never);
        assert_eq!(merge_policy("siliconflow"), MergePolicy::Never);
        assert_eq!(merge_policy("hunyuan"), MergePolicy::Never);
        assert_eq!(merge_policy("lingyi"), MergePolicy::Never);
        assert_eq!(merge_policy("ark"), MergePolicy::Never);
        assert_eq!(merge_policy("ark-cn"), MergePolicy::Never);
        assert_eq!(merge_policy("stepfun"), MergePolicy::Never);
        assert_eq!(merge_policy("stepfun-v2"), MergePolicy::Never);
        assert_eq!(merge_policy("qianfan"), MergePolicy::Never);
        assert_eq!(merge_policy("qianfan-pro"), MergePolicy::Never);
        assert_eq!(merge_policy("mimo"), MergePolicy::Never);
        assert_eq!(merge_policy("mimo-v1"), MergePolicy::Never);
        assert_eq!(merge_policy("totally-unknown"), MergePolicy::Never);
    }

    #[test]
    fn to_models_dev_none_for_never_platforms() {
        assert_eq!(to_models_dev("bedrock"), None);
        assert_eq!(to_models_dev("ark-cn"), None);
        assert_eq!(to_models_dev("siliconflow"), None);
    }

    #[test]
    fn resolve_models_dev_id_falls_back_to_input() {
        assert_eq!(resolve_models_dev_id("anthropic"), "anthropic");
        assert_eq!(resolve_models_dev_id("gemini"), "google");
        assert_eq!(resolve_models_dev_id("custom-thing"), "custom-thing");
    }

    #[test]
    fn reverse_map_picks_last_when_collision() {
        // gemini, gemini-vertex-ai, vertex-ai all map to google — last wins.
        assert_eq!(to_nomifun("google"), Some("vertex-ai"));
        assert_eq!(to_nomifun("anthropic"), Some("claude"));
    }

    #[test]
    fn all_mapped_platforms_covers_enrich_entries() {
        let platforms: Vec<_> = all_mapped_platforms().map(|e| e.platform).collect();
        assert!(platforms.contains(&"anthropic"));
        assert!(platforms.contains(&"gemini"));
        assert!(platforms.contains(&"moonshot-cn"));
        assert!(!platforms.contains(&"bedrock"));
    }

    #[test]
    fn forward_map_matches_to_models_dev() {
        for e in all_mapped_platforms() {
            assert_eq!(forward_map().get(e.platform).copied(), Some(e.models_dev));
            assert_eq!(to_models_dev(e.platform), Some(e.models_dev));
            assert_eq!(merge_policy(e.platform), e.merge_policy);
        }
    }
}
