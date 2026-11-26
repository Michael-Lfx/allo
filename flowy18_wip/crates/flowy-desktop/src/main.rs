use flowy_obs::{BootNoteLevel, LogGuards, Telemetry, TelemetryConfig};
use std::sync::{Arc, OnceLock};

static LOG_GUARDS: OnceLock<LogGuards> = OnceLock::new();

#[cfg(debug_assertions)]
fn validation_root(
    normal: std::path::PathBuf,
) -> std::io::Result<(std::path::PathBuf, bool)> {
    let Some(path) = std::env::var_os("FLOWY_AUTOMATION_VALIDATION_ROOT")
    else {
        return Ok((normal, false));
    };
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(std::io::Error::other(
            "expected an existing absolute directory",
        ));
    }
    let path = path.canonicalize()?;
    let normal = normal.canonicalize().unwrap_or(normal);
    if path.starts_with(&normal) || normal.starts_with(&path) {
        return Err(std::io::Error::other(
            "expected a directory separate from the default data root",
        ));
    }
    Ok((path, true))
}

/// Point pi-rust's temp staging and child shells at the app runtime directory.
///
/// SAFETY: called from `main` before GPUI, tracing, telemetry, or any worker
/// thread starts, so no other thread can read the environment concurrently.
#[allow(unsafe_code)]
fn set_temp_env(dir: &std::path::Path) {
    unsafe {
        std::env::set_var("TMP", dir);
        std::env::set_var("TEMP", dir);
        std::env::set_var("TMPDIR", dir);
    }
}

fn main() {
    // tracing is not live yet — queue notes until init_tracing drains them.
    flowy_obs::record_boot_note(BootNoteLevel::Info, "stage=main enter");
    let data_root = flowy_app::app::default_store_root();
    #[cfg(debug_assertions)]
    let (data_root, validation) = match validation_root(data_root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("Invalid FLOWY_AUTOMATION_VALIDATION_ROOT: {error}");
            return;
        }
    };
    #[cfg(not(debug_assertions))]
    let validation = false;
    flowy_obs::record_boot_note(
        BootNoteLevel::Info,
        format!(
            "stage=main::default_store_root path={}",
            data_root.display()
        ),
    );
    // Held for the whole process. Dropping it releases the lock, and the OS
    // releases it anyway when the process dies, so a crash cannot leave a
    // stale lock behind.
    let bootstrap = match flowy_app::app::OwnedBootstrap::acquire(&data_root) {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!(
                "{} could not acquire its data directory ({error}).",
                flowy_app::brand_api::product()
            );
            return;
        }
    };
    let runtime_temp = flowy_app::session::ensure_runtime_temp(&data_root);
    if let Ok(dir) = &runtime_temp {
        set_temp_env(dir);
        flowy_obs::record_boot_note(
            BootNoteLevel::Info,
            format!("stage=main::ensure_runtime_temp path={}", dir.display()),
        );
    } else if let Err(error) = &runtime_temp {
        flowy_obs::record_boot_note(
            BootNoteLevel::Warn,
            format!(
                "stage=main::ensure_runtime_temp error={error}; using system temp"
            ),
        );
    }
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
    tracing::debug!(
        target: "flowy.boot",
        stage = "main::init_tracing",
        path = %log_dir.display(),
        level = ?level.as_deref(),
        "logging ready"
    );

    let prefs = flowy_app::prefs::Preferences::load(&data_root);
    tracing::debug!(
        target: "flowy.boot",
        stage = "main::Preferences::load",
        locale = %prefs.locale.code(),
        launch_at_login = prefs.launch_at_login,
        "prefs loaded"
    );
    let telemetry = Arc::new(Telemetry::open(TelemetryConfig {
        data_dir: data_root,
        opt_out: validation || prefs.telemetry_opt_out,
    }));
    flowy_app::set_telemetry(Arc::clone(&telemetry));
    let mut props = serde_json::Map::new();
    props.insert("cold_start".into(), serde_json::Value::Bool(true));
    props.insert(
        "locale".into(),
        serde_json::Value::String(prefs.locale.code().to_owned()),
    );
    telemetry.capture("app_launch", props);
    tracing::debug!(
        target: "flowy.boot",
        stage = "main::Telemetry::capture",
        event = "app_launch",
        "telemetry"
    );

    flowy_shell::configure_brand(flowy_shell::Brand {
        product: flowy_app::brand_api::product(),
        bundle_id: flowy_app::brand_api::bundle_id(),
        url_scheme: flowy_app::brand_api::url_scheme(),
        logo_png: flowy_app::brand_api::logo_png(),
    });
    tracing::debug!(
        target: "flowy.boot",
        stage = "main::configure_brand",
        product = flowy_app::brand_api::product(),
        "brand"
    );
    if !validation && prefs.launch_at_login != flowy_shell::autostart_enabled()
    {
        let _ = flowy_shell::set_autostart(prefs.launch_at_login);
        tracing::debug!(
            target: "flowy.boot",
            stage = "main::set_autostart",
            enabled = prefs.launch_at_login,
            "synced"
        );
    }

    let application =
        gpui_platform::application().with_quit_mode(gpui::QuitMode::Explicit);
    application.on_open_urls(|urls| {
        tracing::debug!(
            target: "flowy.boot",
            stage = "main::on_open_urls",
            count = urls.len(),
            "enqueue"
        );
        flowy_shell::enqueue_urls(urls);
    });
    application.on_reopen(|cx| {
        tracing::debug!(
            target: "flowy.boot",
            stage = "main::on_reopen",
            "ensure_main_window"
        );
        flowy_shell::ensure_main_window(cx);
    });
    tracing::debug!(
        target: "flowy.boot",
        stage = "main::application.run",
        "→ app::run"
    );
    application.run(move |cx| {
        if let Err(error) = flowy_app::app::run_owned(bootstrap, cx) {
            tracing::error!(%error, "could not start application host");
            cx.quit();
        } else {
            flowy_app::app::start_automation_host(cx);
        }
    });
}
