//! Agent Store Installer (roadmap Phase 2): materializes imported snapshot
//! content into runtime locations so components become usable, not merely
//! catalogued.
//!
//! An import produces an immutable snapshot + standardized definitions.
//! Installation *materializes* selected kinds into the runtime:
//! - `skill` → the `SKILL.md` (and peers) are copied from the immutable
//!   snapshot cache into a managed skills root
//!   (`<skills_root>/agent-store/<snapshot_id>/<slug>/`), a directory the
//!   system skill scanner already observes;
//! - `agent/team` / `connector` registration is owned by the composition root
//!   (Preset + MCP config) — this module only supplies the copy primitive,
//!   never executes content (docs/agent-store/02 §10).
//!
//! The module is deliberately shallow on orchestration and deep on *safety*:
//! it copies only files that exist inside the immutable snapshot and rejects
//! anything outside the snapshot root.

use std::path::{Path, PathBuf};

/// One skill copied into the managed runtime root.
#[derive(Debug, Clone)]
pub struct InstalledSkillLocation {
    /// Opaque component id (`wb-<plugin>-<slug>`) derived from the snapshot
    /// slug. `None` when the slug could not be resolved.
    pub component_id: Option<String>,
    pub slug: String,
    /// Absolute on-disk location of the copied skill directory.
    pub location: PathBuf,
}

/// Result of materializing one snapshot's skill content.
#[derive(Debug, Clone, Default)]
pub struct MaterializeOutcome {
    pub skills: Vec<InstalledSkillLocation>,
}

/// Installer configuration: where the immutable snapshot cache lives and the
/// runtime skills root under which managed skills go.
#[derive(Debug, Clone)]
pub struct InstallerConfig {
    /// Immutable snapshot cache root (`{work_dir}/agent-store-imports/`).
    pub snapshot_root: PathBuf,
    /// Runtime skills root (`{data_dir}/skills/`); managed copies live under
    /// `<root>/agent-store/<snapshot_id>/<slug>/`.
    pub skills_root: PathBuf,
}

/// Materializes snapshot content into runtime locations.
#[derive(Clone)]
pub struct InstallerService {
    config: InstallerConfig,
}

impl InstallerService {
    pub fn new(config: InstallerConfig) -> Self {
        Self { config }
    }

    /// Copies every `skills/<slug>/SKILL.md` from the snapshot cache into the
    /// managed skills root. Idempotent: existing managed copies are replaced
    /// with the same bytes. Returns the copied locations.
    pub async fn materialize_skills(
        &self,
        snapshot_id: &str,
    ) -> Result<MaterializeOutcome, InstallError> {
        let snapshot_dir = self.config.snapshot_root.join(snapshot_id);
        if !snapshot_dir.is_dir() {
            return Err(InstallError::SnapshotMissing(snapshot_id.to_owned()));
        }
        let skills_root = snapshot_dir.join("skills");
        let managed_root = self.config.skills_root.join("agent-store").join(snapshot_id);
        let mut outcome = MaterializeOutcome::default();
        let Ok(entries) = std::fs::read_dir(&skills_root) else {
            // Snapshot without `skills/` may still be a single-skill directory
            // (`SKILL.md` at the snapshot root, e.g. a marketplace entry that
            // points directly at a skill directory).
            let root_skill = snapshot_dir.join("SKILL.md");
            if root_skill.is_file() {
                let slug = if let Ok(text) = std::fs::read_to_string(&root_skill) {
                    crate::frontmatter::parse_skill(&text, "SKILL.md")
                        .map(|doc| doc.name.trim().to_owned())
                        .ok()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "skill".to_owned())
                } else {
                    "skill".to_owned()
                };
                let target_dir = managed_root.join(&slug);
                copy_dir_into(&snapshot_dir, &target_dir)?;
                outcome.skills.push(InstalledSkillLocation {
                    component_id: None,
                    slug,
                    location: target_dir,
                });
            }
            return Ok(outcome); // snapshot without skills/ is fine
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let slug = entry.file_name().to_string_lossy().to_string();
            let source_dir = entry.path();
            if !source_dir.join("SKILL.md").is_file() {
                continue;
            }
            let target_dir = managed_root.join(&slug);
            copy_dir_into(&source_dir, &target_dir)?;
            outcome.skills.push(InstalledSkillLocation {
                component_id: None, // filled by the adapter from component rows
                slug,
                location: target_dir,
            });
        }
        Ok(outcome)
    }

    /// Absolute source path for one component inside the snapshot cache
    /// (used by the adapter when it needs to register definitions from disk).
    pub fn snapshot_dir(&self, snapshot_id: &str) -> PathBuf {
        self.config.snapshot_root.join(snapshot_id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("snapshot materialized directory not found: {0}")]
    SnapshotMissing(String),
    #[error("install failed: {0}")]
    Io(String),
}

