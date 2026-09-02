//! Agent Store Importer (roadmap Phase 1): CodeBuddy / WorkBuddy local
//! source → immutable PluginSnapshot + standardized definitions +
//! CompatibilityReport, registered into the Catalog.
//!
//! Safety boundaries (docs/agent-store/02 §10): import only reads and copies,
//! never executes; credential values never enter snapshots, logs or public
//! responses; source paths are internal traceability only.

pub mod compat;
pub mod digest;
pub mod frontmatter;
pub mod import;
pub mod manifest;
pub mod models;
pub mod registry;
pub mod walk;

pub use import::{ImportError, ImporterService, ImportRequest};
pub use manifest::{
    ManifestError, MarketManifest, ParsedManifest, PluginManifest, TeamInfo,
    validate_relative_path,
};
pub use models::{
    CompatTriple, Component, SourceKind, component_id, sanitize_slug,
};