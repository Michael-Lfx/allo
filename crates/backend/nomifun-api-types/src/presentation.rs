//! Presentation capability (host-managed `ppt-master` Skill) HTTP DTOs.
//!
//! The backend owns the whole install: it fetches one pinned ppt-master commit,
//! prepares a private Python runtime and self-tests it. These types are the
//! wire contract for `/api/presentation/capability/*`.

use serde::{Deserialize, Serialize};

/// Coarse lifecycle of the capability. Fine-grained progress lives in
/// [`PresentationInstallStage`] while `status == Installing`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresentationCapabilityStatus {
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

/// Step of a running install, in execution order.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresentationInstallStage {
    FetchingSkill,
    PreparingPython,
    InstallingDependencies,
    SelfTest,
    Committing,
}

/// Counters of the current stage; reset when the stage changes. The git fetch
/// fills the object counters, the Python download fills `total_bytes`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationInstallProgress {
    pub received_bytes: u64,
    /// Known size of the current download, `0` when unknown (git fetch).
    #[serde(default)]
    pub total_bytes: u64,
    pub received_objects: u64,
    pub total_objects: u64,
}

/// Why the last install failed, or why the installed snapshot is unusable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationInstallFailure {
    /// `None` when the failure was detected on an already-installed snapshot
    /// (for example a deleted interpreter) rather than during an install.
    pub stage: Option<PresentationInstallStage>,
    pub message: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationCapabilityView {
    pub capability_id: String,
    pub version: String,
    pub source_commit: String,
    pub license: String,
    /// Git remotes tried in order.
    pub sources: Vec<String>,
    /// Rough first-install download size, for the UI and the agent prompt.
    pub estimated_download_bytes: u64,
    pub status: PresentationCapabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<PresentationInstallStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<PresentationInstallProgress>,
    /// Absolute `SKILL_DIR` of the active snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_dir: Option<String>,
    /// Private interpreter of the active snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    /// Remote the active snapshot was fetched from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at_ms: Option<i64>,
    /// Present when `status == Failed`, and also when a re-install failed
    /// while an older snapshot stayed `Ready`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<PresentationInstallFailure>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationInstallRequest {
    /// Re-install even when the capability is already `Ready` (repair).
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationInstallResponse {
    /// `false` when an install was already running, or nothing needed doing.
    pub accepted: bool,
    pub capability: PresentationCapabilityView,
}

/// `POST /api/presentation/capability/cancel`. Cancelling is asynchronous: the
/// running step stops at its next check and the half-built snapshot is
/// removed; the final state arrives on `status` and the realtime event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationCancelResponse {
    /// `false` when no install was running.
    pub cancelled: bool,
    pub capability: PresentationCapabilityView,
}

/// `DELETE /api/presentation/capability`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationUninstallResponse {
    pub capability: PresentationCapabilityView,
}

/// Realtime event name for [`PresentationCapabilityView`] changes. Instance
/// scoped: the capability lives in the data dir, not in a user row.
pub const PRESENTATION_CAPABILITY_STATUS_EVENT: &str = "presentationCapability.status";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_stage_use_snake_case_on_the_wire() {
        assert_eq!(
            serde_json::to_value(PresentationCapabilityStatus::NotInstalled).unwrap(),
            "not_installed"
        );
        assert_eq!(
            serde_json::to_value(PresentationInstallStage::InstallingDependencies).unwrap(),
            "installing_dependencies"
        );
    }

    #[test]
    fn install_request_defaults_and_rejects_unknown_fields() {
        let empty: PresentationInstallRequest = serde_json::from_str("{}").unwrap();
        assert!(!empty.force);
        assert!(serde_json::from_str::<PresentationInstallRequest>(r#"{"url":"x"}"#).is_err());
    }
}
