use std::time::Duration;

use thiserror::Error;

pub type AuxiliaryResult<T> = std::result::Result<T, AuxiliaryError>;

#[derive(Debug, Error)]
pub enum AuxiliaryError {
    #[error("no auxiliary provider available (tried: {tried:?})")]
    NoProviderAvailable { tried: Vec<String> },

    #[error("all auxiliary providers failed: {summary}")]
    AllProvidersFailed {
        errors: Vec<(String, String)>,
        summary: String,
    },

    #[error("invalid auxiliary request: {0}")]
    InvalidRequest(String),

    #[error("auxiliary call exceeded the {0:?} wall-clock budget")]
    Timeout(Duration),

    #[error("LLM error on provider {provider}: {reason}")]
    Llm { provider: String, reason: String },
}
