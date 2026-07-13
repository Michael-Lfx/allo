//! Persisted companion configuration: opt-in collection switches, learning model,
//! persona, appearance and quiet-hours. Stored as `config.json` under the companion
//! dir with atomic temp+rename writes (same pattern as cron skill files).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The roster character every companion falls back to when none is configured.
pub(crate) const DEFAULT_CHARACTER: &str = "mochi";

/// Which event sources the user has opted into collecting. The work-event
/// sources all default OFF; `companion_dialogues` (direct conversations with the
/// companions) defaults ON — talking to the companion is itself the opt-in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CollectConfig {
    pub chat_user_messages: bool,
    pub chat_assistant_replies: bool,
    pub requirements: bool,
    pub cron_runs: bool,
    pub conversation_lifecycle: bool,
    pub terminal_sessions: bool,
    /// Tool-call capture from owner work sessions: tool NAME + normalized param
    /// SHAPE only (sorted top-level arg keys + JSON types), never values. The
    /// primary mining signal for skill self-evolution (design §5.1).
    pub tool_calls: bool,
    /// Companion-dialogue capture: owner messages + companion replies inside companion
    /// (companion / channel-master) conversations. The field-level serde
    /// default keeps it ON for legacy `config.json` files written before the
    /// field existed.
    #[serde(default = "default_true")]
    pub companion_dialogues: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CollectConfig {
    fn default() -> Self {
        Self {
            chat_user_messages: false,
            chat_assistant_replies: false,
            requirements: false,
            cron_runs: false,
            conversation_lifecycle: false,
            terminal_sessions: false,
            tool_calls: false,
            companion_dialogues: true,
        }
    }
}

impl CollectConfig {
    /// Whether any of the opt-in *work-event* sources is enabled (UI
    /// onboarding hint). Deliberately excludes `companion_dialogues`, which is on
    /// by default and would make this vacuously true.
    pub fn any_enabled(&self) -> bool {
        self.chat_user_messages
            || self.chat_assistant_replies
            || self.requirements
            || self.cron_runs
            || self.conversation_lifecycle
            || self.terminal_sessions
            || self.tool_calls
    }
}

/// The model used for learning runs + companion chat.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelConfig {
    pub provider_id: String,
    pub model: String,
}

impl ModelConfig {
    pub fn is_configured(&self) -> bool {
        !self.provider_id.is_empty() && !self.model.is_empty()
    }
}

/// Scheduled learning settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LearnConfig {
    pub enabled: bool,
    /// Minutes between learning runs.
    pub interval_minutes: u32,
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 60,
        }
    }
}

/// Desktop-companion appearance + notification behaviour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppearanceConfig {
    /// Whether the desktop companion window should be visible.
    pub companion_enabled: bool,
    /// Which character renders in the companion window (see the UI character
    /// roster: mochi/ink/roux/pixel/bolt/boo). Unknown values fall back to
    /// the default character on the renderer side.
    pub character: String,
    /// Saved companion window position (physical px), if the user dragged it.
    pub companion_x: Option<i32>,
    pub companion_y: Option<i32>,
    /// Quiet hours "HH:mm" — within this window the companion only accrues badges
    /// and never pops bubbles. Empty strings disable quiet hours.
    pub quiet_start: String,
    pub quiet_end: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            companion_enabled: false,
            character: DEFAULT_CHARACTER.into(),
            companion_x: None,
            companion_y: None,
            quiet_start: String::new(),
            quiet_end: String::new(),
        }
    }
}

/// Stable id used when migrating the legacy free-text `custom` field into
/// [`PersonaConfig::customs`], so re-reading an already-migrated file does not
/// duplicate the entry.
pub const LEGACY_CUSTOM_PERSONA_ID: &str = "legacy-custom";

/// Built-in persona keys (also the only values that use [`crate::prompt::persona_flavor`]).
pub const BUILTIN_PERSONA_KEYS: &[&str] = &["lively", "calm", "sassy"];

/// Soft caps for user-authored custom personas (enforced on patch).
pub const MAX_CUSTOM_PERSONAS: usize = 10;
pub const MAX_CUSTOM_PERSONA_TITLE_CHARS: usize = 20;
pub const MAX_CUSTOM_PERSONA_BODY_CHARS: usize = 2000;

/// One user-authored persona: a chip label (`title`) plus the flavor text
/// injected into the system prompt (`body`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomPersona {
    pub id: String,
    pub title: String,
    pub body: String,
}

