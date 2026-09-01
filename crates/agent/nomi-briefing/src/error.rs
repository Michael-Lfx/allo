use thiserror::Error;

pub type BriefingResult<T> = Result<T, BriefingError>;

#[derive(Debug, Error)]
pub enum BriefingError {
    #[error("briefing session not found: {0}")]
    SessionNotFound(String),
    #[error("{0}")]
    InvalidParams(String),
    #[error("{0}")]
    Hold(String),
    #[error("{0}")]
    Io(String),
    #[error("briefing artifact not found: {0}")]
    ArtifactNotFound(String),
    #[error("{0}")]
    Internal(String),
    /// Speech synthesis failed before compose. `code` is a closed telemetry
    /// token (`tts_unavailable` / `tts_failed`).
    #[error("{message}")]
    Voice { code: String, message: String },
}

impl BriefingError {
    pub fn error_code(&self) -> Option<&str> {
        match self {
            Self::Hold(_) => Some("no_sources"),
            Self::Voice { code, .. } => Some(code.as_str()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BriefingError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::NotFound {
            Self::ArtifactNotFound(err.to_string())
        } else {
            Self::Io(err.to_string())
        }
    }
}

impl From<serde_json::Error> for BriefingError {
    fn from(err: serde_json::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_artifact_not_found() {
        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(matches!(
            BriefingError::from(err),
            BriefingError::ArtifactNotFound(_)
        ));
    }
}
