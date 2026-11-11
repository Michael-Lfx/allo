mod agent;
pub(crate) mod app;
mod brand;
#[cfg(test)]
mod brand_manifest;
mod chat;
mod disk;
mod i18n;
mod persona;
mod prefs;
mod session;
mod shell;
mod theme;

use flowy_obs::{BootNoteLevel, LogGuards, Telemetry, TelemetryConfig};
use std::sync::{Arc, OnceLock};

static LOG_GUARDS: OnceLock<LogGuards> = OnceLock::new();
static TELEMETRY: OnceLock<Arc<Telemetry>> = OnceLock::new();

fn main() {
    let data_root = session::Store::default_root();
    flowy_obs::record_boot_note(
        BootNoteLevel::Info,
        format!("starting {} {}", brand::product(), env!("CARGO_PKG_VERSION")),
    );
    let log_dir = std::env::var_os("FLOWY_LOG_DIR").map_or_else(
        || flowy_obs::logs_dir_for_data(&data_root),
        std::path::PathBuf::from,
    );
    let level = std::env::var("FLOWY_LOG_LEVEL").ok();
    let _ = LOG_GUARDS.set(flowy_obs::init_tracing(&log_dir, level.as_deref()));
    tracing::info!(
        target: "flowy.boot",
        path = %log_dir.display(),
        "logging ready"
    );

    let prefs = prefs::Preferences::load(&data_root);
    let telemetry = Arc::new(Telemetry::open(TelemetryConfig {
        data_dir: data_root,
        opt_out: prefs.telemetry_opt_out,
    }));
    let _ = TELEMETRY.set(Arc::clone(&telemetry));
    let mut props = serde_json::Map::new();
    props.insert("cold_start".into(), serde_json::Value::Bool(true));
    props.insert(
        "locale".into(),
        serde_json::Value::String(prefs.locale.code().to_owned()),
    );
    telemetry.capture("app_launch", props);

    if prefs.launch_at_login != shell::autostart_enabled() {
        let _ = shell::set_autostart(prefs.launch_at_login);
    }

    let application = gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit);
    application.on_open_urls(|urls| {
        shell::enqueue_urls(urls);
    });
    application.on_reopen(|cx| {
        shell::ensure_main_window(cx);
    });
    application.run(app::run);
}

pub(crate) fn telemetry() -> Option<&'static Arc<Telemetry>> {
    TELEMETRY.get()
}
