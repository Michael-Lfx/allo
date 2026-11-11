//! Detect updater artifact kinds from filenames.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    MacAppArchive,
    WindowsSetup,
    WindowsMsi,
    LinuxAppImage,
}

impl PackageKind {
    pub fn from_filename(filename: &str) -> Option<Self> {
        let lower = filename.to_ascii_lowercase();
        if lower.ends_with(".app.tar.gz") {
            Some(Self::MacAppArchive)
        } else if lower.ends_with(".msi") {
            Some(Self::WindowsMsi)
        } else if lower.ends_with("-setup.exe") || lower.ends_with(".exe") {
            Some(Self::WindowsSetup)
        } else if lower.ends_with(".appimage") {
            Some(Self::LinuxAppImage)
        } else {
            None
        }
    }

    pub fn from_url(url: &str) -> Option<Self> {
        crate::mirrors::artifact_filename(url)
            .as_deref()
            .and_then(Self::from_filename)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::MacAppArchive => "app.tar.gz",
            Self::WindowsSetup => "exe",
            Self::WindowsMsi => "msi",
            Self::LinuxAppImage => "AppImage",
        }
    }

    pub fn matches_host(self) -> bool {
        match self {
            Self::MacAppArchive => cfg!(target_os = "macos"),
            Self::WindowsSetup | Self::WindowsMsi => {
                cfg!(target_os = "windows")
            }
            Self::LinuxAppImage => cfg!(target_os = "linux"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_allo_artifact_names() {
        assert_eq!(
            PackageKind::from_filename("Flowy_1.8.1_x64-setup.exe"),
            Some(PackageKind::WindowsSetup)
        );
        assert_eq!(
            PackageKind::from_filename("Flowy.app.tar.gz"),
            Some(PackageKind::MacAppArchive)
        );
        assert_eq!(
            PackageKind::from_filename("Flowy_1.8.1_amd64.AppImage"),
            Some(PackageKind::LinuxAppImage)
        );
        assert_eq!(
            PackageKind::from_filename("Flowy_1.8.1_x64.msi"),
            Some(PackageKind::WindowsMsi)
        );
    }
}
