//! Auxiliary LLM client surface for side tasks (interest extraction, compression, …).

mod client;
mod error;
mod task;

pub use client::{AuxiliaryClient, AuxiliaryRequest, AuxiliaryResponse};
pub use error::{AuxiliaryError, AuxiliaryResult};
pub use task::AuxiliaryTask;
