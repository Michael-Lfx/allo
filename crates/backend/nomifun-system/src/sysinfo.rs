use nomifun_api_types::SystemInfoResponse;
use nomifun_common::storage_paths;

/// Map Rust `std::env::consts::OS` to the Node.js-compatible platform name
/// used by the API contract.
fn map_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other, // "linux" stays "linux"
    }
}

/// Map Rust `std::env::consts::ARCH` to the API contract arch name.
fn map_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn env_path(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(v) = std::env::var(key)
            && !v.is_empty()
        {
            return Some(v);
        }
    }
    None
}

/// Resolve the cache directory for Flowy.
///
/// Priority: `FLOWY_CACHE_DIR` / `NOMIFUN_CACHE_DIR` env →
/// `{cache_dir}/Flowy/runtime`.
fn resolve_cache_dir() -> String {
    if let Some(v) = env_path(&["FLOWY_CACHE_DIR", "NOMIFUN_CACHE_DIR"]) {
        return v;
    }
    dirs::cache_dir()
        .map(|p| {
            p.join(storage_paths::DATA_VENDOR_DIR)
                .join("runtime")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

/// Resolve the work (data) directory for Flowy.
///
/// Priority: `FLOWY_WORK_DIR` / `NOMIFUN_WORK_DIR` env → shared `Flowy/Nomi`
/// default from [`storage_paths::default_data_dir`].
fn resolve_work_dir() -> String {
    if let Some(v) = env_path(&["FLOWY_WORK_DIR", "NOMIFUN_WORK_DIR"]) {
        return v;
    }
    storage_paths::default_data_dir(&nomifun_common::channel::dir_suffix())
        .to_string_lossy()
        .into_owned()
}

/// Resolve the log directory for Flowy.
///
/// Priority: `FLOWY_LOG_DIR` / `NOMIFUN_LOG_DIR` env → `{data_dir}/logs`.
fn resolve_log_dir() -> String {
    if let Some(v) = env_path(&["FLOWY_LOG_DIR", "NOMIFUN_LOG_DIR"]) {
        return v;
    }
    storage_paths::default_data_dir(&nomifun_common::channel::dir_suffix())
        .join("logs")
        .to_string_lossy()
        .into_owned()
}

/// Build the system info response from the current runtime environment.
pub fn get_system_info() -> SystemInfoResponse {
    SystemInfoResponse {
        cache_dir: resolve_cache_dir(),
        work_dir: resolve_work_dir(),
        log_dir: resolve_log_dir(),
        platform: map_platform().to_owned(),
        arch: map_arch().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_platform_known() {
        let p = map_platform();
        // On CI this will be one of the known values
        assert!(["darwin", "win32", "linux"].contains(&p), "unexpected platform: {p}");
    }

    #[test]
    fn test_map_arch_known() {
        let a = map_arch();
        assert!(["x64", "arm64"].contains(&a), "unexpected arch: {a}");
    }

    #[test]
    fn test_get_system_info_fields_non_empty() {
        let info = get_system_info();
        assert!(!info.cache_dir.is_empty(), "cache_dir should not be empty");
        assert!(!info.work_dir.is_empty(), "work_dir should not be empty");
        assert!(!info.log_dir.is_empty(), "log_dir should not be empty");
        assert!(!info.platform.is_empty());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn default_cache_dir_uses_flowy_runtime() {
        let dir = resolve_cache_dir();
        assert!(
            dir.contains("Flowy") && dir.contains("runtime"),
            "cache_dir should use Flowy/runtime fallback, got {dir}"
        );
        assert!(!dir.contains("nomifun"), "cache_dir should not contain nomifun: {dir}");
    }

    #[test]
    fn default_work_dir_uses_flowy_nomi() {
        let dir = resolve_work_dir();
        assert!(
            dir.contains("Flowy") && dir.contains("Nomi"),
            "work_dir should use Flowy/Nomi fallback, got {dir}"
        );
        assert!(!dir.contains("nomifun"), "work_dir should not contain nomifun: {dir}");
    }

    #[test]
    fn default_log_dir_uses_flowy_nomi_logs() {
        let dir = resolve_log_dir();
        assert!(
            dir.contains("Flowy") && dir.ends_with("logs"),
            "log_dir should use Flowy/Nomi/logs fallback, got {dir}"
        );
        assert!(!dir.contains("nomifun"), "log_dir should not contain nomifun: {dir}");
    }
}
