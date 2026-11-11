use crate::i18n::Locale;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    Quiet,
    Queue,
    Interrupt,
}

impl SendMode {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Quiet => "settings.send_quiet",
            Self::Queue => "settings.send_queue",
            Self::Interrupt => "settings.send_interrupt",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    #[default]
    Free,
    Paid,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Max,
    Standard,
    #[default]
    Lite,
    Flex,
}

impl Model {
    pub const ALL: [Self; 4] =
        [Self::Max, Self::Standard, Self::Lite, Self::Flex];

    #[must_use]
    pub const fn name_key(self) -> &'static str {
        match self {
            Self::Max => "model.max",
            Self::Standard => "model.standard",
            Self::Lite => "model.lite",
            Self::Flex => "model.flex",
        }
    }

    #[must_use]
    pub const fn detail_key(self) -> &'static str {
        match self {
            Self::Max => "model.max_detail",
            Self::Standard => "model.standard_detail",
            Self::Lite => "model.lite_detail",
            Self::Flex => "model.flex_detail",
        }
    }

    #[must_use]
    pub const fn is_pro(self) -> bool {
        matches!(self, Self::Max | Self::Standard)
    }

    #[must_use]
    pub const fn can_use(self, plan: Plan) -> bool {
        match self {
            Self::Lite => true,
            Self::Max | Self::Standard => matches!(plan, Plan::Paid),
            Self::Flex => false,
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    Auto,
}

impl ThemeMode {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Light => "look.light",
            Self::Dark => "look.dark",
            Self::Auto => "look.auto",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Accent {
    #[default]
    Default = 0,
    Blue = 1,
    Green = 2,
    Orange = 3,
    Red = 4,
    Purple = 5,
}

impl Accent {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Blue,
        Self::Green,
        Self::Orange,
        Self::Red,
        Self::Purple,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Default => "look.default",
            Self::Blue => "look.blue",
            Self::Green => "look.green",
            Self::Orange => "look.orange",
            Self::Red => "look.red",
            Self::Purple => "look.purple",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Density {
    #[default]
    Compact = 0,
    Regular = 1,
    Comfortable = 2,
}

impl Density {
    pub const ALL: [Self; 3] =
        [Self::Compact, Self::Regular, Self::Comfortable];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Compact => "look.compact",
            Self::Regular => "look.regular",
            Self::Comfortable => "look.comfortable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub locale: Locale,
    pub send_mode: SendMode,
    pub notify_browser: bool,
    pub sound: bool,
    pub early_access: bool,
    pub plan: Plan,
    pub model: Model,
    pub model_info_version: String,
    pub theme: ThemeMode,
    pub accent: Accent,
    pub font_size: u8,
    pub density: Density,
    pub activity: bool,
    pub telemetry_opt_out: bool,
    pub launch_at_login: bool,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    locale: String,
    send_mode: SendMode,
    notify_browser: bool,
    sound: bool,
    early_access: bool,
    #[serde(default)]
    plan: Plan,
    #[serde(default)]
    model: Model,
    #[serde(default)]
    model_info_version: String,
    #[serde(default)]
    theme: ThemeMode,
    #[serde(default)]
    accent: Accent,
    #[serde(default = "default_font_size")]
    font_size: u8,
    #[serde(default)]
    density: Density,
    #[serde(default)]
    activity: bool,
    #[serde(default)]
    telemetry_opt_out: bool,
    #[serde(default)]
    launch_at_login: bool,
}

const fn default_font_size() -> u8 {
    14
}

const fn clamp_font_size(size: u8) -> u8 {
    if size < 12 {
        12
    } else if size > 16 {
        16
    } else {
        size
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale: Locale::ZhCn,
            send_mode: SendMode::Quiet,
            notify_browser: true,
            sound: true,
            early_access: true,
            plan: Plan::Free,
            model: Model::Lite,
            model_info_version: String::new(),
            theme: ThemeMode::Auto,
            accent: Accent::Default,
            font_size: default_font_size(),
            density: Density::Compact,
            activity: false,
            telemetry_opt_out: false,
            launch_at_login: false,
        }
    }
}

impl Preferences {
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let text = fs::read_to_string(root.join("preferences.json")).ok();
        let Some(text) = text else {
            return Self::default();
        };
        let file: File = serde_json::from_str(&text).unwrap_or_else(|_| File {
            version: VERSION,
            locale: Locale::ZhCn.code().to_string(),
            send_mode: SendMode::Quiet,
            notify_browser: true,
            sound: true,
            early_access: true,
            plan: Plan::Free,
            model: Model::Lite,
            model_info_version: String::new(),
            theme: ThemeMode::Auto,
            accent: Accent::Default,
            font_size: default_font_size(),
            density: Density::Compact,
            activity: false,
            telemetry_opt_out: false,
            launch_at_login: false,
        });
        if file.version != VERSION {
            return Self::default();
        }
        let model = if file.model.can_use(file.plan) {
            file.model
        } else {
            Model::Lite
        };
        Self {
            locale: Locale::parse(&file.locale).unwrap_or(Locale::ZhCn),
            send_mode: file.send_mode,
            notify_browser: file.notify_browser,
            sound: file.sound,
            early_access: file.early_access,
            plan: file.plan,
            model,
            model_info_version: file.model_info_version,
            theme: file.theme,
            accent: file.accent,
            font_size: clamp_font_size(file.font_size),
            density: file.density,
            activity: file.activity,
            telemetry_opt_out: file.telemetry_opt_out,
            launch_at_login: file.launch_at_login,
        }
    }

    #[must_use]
    pub const fn app_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    #[must_use]
    pub fn shows_model_info(&self) -> bool {
        self.plan == Plan::Free
            && self.model_info_version != Self::app_version()
    }

    pub fn save(&self, root: &Path) {
        let file = File {
            version: VERSION,
            locale: self.locale.code().to_string(),
            send_mode: self.send_mode,
            notify_browser: self.notify_browser,
            sound: self.sound,
            early_access: self.early_access,
            plan: self.plan,
            model: self.model,
            model_info_version: self.model_info_version.clone(),
            theme: self.theme,
            accent: self.accent,
            font_size: self.font_size,
            density: self.density,
            activity: self.activity,
            telemetry_opt_out: self.telemetry_opt_out,
            launch_at_login: self.launch_at_login,
        };
        let Ok(bytes) = serde_json::to_vec_pretty(&file) else {
            return;
        };
        let _ = crate::disk::commit(&root.join("preferences.json"), &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::{Model, Plan, Preferences};
    use crate::i18n::Locale;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn free_plan_sees_model_info_once_per_version() {
        let mut prefs = Preferences::default();
        assert!(prefs.shows_model_info());
        prefs.model_info_version = Preferences::app_version().to_string();
        assert!(!prefs.shows_model_info());
        prefs.model_info_version.clear();
        prefs.plan = Plan::Paid;
        assert!(!prefs.shows_model_info());
    }

    #[test]
    fn older_preferences_keep_their_locale_and_show_model_info() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("flowy-prefs-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("preferences.json"),
            r#"{"version":1,"locale":"en","send_mode":"quiet","notify_browser":true,"sound":true,"early_access":false}"#,
        )
        .unwrap();
        let prefs = Preferences::load(&dir);
        assert_eq!(prefs.locale, Locale::En);
        assert!(!prefs.early_access);
        assert_eq!(prefs.plan, Plan::Free);
        assert_eq!(prefs.model, Model::Lite);
        assert!(prefs.shows_model_info());
        assert_eq!(prefs.theme, super::ThemeMode::Auto);
        assert_eq!(prefs.accent, super::Accent::Default);
        assert_eq!(prefs.font_size, 14);
        assert_eq!(prefs.density, super::Density::Compact);
        assert!(!prefs.activity);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn free_plan_does_not_keep_a_pro_model() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("flowy-model-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("preferences.json");
        fs::write(
            &path,
            r#"{"version":1,"locale":"zh-CN","send_mode":"quiet","notify_browser":true,"sound":true,"early_access":true,"plan":"free","model":"max"}"#,
        )
        .unwrap();
        assert_eq!(Preferences::load(&dir).model, Model::Lite);
        fs::write(
            &path,
            r#"{"version":1,"locale":"zh-CN","send_mode":"quiet","notify_browser":true,"sound":true,"early_access":true,"plan":"paid","model":"max"}"#,
        )
        .unwrap();
        assert_eq!(Preferences::load(&dir).model, Model::Max);
        assert!(!Model::Flex.can_use(Plan::Paid));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn font_size_stays_between_12_and_16() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("flowy-font-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("preferences.json");
        fs::write(
            &path,
            r#"{"version":1,"locale":"zh-CN","send_mode":"quiet","notify_browser":true,"sound":true,"early_access":true,"font_size":3,"theme":"dark","accent":"green","density":"comfortable","activity":true}"#,
        )
        .unwrap();
        let prefs = Preferences::load(&dir);
        assert_eq!(prefs.font_size, 12);
        assert_eq!(prefs.theme, super::ThemeMode::Dark);
        assert_eq!(prefs.accent, super::Accent::Green);
        assert_eq!(prefs.density, super::Density::Comfortable);
        assert!(prefs.activity);
        fs::write(
            &path,
            r#"{"version":1,"locale":"zh-CN","send_mode":"quiet","notify_browser":true,"sound":true,"early_access":true,"font_size":40}"#,
        )
        .unwrap();
        assert_eq!(Preferences::load(&dir).font_size, 16);
        let _ = fs::remove_dir_all(dir);
    }
}
