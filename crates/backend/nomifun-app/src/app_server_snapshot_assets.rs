//! Plugin-level snapshot asset read face (`import/assets` · `import/asset`).
//!
//! Implements [`SnapshotAssetProvider`] over the immutable import snapshot
//! cache (`{work_dir}/agent-store-imports/`, the same root the public display
//! asset route reads). The files beside the skills — `avatars/`, `bin/`,
//! `scripts/` — are what an expert pack's `assets` names and what the
//! package's own scripts assume the plugin root holds (doc `38`): without this
//! face an exported team points at a `bin/init_task.py` that no read face can
//! produce.
//!
//! ## Path safety is the whole job here
//!
//! Same shape, and the same stakes, as `app_server_skill_files.rs` — every
//! method takes a caller-supplied snapshot id and path, and a provider that
//! trusts either turns an authenticated endpoint into an arbitrary local file
//! read. Four checks stand between a request and the disk:
//!
//! 1. the snapshot id must be a bare UUIDv7-shaped id
//!    ([`snapshot_dir_name`], the exact check the public asset route applies);
//! 2. the snapshot directory must exist, not be a symlink, and canonicalize
//!    under the configured snapshot root;
//! 3. the requested path is rejected lexically first (`..`, absolute, drive
//!    letter, UNC, backslash, `:`) and only then confirmed by
//!    `canonicalize` + `starts_with(canonical_base)`;
//! 4. the path must additionally sit under one of the three **plugin-level
//!    asset directories** (`avatars/`, `bin/`, `scripts/`) — persona text,
//!    skill bodies and the manifest are addressable through their own faces
//!    and never through this one.
//!
//! This is a **convenience and stability boundary, not a confidentiality one**:
//! a same-user process on this machine can read these files directly anyway.
//!
//! See `docs/agent-store/38-expert-pack-assets.zh.md` §2.3 (I1 / I5).

use std::path::{Path, PathBuf};

use nomifun_api_types::{
    AppServerExpertAssetKind, AppServerExpertAssetRef, AppServerImportAssetList,
};
use nomifun_app_server::{
    MAX_SNAPSHOT_ASSET_BYTES, MAX_SNAPSHOT_ASSET_ENTRIES, SkillFileBytes, SkillFileError,
    SnapshotAssetProvider, snapshot_dir_name,
};

use crate::app_server_skill_files::{content_type_for, reject_unsafe_relative};

/// The plugin-level directories this face serves. Everything else in the
/// snapshot — `agents/*.md`, `skills/**`, `.codebuddy-plugin/plugin.json`,
/// `hooks/`, `.mcp.json` — belongs to another face or to no face at all.
const ASSET_ROOTS: [(&str, AppServerExpertAssetKind); 3] = [
    ("avatars", AppServerExpertAssetKind::Avatar),
    ("bin", AppServerExpertAssetKind::Script),
    ("scripts", AppServerExpertAssetKind::Script),
];

/// Read-side plugin-level snapshot assets.
#[derive(Clone)]
pub struct AppServerSnapshotAssets {
    snapshot_root: PathBuf,
}

impl AppServerSnapshotAssets {
    pub fn new(snapshot_root: PathBuf) -> Self {
        Self { snapshot_root }
    }

