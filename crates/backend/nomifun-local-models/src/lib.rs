//! `nomifun-local-models` — the local model runtime domain.
//!
//! Owns the three supported local GGUF models (`Qwen3.8-27B`,
//! `Qwen3.6-35B-A3B`, `Qwen3.6-35B-A3B-MTP`) and the single `llama.cpp`
//! `standard` runtime variant they launch with. The service downloads model
//! files with SHA-256 verification, supervises `llama-server.exe` through
//! `nomi-process-runtime`, and publishes each healthy model as an
//! OpenAI-compatible chat provider through a host-injected seam.
//!
//! Layout under the backend data dir:
//! - `{data_dir}/local-models/models/{id}/` — model files.
//! - `{data_dir}/local-models/runtimes/llama.cpp/standard/` — runtime + exe.
//! - `{data_dir}/local-models/runtimes/houmo/xh2/` — Houmo HLIElLama runtime.
//! - `{data_dir}/local-models/state.json` — install markers + launch options.

mod download;
mod error;
mod houmo;
mod launch;
mod manifest;
mod progress;
mod publisher;
pub mod routes;
mod service;
pub mod state;

pub use error::LocalModelError;
pub use publisher::{
    LocalModelProviderPublisher, LocalModelProviderSpec, ProviderRemoveOutcome,
};
pub use routes::local_models_routes;
pub use service::LocalModelsService;
pub use state::LocalModelsRouterState;

/// Domain root under the backend data dir.
pub const LOCAL_MODELS_REL_DIR: &str = "local-models";

/// The three supported catalog ids, in store order.
pub const SUPPORTED_MODEL_IDS: &[&str] = &[
    "Qwen3.8-27B",
    "Qwen3.6-35B-A3B",
    "Qwen3.6-35B-A3B-MTP",
];
