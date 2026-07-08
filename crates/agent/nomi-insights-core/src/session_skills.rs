//! Track skill slugs touched during the active session (for work package binding).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::paths::state_dir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionSkillsFile {
    session_id: String,
    #[serde(default)]
    slugs: HashSet<String>,
    #[serde(default)]
    patch_count: u32,
    #[serde(default)]
    created: bool,
}

fn session_skills_path(data_dir: &Path) -> PathBuf {
    state_dir(data_dir).join("session_skills.json")
}

pub fn set_active_session(data_dir: &Path, session_id: &str) {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return;
    }
    let path = session_skills_path(data_dir);
    let mut file = SessionSkillsFile {
        session_id: session_id.to_string(),
        slugs: HashSet::new(),
        patch_count: 0,
        created: false,
    };
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(existing) = serde_json::from_str::<SessionSkillsFile>(&raw) {
            if existing.session_id == session_id {
                file = existing;
            } else if existing.session_id.is_empty() {
                // Skill touches may arrive before the session id is bound (gateway first turn).
                file = existing;
                file.session_id = session_id.to_string();
            }
            // Different non-empty session id → fresh session, keep empty slugs.
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, raw);
    }
}

pub fn record_skill_touch(data_dir: &Path, name_slug: &str, created: bool) {
    let slug = name_slug.trim();
    if slug.is_empty() {
        return;
    }
    let path = session_skills_path(data_dir);
    let mut file = read_file(&path);
    file.slugs.insert(slug.to_string());
    if created {
        file.created = true;
    } else {
        file.patch_count = file.patch_count.saturating_add(1);
    }
    write_file(&path, &file);
    debug!(
        session_id = %file.session_id,
        slug,
        created,
        patch_count = file.patch_count,
        "insights: recorded session skill touch"
    );
}

pub fn drain_session_skills(data_dir: &Path, session_id: &str) -> SessionSkillSummary {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return SessionSkillSummary::default();
    }
    let path = session_skills_path(data_dir);
    let file = read_file(&path);
    let session_matches =
        file.session_id == session_id || (file.session_id.is_empty() && !file.slugs.is_empty());
    if !session_matches {
        warn!(
            expected_session_id = session_id,
            file_session_id = %file.session_id,
            slug_count = file.slugs.len(),
            "insights: session_skills drain skipped — session_id mismatch"
        );
        return SessionSkillSummary::default();
    }
    let summary = SessionSkillSummary {
        slugs: file.slugs.into_iter().collect(),
        patch_count: file.patch_count,
        skill_created: file.created,
    };
    let _ = std::fs::remove_file(path);
    debug!(
        session_id,
        slug_count = summary.slugs.len(),
        patch_count = summary.patch_count,
        skill_created = summary.skill_created,
        "insights: drained session skill binding"
    );
    summary
}

#[derive(Debug, Clone, Default)]
pub struct SessionSkillSummary {
    pub slugs: Vec<String>,
    pub patch_count: u32,
    pub skill_created: bool,
}

fn read_file(path: &Path) -> SessionSkillsFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_file(path: &Path, file: &SessionSkillsFile) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(file) {
        let _ = std::fs::write(path, raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn set_active_session_adopts_orphaned_skill_touches() {
        let tmp = TempDir::new().unwrap();
        record_skill_touch(tmp.path(), "my-skill", true);
        set_active_session(tmp.path(), "session-uuid-a");
        let summary = drain_session_skills(tmp.path(), "session-uuid-a");
        assert_eq!(summary.slugs, vec!["my-skill".to_string()]);
        assert!(summary.skill_created);
    }

    #[test]
    fn session_end_does_not_wipe_when_rebinding_same_session() {
        let tmp = TempDir::new().unwrap();
        set_active_session(tmp.path(), "session-uuid-a");
        record_skill_touch(tmp.path(), "my-skill", false);
        set_active_session(tmp.path(), "session-uuid-a");
        let summary = drain_session_skills(tmp.path(), "session-uuid-a");
        assert_eq!(summary.slugs, vec!["my-skill".to_string()]);
    }

    #[test]
    fn new_session_clears_previous_session_slugs() {
        let tmp = TempDir::new().unwrap();
        set_active_session(tmp.path(), "session-a");
        record_skill_touch(tmp.path(), "old-skill", false);
        set_active_session(tmp.path(), "session-b");
        record_skill_touch(tmp.path(), "new-skill", true);
        let summary = drain_session_skills(tmp.path(), "session-b");
        assert_eq!(summary.slugs, vec!["new-skill".to_string()]);
        assert!(summary.skill_created);
    }

    #[test]
    fn drain_accepts_orphan_slugs_without_session_id() {
        let tmp = TempDir::new().unwrap();
        record_skill_touch(tmp.path(), "orphan-skill", true);
        let summary = drain_session_skills(tmp.path(), "session-uuid-a");
        assert_eq!(summary.slugs, vec!["orphan-skill".to_string()]);
    }
}
