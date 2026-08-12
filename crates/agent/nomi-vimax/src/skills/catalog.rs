//! Vertical skill catalog: builtin + user + local hub.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};

use super::builtin::load_builtin_skills;
use super::model::{
    sanitize_skill_name, SkillId, SkillOverlay, SkillSource, SkillVisibility, VerticalSkill,
    VerticalSkillDraft, VerticalSkillSummary,
};
use super::overlay::compose_overlays;
use super::parse::{build_skill_md, load_skill_dir, SKILL_MANIFEST};

/// On-disk layout under `{data_dir}/vimax/skills/`.
pub struct SkillCatalog {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
    builtins: Vec<VerticalSkill>,
}

impl SkillCatalog {
    pub fn open(data_dir: &Path) -> VimaxResult<Self> {
        let root = data_dir.join("vimax").join("skills");
        std::fs::create_dir_all(root.join("user"))?;
        std::fs::create_dir_all(root.join("hub"))?;
        let builtins = load_builtin_skills()?;
        Ok(Self {
            root,
            lock: Arc::new(Mutex::new(())),
            builtins,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn user_dir(&self) -> PathBuf {
        self.root.join("user")
    }

    fn hub_dir(&self) -> PathBuf {
        self.root.join("hub")
    }

    fn scan_source(&self, source: SkillSource) -> VimaxResult<Vec<VerticalSkill>> {
        let dir = match source {
            SkillSource::Builtin => return Ok(self.builtins.clone()),
            SkillSource::User => self.user_dir(),
            SkillSource::Hub => self.hub_dir(),
        };
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if !path.join(SKILL_MANIFEST).exists() {
                continue;
            }
            match load_skill_dir(&path, source) {
                Ok(skill) => out.push(skill),
                Err(err) => {
                    tracing::warn!(
                        dir = %path.display(),
                        error = %err,
                        "skipping invalid vertical skill"
                    );
                }
            }
        }
        out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok(out)
    }

    /// List skills, optionally filtered by mode compatibility and source.
    pub fn list(
        &self,
        mode: Option<WorkflowKind>,
        source: Option<SkillSource>,
    ) -> VimaxResult<Vec<VerticalSkillSummary>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut skills = Vec::new();
        let sources: &[SkillSource] = match source {
            Some(s) => match s {
                SkillSource::Builtin => &[SkillSource::Builtin],
                SkillSource::User => &[SkillSource::User],
                SkillSource::Hub => &[SkillSource::Hub],
            },
            None => &[SkillSource::Builtin, SkillSource::Hub, SkillSource::User],
        };
        for src in sources {
            skills.extend(self.scan_source(*src)?);
        }
        // De-dupe by qualified id (hub may mirror a user publish).
        let mut seen = BTreeMap::new();
        for skill in skills {
            seen.insert(skill.id.qualified(), skill);
        }
        let mut out: Vec<_> = seen.into_values().collect();
        if let Some(mode) = mode {
            out.retain(|s| s.compatible_with(mode));
        }
        out.sort_by(|a, b| {
            (
                a.id.source.as_str(),
                a.category.as_str(),
                a.display_name.as_str(),
            )
                .cmp(&(
                    b.id.source.as_str(),
                    b.category.as_str(),
                    b.display_name.as_str(),
                ))
        });
        Ok(out.iter().map(VerticalSkill::to_summary).collect())
    }

    pub fn get(&self, id: &SkillId) -> VimaxResult<VerticalSkill> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.get_unlocked(id)
    }

    fn get_unlocked(&self, id: &SkillId) -> VimaxResult<VerticalSkill> {
        match id.source {
            SkillSource::Builtin => self
                .builtins
                .iter()
                .find(|s| s.name == id.name)
                .cloned()
                .ok_or_else(|| VimaxError::InvalidParams(format!("skill not found: {id}"))),
            SkillSource::User => {
                let dir = self.user_dir().join(&id.name);
                if !dir.join(SKILL_MANIFEST).exists() {
                    return Err(VimaxError::InvalidParams(format!("skill not found: {id}")));
                }
                load_skill_dir(&dir, SkillSource::User)
            }
            SkillSource::Hub => {
                let dir = self.hub_dir().join(&id.name);
                if !dir.join(SKILL_MANIFEST).exists() {
                    return Err(VimaxError::InvalidParams(format!("skill not found: {id}")));
                }
                load_skill_dir(&dir, SkillSource::Hub)
            }
        }
    }

