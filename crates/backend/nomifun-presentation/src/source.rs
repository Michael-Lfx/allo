//! Fetch one pinned commit of the ppt-master repository and materialize only
//! its `skills/ppt-master` subtree.
//!
//! Integrity comes from the commit id itself: libgit2 hashes every object it
//! receives, so a commit found under the pinned id has exactly the pinned
//! tree. The subtree is written from the object database (no worktree
//! checkout), with names validated and symlinks/submodules rejected.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use git2::{FetchOptions, ObjectType, Oid, ProxyOptions, RemoteCallbacks, Repository, Tree};

use crate::manifest;
use crate::progress::{CANCELLED, InstallProgress, is_cancelled};

/// Where to fetch from and what must arrive.
#[derive(Debug, Clone)]
pub struct SourceSpec {
    pub urls: Vec<String>,
    pub commit: String,
    pub skill_subdir: String,
}

impl SourceSpec {
    /// Pinned production spec, honouring [`manifest::SOURCES_ENV`].
    pub fn pinned() -> Self {
        let urls = std::env::var(manifest::SOURCES_ENV)
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|urls| !urls.is_empty())
            .unwrap_or_else(|| manifest::DEFAULT_SOURCES.iter().map(|url| (*url).to_owned()).collect());
        Self {
            urls,
            commit: manifest::SOURCE_COMMIT.to_owned(),
            skill_subdir: manifest::SKILL_SUBDIR.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedSkill {
    pub url: String,
    pub commit: String,
    /// Tree id of the materialized subtree, recorded for audits.
    pub tree: String,
}

/// Fetch `spec.commit` from the first reachable URL and write the Skill subtree
/// into `dest` (which must not exist). `work_dir` holds the temporary bare
/// repository and is removed by the caller. Blocking; run off the async runtime.
pub fn fetch_skill(
    spec: &SourceSpec,
    work_dir: &Path,
    dest: &Path,
    progress: &InstallProgress,
    cancel: &AtomicBool,
) -> Result<FetchedSkill, String> {
    if spec.urls.is_empty() {
        return Err("no ppt-master source configured".into());
    }
    let mut errors = Vec::new();
    for (index, url) in spec.urls.iter().enumerate() {
        if is_cancelled(cancel) {
            return Err(CANCELLED.into());
        }
        let attempt = work_dir.join(format!("repo-{index}"));
        let _ = fs::remove_dir_all(&attempt);
        let _ = fs::remove_dir_all(dest);
        progress.reset();
        match fetch_one(url, spec, &attempt, dest, progress, cancel) {
            Ok(fetched) => {
                let _ = fs::remove_dir_all(&attempt);
                return Ok(fetched);
            }
            Err(error) => {
                tracing::warn!(url = %url, error = %error, "ppt-master source failed; trying next");
                let _ = fs::remove_dir_all(&attempt);
                if is_cancelled(cancel) {
                    return Err(CANCELLED.into());
                }
                errors.push(format!("{url}: {error}"));
            }
        }
    }
    Err(format!("all ppt-master sources failed: {}", errors.join("; ")))
}

fn fetch_one(
    url: &str,
    spec: &SourceSpec,
    repo_dir: &Path,
    dest: &Path,
    progress: &InstallProgress,
    cancel: &AtomicBool,
) -> Result<FetchedSkill, String> {
    let oid = Oid::from_str(&spec.commit).map_err(|error| format!("invalid pinned commit: {error}"))?;
    let repo = Repository::init_bare(repo_dir).map_err(|error| format!("init repository: {error}"))?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|stats| {
        progress.received_bytes.store(stats.received_bytes() as u64, Ordering::Relaxed);
        progress.received_objects.store(stats.received_objects() as u64, Ordering::Relaxed);
        progress.total_objects.store(stats.total_objects() as u64, Ordering::Relaxed);
        !is_cancelled(cancel)
    });
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks);
    if is_network_url(url) {
        // libgit2's local transport rejects shallow fetches; remotes support it.
        options.depth(1);
        options.proxy_options(proxy_options());
    }
    options.download_tags(git2::AutotagOption::None);

    {
        let mut remote = repo
            .remote_anonymous(url)
            .map_err(|error| format!("open remote: {error}"))?;
        remote
            .fetch(&[spec.commit.as_str()], Some(&mut options), None)
            .map_err(|error| {
                if is_cancelled(cancel) {
                    CANCELLED.to_owned()
                } else {
                    format!("fetch: {error}")
                }
            })?;
    }

    let commit = repo
        .find_commit(oid)
        .map_err(|_| format!("pinned commit {} was not received", spec.commit))?;
    let root = commit.tree().map_err(|error| format!("read commit tree: {error}"))?;
    let entry = root
        .get_path(Path::new(&spec.skill_subdir))
        .map_err(|_| format!("commit has no {}", spec.skill_subdir))?;
    if entry.kind() != Some(ObjectType::Tree) {
        return Err(format!("{} is not a directory", spec.skill_subdir));
    }
    let subtree = repo
        .find_tree(entry.id())
        .map_err(|error| format!("read {}: {error}", spec.skill_subdir))?;

