//! Skill maturity tracking for contribution eligibility.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths::skill_state_path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillStateFile {
    #[serde(default)]
    skills: HashMap<String, SkillMaturityEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillMaturityEntry {
    first_seen_unix: u64,
    #[serde(default)]
    review_patched: bool,
    #[serde(default)]
    last_content_hash: String,
}

pub struct SkillMaturityStore {
    path: PathBuf,
    state: SkillStateFile,
}

impl SkillMaturityStore {
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        let path = skill_state_path(data_dir);
        let state = if let Ok(raw) = std::fs::read_to_string(&path) {
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            SkillStateFile::default()
        };
        Ok(Self { path, state })
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(&self.state).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, raw).map_err(|e| e.to_string())
    }

    pub fn touch_skill(&mut self, skill_key: &str, content_hash: &str) {
        let now = unix_now();
        self.state
            .skills
            .entry(skill_key.to_string())
            .and_modify(|e| {
                e.last_content_hash = content_hash.to_string();
            })
            .or_insert(SkillMaturityEntry {
                first_seen_unix: now,
                review_patched: false,
                last_content_hash: content_hash.to_string(),
            });
    }

    pub fn mark_review_patched_all(&mut self) {
        for entry in self.state.skills.values_mut() {
            entry.review_patched = true;
        }
    }

    pub fn is_eligible(
        &self,
        skill_key: &str,
        min_age_hours: u32,
        content_hash: &str,
    ) -> bool {
        let Some(entry) = self.state.skills.get(skill_key) else {
            return false;
        };
        if entry.last_content_hash != content_hash {
            return false;
        }
        let age_secs = unix_now().saturating_sub(entry.first_seen_unix);
        let min_secs = u64::from(min_age_hours) * 3600;
        if age_secs >= min_secs {
            return true;
        }
        entry.review_patched
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn skill_key_from_dir(skill_dir: &Path) -> String {
    skill_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_ascii_lowercase()
}
