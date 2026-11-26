#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Locale {
    ZhCn,
    Ar,
    Id,
    Cs,
    De,
    En,
    Es,
    Es419,
    ZhTw,
    ZhHk,
    Fr,
    Hi,
    It,
    Ja,
    Ko,
    Nl,
    Pl,
    PtBr,
    PtPt,
    Th,
    Tr,
    Uk,
    Vi,
}

impl Locale {
    pub const ALL: [Self; 23] = [
        Self::Ar,
        Self::Id,
        Self::Cs,
        Self::De,
        Self::En,
        Self::Es,
        Self::Es419,
        Self::Fr,
        Self::Hi,
        Self::It,
        Self::Ja,
        Self::Ko,
        Self::Nl,
        Self::Pl,
        Self::PtBr,
        Self::PtPt,
        Self::ZhCn,
        Self::ZhTw,
        Self::ZhHk,
        Self::Th,
        Self::Vi,
        Self::Tr,
        Self::Uk,
    ];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::Ar => "ar",
            Self::Id => "id",
            Self::Cs => "cs",
            Self::De => "de",
            Self::En => "en",
            Self::Es => "es",
            Self::Es419 => "es-419",
            Self::ZhTw => "zh-TW",
            Self::ZhHk => "zh-HK",
            Self::Fr => "fr",
            Self::Hi => "hi",
            Self::It => "it",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Nl => "nl",
            Self::Pl => "pl",
            Self::PtBr => "pt-BR",
            Self::PtPt => "pt-PT",
            Self::Th => "th",
            Self::Tr => "tr",
            Self::Uk => "uk",
            Self::Vi => "vi",
        }
    }

    #[must_use]
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::Ar => "العربية",
            Self::Id => "Bahasa Indonesia",
            Self::Cs => "Čeština",
            Self::De => "Deutsch",
            Self::En => "English",
            Self::Es => "Español",
            Self::Es419 => "Español (Latinoamérica)",
            Self::ZhTw => "繁體中文",
            Self::ZhHk => "繁體中文（香港）",
            Self::Fr => "Français",
            Self::Hi => "हिन्दी",
            Self::It => "Italiano",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Nl => "Nederlands",
            Self::Pl => "Polski",
            Self::PtBr => "Português (Brasil)",
            Self::PtPt => "Português (Portugal)",
            Self::Th => "ไทย",
            Self::Tr => "Türkçe",
            Self::Uk => "Українська",
            Self::Vi => "Tiếng Việt",
        }
    }

    #[must_use]
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Ar)
    }

    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|locale| locale.code() == code)
    }
}

#[derive(Clone, Debug)]
pub struct I18n {
    locale: Locale,
    product: &'static str,
    messages: HashMap<Locale, HashMap<String, String>>,
}

impl I18n {
    #[must_use]
    pub fn embedded(product: &'static str) -> Self {
        let mut messages = HashMap::new();
        for locale in Locale::ALL {
            messages.insert(locale, parse_map(embedded_json(locale)));
        }
        Self {
            locale: Locale::ZhCn,
            product,
            messages,
        }
    }

    #[must_use]
    pub fn dev_dir() -> Option<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales");
        dir.is_dir().then_some(dir)
    }

    #[must_use]
    pub const fn locale(&self) -> Locale {
        self.locale
    }

    pub const fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn reload_dir(&mut self, dir: &Path) {
        let locale = self.locale;
        let product = self.product;
        *self = Self::embedded(product);
        self.locale = locale;
        self.overlay_dir(dir);
    }

    #[must_use]
    pub fn t(&self, key: &str) -> String {
        let text = self
            .lookup(self.locale, key)
            // English is the source language: a locale that misses a key reads
            // the English wording, not Chinese.
            .or_else(|| self.lookup(Locale::En, key))
            .unwrap_or_else(|| key.to_string());
        text.replace("{product}", self.product)
    }

    #[must_use]
    pub fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let mut text = self.t(key);
        for (name, value) in vars {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        text
    }

    fn lookup(&self, locale: Locale, key: &str) -> Option<String> {
        self.messages
            .get(&locale)
            .and_then(|messages| messages.get(key))
            .cloned()
    }

    fn overlay_dir(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(locale) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(Locale::parse)
            else {
                continue;
            };
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let incoming = parse_map(&text);
            if incoming.is_empty() {
                continue;
            }
            self.messages.entry(locale).or_default().extend(incoming);
        }
    }
}

#[must_use]
pub fn file_stamps(dir: &Path) -> BTreeMap<PathBuf, u128> {
    let mut stamps = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return stamps;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_millis());
        stamps.insert(path, modified);
    }
    stamps
}

