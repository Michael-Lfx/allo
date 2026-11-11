use crate::error::UpdateError;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Replace the running AppImage with the verified package, then relaunch.
pub fn install_appimage_and_exit(package: &Path) -> Result<(), UpdateError> {
    let target = current_appimage()?;
    let parent = target.parent().ok_or_else(|| {
        UpdateError::Install("AppImage has no parent directory".into())
    })?;
    let staging = parent.join(format!(
        ".flowy-ota-{}-{}.AppImage",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    ));
    let backup = target.with_extension("AppImage.bak");

    fs::copy(package, &staging)
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    let mut permissions = fs::metadata(&staging)
        .map_err(|error| UpdateError::Install(error.to_string()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&staging, permissions)
        .map_err(|error| UpdateError::Install(error.to_string()))?;

    let _ = fs::remove_file(&backup);
    if let Err(error) = fs::rename(&target, &backup) {
        let _ = fs::remove_file(&staging);
        return Err(UpdateError::Install(error.to_string()));
    }
    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::rename(&backup, &target);
        let _ = fs::remove_file(&staging);
        return Err(UpdateError::Install(error.to_string()));
    }
    let _ = fs::remove_file(&backup);

    Command::new(&target)
        .spawn()
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    std::process::exit(0);
}

fn current_appimage() -> Result<PathBuf, UpdateError> {
    if let Some(path) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    let exe = std::env::current_exe()
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    let name = exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".appimage") && exe.is_file() {
        return Ok(exe);
    }
    Err(UpdateError::InstallBlocked("ota.reason_not_appimage"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_appimage_env_is_blocked_for_normal_binaries() {
        // Unit tests run as a normal binary, not an AppImage.
        let error = current_appimage().unwrap_err();
        assert!(matches!(
            error,
            UpdateError::InstallBlocked("ota.reason_not_appimage")
        ));
    }
}