    /// Resolve many ids; unknown ids error. Incompatible with mode are skipped with warn.
    pub fn resolve_for_mode(
        &self,
        ids: &[String],
        mode: WorkflowKind,
    ) -> VimaxResult<Vec<VerticalSkill>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for raw in ids {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let id = SkillId::parse(raw).ok_or_else(|| {
                VimaxError::InvalidParams(format!("invalid skill id: {raw}"))
            })?;
            let skill = self.get_unlocked(&id)?;
            if skill.compatible_with(mode) {
                out.push(skill);
            } else {
                tracing::warn!(
                    skill = %id.qualified(),
                    mode = mode.as_str(),
                    "skill not compatible with mode; skipped"
                );
            }
        }
        Ok(out)
    }

    pub fn compose_for_plan(
        &self,
        mode: WorkflowKind,
        skill_ids: &[String],
        base_requirement: &str,
        base_style: &str,
    ) -> VimaxResult<SkillOverlay> {
        let skills = self.resolve_for_mode(skill_ids, mode)?;
        Ok(compose_overlays(
            mode,
            &skills,
            base_requirement,
            base_style,
        ))
    }

    pub fn create_user_skill(&self, draft: &VerticalSkillDraft) -> VimaxResult<VerticalSkill> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let name = sanitize_skill_name(&draft.name).ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill name: {}", draft.name))
        })?;
        let dir = self.user_dir().join(&name);
        if dir.exists() {
            return Err(VimaxError::InvalidParams(format!(
                "skill already exists: user:{name}"
            )));
        }
        std::fs::create_dir_all(&dir)?;
        let md = build_skill_md(draft)?;
        std::fs::write(dir.join(SKILL_MANIFEST), md)?;
        for sub in ["references", "templates"] {
            let _ = std::fs::create_dir_all(dir.join(sub));
        }
        load_skill_dir(&dir, SkillSource::User)
    }

    pub fn update_user_skill(
        &self,
        name: &str,
        draft: &VerticalSkillDraft,
    ) -> VimaxResult<VerticalSkill> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let name = sanitize_skill_name(name).ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill name: {name}"))
        })?;
        let dir = self.user_dir().join(&name);
        if !dir.join(SKILL_MANIFEST).exists() {
            return Err(VimaxError::InvalidParams(format!(
                "skill not found: user:{name}"
            )));
        }
        // Keep directory name stable; force draft.name to match.
        let mut draft = draft.clone();
        draft.name = name.clone();
        let md = build_skill_md(&draft)?;
        std::fs::write(dir.join(SKILL_MANIFEST), md)?;
        load_skill_dir(&dir, SkillSource::User)
    }

    pub fn delete_user_skill(&self, name: &str) -> VimaxResult<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let name = sanitize_skill_name(name).ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill name: {name}"))
        })?;
        let dir = self.user_dir().join(&name);
        if !dir.exists() {
            return Err(VimaxError::InvalidParams(format!(
                "skill not found: user:{name}"
            )));
        }
        std::fs::remove_dir_all(&dir)?;
        // Also remove hub copy if published under same name.
        let hub = self.hub_dir().join(&name);
        if hub.exists() {
            let _ = std::fs::remove_dir_all(hub);
        }
        Ok(())
    }

    /// Copy a user skill into the local hub (publish).
    pub fn publish_user_skill(&self, name: &str) -> VimaxResult<VerticalSkill> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let name = sanitize_skill_name(name).ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill name: {name}"))
        })?;
        let src = self.user_dir().join(&name);
        if !src.join(SKILL_MANIFEST).exists() {
            return Err(VimaxError::InvalidParams(format!(
                "skill not found: user:{name}"
            )));
        }
        let dest = self.hub_dir().join(&name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        copy_dir_recursive(&src, &dest)?;
        // Flip visibility in the hub copy.
        let mut skill = load_skill_dir(&dest, SkillSource::Hub)?;
        skill.visibility = SkillVisibility::Hub;
        let md = std::fs::read_to_string(dest.join(SKILL_MANIFEST))?;
        let patched = patch_visibility(&md, "hub");
        std::fs::write(dest.join(SKILL_MANIFEST), patched)?;
        load_skill_dir(&dest, SkillSource::Hub)
    }

    pub fn unpublish_skill(&self, name: &str) -> VimaxResult<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let name = sanitize_skill_name(name).ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill name: {name}"))
        })?;
        let hub = self.hub_dir().join(&name);
        if !hub.exists() {
            return Err(VimaxError::InvalidParams(format!(
                "hub skill not found: hub:{name}"
            )));
        }
        // Refuse to delete builtins (they are not on disk).
        std::fs::remove_dir_all(hub)?;
        Ok(())
    }

    /// Import a skill directory (containing SKILL.md) into the user catalog.
    pub fn import_skill_dir(&self, source_path: &Path) -> VimaxResult<VerticalSkill> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let dir = if source_path.is_file()
            && source_path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case(SKILL_MANIFEST))
                .unwrap_or(false)
        {
            source_path.parent().unwrap_or(source_path).to_path_buf()
        } else {
            source_path.to_path_buf()
        };
        if !dir.join(SKILL_MANIFEST).exists() {
            return Err(VimaxError::InvalidParams(format!(
                "no SKILL.md in {}",
                dir.display()
            )));
        }
        let loaded = load_skill_dir(&dir, SkillSource::User)?;
        let dest = self.user_dir().join(&loaded.name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        copy_dir_recursive(&dir, &dest)?;
        load_skill_dir(&dest, SkillSource::User)
    }

    /// Export skill directory path for packaging / sharing.
    pub fn skill_dir(&self, id: &SkillId) -> VimaxResult<PathBuf> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match id.source {
            SkillSource::Builtin => Err(VimaxError::InvalidParams(
                "builtin skills are embedded; export via get detail instead".into(),
            )),
            SkillSource::User => {
                let dir = self.user_dir().join(&id.name);
                if dir.join(SKILL_MANIFEST).exists() {
                    Ok(dir)
                } else {
                    Err(VimaxError::InvalidParams(format!("skill not found: {id}")))
                }
            }
            SkillSource::Hub => {
                let dir = self.hub_dir().join(&id.name);
                if dir.join(SKILL_MANIFEST).exists() {
                    Ok(dir)
                } else {
                    Err(VimaxError::InvalidParams(format!("skill not found: {id}")))
                }
            }
        }
    }

    /// Raw SKILL.md text for detail drawer.
    pub fn read_manifest(&self, id: &SkillId) -> VimaxResult<String> {
        let skill = self.get(id)?;
        if !skill.dir.is_empty() {
            let path = PathBuf::from(&skill.dir).join(SKILL_MANIFEST);
            if path.exists() {
                return Ok(std::fs::read_to_string(path)?);
            }
        }
        // Rebuild from structured fields for builtins.
        let draft = VerticalSkillDraft {
            name: skill.name.clone(),
            display_name: Some(skill.display_name.clone()),
            description: skill.description.clone(),
            category: Some(skill.category.clone()),
            version: Some(skill.version.clone()),
            tags: skill.tags.clone(),
            compatible_modes: skill
                .compatible_modes
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
            requirement_overlay: Some(skill.requirement_overlay.clone()),
            style_overlay: Some(skill.style_overlay.clone()),
            playbook: Some(skill.playbook.clone()),
        };
        build_skill_md(&draft)
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> VimaxResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn patch_visibility(md: &str, visibility: &str) -> String {
    let mut lines: Vec<String> = md.lines().map(|l| l.to_string()).collect();
    let mut patched = false;
    for line in &mut lines {
        if line.to_ascii_lowercase().starts_with("visibility:") {
            *line = format!("visibility: {visibility}");
            patched = true;
            break;
        }
    }
    if !patched {
        // Insert after opening ---
        if lines.first().map(|l| l.trim() == "---").unwrap_or(false) {
            lines.insert(1, format!("visibility: {visibility}"));
        }
    }
    let mut out = lines.join("\n");
    if md.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::VerticalSkillDraft;
    use tempfile::tempdir;

    #[test]
    fn catalog_create_publish_and_compose() {
        let dir = tempdir().unwrap();
        let catalog = SkillCatalog::open(dir.path()).unwrap();
        let list = catalog.list(None, None).unwrap();
        assert!(list.iter().any(|s| s.id == "builtin:luxury-tvc"));

        let created = catalog
            .create_user_skill(&VerticalSkillDraft {
                name: "my-tvc".into(),
                display_name: Some("我的TVC".into()),
                description: "custom tvc".into(),
                category: Some("advertising".into()),
                version: Some("1.0.0".into()),
                tags: vec!["test".into()],
                compatible_modes: vec!["idea2video".into()],
                requirement_overlay: Some("Keep it premium.".into()),
                style_overlay: Some("luxury light".into()),
                playbook: Some("# Custom\nDo premium.".into()),
            })
            .unwrap();
        assert_eq!(created.id.qualified(), "user:my-tvc");

        let published = catalog.publish_user_skill("my-tvc").unwrap();
        assert_eq!(published.id.qualified(), "hub:my-tvc");

        let overlay = catalog
            .compose_for_plan(
                WorkflowKind::Idea2Video,
                &["builtin:luxury-tvc".into(), "user:my-tvc".into()],
                "brand soft",
                "cinematic",
            )
            .unwrap();
        assert!(overlay.user_requirement.contains("VERTICAL_SKILLS"));
        assert!(overlay.style.contains("cinematic"));
        assert_eq!(overlay.applied_skill_ids.len(), 2);
    }
}