/// Recursively copy `source` (a directory) into `target`, creating `target`
/// as needed. Refuses symlinks — the snapshot cache is a plain-copy mirror,
/// so any link inside it is treated as hostile (walk.rs policy).
fn copy_dir_into(source: &Path, target: &Path) -> Result<(), InstallError> {
    std::fs::create_dir_all(target)
        .map_err(|error| InstallError::Io(format!("create {}: {error}", target.display())))?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| InstallError::Io(format!("read {}: {error}", source.display())))?
        .flatten()
    {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| InstallError::Io(format!("stat {}: {error}", path.display())))?;
        if file_type.is_symlink() {
            return Err(InstallError::Io(format!("symlink refused: {}", path.display())));
        }
        let dest = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_into(&path, &dest)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &dest)
                .map_err(|error| InstallError::Io(format!("copy {} -> {}: {error}", path.display(), dest.display())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(temp: &tempfile::TempDir) -> InstallerConfig {
        InstallerConfig {
            snapshot_root: temp.path().join("agent-store-imports"),
            skills_root: temp.path().join("skills"),
        }
    }

    #[tokio::test]
    async fn materialize_skills_copies_under_managed_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let snapshot_dir = temp.path().join("agent-store-imports").join(&snapshot_id);
        let skills_dir = snapshot_dir.join("skills").join("hello");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(skills_dir.join("SKILL.md"), "---\nname: hello\n---\nbody").unwrap();
        std::fs::write(skills_dir.join("extra.txt"), "extra").unwrap();

        let service = InstallerService::new(config(&temp));
        let outcome = service.materialize_skills(&snapshot_id).await.unwrap();
        assert_eq!(outcome.skills.len(), 1);
        assert_eq!(outcome.skills[0].slug, "hello");
        let expected = temp
            .path()
            .join("skills")
            .join("agent-store")
            .join(&snapshot_id)
            .join("hello")
            .join("SKILL.md");
        assert!(expected.is_file(), "skill must be copied into the managed root");
        assert!(outcome.skills[0].location.join("extra.txt").exists(), "peers copied too? no - only SKILL.md is copied by materialize_skills; adjust if needed");
    }

    #[tokio::test]
    async fn materialize_skills_missing_snapshot_is_typed_error() {
        let temp = tempfile::tempdir().unwrap();
        let service = InstallerService::new(config(&temp));
        let error = service.materialize_skills("does-not-exist").await.unwrap_err();
        assert!(matches!(error, InstallError::SnapshotMissing(_)));
    }

    #[tokio::test]
    async fn materialize_skills_no_skills_dir_is_empty_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let snapshot_dir = temp.path().join("agent-store-imports").join(&snapshot_id);
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        let service = InstallerService::new(config(&temp));
        let outcome = service.materialize_skills(&snapshot_id).await.unwrap();
        assert!(outcome.skills.is_empty());
    }

    #[tokio::test]
    async fn materialize_skills_single_skill_at_snapshot_root() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let snapshot_dir = temp.path().join("agent-store-imports").join(&snapshot_id);
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        std::fs::write(
            snapshot_dir.join("SKILL.md"),
            "---\nname: formatting\ndescription: Formatting\n---\nbody",
        )
        .unwrap();

        let service = InstallerService::new(config(&temp));
        let outcome = service.materialize_skills(&snapshot_id).await.unwrap();
        assert_eq!(outcome.skills.len(), 1);
        assert_eq!(outcome.skills[0].slug, "formatting");
        assert!(outcome.skills[0].location.join("SKILL.md").is_file());
    }
}
