//! Host-managed presentation capability for the `ppt-master` Skill.
//!
//! The backend fetches one pinned ppt-master commit (AtomGit, then GitHub),
//! keeps only `skills/ppt-master`, downloads a pinned portable CPython,
//! installs hash-locked dependencies into a private venv, and self-tests it
//! before reporting `Ready`. Agents never choose what is downloaded or which
//! interpreter runs. Design: `docs/architecture/presentation-ppt-master.zh.md`.

pub mod manifest;
pub mod progress;
pub mod python_dist;
mod routes;
pub mod runtime;
pub mod scripts;
mod service;
pub mod source;

pub use progress::InstallProgress;
pub use python_dist::PythonSource;
pub use routes::{PresentationRouterState, presentation_routes};
pub use scripts::{ScriptError, ScriptRun, skill_host_note, usage_contract, with_host_note};
pub use runtime::{ManagedPythonRuntime, PrepareContext, PreparedRuntime, RuntimeLayout, RuntimePreparer};
pub use service::{PresentationCapabilityService, ReadySkill, UninstallError, ready_skill};
pub use source::{FetchedSkill, SourceSpec};
