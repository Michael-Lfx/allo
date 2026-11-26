use flowy_obs::{BootNoteLevel, LogGuards, Telemetry, TelemetryConfig};
use std::sync::{Arc, OnceLock};

static LOG_GUARDS: OnceLock<LogGuards> = OnceLock::new();

/// Exclusive lock on the data directory, one per running instance.
///
/// Two instances would each write `preferences.json`, each hold their own
/// session list, and each add a tray icon, so the second one exits instead.
/// The lock is an advisory file lock rather than a sentinel file: the OS drops
/// it when the process dies, so a crash never leaves the app unable to start.
struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    fn acquire(data_root: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(data_root)?;
        let path = data_root.join("instance.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} is locked by another instance", path.display()),
            ),
            std::fs::TryLockError::Error(error) => error,
        })?;
        Ok(Self { _file: file })
    }
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

/// Filter raw CLI launch arguments down to valid deep-link URLs for `scheme`
/// without requiring `flowy_shell::configure_brand` to have run first.
fn extract_deep_link_args(args: &[String], scheme: &str) -> Vec<String> {
    args.iter()
        .map(|raw| raw.trim())
        .filter(|arg| {
            !arg.is_empty()
                && flowy_shell::parse_deep_link_with_scheme(arg, scheme)
                    .is_some()
        })
        .map(str::to_owned)
        .collect()
}

fn main() {
    // tracing is not live yet — queue notes until init_tracing drains them.
    flowy_obs::record_boot_note(BootNoteLevel::Info, "stage=main enter");
    let data_root = flowy_app::app::default_store_root();
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
    let _instance = match InstanceLock::acquire(&data_root) {
        Ok(lock) => lock,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                let args: Vec<String> = std::env::args().skip(1).collect();
                let urls = extract_deep_link_args(
                    &args,
                    flowy_app::brand_api::url_scheme(),
                );
                if let Err(wake_error) =
                    flowy_shell::signal_wake(&data_root, &urls)
                {
                    eprintln!(
                        "{} is already running, and wake signal failed ({wake_error}).",
                        flowy_app::brand_api::product()
                    );
                }
            } else {
                eprintln!(
                    "{} is already running ({error}).",
                    flowy_app::brand_api::product()
                );
            }
            return;
        }
    };
    match flowy_app::session::snapshot_sessions_before_boot(&data_root) {
        Ok(true) => flowy_obs::record_boot_note(
            BootNoteLevel::Info,
            "stage=main::snapshot_sessions_before_boot created=true",
        ),
        Ok(false) => {}
        Err(error) => flowy_obs::record_boot_note(
            BootNoteLevel::Warn,
            format!("stage=main::snapshot_sessions_before_boot error={error}"),
        ),
    }
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
    let cold_args: Vec<String> = std::env::args().skip(1).collect();
    let cold_urls =
        extract_deep_link_args(&cold_args, flowy_app::brand_api::url_scheme());
    if !cold_urls.is_empty() {
        tracing::debug!(
            target: "flowy.boot",
            stage = "main::cold_deep_links",
            count = cold_urls.len(),
            "enqueue"
        );
        flowy_shell::enqueue_urls(cold_urls);
    }
    if prefs.launch_at_login != flowy_shell::autostart_enabled() {
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
    application.run(flowy_app::app::run);
}

#[cfg(test)]
mod tests {
    use super::extract_deep_link_args;

    #[test]
    fn extract_deep_link_args_filters_non_deep_links_without_brand_init() {
        let args = vec![
            "--minimized".to_owned(),
            r"C:\temp\notes.txt".to_owned(),
            "https://example.com/invite/ABC".to_owned(),
            "  flowy://invite/INV_123  ".to_owned(),
            "flowy://open".to_owned(),
            "flowy://invite/".to_owned(),
            String::new(),
        ];
        assert_eq!(
            extract_deep_link_args(&args, "flowy"),
            vec![
                "flowy://invite/INV_123".to_owned(),
                "flowy://open".to_owned(),
            ]
        );
    }
}