    /// Resolve a snapshot id to its canonical directory.
    ///
    /// The id check is the same one the public asset route applies, and the
    /// directory must exist as a real (non-symlink) directory inside the
    /// configured root — a snapshot the importer never wrote is `not_found`,
    /// not a path the caller guessed.
    fn snapshot_directory(&self, snapshot_id: &str) -> Result<PathBuf, SkillFileError> {
        let Some(dir_name) = snapshot_dir_name(snapshot_id) else {
            return Err(SkillFileError::InvalidRequest("invalid snapshot id".into()));
        };
        let base = self.snapshot_root.join(dir_name);
        let metadata = std::fs::symlink_metadata(&base).map_err(|_| {
            SkillFileError::NotFound(format!("snapshot {snapshot_id} not found"))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SkillFileError::InvalidRequest(
                "snapshot location is not a directory".into(),
            ));
        }
        let canonical_root = std::fs::canonicalize(&self.snapshot_root)
            .map_err(|error| SkillFileError::Internal(format!("snapshot root: {error}")))?;
        let canonical = std::fs::canonicalize(&base).map_err(|_| {
            SkillFileError::NotFound(format!("snapshot {snapshot_id} not found"))
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(SkillFileError::InvalidRequest(
                "snapshot directory escapes the snapshot root".into(),
            ));
        }
        Ok(canonical)
    }

    /// The asset class of a snapshot-relative path, or `None` when the path is
    /// outside the three plugin-level asset directories.
    ///
    /// The first segment decides: `bin/init_task.py` is a script, and so is
    /// `bin/sub/inner.sh` — the allowlist is on the directory, not on the file
    /// extension (an extensionless `bin/init_task` launcher is exactly the
    /// shape real packages ship).
    fn asset_kind(relative: &str) -> Option<AppServerExpertAssetKind> {
        let (root, _) = relative.split_once('/')?;
        ASSET_ROOTS
            .iter()
            .find(|(name, _)| *name == root)
            .map(|(_, kind)| *kind)
    }
}

/// Walk the three plugin-level asset directories, collecting regular files.
///
/// Symlinks are never followed (they are skipped in the listing and refused on
/// read, matching the skill file face). Entries beyond
/// [`MAX_SNAPSHOT_ASSET_ENTRIES`] set `truncated` instead of claiming
/// completeness; the walk stops early once the ceiling is reached.
fn collect_assets(root: &Path) -> (Vec<AppServerExpertAssetRef>, bool) {
    let mut assets = Vec::new();
    let mut truncated = false;
    let mut stack: Vec<PathBuf> = ASSET_ROOTS
        .iter()
        .map(|(name, _)| root.join(name))
        .collect();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // A package with no `bin/` is the normal case, not an error.
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // `file_type()` reports the link itself (no follow), so a symlink
            // is skipped here instead of being traversed.
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if assets.len() >= MAX_SNAPSHOT_ASSET_ENTRIES {
                truncated = true;
                return (assets, truncated);
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Some(kind) = AppServerSnapshotAssets::asset_kind(
                &relative.to_string_lossy().replace('\\', "/"),
            ) else {
                continue;
            };
            assets.push(AppServerExpertAssetRef {
                path: relative.to_string_lossy().replace('\\', "/"),
                kind,
                size: bytes.len() as u64,
                digest: nomifun_importer::digest::sha256_hex(&bytes),
            });
        }
    }

    assets.sort_by(|left, right| left.path.cmp(&right.path));
    (assets, truncated)
}

#[async_trait::async_trait]
impl SnapshotAssetProvider for AppServerSnapshotAssets {
    async fn assets(&self, snapshot_id: &str) -> Result<AppServerImportAssetList, SkillFileError> {
        let root = self.snapshot_directory(snapshot_id)?;
        let (assets, truncated) = collect_assets(&root);
        Ok(AppServerImportAssetList {
            snapshot_id: snapshot_id.to_owned(),
            assets,
            truncated,
        })
    }

