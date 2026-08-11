//! `nomi-montage` — an OpenMontage-mechanism-faithful, Flowy-only agent film
//! production runtime.
//!
//! This crate is an **original** Rust implementation inspired by the
//! OpenMontage project's architecture (multi-stage pipelines driven by an
//! Executive Producer, schema-validated artifacts, human-in-the-loop
//! checkpoints). No text, YAML, or code from OpenMontage is copied; every
//! asset under `assets/` and every module under `src/` is written from
//! scratch against that architectural shape. See `assets/CONTRACT.md` for the
//! product contract this runtime enforces (Rule Zero, HITL, no silent
//! downgrade).
//!
//! Entry point: [`service::MontageService`].

pub mod artifacts;
pub mod checkpoint;
pub mod config;
pub mod creative;
pub mod error;
pub mod events;
pub mod governance;
pub mod modes;
pub mod orchestrator;
pub mod paths;
pub mod pipeline;
pub mod project;
pub mod service;
pub mod styles;
pub mod tools;

pub use error::{MontageError, MontageResult};
pub use modes::VideoGenMode;
pub use service::MontageService;

/// Root of the embedded asset tree (`assets/`), resolved relative to the
/// crate manifest at compile time. Dev-friendly: no build step, no embedded
/// binary blob — assets are read straight off disk, which is exactly where
/// they ship in a normal source/installed layout.
pub fn assets_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

#[cfg(test)]
mod tests {
    #[test]
    fn assets_root_exists() {
        assert!(super::assets_root().is_dir(), "assets/ directory must exist");
    }
}
