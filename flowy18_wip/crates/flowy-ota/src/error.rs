use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("network: {0}")]
    Network(String),
    #[error("manifest: {0}")]
    Manifest(String),
    #[error("verify: {0}")]
    Verify(String),
    #[error("install: {0}")]
    Install(String),
    #[error("this platform cannot auto-install yet")]
    PlatformUnsupported,
    #[error("automatic install is blocked: {0}")]
    InstallBlocked(&'static str),
    #[error("no update package is ready")]
    NotReady,
    #[error("update slot busy: {0}")]
    Busy(String),
    #[error("version mismatch: wanted {wanted}, got {got}")]
    VersionMismatch { wanted: String, got: String },
}

impl UpdateError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Network(_) => "ota.err_network",
            Self::Manifest(_) => "ota.err_manifest",
            Self::Verify(_) => "ota.err_verify",
            Self::Install(_) => "ota.err_install",
            Self::PlatformUnsupported => "ota.err_platform",
            Self::InstallBlocked(_) => "ota.err_blocked",
            Self::NotReady => "ota.err_not_ready",
            Self::Busy(_) => "ota.err_busy",
            Self::VersionMismatch { .. } => "ota.err_version",
        }
    }

    pub fn from_take_ready(error: crate::cache::TakeReadyError) -> Self {
        match error {
            crate::cache::TakeReadyError::NotRetained { .. } => Self::NotReady,
            other => Self::Busy(other.message()),
        }
    }
}
