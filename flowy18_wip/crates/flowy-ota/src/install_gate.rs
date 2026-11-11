use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallReason {
    AppBundleNotFound,
    AppTranslocation,
    MountedVolume,
    CrossDevice,
    MetadataUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallDecision {
    Supported,
    Unsupported(InstallReason),
}

#[derive(Debug, Clone)]
pub struct InstallContext {
    app_bundle: Option<PathBuf>,
    extract_dir: PathBuf,
    auto_install_supported: bool,
    reason: Option<InstallReason>,
}

impl InstallContext {
    pub fn app_bundle(&self) -> Option<&Path> {
        self.app_bundle.as_deref()
    }

    pub fn extract_dir(&self) -> &Path {
        &self.extract_dir
    }

    pub fn auto_install_supported(&self) -> bool {
        self.auto_install_supported
    }

    pub fn reason_key(&self) -> Option<&'static str> {
        match self.reason {
            Some(InstallReason::AppBundleNotFound) => Some("ota.reason_bundle"),
            Some(InstallReason::AppTranslocation) => {
                Some("ota.reason_translocation")
            }
            Some(InstallReason::MountedVolume) => Some("ota.reason_volume"),
            Some(InstallReason::CrossDevice) => Some("ota.reason_cross_device"),
            Some(InstallReason::MetadataUnavailable) => {
                Some("ota.reason_metadata")
            }
            None => None,
        }
    }
}

fn classify_macos_install(
    app_bundle: Option<&Path>,
    app_device_id: Option<u64>,
    extract_device_id: Option<u64>,
) -> InstallDecision {
    let Some(app_bundle) = app_bundle else {
        return InstallDecision::Unsupported(InstallReason::AppBundleNotFound);
    };
    let display = app_bundle.to_string_lossy();
    if display.contains("/AppTranslocation/") {
        return InstallDecision::Unsupported(InstallReason::AppTranslocation);
    }
    if app_bundle.starts_with("/Volumes") {
        return InstallDecision::Unsupported(InstallReason::MountedVolume);
    }
    match (app_device_id, extract_device_id) {
        (Some(app), Some(extract)) if app == extract => {
            InstallDecision::Supported
        }
        (Some(_), Some(_)) => {
            InstallDecision::Unsupported(InstallReason::CrossDevice)
        }
        _ => InstallDecision::Unsupported(InstallReason::MetadataUnavailable),
    }
}

fn classify_install(
    platform: &str,
    app_bundle: Option<&Path>,
    app_device_id: Option<u64>,
    extract_device_id: Option<u64>,
) -> InstallDecision {
    if platform != "macos" {
        return InstallDecision::Supported;
    }
    classify_macos_install(app_bundle, app_device_id, extract_device_id)
}

#[cfg(any(test, target_os = "macos"))]
fn app_bundle_from_executable(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        })
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn device_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path).ok().map(|metadata| metadata.dev())
}

/// Inspect whether the running install can be replaced in place.
///
/// Extraction uses the parent of the `.app` (same volume), not `/tmp`.
pub fn install_context() -> InstallContext {
    let platform = std::env::consts::OS;

    #[cfg(target_os = "macos")]
    let app_bundle = std::env::current_exe()
        .ok()
        .and_then(|executable| app_bundle_from_executable(&executable));
    #[cfg(target_os = "macos")]
    let extract_dir = app_bundle
        .as_ref()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(target_os = "macos")]
    let app_device_id = app_bundle.as_deref().and_then(device_id);
    #[cfg(target_os = "macos")]
    let extract_device_id = device_id(&extract_dir);

    #[cfg(not(target_os = "macos"))]
    let app_bundle: Option<PathBuf> = None;
    #[cfg(not(target_os = "macos"))]
    let extract_dir = std::env::temp_dir();
    #[cfg(not(target_os = "macos"))]
    let app_device_id = None;
    #[cfg(not(target_os = "macos"))]
    let extract_device_id = None;

    let decision = classify_install(
        platform,
        app_bundle.as_deref(),
        app_device_id,
        extract_device_id,
    );
    let (auto_install_supported, reason) = match decision {
        InstallDecision::Supported => (true, None),
        InstallDecision::Unsupported(reason) => (false, Some(reason)),
    };
    InstallContext {
        app_bundle,
        extract_dir,
        auto_install_supported,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(
        path: Option<&str>,
        app_dev: Option<u64>,
        extract_dev: Option<u64>,
    ) -> InstallDecision {
        classify_macos_install(path.map(Path::new), app_dev, extract_dev)
    }

    #[test]
    fn applications_bundle_on_same_device_is_supported() {
        assert_eq!(
            classify(Some("/Applications/Flowy.app"), Some(7), Some(7)),
            InstallDecision::Supported,
        );
    }

    #[test]
    fn mounted_volume_is_rejected_before_device_comparison() {
        assert_eq!(
            classify(Some("/Volumes/Flowy/Flowy.app"), Some(9), Some(7)),
            InstallDecision::Unsupported(InstallReason::MountedVolume),
        );
    }

    #[test]
    fn app_translocation_is_rejected() {
        assert_eq!(
            classify(
                Some("/private/var/folders/x/AppTranslocation/ABC/d/Flowy.app"),
                Some(7),
                Some(7),
            ),
            InstallDecision::Unsupported(InstallReason::AppTranslocation),
        );
    }

    #[test]
    fn different_devices_are_rejected() {
        assert_eq!(
            classify(Some("/Users/muri/Apps/Flowy.app"), Some(9), Some(7)),
            InstallDecision::Unsupported(InstallReason::CrossDevice),
        );
    }

    #[test]
    fn missing_bundle_or_metadata_fail_closed() {
        assert_eq!(
            classify(None, Some(7), Some(7)),
            InstallDecision::Unsupported(InstallReason::AppBundleNotFound),
        );
        assert_eq!(
            classify(Some("/Applications/Flowy.app"), None, Some(7)),
            InstallDecision::Unsupported(InstallReason::MetadataUnavailable),
        );
    }

    #[test]
    fn non_macos_platforms_keep_existing_updater_support() {
        assert_eq!(
            classify_install("windows", None, None, None),
            InstallDecision::Supported
        );
        assert_eq!(
            classify_install("linux", None, None, None),
            InstallDecision::Supported
        );
    }

    #[test]
    fn finds_app_bundle_ancestor_from_executable() {
        assert_eq!(
            app_bundle_from_executable(Path::new(
                "/Applications/Flowy.app/Contents/MacOS/flowy-app"
            )),
            Some(PathBuf::from("/Applications/Flowy.app")),
        );
    }
}
