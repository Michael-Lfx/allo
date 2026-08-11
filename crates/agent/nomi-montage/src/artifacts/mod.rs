//! Artifact schema names and JSON Schema validation.

pub mod names;
pub mod validate;

pub use names::{ARTIFACT_NAMES, ArtifactRef, ArtifactRefKind};
pub use validate::ArtifactRegistry;
