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
//! The module is deliberately shallow on coordination and deep on *safety*:
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

    /// Removes one materialized skill directory
    /// (`<skills_root>/agent-store/<snapshot_id>/<slug>/`), pruning the snapshot
    /// root when that was its last skill.
    ///
    /// Returns `Ok(false)` when the directory is already gone: uninstall must be
    /// re-entrant, and "the artifact I was asked to remove is not there" is the
    /// state the caller asked for, not a failure.
    ///
    /// Both path segments are validated rather than trusted. They reach this
    /// function from a database column (`runtime_ref.location`), so a corrupted
    /// or hand-edited row must not be able to steer a recursive delete: only a
    /// single normal segment each, and a symlink anywhere along the managed path
    /// is refused outright (the same hostile-link policy `copy_dir_into`
    /// applies on the way in).
    pub fn remove_materialized(&self, snapshot_id: &str, slug: &str) -> Result<bool, InstallError> {
        normal_segment(snapshot_id, "snapshot id")?;
        normal_segment(slug, "skill slug")?;
        let managed_root = self.config.skills_root.join("agent-store");
        let snapshot_dir = managed_root.join(snapshot_id);
        let target = snapshot_dir.join(slug);
        refuse_symlink(&managed_root)?;
        refuse_symlink(&snapshot_dir)?;
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(InstallError::UnsafePath(format!(
                "refusing to remove a symlinked skill directory: {}",
                target.display()
            ))),
            Ok(metadata) if metadata.is_dir() => {
                std::fs::remove_dir_all(&target).map_err(|error| {
                    InstallError::Io(format!("remove {}: {error}", target.display()))
                })?;
                // Prune the snapshot root once its last skill is gone, so an
                // emptied install does not leave a shell directory that the
                // skill scanner still has to walk. `remove_dir` refuses a
                // non-empty directory, which is exactly the guard needed here.
                let _ = std::fs::remove_dir(&snapshot_dir);
                Ok(true)
            }
            Ok(_) => Err(InstallError::UnsafePath(format!(
                "managed skill path is not a directory: {}",
                target.display()
            ))),
            Err(_) => Ok(false),
        }
    }

    /// Removes every materialized skill of one snapshot, leaving no empty
    /// `<skills_root>/agent-store/<snapshot_id>/` shell behind.
    pub fn remove_snapshot(&self, snapshot_id: &str) -> Result<bool, InstallError> {
        normal_segment(snapshot_id, "snapshot id")?;
        let managed_root = self.config.skills_root.join("agent-store");
        let snapshot_dir = managed_root.join(snapshot_id);
        refuse_symlink(&managed_root)?;
        match std::fs::symlink_metadata(&snapshot_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(InstallError::UnsafePath(format!(
                "refusing to remove a symlinked snapshot directory: {}",
                snapshot_dir.display()
            ))),
            Ok(metadata) if metadata.is_dir() => {
                std::fs::remove_dir_all(&snapshot_dir).map_err(|error| {
                    InstallError::Io(format!("remove {}: {error}", snapshot_dir.display()))
                })?;
                Ok(true)
            }
            Ok(_) => Err(InstallError::UnsafePath(format!(
                "managed snapshot path is not a directory: {}",
                snapshot_dir.display()
            ))),
            Err(_) => Ok(false),
        }
    }
}

/// Rejects anything that is not a single, ordinary path segment.
fn normal_segment(value: &str, what: &str) -> Result<(), InstallError> {
    let rejected = value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0');
    if rejected {
        return Err(InstallError::UnsafePath(format!(
            "{what} is not a single path segment: {value:?}"
        )));
    }
    Ok(())
}

