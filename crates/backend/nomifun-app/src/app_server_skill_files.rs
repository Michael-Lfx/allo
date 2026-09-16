//! Skill file-tree read face (`skill/files` · `skill/file`).
//!
//! Implements [`SkillFileProvider`] over the system skill corpus. The catalog
//! provider (`app_server_catalog.rs`) describes a Skill as a catalog entry;
//! this one serves the *directory* that entry lives in, because `skill/get`
//! can only ever return a bounded summary of `SKILL.md` and says nothing about
//! the files beside it (`references/`, `scripts/`, `templates/`, `assets/` —
//! `02` §5, `17` §5).
//!
//! ## Path safety is the whole job here
//!
//! Every method takes a caller-supplied skill id and (for `read`) a path. A
//! provider that trusts either turns an authenticated endpoint into an
//! arbitrary local file read. The protocol layer deliberately does **not**
//! second-guess the resolved path — only this module knows which directory an
//! id resolved to — so the checks below are the only ones standing:
//!
//! 1. the id resolves through the same catalog the read face uses, so a caller
//!    cannot name a directory that is not a listed Skill;
//! 2. the skill directory itself must not be a symlink, and its canonical form
//!    must stay inside the user skills root;
//! 3. the requested path is rejected lexically first (`..`, absolute, drive
//!    letter, UNC) and only then confirmed by `canonicalize` +
//!    `starts_with(canonical_base)` — the same two-step the public asset route
//!    uses, with the lexical pass first so a traversal attempt never depends on
//!    the target existing;
//! 4. symlinks inside the tree are never followed and never served.
//!
//! This is a **convenience and stability boundary, not a confidentiality one**:
//! a same-user process on this machine can read these files directly anyway.
//! The checks exist so the endpoint does not degrade into an unauthenticated
//! arbitrary-file-read primitive.
//!
//! See `docs/agent-store/24-external-agent-skill-and-mcp-access.zh.md` §4.

use async_trait::async_trait;
use std::path::{Component, Path, PathBuf};

use nomifun_app_server::{MAX_SKILL_FILE_BYTES, SkillFileBytes, SkillFileError, SkillFileProvider};
use nomifun_api_types::{AppServerSkillFile, AppServerSkillFileList};
use nomifun_extension::skill_service::{self, SkillPaths};

/// Largest inventory this face will return. Beyond it `truncated` is set.
const MAX_SKILL_FILE_ENTRIES: usize = 2000;

/// Read-side Skill file tree over the system skill corpus.
#[derive(Clone)]
pub struct AppServerSkillFiles {
    paths: SkillPaths,
}

impl AppServerSkillFiles {
    pub fn new(paths: SkillPaths) -> Self {
        Self { paths }
    }

    /// Resolve a public skill id to its on-disk directory.
    ///
    /// Goes through the same listing the read face uses, so the id space is
    /// identical: an id that `skill/list` publishes resolves here, and one that
    /// does not is `not_found` rather than a path the caller guessed. The
    /// newest managed snapshot wins, matching `list_available_skills`.
    async fn skill_directory(&self, skill_id: &str) -> Result<PathBuf, SkillFileError> {
        let items = skill_service::list_available_skills(&self.paths)
            .await
            .map_err(|error| SkillFileError::Internal(format!("list skills: {error}")))?;
        let item = items
            .iter()
            .find(|item| item.name == skill_id)
            .ok_or_else(|| SkillFileError::NotFound(format!("skill {skill_id} not found")))?;
        let location = PathBuf::from(&item.location);
        // `skill_manifest_path` normalizes the two catalog shapes: built-ins
        // record the manifest *file*, user-root skills record the directory.
        // Only a directory has a tree to enumerate.
        if !location.is_dir() {
            return Err(SkillFileError::InvalidRequest(format!(
                "skill {skill_id} is a single manifest with no file tree"
            )));
        }
        Ok(location)
    }

    /// Confirm `dir` is a real directory whose canonical form sits under the
    /// user skills root or the built-in skills root, and that it is not itself
    /// reached through a link.
    fn checked_root(&self, dir: &Path) -> Result<PathBuf, SkillFileError> {
        let metadata = std::fs::symlink_metadata(dir)
            .map_err(|_| SkillFileError::NotFound("skill directory not found".into()))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillFileError::InvalidRequest(
                "skill directory is a symlink; refusing to serve it".into(),
            ));
        }
        if !metadata.is_dir() {
            return Err(SkillFileError::InvalidRequest(
                "skill location is not a directory".into(),
            ));
        }
        let canonical = std::fs::canonicalize(dir)
            .map_err(|_| SkillFileError::NotFound("skill directory not found".into()))?;
        // Both roots are legitimate skill homes. Everything else (an unexpected
        // catalog root, or a location that drifted) is refused rather than
        // trusted: this face would otherwise serve any directory the catalog
        // happened to name.
        let inside = [&self.paths.user_skills_dir, &self.paths.builtin_skills_dir]
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .any(|root| canonical.starts_with(&root));
        if !inside {
            return Err(SkillFileError::InvalidRequest(
                "skill directory is outside the managed skill roots".into(),
            ));
        }
        Ok(canonical)
    }
}

