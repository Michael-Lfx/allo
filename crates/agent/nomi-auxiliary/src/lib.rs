//! Lightweight auxiliary LLM routing types for side tasks.
//!
//! Full multi-provider routing can be wired later; this crate exposes the
//! API surface used by POI extraction and session resolution labeling.

mod client;
mod error;
mod task;

pub use client::{
    AuxiliaryClient, AuxiliaryClientBuilder, AuxiliaryRequest, AuxiliaryResponse, ChatLlmProvider,
    text_message,
};
pub use error::{AuxiliaryError, AuxiliaryResult};
pub use task::AuxiliaryTask;
