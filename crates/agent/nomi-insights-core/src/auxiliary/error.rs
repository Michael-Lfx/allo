#[derive(Debug, thiserror::Error)]
pub enum AuxiliaryError {
    #[error("no auxiliary providers configured")]
    NoProviders,
    #[error("{0}")]
    CallFailed(String),
}

pub type AuxiliaryResult<T> = Result<T, AuxiliaryError>;
