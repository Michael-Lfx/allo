//! Project checkpoint — durable "where are we" state, resumable across restarts.

pub mod canonical;
pub mod schema;

pub use canonical::{CANONICAL_STAGE_ARTIFACTS, canonical_artifact_for_stage};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::MontageResult;
use crate::paths::ProjectPaths;
use crate::pipeline::StageStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRunStatus {
    #[default]
    Idle,
    Running,
    AwaitingHuman,
    Succeeded,
    Failed,
    Cancelled,
}

impl ProjectRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::AwaitingHuman => "awaiting_human",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Per-stage counters used to enforce `orchestration.max_revisions_per_stage`
/// and `max_send_backs` (see `assets/pipeline_defs/*.yaml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageCounters {
    #[serde(default)]
    pub revisions: u32,
    #[serde(default)]
    pub send_backs: u32,
    #[serde(default)]
    pub tool_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub project_id: String,
    pub pipeline: String,
    pub current_stage: String,
    #[serde(default)]
    pub stage_status: BTreeMap<String, StageStatus>,
    #[serde(default)]
    pub stage_counters: BTreeMap<String, StageCounters>,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub status: ProjectRunStatus,
    #[serde(default)]
    pub awaiting_human_stage: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Checkpoint {
    pub fn new(project_id: impl Into<String>, pipeline: impl Into<String>, first_stage: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            project_id: project_id.into(),
            pipeline: pipeline.into(),
            current_stage: first_stage.to_string(),
            stage_status: BTreeMap::new(),
            stage_counters: BTreeMap::new(),
            started_at: now.clone(),
            updated_at: now,
            status: ProjectRunStatus::Idle,
            awaiting_human_stage: None,
            last_error: None,
            notes: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now().to_rfc3339();
    }

    pub fn stage_status_of(&self, stage: &str) -> StageStatus {
        self.stage_status.get(stage).copied().unwrap_or_default()
    }

    pub fn set_stage_status(&mut self, stage: &str, status: StageStatus) {
        self.stage_status.insert(stage.to_string(), status);
        self.touch();
    }

    pub fn counters_mut(&mut self, stage: &str) -> &mut StageCounters {
        self.stage_counters.entry(stage.to_string()).or_default()
    }

    pub fn counters_of(&self, stage: &str) -> StageCounters {
        self.stage_counters.get(stage).cloned().unwrap_or_default()
    }
}

/// Reads/writes the project checkpoint file, validating against the JSON schema
/// and keeping a timestamped audit trail under `history/`.
pub struct CheckpointStore {
    path: PathBuf,
    history_dir: PathBuf,
}

impl CheckpointStore {
    pub fn new(paths: &ProjectPaths) -> Self {
        Self {
            path: paths.checkpoint_path(),
            history_dir: paths.history_dir(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> MontageResult<Option<Checkpoint>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&self.path)?;
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        schema::validate_checkpoint_value(&value)?;
        let checkpoint: Checkpoint = serde_json::from_value(value)?;
        Ok(Some(checkpoint))
    }

    pub fn write(&self, checkpoint: &Checkpoint) -> MontageResult<()> {
        let value = serde_json::to_value(checkpoint)?;
        schema::validate_checkpoint_value(&value)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pretty = serde_json::to_string_pretty(&value)?;
        atomic_write(&self.path, &pretty)?;

        // Best-effort audit snapshot; failures here must never break the run.
        if std::fs::create_dir_all(&self.history_dir).is_ok() {
            let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3f");
            let snapshot = self
                .history_dir
                .join(format!("checkpoint_{stamp}.json"));
            let _ = std::fs::write(&snapshot, &pretty);
        }
        Ok(())
    }
}

fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    let part = path.with_extension("json.part");
    std::fs::write(&part, contents)?;
    std::fs::rename(&part, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_roundtrip_through_store() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), "proj-1");
        paths.ensure_dirs().unwrap();
        let store = CheckpointStore::new(&paths);

        assert!(store.read().unwrap().is_none());

        let mut cp = Checkpoint::new("proj-1", "cinematic", "research");
        cp.status = ProjectRunStatus::Running;
        cp.set_stage_status("research", StageStatus::InProgress);
        store.write(&cp).unwrap();

        let loaded = store.read().unwrap().expect("checkpoint present");
        assert_eq!(loaded.project_id, "proj-1");
        assert_eq!(loaded.pipeline, "cinematic");
        assert_eq!(loaded.stage_status_of("research"), StageStatus::InProgress);
        assert_eq!(loaded.status, ProjectRunStatus::Running);
    }

    #[test]
    fn history_snapshots_accumulate() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), "proj-2");
        paths.ensure_dirs().unwrap();
        let store = CheckpointStore::new(&paths);
        let cp = Checkpoint::new("proj-2", "framework-smoke", "script");
        store.write(&cp).unwrap();
        let entries: Vec<_> = std::fs::read_dir(paths.history_dir()).unwrap().collect();
        assert!(!entries.is_empty());
    }
}