fn parse_map(text: &str) -> HashMap<String, String> {
    serde_json::from_str(text).unwrap_or_default()
}

const fn embedded_json(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => include_str!("../locales/zh-CN.json"),
        Locale::Ar => include_str!("../locales/ar.json"),
        Locale::Id => include_str!("../locales/id.json"),
        Locale::Cs => include_str!("../locales/cs.json"),
        Locale::De => include_str!("../locales/de.json"),
        Locale::En => include_str!("../locales/en.json"),
        Locale::Es => include_str!("../locales/es.json"),
        Locale::Es419 => include_str!("../locales/es-419.json"),
        Locale::ZhTw => include_str!("../locales/zh-TW.json"),
        Locale::ZhHk => include_str!("../locales/zh-HK.json"),
        Locale::Fr => include_str!("../locales/fr.json"),
        Locale::Hi => include_str!("../locales/hi.json"),
        Locale::It => include_str!("../locales/it.json"),
        Locale::Ja => include_str!("../locales/ja.json"),
        Locale::Ko => include_str!("../locales/ko.json"),
        Locale::Nl => include_str!("../locales/nl.json"),
        Locale::Pl => include_str!("../locales/pl.json"),
        Locale::PtBr => include_str!("../locales/pt-BR.json"),
        Locale::PtPt => include_str!("../locales/pt-PT.json"),
        Locale::Th => include_str!("../locales/th.json"),
        Locale::Tr => include_str!("../locales/tr.json"),
        Locale::Uk => include_str!("../locales/uk.json"),
        Locale::Vi => include_str!("../locales/vi.json"),
    }
}

#[cfg(test)]
fn key_set(messages: &HashMap<String, String>) -> BTreeSet<&str> {
    messages.keys().map(String::as_str).collect()
}