    async fn read(
        &self,
        snapshot_id: &str,
        path: &str,
    ) -> Result<SkillFileBytes, SkillFileError> {
        reject_unsafe_relative(path)?;
        let Some(kind) = Self::asset_kind(path) else {
            return Err(SkillFileError::InvalidRequest(
                "path is outside the plugin-level asset directories (avatars/ bin/ scripts/)"
                    .into(),
            ));
        };
        let root = self.snapshot_directory(snapshot_id)?;

        let target = root.join(path);
        // Second pass, now that the target must exist: the canonical form has
        // to stay inside the canonical snapshot. This is what actually stops a
        // symlink planted inside `bin/` from reaching outside it.
        let canonical = std::fs::canonicalize(&target).map_err(|_| {
            SkillFileError::NotFound(format!("asset {path} not found in snapshot {snapshot_id}"))
        })?;
        if !canonical.starts_with(&root) {
            return Err(SkillFileError::InvalidRequest(
                "asset path escapes the snapshot".into(),
            ));
        }
        let metadata = std::fs::symlink_metadata(&target)
            .map_err(|_| SkillFileError::NotFound(format!("asset {path} not found")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SkillFileError::InvalidRequest(
                "path must name a regular file".into(),
            ));
        }
        // Checked from metadata, before any read: an oversized file is refused
        // rather than loaded into memory and then rejected.
        if metadata.len() > MAX_SNAPSHOT_ASSET_BYTES {
            return Err(SkillFileError::TooLarge {
                size: metadata.len(),
                limit: MAX_SNAPSHOT_ASSET_BYTES,
            });
        }
        let bytes = std::fs::read(&canonical)
            .map_err(|_| SkillFileError::NotFound(format!("asset {path} not found")))?;
        // The wire shape is the skill file face's, so the content type table is
        // too — a `bin/init_task.py` and a skill's `scripts/render.py` are the
        // same kind of payload.
        let _ = kind;
        Ok(SkillFileBytes {
            bytes,
            content_type: content_type_for(path).to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A UUIDv7-shaped id: the CHECK constraint on `plugin_snapshots.snapshot_id`
    /// requires this shape, and the path-safety gate does too.
    const SNAPSHOT: &str = "0190f5fe-7c00-7a00-8000-00000000de38";

    /// One snapshot with every asset class plus the files that must NOT be
    /// addressable.
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "snapshot-assets-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&root);
        let snapshot = root.join(SNAPSHOT);
        for (rel, body) in [
            ("avatars/team.png", "\u{89}PNG-data"),
            ("bin/init_task.py", "print('start')\n"),
            ("bin/init_task", "sh launcher"),
            ("scripts/check.sh", "echo ok\n"),
            ("agents/lead.md", "---\nname: lead\n---\npersona"),
            ("skills/hello/SKILL.md", "# hello"),
            (".codebuddy-plugin/plugin.json", r#"{"name":"p"}"#),
        ] {
            let target = snapshot.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, body).unwrap();
        }
        (root, snapshot)
    }

    #[test]
    fn asset_kind_covers_the_three_roots_and_nothing_else() {
        assert_eq!(
            AppServerSnapshotAssets::asset_kind("avatars/team.png"),
            Some(AppServerExpertAssetKind::Avatar)
        );
        assert_eq!(
            AppServerSnapshotAssets::asset_kind("bin/init_task.py"),
            Some(AppServerExpertAssetKind::Script)
        );
        assert_eq!(
            AppServerSnapshotAssets::asset_kind("scripts/check.sh"),
            Some(AppServerExpertAssetKind::Script)
        );
        // The same directory names deeper down belong to other faces' trees.
        assert_eq!(AppServerSnapshotAssets::asset_kind("skills/bin/x"), None);
        assert_eq!(AppServerSnapshotAssets::asset_kind("agents/lead.md"), None);
        assert_eq!(AppServerSnapshotAssets::asset_kind("plugin.json"), None);
        assert_eq!(AppServerSnapshotAssets::asset_kind("avatars"), None);
    }

    #[tokio::test]
    async fn lists_only_plugin_level_assets_sorted_by_path() {
        let (root, _snapshot) = fixture("list");
        let provider = AppServerSnapshotAssets::new(root.clone());

        let listing = provider.assets(SNAPSHOT).await.unwrap();

        let paths: Vec<&str> = listing.assets.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["avatars/team.png", "bin/init_task", "bin/init_task.py", "scripts/check.sh"]
        );
        assert!(!listing.truncated);
        // The digest is the file's own sha256, same rule as `skill/files`.
        let bytes = fs::read(root.join(SNAPSHOT).join("bin/init_task.py")).unwrap();
        assert_eq!(
            listing.assets.iter().find(|a| a.path == "bin/init_task.py").unwrap().digest,
            nomifun_importer::digest::sha256_hex(&bytes)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn reads_an_asset_but_refuses_everything_outside_the_roots() {
        let (root, snapshot) = fixture("read");
        let provider = AppServerSnapshotAssets::new(root.clone());

        let asset = provider.read(SNAPSHOT, "bin/init_task.py").await.unwrap();
        assert_eq!(asset.bytes, b"print('start')\n");
        assert_eq!(asset.content_type, "text/x-python; charset=utf-8");

        // Persona text is addressable through `agent/export` and nothing else.
        assert!(provider.read(SNAPSHOT, "agents/lead.md").await.is_err());
        assert!(provider.read(SNAPSHOT, "skills/hello/SKILL.md").await.is_err());
        assert!(provider.read(SNAPSHOT, ".codebuddy-plugin/plugin.json").await.is_err());
        // A file under the snapshot root but outside all roots is not
        // addressable even though no traversal is involved.
        fs::write(snapshot.join("README.md"), "notes").unwrap();
        assert!(provider.read(SNAPSHOT, "README.md").await.is_err());

        // Lexical traversal never reaches the filesystem.
        assert!(provider.read(SNAPSHOT, "../outside.txt").await.is_err());
        assert!(provider.read(SNAPSHOT, "avatars/../../outside").await.is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn an_unknown_or_malformed_snapshot_is_refused() {
        let (root, _snapshot) = fixture("unknown");
        let provider = AppServerSnapshotAssets::new(root);

        assert!(matches!(
            provider.assets("0190f5fe-7c00-7a00-8000-00000000dead").await,
            Err(SkillFileError::NotFound(_))
        ));
        // Not a UUIDv7-shaped id: refused before the filesystem is touched.
        assert!(matches!(
            provider.assets("../../etc").await,
            Err(SkillFileError::InvalidRequest(_))
        ));
        assert!(matches!(
            provider.read("../../etc", "bin/x").await,
            Err(SkillFileError::InvalidRequest(_))
        ));
    }

    /// A symlink *inside* an asset directory is the case a purely lexical guard
    /// cannot catch. The walk skips it and the read refuses it — either outcome
    /// is safe; leaking is not.
    #[tokio::test]
    async fn refuses_a_symlink_that_escapes_the_asset_directory() {
        let (root, snapshot) = fixture("symlink");
        let secret = root.join("secret.txt");
        fs::write(&secret, "outside the snapshot").unwrap();
        let link = snapshot.join("bin").join("link.py");

        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&secret, &link).is_ok();
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(&secret, &link).is_ok();
        if !linked {
            eprintln!("skipping: this environment cannot create symlinks");
            let _ = fs::remove_dir_all(&root);
            return;
        }

        let provider = AppServerSnapshotAssets::new(root.clone());
        let listing = provider.assets(SNAPSHOT).await.unwrap();
        assert!(
            !listing.assets.iter().any(|a| a.path == "bin/link.py"),
            "a symlink must not appear in the inventory"
        );
        assert!(provider.read(SNAPSHOT, "bin/link.py").await.is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn refuses_an_oversized_asset_from_metadata_not_after_reading() {
        let (root, snapshot) = fixture("big");
        let big = "x".repeat(MAX_SNAPSHOT_ASSET_BYTES as usize + 1);
        fs::create_dir_all(snapshot.join("bin")).unwrap();
        fs::write(snapshot.join("bin/blob.bin"), &big).unwrap();
        let provider = AppServerSnapshotAssets::new(root.clone());

        match provider.read(SNAPSHOT, "bin/blob.bin").await {
            Err(SkillFileError::TooLarge { size, limit }) => {
                assert!(size > limit);
                assert_eq!(limit, MAX_SNAPSHOT_ASSET_BYTES);
            }
            Ok(_) => panic!("an oversized asset must not be served"),
            Err(other) => panic!("expected TooLarge, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn truncates_the_inventory_rather_than_claiming_completeness() {
        let (root, snapshot) = fixture("many");
        fs::create_dir_all(snapshot.join("scripts")).unwrap();
        for index in 0..MAX_SNAPSHOT_ASSET_ENTRIES + 5 {
            fs::write(snapshot.join("scripts").join(format!("f{index}.sh")), "x").unwrap();
        }
        let provider = AppServerSnapshotAssets::new(root);

        let listing = provider.assets(SNAPSHOT).await.unwrap();

        assert!(listing.truncated, "an incomplete inventory must say so");
        assert!(listing.assets.len() <= MAX_SNAPSHOT_ASSET_ENTRIES);
    }
}