    fs::create_dir_all(dest).map_err(|error| format!("create {}: {error}", dest.display()))?;
    let mut budget = Budget::default();
    write_tree(&repo, &subtree, dest, &mut budget, cancel)?;

    Ok(FetchedSkill {
        url: url.to_owned(),
        commit: commit.id().to_string(),
        tree: subtree.id().to_string(),
    })
}

fn write_tree(
    repo: &Repository,
    tree: &Tree<'_>,
    dir: &Path,
    budget: &mut Budget,
    cancel: &AtomicBool,
) -> Result<(), String> {
    for entry in tree.iter() {
        if is_cancelled(cancel) {
            return Err(CANCELLED.into());
        }
        let name = entry
            .name()
            .ok_or_else(|| "non-UTF-8 file name in Skill tree".to_owned())?;
        validate_entry_name(name)?;
        let target = dir.join(name);
        match (entry.kind(), entry.filemode()) {
            (Some(ObjectType::Tree), _) => {
                let child = repo
                    .find_tree(entry.id())
                    .map_err(|error| format!("read tree {name}: {error}"))?;
                fs::create_dir(&target).map_err(|error| format!("create {}: {error}", target.display()))?;
                write_tree(repo, &child, &target, budget, cancel)?;
            }
            (Some(ObjectType::Blob), mode @ (0o100644 | 0o100755)) => {
                let blob = repo
                    .find_blob(entry.id())
                    .map_err(|error| format!("read blob {name}: {error}"))?;
                budget.add_file(blob.size() as u64)?;
                fs::write(&target, blob.content())
                    .map_err(|error| format!("write {}: {error}", target.display()))?;
                set_executable(&target, mode == 0o100755)?;
            }
            (_, mode) => {
                // 0o120000 symlink, 0o160000 submodule, anything else.
                return Err(format!("unsupported entry {name} (mode {mode:o}) in Skill tree"));
            }
        }
    }
    Ok(())
}

/// One path component from git: reject anything that could escape `dir` or
/// alias special files.
pub(crate) fn validate_entry_name(name: &str) -> Result<(), String> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.eq_ignore_ascii_case(".git")
        || name.contains(['/', '\\', ':', '\0']);
    if bad {
        Err(format!("unsafe file name {name:?} in Skill tree"))
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct Budget {
    files: usize,
    bytes: u64,
}

impl Budget {
    fn add_file(&mut self, size: u64) -> Result<(), String> {
        self.files += 1;
        self.bytes = self.bytes.saturating_add(size);
        if self.files > manifest::MAX_SKILL_FILES {
            return Err(format!("Skill tree has more than {} files", manifest::MAX_SKILL_FILES));
        }
        if self.bytes > manifest::MAX_SKILL_BYTES {
            return Err(format!("Skill tree is larger than {} bytes", manifest::MAX_SKILL_BYTES));
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn set_executable(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("chmod {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn set_executable(_path: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

fn is_network_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// libgit2 reads `https_proxy`/`HTTPS_PROXY` itself (`auto`), but not the OS
/// proxy (Windows registry, macOS settings). Fall back to the proxy Flowy
/// detects for child processes; libgit2 cannot speak SOCKS, so skip it.
fn proxy_options() -> ProxyOptions<'static> {
    let mut options = ProxyOptions::new();
    match detected_http_proxy() {
        Some(url) => {
            options.url(&url);
        }
        None => {
            options.auto();
        }
    }
    options
}

fn detected_http_proxy() -> Option<String> {
    let vars = nomifun_net::proxy::child_proxy_env(std::iter::empty());
    ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"].iter().find_map(|key| {
        vars.iter()
            .find(|(name, value)| name == key && value.starts_with("http"))
            .map(|(_, value)| value.clone())
    })
}

/// `<root>/<id>`, refusing ids that are not a single plain component.
pub(crate) fn child_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    validate_entry_name(id)?;
    Ok(root.join(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_that_escape_or_alias_are_rejected() {
        for name in ["", ".", "..", ".git", ".GIT", "a/b", "a\\b", "c:", "x\0"] {
            assert!(validate_entry_name(name).is_err(), "{name:?} should be rejected");
        }
        for name in ["SKILL.md", ".gitignore", "svg_to_pptx", "中文.md"] {
            assert!(validate_entry_name(name).is_ok(), "{name:?} should be accepted");
        }
    }

    #[test]
    fn only_http_urls_get_shallow_depth() {
        assert!(is_network_url("https://atomgit.com/x.git"));
        assert!(!is_network_url("/tmp/repo.git"));
        assert!(!is_network_url("file:///tmp/repo.git"));
    }
}