/// Refuses to walk into a directory reachable through a symlink. An absent path
/// is fine — nothing is reachable through it.
fn refuse_symlink(path: &Path) -> Result<(), InstallError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(InstallError::UnsafePath(format!(
            "refusing to walk a symlinked managed path: {}",
            path.display()
        ))),
        _ => Ok(()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("snapshot materialized directory not found: {0}")]
    SnapshotMissing(String),
    #[error("install failed: {0}")]
    Io(String),
    /// A path derived from a database row was not shaped like a managed path.
    #[error("refusing an unsafe managed path: {0}")]
    UnsafePath(String),
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

    /// Two skills installed, one removed: the sibling must survive. Uninstall
    /// acting on the whole managed root would take out a skill the caller never
    /// named.
    #[tokio::test]
    async fn remove_materialized_deletes_only_the_named_slug() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let snapshot_dir = temp.path().join("agent-store-imports").join(&snapshot_id);
        for slug in ["keep", "drop"] {
            let dir = snapshot_dir.join("skills").join(slug);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!("---\nname: {slug}\n---\nbody")).unwrap();
        }
        let service = InstallerService::new(config(&temp));
        let outcome = service.materialize_skills(&snapshot_id).await.unwrap();
        assert_eq!(outcome.skills.len(), 2);

        assert!(service.remove_materialized(&snapshot_id, "drop").unwrap());
        let managed = temp.path().join("skills").join("agent-store").join(&snapshot_id);
        assert!(!managed.join("drop").exists(), "the named slug must be gone");
        assert!(managed.join("keep").join("SKILL.md").is_file(), "the sibling must survive");
    }

    #[tokio::test]
    async fn remove_materialized_is_reentrant() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let dir = temp
            .path()
            .join("skills")
            .join("agent-store")
            .join(&snapshot_id)
            .join("only");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: only\n---\nbody").unwrap();

        let service = InstallerService::new(config(&temp));
        assert!(service.remove_materialized(&snapshot_id, "only").unwrap());
        // "Already gone" is the state the caller asked for, not a failure.
        assert!(!service.remove_materialized(&snapshot_id, "only").unwrap());
    }

    #[tokio::test]
    async fn remove_snapshot_removes_every_materialized_skill() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = nomifun_common::generate_id();
        let managed = temp.path().join("skills").join("agent-store").join(&snapshot_id);
        for slug in ["a", "b"] {
            std::fs::create_dir_all(managed.join(slug)).unwrap();
            std::fs::write(managed.join(slug).join("SKILL.md"), "---\nname: x\n---\nb").unwrap();
        }
        let service = InstallerService::new(config(&temp));
        assert!(service.remove_snapshot(&snapshot_id).unwrap());
        assert!(!managed.exists(), "the whole managed snapshot tree must be gone");
        assert!(!service.remove_snapshot(&snapshot_id).unwrap());
    }

    /// A corrupted `runtime_ref.location` must not be able to steer a recursive
    /// delete outside the managed root.
    #[test]
    fn managed_path_segments_cannot_escape() {
        let temp = tempfile::tempdir().unwrap();
        let service = InstallerService::new(config(&temp));
        for bad in ["", ".", "..", "a/b", "a\\b", "../escape"] {
            assert!(
                matches!(
                    service.remove_materialized(bad, "slug"),
                    Err(InstallError::UnsafePath(_))
                ),
                "snapshot id {bad:?} must be refused"
            );
            assert!(
                matches!(
                    service.remove_materialized("snap", bad),
                    Err(InstallError::UnsafePath(_))
                ),
                "slug {bad:?} must be refused"
            );
        }
    }

    /// A symlinked managed path is refused instead of followed: the same
    /// hostile-link policy `copy_dir_into` applies on the way in.
    #[cfg(unix)]
    #[test]
    fn managed_path_refuses_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot_id = "snap";
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious.txt"), "keep me").unwrap();
        let managed = temp.path().join("skills").join("agent-store");
        std::fs::create_dir_all(&managed).unwrap();
        std::os::unix::fs::symlink(&outside, managed.join(snapshot_id)).unwrap();

        let service = InstallerService::new(config(&temp));
        assert!(matches!(
            service.remove_materialized(snapshot_id, "slug"),
            Err(InstallError::UnsafePath(_))
        ));
        assert!(matches!(
            service.remove_snapshot(snapshot_id),
            Err(InstallError::UnsafePath(_))
        ));
        assert!(outside.join("precious.txt").is_file(), "the link target must be untouched");
    }
}