/// Persona settings injected into the chat/learn system prompts.
///
/// `selected` is either a built-in key (`lively` / `calm` / `sassy`) or the
/// `id` of an entry in `customs`. Legacy JSON with `preset` + `custom` is
/// accepted on read and rewritten in the new shape on the next save.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PersonaConfig {
    /// `lively` | `calm` | `sassy` | custom persona id.
    pub selected: String,
    pub customs: Vec<CustomPersona>,
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self {
            selected: "lively".into(),
            customs: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for PersonaConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            selected: Option<String>,
            /// Legacy field (pre multi-custom-persona).
            #[serde(default)]
            preset: Option<String>,
            /// Legacy free-text supplement; migrated into `customs`.
            #[serde(default)]
            custom: Option<String>,
            #[serde(default)]
            customs: Vec<CustomPersona>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let selected = raw
            .selected
            .filter(|s| !s.trim().is_empty())
            .or_else(|| raw.preset.filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| "lively".into());

        let mut customs = raw.customs;
        if let Some(legacy) = raw.custom {
            let body = legacy.trim();
            if !body.is_empty() && !customs.iter().any(|c| c.id == LEGACY_CUSTOM_PERSONA_ID) {
                customs.insert(
                    0,
                    CustomPersona {
                        id: LEGACY_CUSTOM_PERSONA_ID.into(),
                        title: "我的设定".into(),
                        body: body.to_owned(),
                    },
                );
            }
        }

        Ok(Self { selected, customs }.normalized())
    }
}

impl PersonaConfig {
    /// Whether `key` is one of the three built-in persona presets.
    pub fn is_builtin(key: &str) -> bool {
        BUILTIN_PERSONA_KEYS.contains(&key)
    }

    /// Fall back `selected` to `lively` when it points at neither a built-in
    /// nor an existing custom id. Does not mutate customs.
    pub fn normalized(mut self) -> Self {
        self.normalize_selected();
        self
    }

    pub fn normalize_selected(&mut self) {
        let ok = Self::is_builtin(&self.selected) || self.customs.iter().any(|c| c.id == self.selected);
        if !ok {
            self.selected = "lively".into();
        }
    }

    /// Validate customs for a user patch (length / empties / caps). Callers
    /// should also run [`Self::normalize_selected`] after a successful check.
    pub fn validate_for_save(&self) -> Result<(), String> {
        if self.customs.len() > MAX_CUSTOM_PERSONAS {
            return Err(format!(
                "at most {MAX_CUSTOM_PERSONAS} custom personas are allowed"
            ));
        }
        for (i, c) in self.customs.iter().enumerate() {
            if c.id.trim().is_empty() {
                return Err(format!("customs[{i}].id must not be empty"));
            }
            let title = c.title.trim();
            if title.is_empty() {
                return Err(format!("customs[{i}].title must not be empty"));
            }
            if title.chars().count() > MAX_CUSTOM_PERSONA_TITLE_CHARS {
                return Err(format!(
                    "customs[{i}].title must be at most {MAX_CUSTOM_PERSONA_TITLE_CHARS} characters"
                ));
            }
            let body = c.body.trim();
            if body.is_empty() {
                return Err(format!("customs[{i}].body must not be empty"));
            }
            if body.chars().count() > MAX_CUSTOM_PERSONA_BODY_CHARS {
                return Err(format!(
                    "customs[{i}].body must be at most {MAX_CUSTOM_PERSONA_BODY_CHARS} characters"
                ));
            }
        }
        // Duplicate ids would make selection ambiguous.
        let mut seen = std::collections::HashSet::new();
        for c in &self.customs {
            if !seen.insert(c.id.as_str()) {
                return Err(format!("duplicate custom persona id '{}'", c.id));
            }
        }
        Ok(())
    }
}

/// The full persisted companion configuration.
///
/// LEGACY: this is the pre-multi-companion single-config shape, kept only so boot
/// can read an old `companion/nomi/config.json` and migrate it into the new
/// per-companion [`crate::profile::CompanionProfileConfig`] + shared
/// [`crate::profile::SharedCompanionConfig`] split. Do not extend it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CompanionConfig {
    pub collect: CollectConfig,
    pub model: ModelConfig,
    pub learn: LearnConfig,
    pub appearance: AppearanceConfig,
    pub persona: PersonaConfig,
}

impl CompanionConfig {
    pub fn config_path(companion_dir: &Path) -> PathBuf {
        companion_dir.join("config.json")
    }

