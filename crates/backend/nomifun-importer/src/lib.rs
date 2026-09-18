//! Agent Store Importer (roadmap Phase 1): CodeBuddy / WorkBuddy local
//! source → immutable PluginSnapshot + standardized definitions +
//! CompatibilityReport, registered into the Catalog.
//!
//! Safety boundaries (docs/agent-store/02 §10): import only reads and copies,
//! never executes; credential values never enter snapshots, logs or public
//! responses; source paths are internal traceability only.

pub mod compat;
pub mod dependency;
pub mod digest;
pub mod frontmatter;
pub mod import;
pub mod install;
pub mod manifest;
pub mod models;
pub mod registry;
pub mod walk;

pub use dependency::{
    CatalogIndex, DeclaredDependency, DependencyProblem, check_dependencies,
};
pub use import::{ImportError, ImportRequest, ImporterService, blocked_import_result};
pub use install::{
    InstallError, InstallerConfig, InstallerService, InstalledSkillLocation, MaterializeOutcome,
};
pub use manifest::{
    CliManifest, LocalizedText, ManifestError, MarketManifest, ParsedManifest, PluginManifest,
    TeamInfo, platform_summary, read_plugin_display, validate_relative_path,
};
pub use models::{
    CompatTriple, Component, SourceKind, component_id, sanitize_slug,
};