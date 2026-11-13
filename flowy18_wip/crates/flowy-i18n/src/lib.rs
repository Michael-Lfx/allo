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
    Ko,
}

impl Locale {
    pub const ALL: [Self; 12] = [
        Self::Ar,
        Self::Id,
        Self::Cs,
        Self::De,
        Self::En,
        Self::Es,
        Self::Es419,
        Self::ZhCn,
        Self::ZhTw,
        Self::ZhHk,
        Self::Fr,
        Self::Ko,
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
            Self::Ko => "ko",
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
            Self::Ko => "한국어",
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
            .or_else(|| self.lookup(Locale::ZhCn, key))
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
        Locale::Ko => include_str!("../locales/ko.json"),
    }
}

#[cfg(test)]
fn key_set(messages: &HashMap<String, String>) -> BTreeSet<&str> {
    messages.keys().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::{I18n, Locale, key_set};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn every_locale_has_the_same_keys() {
        let i18n = I18n::embedded("Flowy");
        let base = key_set(i18n.messages.get(&Locale::ZhCn).unwrap());
        assert!(base.len() > 20);
        for locale in Locale::ALL {
            let keys = key_set(i18n.messages.get(&locale).unwrap());
            assert_eq!(keys, base, "{}", locale.code());
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
    fn product_name_is_filled() {
        let i18n = I18n::embedded("Flowy");
        let text = i18n.t("model.lite");
        assert!(text.contains("Flowy"));
        assert!(!text.contains("{product}"));
    }

    #[test]
    fn placeholders_are_replaced() {
        let i18n = I18n::embedded("Flowy");
        let text = i18n.tf("project.untitled", &[("index", "3")]);
        assert!(text.contains('3'));
        assert!(!text.contains("{index}"));
    }
}