    /// Load from `{companion_dir}/config.json`, falling back to defaults when the
    /// file is missing or unreadable (a corrupt config must never brick boot).
    pub fn load(companion_dir: &Path) -> Self {
        crate::fsio::load_json_or_default(&Self::config_path(companion_dir))
    }

    /// Atomically persist to `{companion_dir}/config.json` (unique temp file +
    /// rename, so two concurrent saves can never rename each other's
    /// half-written temp into place).
    pub fn save(&self, companion_dir: &Path) -> std::io::Result<()> {
        crate::fsio::save_json_atomic(companion_dir, "config.json", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_default_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = CompanionConfig::load(dir.path());
        assert_eq!(loaded, CompanionConfig::default());
        assert!(!loaded.collect.any_enabled());

        let mut cfg = CompanionConfig::default();
        cfg.collect.chat_user_messages = true;
        cfg.model.provider_id = "prov_x".into();
        cfg.model.model = "claude-fable-5".into();
        cfg.learn.enabled = true;
        cfg.save(dir.path()).unwrap();

        let again = CompanionConfig::load(dir.path());
        assert_eq!(again, cfg);
        assert!(again.model.is_configured());
    }

    #[test]
    fn corrupt_config_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(CompanionConfig::config_path(dir.path()), "{not json").unwrap();
        assert_eq!(CompanionConfig::load(dir.path()), CompanionConfig::default());
    }

    #[test]
    fn legacy_collect_json_defaults_companion_dialogues_on() {
        // Stored configs written before the field existed must come back ON.
        let legacy: CollectConfig = serde_json::from_str(r#"{"chat_user_messages":true}"#).unwrap();
        assert!(legacy.companion_dialogues);
        assert!(legacy.chat_user_messages);
        // …and an explicit false is respected.
        let off: CollectConfig = serde_json::from_str(r#"{"companion_dialogues":false}"#).unwrap();
        assert!(!off.companion_dialogues);

        // Full legacy config.json on disk (no companion_dialogues key) roundtrips
        // through the file loader with the field defaulted ON.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            CompanionConfig::config_path(dir.path()),
            r#"{"collect":{"requirements":true}}"#,
        )
        .unwrap();
        let loaded = CompanionConfig::load(dir.path());
        assert!(loaded.collect.companion_dialogues);
        assert!(loaded.collect.requirements);
        // companion_dialogues is excluded from the work-event onboarding hint.
        assert!(CollectConfig::default().companion_dialogues);
        assert!(!CollectConfig::default().any_enabled());
    }

    #[test]
    fn legacy_persona_json_migrates_preset_and_custom() {
        let persona: PersonaConfig = serde_json::from_str(
            r#"{"preset":"calm","custom":"多用颜文字"}"#,
        )
        .unwrap();
        assert_eq!(persona.selected, "calm");
        assert_eq!(persona.customs.len(), 1);
        assert_eq!(persona.customs[0].id, LEGACY_CUSTOM_PERSONA_ID);
        assert_eq!(persona.customs[0].title, "我的设定");
        assert_eq!(persona.customs[0].body, "多用颜文字");

        // Re-serializing must not emit the legacy keys.
        let v = serde_json::to_value(&persona).unwrap();
        assert!(v.get("preset").is_none());
        assert!(v.get("custom").is_none());
        assert_eq!(v["selected"], "calm");
    }

    #[test]
    fn legacy_custom_does_not_duplicate_on_remigrate() {
        let persona: PersonaConfig = serde_json::from_str(
            r#"{
                "selected":"calm",
                "custom":"多用颜文字",
                "customs":[{"id":"legacy-custom","title":"我的设定","body":"多用颜文字"}]
            }"#,
        )
        .unwrap();
        assert_eq!(persona.customs.len(), 1);
    }

    #[test]
    fn unknown_selected_falls_back_to_lively() {
        let persona: PersonaConfig =
            serde_json::from_str(r#"{"selected":"missing-id","customs":[]}"#).unwrap();
        assert_eq!(persona.selected, "lively");
    }

    #[test]
    fn validate_rejects_too_many_and_empty_fields() {
        let mut persona = PersonaConfig::default();
        persona.customs = (0..MAX_CUSTOM_PERSONAS + 1)
            .map(|i| CustomPersona {
                id: format!("c{i}"),
                title: format!("t{i}"),
                body: "body".into(),
            })
            .collect();
        assert!(persona.validate_for_save().is_err());

        persona.customs = vec![CustomPersona {
            id: "c1".into(),
            title: "  ".into(),
            body: "body".into(),
        }];
        assert!(persona.validate_for_save().is_err());
    }
}
