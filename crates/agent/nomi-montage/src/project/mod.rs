//! Project CRUD: `project.json` — the user-facing record (topic, pipeline,
//! playbook, budget, model triplet) layered on top of the machine-facing
//! [`crate::checkpoint::Checkpoint`].

pub mod board_state;
pub mod export;

pub use board_state::{BoardState, BoardStage};
pub use export::{export_project_zip, import_project_zip};

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::CheckpointPolicy;
use crate::error::{MontageError, MontageResult};
use crate::modes::VideoGenMode;
use crate::paths::{self, ProjectPaths};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    #[serde(default = "default_aspect")]
    pub aspect: String,
    #[serde(default = "default_resolution")]
    pub resolution: String,
    #[serde(default = "default_fps")]
    pub fps: u32,
    /// Target length of the finished film (seconds). Per-clip duration is still
    /// chosen per shot (API clip limits apply); this is a planning budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration_secs: Option<u32>,
}

fn default_aspect() -> String {
    "16:9".into()
}
fn default_resolution() -> String {
    "1080p".into()
}
fn default_fps() -> u32 {
    24
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            aspect: default_aspect(),
            resolution: default_resolution(),
            fps: default_fps(),
            target_duration_secs: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    pub title: String,
    pub pipeline: String,
    pub prompt: String,
    #[serde(default)]
    pub style_playbook: Option<String>,
    #[serde(default)]
    pub checkpoint_policy: Option<CheckpointPolicy>,
    #[serde(default)]
    pub models: ModelSelection,
    #[serde(default)]
    pub output: Option<OutputSettings>,
    #[serde(default)]
    pub budget_credits: Option<u64>,
    #[serde(default)]
    pub reference_video_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub title: String,
    pub pipeline: String,
    pub mode: VideoGenMode,
    pub prompt: String,
    #[serde(default)]
    pub style_playbook: Option<String>,
    pub checkpoint_policy: CheckpointPolicy,
    #[serde(default)]
    pub models: ModelSelection,
    pub output: OutputSettings,
    pub budget_credits: u64,
    #[serde(default)]
    pub reference_video_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ProjectRecord {
    pub fn new(req: CreateProjectRequest, mode: VideoGenMode, default_budget: u64) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            title: req.title,
            pipeline: req.pipeline,
            mode,
            prompt: req.prompt,
            style_playbook: req.style_playbook,
            checkpoint_policy: req.checkpoint_policy.unwrap_or_default(),
            models: req.models,
            output: req.output.unwrap_or_default(),
            budget_credits: req.budget_credits.unwrap_or(default_budget),
            reference_video_path: req.reference_video_path,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// Reads/writes `project.json` and lists project ids under `{data_dir}/montage/projects/`.
pub struct ProjectStore {
    data_dir: std::path::PathBuf,
}

impl ProjectStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn paths(&self, id: &str) -> ProjectPaths {
        ProjectPaths::new(&self.data_dir, id)
    }

    pub fn save(&self, record: &ProjectRecord) -> MontageResult<()> {
        let paths = self.paths(&record.id);
        paths.ensure_dirs()?;
        let pretty = serde_json::to_string_pretty(record)?;
        std::fs::write(paths.project_json(), pretty)?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> MontageResult<ProjectRecord> {
        let path = self.paths(id).project_json();
        if !path.exists() {
            return Err(MontageError::ProjectNotFound(id.to_string()));
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn list(&self) -> MontageResult<Vec<ProjectRecord>> {
        let dir = paths::projects_dir(&self.data_dir);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(id) = entry.file_name().to_str() {
                if let Ok(record) = self.load(id) {
                    out.push(record);
                }
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> MontageResult<()> {
        let root = self.paths(id).root;
        if root.is_dir() {
            std::fs::remove_dir_all(root)?;
        }
        Ok(())
    }

    pub fn touch(&self, id: &str) -> MontageResult<ProjectRecord> {
        let mut record = self.load(id)?;
        record.updated_at = Utc::now().to_rfc3339();
        self.save(&record)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_load_list_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        let record = ProjectRecord::new(
            CreateProjectRequest {
                title: "Brand film".into(),
                pipeline: "cinematic".into(),
                prompt: "A coffee brand launch film".into(),
                style_playbook: None,
                checkpoint_policy: None,
                models: ModelSelection::default(),
                output: None,
                budget_credits: None,
                reference_video_path: None,
            },
            VideoGenMode::Agent,
            1500,
        );
        let id = record.id.clone();
        store.save(&record).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.title, "Brand film");
        assert_eq!(loaded.budget_credits, 1500);

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);

        store.delete(&id).unwrap();
        assert!(store.load(&id).is_err());
    }
}
