//! Errors for the local model runtime, mapped onto [`nomifun_common::AppError`].

use nomifun_common::AppError;
use nomifun_api_types::{LocalModelDownloadError, LocalModelDownloadErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum LocalModelError {
    #[error("unknown local model: {0}")]
    UnknownModel(String),

    #[error("model is not installed: {0}")]
    NotInstalled(String),

    #[error("model is already running: {0}")]
    AlreadyRunning(String),

    #[error("model is not running: {0}")]
    NotRunning(String),

    #[error("model is busy: {0}")]
    Busy(String),

    #[error("download cancelled")]
    Cancelled,

    #[error("download header timed out: {0}")]
    HeaderTimeout(String),

    #[error("download failed: {0}")]
    Download(LocalModelDownloadError),

    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("runtime launch failed: {0}")]
    Launch(String),

    #[error("local model io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("local model internal error: {0}")]
    Internal(String),
}

impl LocalModelError {
    /// Build a structured download error.
    pub(crate) fn download(
        code: LocalModelDownloadErrorCode,
        detail: impl Into<Option<String>>,
    ) -> Self {
        Self::Download(LocalModelDownloadError::new(code, detail))
    }

    /// The user-facing failure footprint recorded on [`LocalModelStatus`], or
    /// `None` for states that are not download failures (e.g. cancellation).
    ///
    /// [`LocalModelStatus`]: nomifun_api_types::LocalModelStatus
    pub(crate) fn download_footprint(&self) -> Option<LocalModelDownloadError> {
        match self {
            Self::Cancelled => None,
            // A stalled header is a timeout for the user, but the caller treats
            // it as a reason to switch mirrors rather than retry the same one.
            Self::HeaderTimeout(detail) => Some(LocalModelDownloadError::new(
                LocalModelDownloadErrorCode::Timeout,
                detail.clone(),
            )),
            Self::Download(error) => Some(error.clone()),
            Self::Checksum { file, .. } => Some(LocalModelDownloadError::new(
                LocalModelDownloadErrorCode::Checksum,
                file.clone(),
            )),
            Self::Io(error) => Some(LocalModelDownloadError::new(
                LocalModelDownloadErrorCode::Disk,
                error.to_string(),
            )),
            Self::UnknownModel(_)
            | Self::NotInstalled(_)
            | Self::AlreadyRunning(_)
            | Self::NotRunning(_)
            | Self::Busy(_)
            | Self::Launch(_)
            | Self::Internal(_) => None,
        }
    }
}

impl From<LocalModelError> for AppError {
    fn from(error: LocalModelError) -> Self {
        match error {
            LocalModelError::UnknownModel(id) => {
                AppError::NotFound(format!("unknown local model: {id}"))
            }
            LocalModelError::NotInstalled(id) => {
                AppError::BadRequest(format!("model is not installed: {id}"))
            }
            LocalModelError::AlreadyRunning(id) => {
                AppError::Conflict(format!("model is already running: {id}"))
            }
            LocalModelError::NotRunning(id) => {
                AppError::BadRequest(format!("model is not running: {id}"))
            }
            LocalModelError::Busy(message) => AppError::Conflict(message),
            LocalModelError::Cancelled => AppError::BadRequest("download cancelled".to_owned()),
            LocalModelError::Download(error) => AppError::Internal(error.to_string()),
            LocalModelError::HeaderTimeout(message)
            | LocalModelError::Launch(message)
            | LocalModelError::Internal(message) => AppError::Internal(message),
            LocalModelError::Checksum {
                file,
                expected,
                actual,
            } => AppError::Internal(format!(
                "checksum mismatch for {file}: expected {expected}, got {actual}"
            )),
            LocalModelError::Io(error) => AppError::Internal(error.to_string()),
        }
    }
}

pub type LocalModelResult<T> = Result<T, LocalModelError>;
