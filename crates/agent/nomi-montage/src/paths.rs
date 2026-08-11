//! On-disk layout: `{data_dir}/montage/projects/<id>/...`
//!
//! Mirrors the OpenMontage `projects/<id>/` convention (artifacts / assets /
//! renders / checkpoint / history / events), adapted to allo's data directory.

use std::path::{Path, PathBuf};

/// Root of the whole Montage workspace under the agent data directory.
pub fn montage_root(data_dir: &Path) -> PathBuf {
    data_dir.join("montage")
}

/// Root of the per-project directory tree.
pub fn projects_dir(data_dir: &Path) -> PathBuf {
    montage_root(data_dir).join("projects")
}

/// Optional project index file (fast listing without scanning every `project.json`).
pub fn library_json(data_dir: &Path) -> PathBuf {
    montage_root(data_dir).join("library.json")
}

/// Absolute, pre-resolved paths for a single project.
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
}

impl ProjectPaths {
    pub fn new(data_dir: &Path, project_id: &str) -> Self {
        Self {
            root: projects_dir(data_dir).join(project_id),
        }
    }

    pub fn project_json(&self) -> PathBuf {
        self.root.join("project.json")
    }

    pub fn artifacts_dir(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    pub fn artifact_path(&self, name: &str) -> PathBuf {
        self.artifacts_dir().join(format!("{name}.json"))
    }

    pub fn decision_log_path(&self) -> PathBuf {
        self.artifact_path("decision_log")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn assets_images_dir(&self) -> PathBuf {
        self.assets_dir().join("images")
    }

    pub fn assets_video_dir(&self) -> PathBuf {
        self.assets_dir().join("video")
    }

    pub fn assets_audio_dir(&self) -> PathBuf {
        self.assets_dir().join("audio")
    }

    pub fn assets_music_dir(&self) -> PathBuf {
        self.assets_dir().join("music")
    }

    pub fn renders_dir(&self) -> PathBuf {
        self.root.join("renders")
    }

    pub fn final_video_path(&self) -> PathBuf {
        self.renders_dir().join("final.mp4")
    }

    pub fn pipeline_dir(&self) -> PathBuf {
        self.root.join("pipeline")
    }

    pub fn checkpoint_path(&self) -> PathBuf {
        self.pipeline_dir().join("checkpoint.json")
    }

    pub fn history_dir(&self) -> PathBuf {
        self.root.join("history")
    }

    pub fn events_jsonl(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }

    /// Relative path from the project root when the canonical final cut exists.
    pub fn final_video_relpath_if_present(&self) -> Option<String> {
        let path = self.final_video_path();
        if path.is_file() {
            Some("renders/final.mp4".into())
        } else {
            None
        }
    }

    /// Discover playable mp4 clips under `renders/` and `assets/video/` (relative paths).
    pub fn list_media_clips(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (dir, prefix) in [
            (self.renders_dir(), "renders"),
            (self.assets_video_dir(), "assets/video"),
        ] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok()?;
                    let lower = name.to_ascii_lowercase();
                    if lower.ends_with(".mp4") || lower.ends_with(".webm") || lower.ends_with(".mov")
                    {
                        Some(format!("{prefix}/{name}"))
                    } else {
                        None
                    }
                })
                .collect();
            names.sort();
            out.extend(names);
        }
        // Prefer canonical final cut first when present.
        if let Some(final_rel) = self.final_video_relpath_if_present() {
            out.retain(|p| p != &final_rel);
            out.insert(0, final_rel);
        }
        out
    }

    /// Create every directory in the project tree (idempotent).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            self.root.clone(),
            self.artifacts_dir(),
            self.assets_images_dir(),
            self.assets_video_dir(),
            self.assets_audio_dir(),
            self.assets_music_dir(),
            self.renders_dir(),
            self.pipeline_dir(),
            self.history_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_convention() {
        let data_dir = PathBuf::from("/data");
        let p = ProjectPaths::new(&data_dir, "abc123");
        assert_eq!(p.root, PathBuf::from("/data/montage/projects/abc123"));
        assert_eq!(
            p.artifact_path("script"),
            PathBuf::from("/data/montage/projects/abc123/artifacts/script.json")
        );
        assert_eq!(
            p.checkpoint_path(),
            PathBuf::from("/data/montage/projects/abc123/pipeline/checkpoint.json")
        );
    }
}
