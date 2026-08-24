//! Register a per-user Windows AppUserModelID so toast notifications show as
//! "Flowy" (with the app icon) instead of "Windows PowerShell".
//!
//! Unpackaged / `tauri dev` builds never get an installer-written Start Menu
//! shortcut AUMID, so WinRT falls back to PowerShell unless we:
//! 1. write `HKCU\Software\Classes\AppUserModelId\{id}` with DisplayName (+ IconUri)
//! 2. pin the process with `SetCurrentProcessExplicitAppUserModelID`
//! 3. pass the same id to `Toast::new`

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tracing::{info, warn};

/// Idempotent: safe to call every launch. Failures are logged and non-fatal —
/// toasts still work, they just may keep the PowerShell brand.
pub fn register_for_toasts(app: &AppHandle) {
    let aumid = app.config().identifier.clone();
    let display_name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "Flowy".to_owned());
    let icon = resolve_toast_icon(app);

    if let Err(error) = write_aumid_registry(&aumid, &display_name, icon.as_deref()) {
        warn!(%aumid, %error, "failed to register AppUserModelId for toasts");
    }
    if let Err(error) = set_process_aumid(&aumid) {
        warn!(%aumid, %error, "failed to set process AppUserModelId");
        return;
    }
    info!(
        %aumid,
        %display_name,
        icon = icon.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        "Windows AppUserModelId registered for notifications"
    );
}

fn write_aumid_registry(
    aumid: &str,
    display_name: &str,
    icon: Option<&Path>,
) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(r"Software\Classes\AppUserModelId\{aumid}");
    let (key, _) = hkcu
        .create_subkey(&path)
        .map_err(|e| format!("create_subkey {path}: {e}"))?;
    key.set_value("DisplayName", &display_name)
        .map_err(|e| format!("set DisplayName: {e}"))?;
    if let Some(icon) = icon {
        // WinRT IconUri wants a filesystem path; forward slashes also work.
        let uri = icon.to_string_lossy().replace('/', "\\");
        key.set_value("IconUri", &uri)
            .map_err(|e| format!("set IconUri: {e}"))?;
    }
    Ok(())
}

fn set_process_aumid(aumid: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    unsafe {
        SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(aumid))
            .map_err(|e| e.to_string())
    }
}

fn resolve_toast_icon(app: &AppHandle) -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "icons/icon.ico",
        "icon.ico",
        "icons/128x128.png",
        "icon.png",
        "icons/icon.png",
    ];

    if let Ok(dir) = app.path().resource_dir() {
        for name in CANDIDATES {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in CANDIDATES {
                let path = parent.join(name);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }

    let manifest_icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("icons")
        .join("icon.ico");
    manifest_icon.is_file().then_some(manifest_icon)
}
