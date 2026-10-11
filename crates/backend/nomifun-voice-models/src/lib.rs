//! Flowy Voice Models Management Crate
//!
//! Provides model lifecycle management, dual-source downloading, SHA256 integrity verification,
//! and REST/SSE progress reporting.
//!
//! Authoritative references:
//! - `docs/architecture/voice-agent/interaction.zh.md` §3.4.4
//! - `docs/architecture/voice-agent/model-management.zh.md` §4

pub mod layout;
pub mod manager;
pub mod routes;

pub use layout::{check_model_layout, inspect_model_files, EXPECTED_FILES};
pub use manager::{ModelManager, ModelManagerError};
pub use nomifun_api_types::voice::*;
pub use routes::voice_model_routes;