/// Reject a caller-supplied relative path lexically, before touching the disk.
///
/// The lexical pass runs first on purpose: a traversal attempt must be refused
/// on its own terms, not incidentally because the target failed to resolve.
/// `..`, root/prefix components, backslashes and colons are refused; only
/// `Normal` components survive.
///
/// One deliberate subtlety: `Path::components` *normalizes away* an interior
/// `.` and repeated separators, so `a/./b` arrives as `a/b` and is accepted.
/// That is correct rather than a hole — it resolves inside the tree, and the
/// `canonicalize` + `starts_with` pass in [`AppServerSkillFiles::read`] is the
/// real guard for anything that could leave it. Only a **leading** `.` (`"."`,
/// `"./a"`) is seen as `CurDir` and refused here.
fn reject_unsafe_relative(path: &str) -> Result<(), SkillFileError> {
    if path.is_empty() {
        return Err(SkillFileError::InvalidRequest("path must not be empty".into()));
    }
    // A backslash is a separator on Windows and a plain character elsewhere;
    // rejecting it outright keeps one spelling from meaning two things.
    if path.contains('\\') {
        return Err(SkillFileError::InvalidRequest(
            "path must use forward slashes".into(),
        ));
    }
    if path.starts_with('/') {
        return Err(SkillFileError::InvalidRequest("path must be relative".into()));
    }
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                return Err(SkillFileError::InvalidRequest(
                    "path must not contain '.' segments".into(),
                ));
            }
            Component::ParentDir => {
                return Err(SkillFileError::InvalidRequest(
                    "path must not contain '..' segments".into(),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(SkillFileError::InvalidRequest("path must be relative".into()));
            }
        }
    }
    // Windows drive-relative forms (`C:foo`) parse as `Prefix` above, but a
    // bare `C:` survives some spellings; reject the colon outright. A skill
    // file legitimately named with ':' is not a case worth serving.
    if path.contains(':') {
        return Err(SkillFileError::InvalidRequest(
            "path must not contain ':'".into(),
        ));
    }
    Ok(())
}

/// Walk a skill directory, collecting regular files with their digests.
///
/// Symlinks are never followed. Entries that cannot be read are skipped rather
/// than failing the whole listing: one unreadable file must not hide the rest
/// of the tree.
fn collect_files(root: &Path) -> (Vec<AppServerSkillFile>, bool) {
    let mut files = Vec::new();
    let mut truncated = false;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
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
            if files.len() >= MAX_SKILL_FILE_ENTRIES {
                truncated = true;
                return (files, truncated);
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            files.push(AppServerSkillFile {
                path: relative.to_string_lossy().replace('\\', "/"),
                size: bytes.len() as u64,
                digest: nomifun_importer::digest::sha256_hex(&bytes),
            });
        }
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    (files, truncated)
}

/// Content type from the file extension, falling back to
/// `application/octet-stream`.
///
/// Deliberately not `mime_guess`: that would add a dependency for a table of
/// the handful of extensions a Skill actually ships, and its answers for
/// source files (`text/x-rust`, `.ts` → `video/mp2t` among others) are more
/// surprising than useful here. Unknown extensions get the honest default.
///
/// Serving `text/html` / `image/svg+xml` is safe *here* only because the route
/// is gated on a connection header (`require_ready`): a browser navigating to
/// the URL directly cannot send `x-app-server-connection-id`, so the response
/// is never loaded as a document in this origin. If the gate is ever relaxed,
/// these two entries become a stored-XSS vector and must be forced to
/// `text/plain` (`Content-Disposition: attachment`).
fn content_type_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "txt" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "csv" => "text/csv; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "ts" | "tsx" => "text/plain; charset=utf-8",
        "py" => "text/x-python; charset=utf-8",
        "sh" | "bash" => "text/x-shellscript; charset=utf-8",
        "ps1" => "text/plain; charset=utf-8",
        "rs" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[async_trait]