#[cfg(test)]
fn placeholders(value: &str) -> BTreeSet<&str> {
    value
        .split('{')
        .skip(1)
        .filter_map(|tail| tail.split_once('}'))
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{I18n, Locale, key_set};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn every_locale_preserves_english_placeholders() {
        let i18n = I18n::embedded("Flowy");
        let english = i18n.messages.get(&Locale::En).unwrap();
        for locale in Locale::ALL {
            let messages = i18n.messages.get(&locale).unwrap();
            for (key, expected) in
                english.iter().filter(|(key, _)| key.starts_with("auto."))
            {
                assert_eq!(
                    super::placeholders(messages.get(key).unwrap()),
                    super::placeholders(expected),
                    "{} / {key}",
                    locale.code(),
                );
            }
        }
    }

    #[test]
    fn every_locale_has_the_same_keys() {
        let i18n = I18n::embedded("Flowy");
        let base = key_set(i18n.messages.get(&Locale::ZhCn).unwrap());
        assert!(base.len() > 20);
        for locale in Locale::ALL {
            let messages = i18n.messages.get(&locale).unwrap();
            let keys = key_set(messages);
            assert_eq!(keys, base, "{}", locale.code());
            for (key, value) in
                messages.iter().filter(|(key, _)| key.starts_with("auto."))
            {
                assert!(!value.trim().is_empty(), "{} / {key}", locale.code());
            }
        }
    }

    #[test]
    fn delivery_failure_and_publish_copy_are_localized() {
        let mut i18n = I18n::embedded("Flowy");
        i18n.set_locale(Locale::ZhCn);
        assert_eq!(i18n.t("chat.status_delivery_failed"), "交付文件保存失败");
        assert_eq!(i18n.t("chat.retry_delivery"), "重试交付");
        assert_eq!(
            i18n.t("chat.publish.staged"),
            "文件已暂存，本轮成功完成后将交付。"
        );
        i18n.set_locale(Locale::En);
        assert_eq!(
            i18n.t("chat.status_delivery_failed"),
            "Couldn't save delivered files"
        );
        assert_eq!(i18n.t("chat.retry_delivery"), "Retry delivery");
        assert_eq!(
            i18n.t("chat.publish.staged"),
            "File staged. It will be delivered when the turn completes."
        );
    }

    #[test]
    fn automation_onboarding_is_localized() {
        let mut i18n = I18n::embedded("Flowy");
        for (locale, lead, schedule, trigger) in [
            (
                Locale::ZhCn,
                "安排周期性工作，或在特定事件发生时启动任务。",
                "定时任务",
                "触发器",
            ),
            (
                Locale::En,
                "Schedule recurring work or trigger it when a specific event happens.",
                "Scheduled task",
                "Trigger",
            ),
        ] {
            i18n.set_locale(locale);
            assert_eq!(i18n.t("auto.lead"), lead);
            assert_eq!(i18n.t("auto.schedule_title"), schedule);
            assert_eq!(i18n.t("auto.trigger_title"), trigger);
        }
    }

    #[test]
    fn library_preview_controls_have_chinese_and_english_copy() {
        let mut i18n = I18n::embedded("Flowy");
        i18n.set_locale(Locale::ZhCn);
        assert_eq!(i18n.t("library.add_files"), "添加文件");
        assert_eq!(i18n.t("library.open_default"), "用默认应用打开");
        assert_eq!(i18n.t("library.preview.next"), "下一页");
        assert_eq!(
            i18n.t("library.error.large"),
            "文件超出预览限制，请用默认应用打开。"
        );
        i18n.set_locale(Locale::En);
        assert_eq!(i18n.t("library.add_files"), "Add files");
        assert_eq!(i18n.t("library.open_default"), "Open with default app");
        assert_eq!(i18n.t("library.preview.next"), "Next page");
        assert_eq!(
            i18n.t("library.error.large"),
            "File exceeds the preview limits. Open it with the default app."
        );
    }

    #[test]
    fn my_skills_library_copy_is_localized() {
        let mut i18n = I18n::embedded("Flowy");
        let keys = [
            "skills.library_installed",
            "skills.library_explore",
            "skills.library_no_saved",
            "skills.library_search",
        ];
        for (locale, expected) in [
            (
                Locale::ZhCn,
                ["已安装", "探索技能", "还没有收藏技能。", "搜索我的技能"],
            ),
            (
                Locale::En,
                [
                    "Installed",
                    "Explore skills",
                    "No saved skills yet.",
                    "Search your skills",
                ],
            ),
        ] {
            i18n.set_locale(locale);
            for (key, value) in keys.into_iter().zip(expected) {
                assert_eq!(i18n.t(key), value);
            }
        }
    }

    #[test]
    fn conversation_artifact_actions_have_chinese_and_english_copy() {
        let mut i18n = I18n::embedded("Flowy");
        for (locale, save, source, delete_body) in [
            (
                Locale::ZhCn,
                "另存为",
                "回到原对话",
                "这会永久删除对话历史和资料库中的相关文件，且无法恢复。",
            ),
            (
                Locale::En,
                "Save as",
                "Return to conversation",
                "Conversation history and related files in the library will be permanently deleted. This cannot be undone.",
            ),
        ] {
            i18n.set_locale(locale);
            assert_eq!(i18n.t("library.save_as"), save);
            assert_eq!(i18n.t("library.locate_task"), source);
            assert_eq!(i18n.t("chat.delete_confirm.body"), delete_body);
            for key in [
                "library.empty.true.body",
                "library.empty.search.body",
                "library.source.deleted",
                "library.source.unavailable",
                "chat.delete_confirm.body",
                "chat.status_saving_files",
            ] {
                let message = i18n.t(key);
                assert_ne!(message, "");
                assert_ne!(message, key, "{}: {key}", locale.code());
            }
        }
    }

    #[test]
    fn disk_reload_overrides_one_key_and_keeps_the_rest() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("flowy-i18n-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("en.json"),
            r#"{"login.sign_in_title":"Sign in now"}"#,
        )
        .unwrap();
        let mut i18n = I18n::embedded("Flowy");
        i18n.set_locale(Locale::En);
        let original = i18n.t("login.email");
        i18n.reload_dir(&dir);
        assert_eq!(i18n.t("login.sign_in_title"), "Sign in now");
        assert_eq!(i18n.t("login.email"), original);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_key_falls_back_to_english_not_chinese() {
        let mut i18n = I18n::embedded("Flowy");
        i18n.set_locale(Locale::Ja);
        // `t` substitutes `{product}`, so compare against the substituted form.
        let english = i18n.messages[&Locale::En]["home.title"]
            .replace("{product}", "Flowy");
        assert_ne!(
            english,
            i18n.messages[&Locale::ZhCn]["home.title"],
            "test needs locales that differ for this key"
        );
        i18n.messages
            .get_mut(&Locale::Ja)
            .expect("ja locale is embedded")
            .remove("home.title");
        assert_eq!(i18n.t("home.title"), english);
    }

    #[test]
    fn product_name_is_filled() {
        let i18n = I18n::embedded("Flowy");
        let text = i18n.t("model.lite");
        assert!(text.contains("Flowy"));
        assert!(!text.contains("{product}"));
    }

    #[test]
    fn placeholders_are_replaced() {
        let i18n = I18n::embedded("Flowy");
        let text = i18n.tf("chat.duration_secs", &[("n", "3")]);
        assert!(text.contains('3'));
        assert!(!text.contains("{n}"));
    }
}
