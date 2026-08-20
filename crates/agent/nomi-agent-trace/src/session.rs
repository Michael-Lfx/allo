//! Classify how a turn was initiated (session dialogue vs companion / cron / …).

/// Returns `true` when this turn is ordinary interactive session dialogue:
/// no origin, not companion, and no channel platform.
pub fn is_session_dialogue(
    origin: Option<&str>,
    companion: bool,
    channel_platform: Option<&str>,
) -> bool {
    let origin_empty = origin.map(str::trim).filter(|s| !s.is_empty()).is_none();
    let channel_empty = channel_platform
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none();
    origin_empty && !companion && channel_empty
}

/// Classify into a stable `session_kind` string for observation payloads.
///
/// Priority:
/// 1. `companion == true` → `"companion"`
/// 2. non-empty `channel_platform` → `"channel"`
/// 3. non-empty `origin` mapped to a known kind (or `"other"`)
/// 4. otherwise → `"session_dialogue"`
pub fn classify_session_kind(
    origin: Option<&str>,
    companion: bool,
    channel_platform: Option<&str>,
) -> &'static str {
    if companion {
        return "companion";
    }
    if channel_platform
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        return "channel";
    }
    let Some(origin) = origin.map(str::trim).filter(|s| !s.is_empty()) else {
        return "session_dialogue";
    };
    match origin.to_ascii_lowercase().as_str() {
        "session_dialogue" | "session-dialogue" | "dialogue" => "session_dialogue",
        "companion" => "companion",
        "cron" => "cron",
        "autowork" | "auto_work" | "auto-work" => "autowork",
        "idmm" => "idmm",
        "agent_execution" | "agent-execution" | "agentexecution" => "agent_execution",
        "channel" => "channel",
        "eval" | "agent_eval" | "agent-eval" => "eval",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_dialogue_when_all_empty() {
        assert!(is_session_dialogue(None, false, None));
        assert!(is_session_dialogue(Some(""), false, Some("  ")));
        assert_eq!(
            classify_session_kind(None, false, None),
            "session_dialogue"
        );
    }

    #[test]
    fn companion_wins() {
        assert!(!is_session_dialogue(None, true, None));
        assert_eq!(classify_session_kind(None, true, None), "companion");
        assert_eq!(
            classify_session_kind(Some("cron"), true, Some("discord")),
            "companion"
        );
    }

    #[test]
    fn channel_platform_wins_over_origin() {
        assert!(!is_session_dialogue(None, false, Some("telegram")));
        assert_eq!(
            classify_session_kind(Some("cron"), false, Some("telegram")),
            "channel"
        );
    }

    #[test]
    fn origin_known_kinds() {
        for (origin, kind) in [
            ("cron", "cron"),
            ("Autowork", "autowork"),
            ("auto_work", "autowork"),
            ("idmm", "idmm"),
            ("agent_execution", "agent_execution"),
            ("agent-execution", "agent_execution"),
            ("eval", "eval"),
            ("agent_eval", "eval"),
            ("weird_source", "other"),
        ] {
            assert_eq!(classify_session_kind(Some(origin), false, None), kind);
            assert!(!is_session_dialogue(Some(origin), false, None));
        }
    }
}
