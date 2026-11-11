use crate::error::UpdateError;
use crate::package::PackageKind;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hand the verified installer to Windows and exit this process.
pub fn install_and_exit(
    path: &Path,
    kind: PackageKind,
) -> Result<(), UpdateError> {
    match kind {
        PackageKind::WindowsSetup => {
            Command::new(path)
                .arg("/S")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|error| UpdateError::Install(error.to_string()))?;
        }
        PackageKind::WindowsMsi => {
            Command::new("msiexec")
                .args(["/i", &path.display().to_string(), "/qn", "/norestart"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|error| UpdateError::Install(error.to_string()))?;
        }
        PackageKind::MacAppArchive | PackageKind::LinuxAppImage => {
            return Err(UpdateError::PlatformUnsupported);
        }
    }
    std::process::exit(0);
}
