//! App UI language normalization and a best-effort read of the installation
//! preference. Matches the TypeScript `normalizeLanguageCode` in
//! `ui/src/common/config/i18n.ts`.

use std::path::Path;

use serde_json::Value;

use crate::dir_config;

/// Supported UI languages. Keep in lockstep with `i18n-config.json`.
pub const DEFAULT_UI_LANGUAGE: &str = "en-US";
pub const ZH_CN_LANGUAGE: &str = "zh-CN";

/// Filename of the installation-scoped preference JSON under `data_dir`.
/// Keep in lockstep with `nomifun-system::installation_preferences`.
pub const INSTALLATION_PREFERENCES_FILE: &str = "installation-preferences.json";

/// Cap for the best-effort language read. Matches the installation store.
const MAX_INSTALLATION_PREFERENCES_BYTES: u64 = 4 * 1024 * 1024;

/// Normalize an OS / persisted locale tag to a supported app UI language.
///
/// - any Chinese tag (`zh`, `zh_CN`, `zh-Hans`, `zh-TW`, …) → `zh-CN`
/// - `en-US` stays `en-US`
/// - everything else (`ja`, `fr`, `C`, `POSIX`, empty, `None`) → `en-US`
pub fn normalize_ui_language(code: Option<&str>) -> String {
    let Some(raw) = code.map(str::trim).filter(|value| !value.is_empty()) else {
        return DEFAULT_UI_LANGUAGE.to_owned();
    };
    let normalized = raw.replace('_', "-");
    if normalized == DEFAULT_UI_LANGUAGE || normalized == ZH_CN_LANGUAGE {
        return normalized;
    }
    let lang_only = normalized
        .split('-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if lang_only == "zh" {
        ZH_CN_LANGUAGE.to_owned()
    } else {
        DEFAULT_UI_LANGUAGE.to_owned()
    }
}

/// Best-effort read of the persisted installation `language` key.
///
/// Missing, unreadable, or empty values return `None` so callers can fall
/// back to the host OS locale. This does not take the installation-store
/// file lock.
pub fn read_installation_language(data_dir: &Path) -> Option<String> {
    let path = data_dir.join(INSTALLATION_PREFERENCES_FILE);
    let bytes = dir_config::read_bounded_regular_file(&path, MAX_INSTALLATION_PREFERENCES_BYTES).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let language = value.get("language")?.as_str()?.trim();
    if language.is_empty() {
        None
    } else {
        Some(language.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn normalize_ui_language_folds_chinese_and_falls_back_to_english() {
        assert_eq!(normalize_ui_language(Some("zh-CN")), "zh-CN");
        assert_eq!(normalize_ui_language(Some("en-US")), "en-US");
        for zh in ["zh", "zh_CN", "zh-Hans", "zh-Hans-CN", "zh-TW", "zh-HK", "ZH-cn"] {
            assert_eq!(normalize_ui_language(Some(zh)), "zh-CN", "{zh}");
        }
        for other in ["ja-JP", "fr-FR", "C", "POSIX", "C.UTF-8", "en-GB", ""] {
            assert_eq!(normalize_ui_language(Some(other)), "en-US", "{other}");
        }
        assert_eq!(normalize_ui_language(None), "en-US");
        assert_eq!(normalize_ui_language(Some("   ")), "en-US");
    }

    #[test]
    fn read_installation_language_returns_none_when_missing_or_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_installation_language(dir.path()), None);

        fs::write(
            dir.path().join(INSTALLATION_PREFERENCES_FILE),
            br#"{"theme":"dark"}"#,
        )
        .unwrap();
        assert_eq!(read_installation_language(dir.path()), None);

        fs::write(
            dir.path().join(INSTALLATION_PREFERENCES_FILE),
            br#"{"language":"   "}"#,
        )
        .unwrap();
        assert_eq!(read_installation_language(dir.path()), None);
    }

    #[test]
    fn read_installation_language_returns_stored_string() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(INSTALLATION_PREFERENCES_FILE),
            br#"{"language":"zh-CN"}"#,
        )
        .unwrap();
        assert_eq!(
            read_installation_language(dir.path()).as_deref(),
            Some("zh-CN")
        );
    }
}
