use flowy_obs::{BootNoteLevel, LogGuards, Telemetry, TelemetryConfig};
use std::sync::{Arc, OnceLock};

static LOG_GUARDS: OnceLock<LogGuards> = OnceLock::new();

fn main() {
    let data_root = flowy_app::app::default_store_root();
    flowy_obs::record_boot_note(
        BootNoteLevel::Info,
        format!(
            "starting {} {}",
            flowy_app::brand_api::product(),
            env!("CARGO_PKG_VERSION")
        ),
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

    let prefs = flowy_app::prefs::Preferences::load(&data_root);
    let telemetry = Arc::new(Telemetry::open(TelemetryConfig {
        data_dir: data_root,
        opt_out: prefs.telemetry_opt_out,
    }));
    flowy_app::set_telemetry(Arc::clone(&telemetry));
    let mut props = serde_json::Map::new();
    props.insert("cold_start".into(), serde_json::Value::Bool(true));
    props.insert(
        "locale".into(),
        serde_json::Value::String(prefs.locale.code().to_owned()),
    );
    telemetry.capture("app_launch", props);

    flowy_shell::configure_brand(flowy_shell::Brand {
        product: flowy_app::brand_api::product(),
        bundle_id: flowy_app::brand_api::bundle_id(),
        url_scheme: flowy_app::brand_api::url_scheme(),
        logo_png: flowy_app::brand_api::logo_png(),
    });
    if prefs.launch_at_login != flowy_shell::autostart_enabled() {
        let _ = flowy_shell::set_autostart(prefs.launch_at_login);
    }

    let application = gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit);
    application.on_open_urls(|urls| {
        flowy_shell::enqueue_urls(urls);
    });
    application.on_reopen(|cx| {
        flowy_shell::ensure_main_window(cx);
    });
    application.run(flowy_app::app::run);
}