impl SkillFileProvider for AppServerSkillFiles {
    async fn files(&self, skill_id: &str) -> Result<AppServerSkillFileList, SkillFileError> {
        let dir = self.skill_directory(skill_id).await?;
        let root = self.checked_root(&dir)?;
        let (files, truncated) = collect_files(&root);
        Ok(AppServerSkillFileList {
            skill_id: skill_id.to_owned(),
            content_digest: nomifun_importer::digest::tree_digest_of_dir(&root),
            files,
            truncated,
        })
    }

    async fn read(&self, skill_id: &str, path: &str) -> Result<SkillFileBytes, SkillFileError> {
        reject_unsafe_relative(path)?;
        let dir = self.skill_directory(skill_id).await?;
        let root = self.checked_root(&dir)?;

        let target = root.join(path);
        // Second pass, now that the target must exist: the canonical form has
        // to stay inside the canonical root. This is what actually stops a
        // symlink planted *inside* the tree from reaching outside it.
        let canonical = std::fs::canonicalize(&target).map_err(|_| {
            SkillFileError::NotFound(format!("file {path} not found in skill {skill_id}"))
        })?;
        if !canonical.starts_with(&root) {
            return Err(SkillFileError::InvalidRequest(
                "file path escapes the skill directory".into(),
            ));
        }
        let metadata = std::fs::symlink_metadata(&target)
            .map_err(|_| SkillFileError::NotFound(format!("file {path} not found")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SkillFileError::InvalidRequest(
                "path must name a regular file".into(),
            ));
        }
        // Checked from metadata, before any read: an oversized file is refused
        // rather than loaded into memory and then rejected.
        if metadata.len() > MAX_SKILL_FILE_BYTES {
            return Err(SkillFileError::TooLarge {
                size: metadata.len(),
                limit: MAX_SKILL_FILE_BYTES,
            });
        }
        let bytes = std::fs::read(&canonical)
            .map_err(|_| SkillFileError::NotFound(format!("file {path} not found")))?;
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

    fn paths_at(root: &Path) -> SkillPaths {
        SkillPaths {
            data_dir: root.to_path_buf(),
            user_skills_dir: root.join("skills"),
            cron_skills_dir: root.join("cron-skills"),
            builtin_skills_dir: root.join("builtin-skills"),
            builtin_rules_dir: root.join("builtin-rules"),
            preset_rules_dir: root.join("preset-rules"),
            preset_skills_dir: root.join("preset-skills"),
            catalog_roots: Default::default(),
        }
    }

    /// Build a temp root holding one user skill with the given extra files.
    fn fixture(name: &str, extra: &[(&str, &str)]) -> (std::path::PathBuf, SkillPaths) {
        let root = std::env::temp_dir().join(format!(
            "skill-files-{name}-{}-{}",
            std::process::id(),
            // Distinct per test even within one process.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&root);
        let paths = paths_at(&root);
        let dir = paths.user_skills_dir.join("demo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: demo\ndescription: d\n---\nbody").unwrap();
        for (rel, body) in extra {
            let target = dir.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(target, body).unwrap();
        }
        (root, paths)
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in [
            "",
            "../outside",
            "a/../../outside",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "C:foo",
            "..\\outside",
            "a\\b",
            ".",
            "./a",
            "a/..",
        ] {
            assert!(
                reject_unsafe_relative(bad).is_err(),
                "{bad:?} must be rejected lexically"
            );
        }
    }

    #[test]
    fn accepts_ordinary_relative_paths() {
        for good in ["SKILL.md", "references/guide.md", "scripts/run.sh", "a/b/c.txt"] {
            assert!(reject_unsafe_relative(good).is_ok(), "{good:?} must be accepted");
        }
    }

    #[test]
    fn interior_dot_is_normalized_not_treated_as_traversal() {
        // `Path::components` collapses an interior `.`, so this resolves to
        // `a/b` — inside the tree. Refusing it would be superstition; the
        // canonicalize pass is what actually guards the boundary.
        assert!(reject_unsafe_relative("a/./b").is_ok());
        assert!(reject_unsafe_relative("a//b").is_ok());
    }

    #[test]
    fn content_type_covers_skill_shapes_and_defaults_honestly() {
        assert_eq!(content_type_for("SKILL.md"), "text/markdown; charset=utf-8");
        assert_eq!(content_type_for("scripts/run.sh"), "text/x-shellscript; charset=utf-8");
        assert_eq!(content_type_for("data/config.json"), "application/json");
        // Unknown extensions must not be guessed into something executable.
        assert_eq!(content_type_for("blob.xyz"), "application/octet-stream");
    }

    #[tokio::test]
    async fn lists_attached_files_not_just_the_manifest() {
        // The whole reason this face exists: `skill/get` returns a bounded
        // summary of SKILL.md and says nothing about the files beside it.
        let (root, paths) = fixture(
            "attach",
            &[
                ("references/guide.md", "guide"),
                ("scripts/run.sh", "echo hi"),
            ],
        );
        let provider = AppServerSkillFiles::new(paths);

        let listing = provider.files("demo").await.unwrap();

        let names: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, vec!["SKILL.md", "references/guide.md", "scripts/run.sh"]);
        assert!(!listing.truncated);
        // Digest must match an independent computation over the same directory.
        let dir = root.join("skills").join("demo");
        assert_eq!(
            listing.content_digest,
            nomifun_importer::digest::tree_digest_of_dir(&dir)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn reads_an_attached_file_but_refuses_traversal() {
        let (root, paths) = fixture("read", &[("references/guide.md", "guide body")]);
        let provider = AppServerSkillFiles::new(paths.clone());

        let file = provider.read("demo", "references/guide.md").await.unwrap();
        assert_eq!(file.bytes, b"guide body");
        assert_eq!(file.content_type, "text/markdown; charset=utf-8");

        // A lexical traversal never reaches the filesystem.
        let bad = provider.read("demo", "../outside.txt").await;
        assert!(bad.is_err(), "traversal must be refused");

        // A file outside the skill, named without traversal, is not addressable.
        fs::write(paths.user_skills_dir.join("outside.txt"), "secret").unwrap();
        assert!(provider.read("demo", "outside.txt").await.is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn an_unknown_skill_is_not_found() {
        let (root, paths) = fixture("unknown", &[]);
        let provider = AppServerSkillFiles::new(paths);
        assert!(matches!(
            provider.files("nope").await,
            Err(SkillFileError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn refuses_an_oversized_file_from_metadata_not_after_reading() {
        let big = "x".repeat(MAX_SKILL_FILE_BYTES as usize + 1);
        let (root, paths) = fixture("big", &[("big.txt", &big)]);
        let provider = AppServerSkillFiles::new(paths);

        match provider.read("demo", "big.txt").await {
            Err(SkillFileError::TooLarge { size, limit }) => {
                assert!(size > limit, "the refusal must report the real size");
                assert_eq!(limit, MAX_SKILL_FILE_BYTES);
            }
            // Deliberately not printing the Ok variant: `SkillFileBytes` holds
            // file contents and has no `Debug`, which keeps a body out of logs
            // and out of a panic message here.
            Ok(_) => panic!("an oversized file must not be served"),
            Err(other) => panic!("expected TooLarge, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn truncates_the_inventory_rather_than_claiming_completeness() {        let extras: Vec<(String, String)> = (0..MAX_SKILL_FILE_ENTRIES + 5)
            .map(|index| (format!("many/f{index}.txt"), "x".to_owned()))
            .collect();
        let borrowed: Vec<(&str, &str)> = extras
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str()))
            .collect();
        let (root, paths) = fixture("many", &borrowed);
        let provider = AppServerSkillFiles::new(paths);

        let listing = provider.files("demo").await.unwrap();

        assert!(listing.truncated, "an incomplete inventory must say so");
        assert!(listing.files.len() <= MAX_SKILL_FILE_ENTRIES);
        let _ = fs::remove_dir_all(&root);
    }

    /// A symlink *inside* the tree is the case a purely lexical guard cannot
    /// catch: nothing about the string "link.txt" says it leaves the directory.
    /// The `canonicalize` + `starts_with` pass is what stops it.
    #[tokio::test]
    async fn refuses_a_symlink_that_escapes_the_skill_directory() {
        let (root, paths) = fixture("symlink", &[]);
        let secret = root.join("secret.txt");
        fs::write(&secret, "outside the skill").unwrap();
        let link = paths.user_skills_dir.join("demo").join("link.txt");

        if !try_symlink(&secret, &link) {
            // Creating symlinks needs a privilege we may not hold (Windows
            // without developer mode). Skipping is honest; asserting nothing
            // would not be.
            eprintln!("skipping: this environment cannot create symlinks");
            let _ = fs::remove_dir_all(&root);
            return;
        }

        let provider = AppServerSkillFiles::new(paths);
        // Either the listing omits it (walk never follows links) …
        let listing = provider.files("demo").await.unwrap();
        assert!(
            !listing.files.iter().any(|f| f.path == "link.txt"),
            "a symlink must not appear in the inventory: {:?}",
            listing.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        // … or reading it is refused. Both outcomes are safe; leaking is not.
        assert!(provider.read("demo", "link.txt").await.is_err());

        let _ = fs::remove_dir_all(&root);
    }

    /// Best-effort symlink creation, so the test above can opt out honestly.
    #[cfg(windows)]
    fn try_symlink(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    #[cfg(not(windows))]
    fn try_symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}
