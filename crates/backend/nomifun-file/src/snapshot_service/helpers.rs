//! Pure helper functions for the snapshot service.
//!
//! All functions here are synchronous and take no `&self` — they can be
//! called safely inside `spawn_blocking`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use git2::{IndexAddOption, Repository, Signature, Status, StatusOptions};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use nomifun_common::{AppError, FileChangeOperation};
use sha2::{Digest, Sha256};

use crate::types::{CompareResult, FileChangeInfo, SnapshotInfo, SnapshotMode};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Prefix for snapshot directories under the durable snapshot root.
pub(super) const SNAPSHOT_DIR_PREFIX: &str = "nomifun-snapshot-";
/// Marker file stored next to the snapshot `.git` so reuse does not depend on
/// git2 resolving `core.worktree` (unreliable across Windows `\\?\` paths).
const WORKSPACE_MARKER_FILE: &str = "nomifun-workspace";

/// Exclude rules written to `<git-dir>/info/exclude` for snapshot mode.
/// These patterns prevent large/generated directories from being tracked.
const SNAPSHOT_EXCLUDE_RULES: &str = "\
node_modules/
dist/
build/
target/
.venv/
__pycache__/
.DS_Store
Thumbs.db
*.pyc
.env
.env.local
.next/
.nuxt/
.output/
";

/// Signature name used for snapshot commits.
const SNAPSHOT_SIG_NAME: &str = "nomifun";
/// Signature email used for snapshot commits.
const SNAPSHOT_SIG_EMAIL: &str = "snapshot@nomifun.local";
/// Commit message for the initial snapshot baseline.
const SNAPSHOT_INITIAL_MSG: &str = "Initial snapshot";
/// Commit message prefix for coding-mode turn checkpoints.
const TURN_CHECKPOINT_MSG_PREFIX: &str = "nomifun turn checkpoint";
/// Ref namespace for turn checkpoints (never the user's branch HEAD).
const TURN_CHECKPOINT_REF_PREFIX: &str = "refs/nomifun/turns";

// ---------------------------------------------------------------------------
// Snapshot-branch safety guard
// ---------------------------------------------------------------------------

/// Max number of (non-excluded) files a non-git workspace may contain before
/// snapshot tracking is refused. Mirrors `service::MAX_WORKSPACE_FILES`.
const SNAPSHOT_MAX_FILES: usize = 20_000;

/// Max cumulative bytes of (non-excluded) files before snapshot tracking is
/// refused (~384 MB).
const SNAPSHOT_MAX_BYTES: u64 = 384 * 1024 * 1024;

/// Wall-clock budget for the pre-walk itself, so the *check* is bounded even on
/// a pathologically large/slow tree. On timeout we refuse (fail-closed).
const SNAPSHOT_GUARD_DEADLINE: Duration = Duration::from_secs(5);

/// Decide whether snapshot tracking should be refused for `canonical`.
///
/// Returns `Some(reason)` to refuse (caller maps to `SnapshotMode::Disabled`),
/// `None` to allow. Only ever called on the non-git **Snapshot** branch — the
/// cheap `GitRepo` path never consults this.
///
/// Checks run cheap → expensive:
/// 1. Drive / filesystem root.
/// 2. Well-known system directory denylist.
/// 3. Bounded pre-walk (file count + cumulative bytes), applying the snapshot
///    exclude rules, with a wall-clock deadline.
pub(super) fn snapshot_guard(canonical: &Path) -> Option<String> {
    snapshot_guard_with_limits(canonical, SNAPSHOT_MAX_FILES, SNAPSHOT_MAX_BYTES, SNAPSHOT_GUARD_DEADLINE)
}

/// Whether `path` is a drive root (Windows bare `X:\`) or filesystem root
/// (Unix `/`). Operates on the canonical path.
fn is_fs_root(path: &Path) -> bool {
    // Unix `/` and Windows `\\?\C:\` / `C:\` all have no parent component...
    // but `Path::parent` on Windows verbatim roots can be subtle, so check both
    // an empty/absent parent and the "only a prefix + root, no normal component"
    // shape.
    if path.parent().is_none() {
        return true;
    }
    // A drive root like `C:\` (or verbatim `\\?\C:\`) consists solely of a
    // Prefix component followed by a RootDir component and nothing else.
    use std::path::Component;
    let mut comps = path.components();
    let mut has_normal = false;
    let mut has_root = false;
    for c in comps.by_ref() {
        match c {
            Component::Prefix(_) | Component::RootDir => has_root = true,
            Component::Normal(_) | Component::CurDir | Component::ParentDir => {
                has_normal = true;
                break;
            }
        }
    }
    has_root && !has_normal
}

/// Build the denylist of well-known directories that must never be snapshotted.
/// Entries are canonicalized where possible so comparison is robust.
fn snapshot_denylist() -> Vec<PathBuf> {
    let mut deny: Vec<PathBuf> = Vec::new();

    let mut push = |p: PathBuf| {
        let canonical = std::fs::canonicalize(&p).unwrap_or(p);
        if !deny.contains(&canonical) {
            deny.push(canonical);
        }
    };

    // User home root and the system temp root.
    if let Some(home) = dirs::home_dir() {
        push(home);
    }
    push(std::env::temp_dir());

    #[cfg(windows)]
    {
        if let Some(sys) = std::env::var_os("SystemRoot") {
            push(PathBuf::from(sys)); // typically C:\Windows
        } else {
            push(PathBuf::from("C:\\Windows"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            push(PathBuf::from(pf));
        } else {
            push(PathBuf::from("C:\\Program Files"));
        }
        if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
            push(PathBuf::from(pf86));
        }
    }

    #[cfg(not(windows))]
    {
        push(PathBuf::from("/usr"));
        push(PathBuf::from("/"));
    }

    deny
}

/// Build an `Override` matcher from the snapshot exclude rules so the pre-walk
/// counts only what the snapshot would actually track. Each exclude pattern is
/// added as a blacklist glob (prefix `!`); with no whitelist globs present, the
/// `ignore` crate includes everything except blacklisted matches.
fn build_exclude_overrides(root: &Path) -> Option<ignore::overrides::Override> {
    let mut builder = OverrideBuilder::new(root);
    for line in SNAPSHOT_EXCLUDE_RULES.lines() {
        let pat = line.trim();
        if pat.is_empty() {
            continue;
        }
        // Blacklist (ignore) this pattern. Append `**` to dir patterns so the
        // whole subtree is excluded, matching gitignore directory semantics.
        let glob = if pat.ends_with('/') {
            format!("!{}**", pat)
        } else {
            format!("!{}", pat)
        };
        if builder.add(&glob).is_err() {
            return None;
        }
    }
    builder.build().ok()
}

/// Testable core of [`snapshot_guard`]: thresholds are parameters so tests can
/// exercise the count/byte logic without materializing 20k files.
fn snapshot_guard_with_limits(canonical: &Path, max_files: usize, max_bytes: u64, deadline: Duration) -> Option<String> {
    // 1. Drive / filesystem root.
    if is_fs_root(canonical) {
        return Some(format!(
            "Refusing to snapshot a drive/filesystem root: {}",
            canonical.display()
        ));
    }

    // 2. Well-known system directory denylist.
    for deny in snapshot_denylist() {
        if canonical == deny.as_path() {
            return Some(format!("Refusing to snapshot a protected system directory: {}", canonical.display()));
        }
    }

    // 3. Bounded pre-walk: early-abort on file count, cumulative bytes, or a
    //    wall-clock deadline (fail-closed on timeout).
    let overrides = match build_exclude_overrides(canonical) {
        Some(o) => o,
        // If the override matcher can't be built, fall back to no exclusions
        // (more conservative: counts more files).
        None => OverrideBuilder::new(canonical).build().expect("empty override builds"),
    };

    let walker = WalkBuilder::new(canonical)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .overrides(overrides)
        .build();

    let started = Instant::now();
    let mut file_count: usize = 0;
    let mut byte_count: u64 = 0;

    for entry in walker {
        if started.elapsed() > deadline {
            return Some(format!(
                "Refusing to snapshot: scanning {} exceeded the {}s safety deadline",
                canonical.display(),
                deadline.as_secs()
            ));
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // unreadable entry: skip, don't fail the whole check
        };
        // Count files only (directories don't contribute to the snapshot size).
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        file_count += 1;
        if file_count > max_files {
            return Some(format!(
                "Refusing to snapshot: {} contains more than {} files",
                canonical.display(),
                max_files
            ));
        }
        if let Ok(meta) = entry.metadata() {
            byte_count = byte_count.saturating_add(meta.len());
            if byte_count > max_bytes {
                return Some(format!(
                    "Refusing to snapshot: {} exceeds {} bytes of tracked content",
                    canonical.display(),
                    max_bytes
                ));
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Tracked state for an initialized workspace.
#[derive(Clone, Debug)]
pub(super) struct WorkspaceState {
    pub mode: SnapshotMode,
    /// Path to the git directory.
    /// - git-repo mode: the workspace path itself (contains `.git/`).
    /// - snapshot mode: `{snapshot_root}/nomifun-snapshot-{hash}`.
    pub repo_path: PathBuf,
    /// Canonical path to the actual workspace directory.
    pub workspace_path: PathBuf,
    /// Number of outstanding `init` calls. Each `init` of an already-tracked
    /// workspace increments it; each `dispose` decrements it. The DashMap entry
    /// is removed at 0. Snapshot-mode repos stay on disk so a later init
    /// (including after process restart) can reopen the original baseline.
    pub refcount: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stable hex identity for a workspace path. Must not use `DefaultHasher`
/// (SipHash keys are process-randomized, so a restart would miss the repo).
pub(super) fn stable_workspace_hash(workspace: &str) -> String {
    let digest = Sha256::digest(workspace.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Snapshot repo directory under `root` for a (usually canonical) workspace path.
pub(super) fn snapshot_repo_path(root: &Path, workspace: &str) -> PathBuf {
    root.join(format!("{}{}", SNAPSHOT_DIR_PREFIX, stable_workspace_hash(workspace)))
}

/// Test helper: snapshot repo under the process temp dir.
#[cfg(test)]
pub(super) fn temp_repo_path(workspace: &str) -> PathBuf {
    snapshot_repo_path(&std::env::temp_dir(), workspace)
}

fn workspace_marker_path(snapshot_dir: &Path) -> PathBuf {
    snapshot_dir.join(WORKSPACE_MARKER_FILE)
}

fn write_workspace_marker(snapshot_dir: &Path, workspace: &Path) -> Result<(), AppError> {
    std::fs::write(workspace_marker_path(snapshot_dir), workspace.to_string_lossy().as_bytes()).map_err(|e| {
        AppError::Internal(format!(
            "Failed to write snapshot workspace marker in {}: {}",
            snapshot_dir.display(),
            e
        ))
    })
}

fn read_workspace_marker(snapshot_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(workspace_marker_path(snapshot_dir)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Open the git repository for a workspace state.
/// Snapshot-mode always rebinds `workdir` so status/diff use the workspace tree
/// even when `core.worktree` failed to round-trip across Windows path forms.
pub(super) fn open_repo(state: &WorkspaceState) -> Result<Repository, AppError> {
    let repo = Repository::open(&state.repo_path).map_err(|e| {
        AppError::Internal(format!(
            "Failed to open git repo at {}: {}",
            state.repo_path.display(),
            e
        ))
    })?;
    if matches!(state.mode, SnapshotMode::Snapshot) {
        repo.set_workdir(&state.workspace_path, false).map_err(|e| {
            AppError::Internal(format!(
                "Failed to set snapshot workdir to {}: {}",
                state.workspace_path.display(),
                e
            ))
        })?;
    }
    Ok(repo)
}

fn same_workspace_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => {
            let norm = |path: &Path| {
                path.to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches('/')
                    .to_ascii_lowercase()
            };
            norm(left) == norm(right)
        }
    }
}

/// Reopen a leftover snapshot-mode repo when it still belongs to `workspace`.
///
/// Returning `true` means the original baseline commit is intact and must not
/// be recaptured — recapturing would fold agent edits into HEAD so the Changes
/// rail looks empty after leaving a session or restarting the app.
pub(super) fn try_reuse_snapshot_repo(workspace: &Path, snapshot_dir: &Path) -> bool {
    if !snapshot_dir.join(".git").is_dir() {
        return false;
    }
    let Ok(repo) = Repository::open(snapshot_dir) else {
        return false;
    };
    if repo.head().ok().and_then(|head| head.target()).is_none() {
        return false;
    }

    let marker_ok = read_workspace_marker(snapshot_dir)
        .map(|marked| same_workspace_path(&marked, workspace))
        .unwrap_or(false);
    let workdir_ok = repo
        .workdir()
        .map(|wd| same_workspace_path(wd, workspace))
        .unwrap_or(false);
    if !marker_ok && !workdir_ok {
        return false;
    }

    if repo.set_workdir(workspace, false).is_err() {
        return false;
    }
    if let Ok(mut config) = repo.config() {
        let _ = config.set_str("core.worktree", &workspace.to_string_lossy());
    }
    let _ = write_workspace_marker(snapshot_dir, workspace);
    true
}

/// Pick up a leftover OS-temp snapshot from older builds (`DefaultHasher` /
/// `std::env::temp_dir()`). Those directories survive a UI remount but are
/// invisible to the durable `{data_dir}/file-snapshots` path.
pub(super) fn find_legacy_os_temp_snapshot(workspace: &Path) -> Option<PathBuf> {
    let temp = std::env::temp_dir();
    let entries = std::fs::read_dir(&temp).ok()?;
    let mut seen = 0usize;
    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if !name.starts_with(SNAPSHOT_DIR_PREFIX) {
            continue;
        }
        seen += 1;
        if seen > 128 {
            break;
        }
        let path = entry.path();
        if try_reuse_snapshot_repo(workspace, &path) {
            return Some(path);
        }
    }
    None
}

/// Move a leftover snapshot into the durable root when the filesystem allows
/// it (same volume). Cross-volume rename keeps using `from`.
pub(super) fn adopt_snapshot_repo(from: &Path, to: &Path) -> PathBuf {
    if from == to {
        return to.to_path_buf();
    }
    if to.exists() {
        return from.to_path_buf();
    }
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(from, to).is_ok() {
        to.to_path_buf()
    } else {
        from.to_path_buf()
    }
}

/// Initialize a snapshot-mode temp repository for a non-git workspace.
///
/// 1. Reuses an existing temp repo for this workspace when it is still valid.
/// 2. Otherwise creates the temp directory with a standard `.git` layout.
/// 3. Sets `core.worktree` to point at the real workspace.
/// 4. Writes exclude rules to `.git/info/exclude`.
/// 5. Adds all workspace files and creates an initial commit as the baseline.
pub(super) fn init_snapshot_repo(workspace: &Path, temp_dir: &Path) -> Result<(), AppError> {
    if try_reuse_snapshot_repo(workspace, temp_dir) {
        return Ok(());
    }
    if temp_dir.exists() {
        std::fs::remove_dir_all(temp_dir).map_err(|e| {
            AppError::Internal(format!(
                "Failed to clean up existing snapshot dir {}: {}",
                temp_dir.display(),
                e
            ))
        })?;
    }
    std::fs::create_dir_all(temp_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create snapshot dir {}: {}", temp_dir.display(), e)))?;

    // Init a standard repo (creates .git/ inside temp_dir)
    let repo = Repository::init(temp_dir)
        .map_err(|e| AppError::Internal(format!("Failed to init snapshot repo at {}: {}", temp_dir.display(), e)))?;

    // Set workdir to the actual workspace (in-memory)
    repo.set_workdir(workspace, false)
        .map_err(|e| AppError::Internal(format!("Failed to set workdir to {}: {}", workspace.display(), e)))?;

    // Persist core.worktree in config so future opens resolve the workdir
    let mut config = repo
        .config()
        .map_err(|e| AppError::Internal(format!("Failed to open repo config: {}", e)))?;
    let ws_str = workspace.to_string_lossy();
    config
        .set_str("core.worktree", &ws_str)
        .map_err(|e| AppError::Internal(format!("Failed to set core.worktree to {}: {}", ws_str, e)))?;

    // Write exclude rules to .git/info/exclude (avoids polluting the workspace)
    let git_dir = repo.path(); // .git/ directory
    let info_dir = git_dir.join("info");
    std::fs::create_dir_all(&info_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create info dir {}: {}", info_dir.display(), e)))?;
    std::fs::write(info_dir.join("exclude"), SNAPSHOT_EXCLUDE_RULES)
        .map_err(|e| AppError::Internal(format!("Failed to write exclude rules: {}", e)))?;

    // Stage all workspace files
    let mut index = repo
        .index()
        .map_err(|e| AppError::Internal(format!("Failed to get index: {}", e)))?;
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(|e| AppError::Internal(format!("Failed to add files to index: {}", e)))?;
    index
        .write()
        .map_err(|e| AppError::Internal(format!("Failed to write index: {}", e)))?;

    // Create initial commit
    let tree_oid = index
        .write_tree()
        .map_err(|e| AppError::Internal(format!("Failed to write tree: {}", e)))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| AppError::Internal(format!("Failed to find tree: {}", e)))?;
    let sig = Signature::now(SNAPSHOT_SIG_NAME, SNAPSHOT_SIG_EMAIL)
        .map_err(|e| AppError::Internal(format!("Failed to create signature: {}", e)))?;
    repo.commit(Some("HEAD"), &sig, &sig, SNAPSHOT_INITIAL_MSG, &tree, &[])
        .map_err(|e| AppError::Internal(format!("Failed to create initial commit: {}", e)))?;

    write_workspace_marker(temp_dir, workspace)?;
    Ok(())
}

/// Get the current branch name from a repository.
/// Returns `None` if HEAD is detached or the repo has no commits.
pub(super) fn current_branch(repo: &Repository) -> Option<String> {
    repo.head().ok().and_then(|head| head.shorthand().map(String::from))
}

/// Build a `SnapshotInfo` from mode and repository.
pub(super) fn build_info(mode: SnapshotMode, repo: &Repository) -> SnapshotInfo {
    let branch = match mode {
        SnapshotMode::GitRepo => current_branch(repo),
        SnapshotMode::Snapshot | SnapshotMode::Disabled { .. } => None,
    };
    SnapshotInfo { mode, branch }
}

/// Map git2 index (staging area) status flags to `FileChangeOperation`.
pub(super) fn index_operation(status: Status) -> Option<FileChangeOperation> {
    if status.intersects(Status::INDEX_NEW) {
        Some(FileChangeOperation::Create)
    } else if status.intersects(Status::INDEX_MODIFIED) {
        Some(FileChangeOperation::Modify)
    } else if status.intersects(Status::INDEX_DELETED) {
        Some(FileChangeOperation::Delete)
    } else {
        None
    }
}

/// Map git2 working-tree status flags to `FileChangeOperation`.
pub(super) fn worktree_operation(status: Status) -> Option<FileChangeOperation> {
    if status.intersects(Status::WT_NEW) {
        Some(FileChangeOperation::Create)
    } else if status.intersects(Status::WT_MODIFIED) {
        Some(FileChangeOperation::Modify)
    } else if status.intersects(Status::WT_DELETED) {
        Some(FileChangeOperation::Delete)
    } else {
        None
    }
}

/// Parse git2 statuses into staged and unstaged change lists.
pub(super) fn parse_statuses(repo: &Repository, workspace: &Path) -> Result<CompareResult, AppError> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| AppError::Internal(format!("Failed to get git status: {}", e)))?;

    let ws_str = workspace.to_string_lossy();
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();
        let rel_path = match entry.path() {
            Some(p) => p.to_string(),
            None => continue,
        };
        let full_path = format!("{}/{}", ws_str.trim_end_matches('/'), &rel_path);

        if let Some(op) = index_operation(status) {
            staged.push(FileChangeInfo {
                file_path: full_path.clone(),
                relative_path: rel_path.clone(),
                operation: op,
            });
        }
        if let Some(op) = worktree_operation(status) {
            unstaged.push(FileChangeInfo {
                file_path: full_path,
                relative_path: rel_path,
                operation: op,
            });
        }
    }

    Ok(CompareResult { staged, unstaged })
}

/// Read a file's content from HEAD.
/// Returns `None` if the file is not tracked or the repo has no commits.
pub(super) fn read_baseline(repo: &Repository, rel_path: &str) -> Result<Option<String>, AppError> {
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };
    let commit = head
        .peel_to_commit()
        .map_err(|e| AppError::Internal(format!("Failed to peel HEAD to commit: {}", e)))?;
    let tree = commit
        .tree()
        .map_err(|e| AppError::Internal(format!("Failed to get commit tree: {}", e)))?;

    let entry = match tree.get_path(Path::new(rel_path)) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };

    let blob = repo
        .find_blob(entry.id())
        .map_err(|e| AppError::Internal(format!("Failed to read blob: {}", e)))?;

    match std::str::from_utf8(blob.content()) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => Ok(None), // Binary file -- no text baseline
    }
}

/// Canonicalize a workspace path and validate it exists.
pub(super) fn resolve_workspace(workspace: &str) -> Result<PathBuf, AppError> {
    let path = Path::new(workspace);
    if !path.exists() {
        return Err(AppError::NotFound(format!("Workspace not found: {}", workspace)));
    }
    std::fs::canonicalize(path)
        .map_err(|e| AppError::Internal(format!("Failed to canonicalize workspace path {}: {}", workspace, e)))
}

/// Stage all changes including deletions.
///
/// `index.add_all` with `DEFAULT` only handles new/modified files.
/// Deleted files must be explicitly removed from the index.
pub(super) fn stage_all_with_deletions(repo: &Repository) -> Result<(), AppError> {
    let mut index = repo
        .index()
        .map_err(|e| AppError::Internal(format!("Failed to get index: {}", e)))?;

    // Stage new and modified files
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(|e| AppError::Internal(format!("Failed to stage all files: {}", e)))?;

    // Find and remove deleted files from the index
    let mut opts = StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| AppError::Internal(format!("Failed to get status: {}", e)))?;
    for entry in statuses.iter() {
        if entry.status().intersects(Status::WT_DELETED)
            && let Some(path) = entry.path()
        {
            index
                .remove_path(Path::new(path))
                .map_err(|e| AppError::Internal(format!("Failed to remove deleted file {} from index: {}", path, e)))?;
        }
    }

    index
        .write()
        .map_err(|e| AppError::Internal(format!("Failed to write index: {}", e)))?;
    Ok(())
}

/// Stage a single file, handling both existing and deleted files.
///
/// For existing files, adds to the index. For deleted files, removes from
/// the index (equivalent to `git add <deleted-file>`).
pub(super) fn stage_single_file(repo: &Repository, rel_path: &str) -> Result<(), AppError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| AppError::Internal("Repository has no workdir".into()))?;
    let abs_path = workdir.join(rel_path);

    let mut index = repo
        .index()
        .map_err(|e| AppError::Internal(format!("Failed to get index: {}", e)))?;

    if abs_path.exists() {
        index
            .add_path(Path::new(rel_path))
            .map_err(|e| AppError::Internal(format!("Failed to stage file {}: {}", rel_path, e)))?;
    } else {
        // File was deleted from disk; remove from index
        index
            .remove_path(Path::new(rel_path))
            .map_err(|e| AppError::Internal(format!("Failed to stage deleted file {}: {}", rel_path, e)))?;
    }

    index
        .write()
        .map_err(|e| AppError::Internal(format!("Failed to write index: {}", e)))?;
    Ok(())
}

/// Unstage a single file (reset it in the index to match HEAD).
pub(super) fn unstage_single_file(repo: &Repository, rel_path: &str) -> Result<(), AppError> {
    let head = repo
        .head()
        .map_err(|e| AppError::Internal(format!("Failed to get HEAD: {}", e)))?;
    let commit = head
        .peel_to_commit()
        .map_err(|e| AppError::Internal(format!("Failed to peel HEAD: {}", e)))?;
    // reset_default expects a commit-ish object, not a tree
    repo.reset_default(Some(commit.as_object()), [rel_path])
        .map_err(|e| AppError::Internal(format!("Failed to unstage file {}: {}", rel_path, e)))?;
    Ok(())
}

/// Unstage all staged changes (mixed reset to HEAD).
pub(super) fn unstage_all_files(repo: &Repository) -> Result<(), AppError> {
    let head = repo
        .head()
        .map_err(|e| AppError::Internal(format!("Failed to get HEAD: {}", e)))?;
    let commit = head
        .peel_to_commit()
        .map_err(|e| AppError::Internal(format!("Failed to peel HEAD: {}", e)))?;
    repo.reset(commit.as_object(), git2::ResetType::Mixed, None)
        .map_err(|e| AppError::Internal(format!("Failed to unstage all: {}", e)))?;
    Ok(())
}

/// Discard working-tree changes for a single file.
///
/// - `Create`: delete the new file from disk.
/// - `Modify`: restore file content from HEAD.
/// - `Delete`: restore the deleted file from HEAD.
pub(super) fn discard_single_file(
    repo: &Repository,
    workspace: &Path,
    rel_path: &str,
    operation: FileChangeOperation,
) -> Result<(), AppError> {
    match operation {
        FileChangeOperation::Create => {
            // New/untracked file: just delete it
            let abs_path = workspace.join(rel_path);
            if abs_path.exists() {
                std::fs::remove_file(&abs_path)
                    .map_err(|e| AppError::Internal(format!("Failed to delete file {}: {}", abs_path.display(), e)))?;
            }
            Ok(())
        }
        FileChangeOperation::Modify | FileChangeOperation::Delete => {
            // Restore file from HEAD using checkout
            checkout_path_from_head(repo, rel_path)
        }
    }
}

/// Reset a file completely: unstage (if staged) and restore working tree.
///
/// - `Create`: unstage + delete file.
/// - `Modify`: unstage + restore from HEAD.
/// - `Delete`: unstage + restore from HEAD.
pub(super) fn reset_single_file(
    repo: &Repository,
    workspace: &Path,
    rel_path: &str,
    operation: FileChangeOperation,
) -> Result<(), AppError> {
    // Step 1: unstage (ignore errors for files not in index)
    let _ = unstage_single_file(repo, rel_path);

    // Step 2: restore working tree
    discard_single_file(repo, workspace, rel_path, operation)
}

/// Checkout a single file from HEAD, restoring it in the working tree.
fn checkout_path_from_head(repo: &Repository, rel_path: &str) -> Result<(), AppError> {
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force().path(rel_path);

    repo.checkout_head(Some(&mut cb))
        .map_err(|e| AppError::Internal(format!("Failed to checkout {} from HEAD: {}", rel_path, e)))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Turn checkpoints (coding-mode rollback)
// ---------------------------------------------------------------------------

/// Sanitize one id segment for use inside a git ref name.
fn sanitize_ref_segment(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "turn checkpoint id must be non-empty".to_owned(),
        ));
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.contains("..") || out.starts_with('.') || out.ends_with('.') {
        return Err(AppError::BadRequest(
            "turn checkpoint id produced an invalid git ref segment".to_owned(),
        ));
    }
    Ok(out)
}

/// Build `refs/nomifun/turns/{conversation_id}/{message_id}`.
pub(super) fn turn_checkpoint_ref(conversation_id: &str, message_id: &str) -> Result<String, AppError> {
    let conv = sanitize_ref_segment(conversation_id)?;
    let msg = sanitize_ref_segment(message_id)?;
    Ok(format!("{TURN_CHECKPOINT_REF_PREFIX}/{conv}/{msg}"))
}

/// Capture the current worktree into a commit under the turn checkpoint ref.
///
/// Stages into the in-memory index only (`write_tree` without `index.write`),
/// then reloads the on-disk index so GitRepo mode does not leave the user's
/// staging area dirty. Never updates `HEAD`.
pub(super) fn create_turn_checkpoint_commit(
    repo: &Repository,
    conversation_id: &str,
    message_id: &str,
) -> Result<crate::types::TurnCheckpoint, AppError> {
    let ref_name = turn_checkpoint_ref(conversation_id, message_id)?;

    let mut index = repo
        .index()
        .map_err(|e| AppError::Internal(format!("Failed to open index for turn checkpoint: {e}")))?;
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(|e| AppError::Internal(format!("Failed to stage worktree for turn checkpoint: {e}")))?;
    // Pick up deletions of previously tracked paths.
    index
        .update_all(["*"].iter(), None)
        .map_err(|e| AppError::Internal(format!("Failed to update index for turn checkpoint: {e}")))?;
    let tree_oid = index
        .write_tree()
        .map_err(|e| AppError::Internal(format!("Failed to write turn checkpoint tree: {e}")))?;
    // Discard in-memory staging; leave the on-disk index untouched.
    index
        .read(true)
        .map_err(|e| AppError::Internal(format!("Failed to reload index after turn checkpoint: {e}")))?;

    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| AppError::Internal(format!("Failed to find turn checkpoint tree: {e}")))?;
    let sig = Signature::now(SNAPSHOT_SIG_NAME, SNAPSHOT_SIG_EMAIL)
        .map_err(|e| AppError::Internal(format!("Failed to create turn checkpoint signature: {e}")))?;

    let parents: Vec<git2::Commit<'_>> = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
        Some(head) => vec![head],
        None => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
    let message = format!("{TURN_CHECKPOINT_MSG_PREFIX} {conversation_id}/{message_id}");
    let oid = repo
        .commit(None, &sig, &sig, &message, &tree, &parent_refs)
        .map_err(|e| AppError::Internal(format!("Failed to create turn checkpoint commit: {e}")))?;

    repo.reference(&ref_name, oid, true, "nomifun turn checkpoint")
        .map_err(|e| AppError::Internal(format!("Failed to update turn checkpoint ref {ref_name}: {e}")))?;

    Ok(crate::types::TurnCheckpoint {
        oid: oid.to_string(),
        ref_name,
    })
}

/// Restore the worktree to the tree of a turn checkpoint commit.
///
/// Uses force checkout without updating the index so GitRepo staging stays
/// intact. Removes worktree paths that are absent from the checkpoint tree.
pub(super) fn restore_turn_checkpoint_tree(
    repo: &Repository,
    conversation_id: &str,
    message_id: &str,
) -> Result<(), AppError> {
    let ref_name = turn_checkpoint_ref(conversation_id, message_id)?;
    let reference = repo.find_reference(&ref_name).map_err(|e| {
        AppError::BadRequest(format!(
            "Turn checkpoint not found for this message ({ref_name}): {e}"
        ))
    })?;
    let commit = reference.peel_to_commit().map_err(|e| {
        AppError::Internal(format!("Failed to peel turn checkpoint {ref_name}: {e}"))
    })?;
    let tree = commit
        .tree()
        .map_err(|e| AppError::Internal(format!("Failed to read turn checkpoint tree: {e}")))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force()
        .remove_untracked(true)
        .update_index(false)
        .recreate_missing(true);

    repo.checkout_tree(tree.as_object(), Some(&mut cb))
        .map_err(|e| AppError::Internal(format!("Failed to restore turn checkpoint worktree: {e}")))?;
    Ok(())
}

/// Whether the turn checkpoint ref exists.
pub(super) fn turn_checkpoint_exists(
    repo: &Repository,
    conversation_id: &str,
    message_id: &str,
) -> Result<bool, AppError> {
    let ref_name = turn_checkpoint_ref(conversation_id, message_id)?;
    Ok(repo.find_reference(&ref_name).is_ok())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- temp_repo_path --

    #[test]
    fn temp_repo_path_deterministic() {
        let a = temp_repo_path("/home/user/project");
        let b = temp_repo_path("/home/user/project");
        assert_eq!(a, b);
    }

    #[test]
    fn temp_repo_path_different_for_different_workspaces() {
        let a = temp_repo_path("/home/user/project-a");
        let b = temp_repo_path("/home/user/project-b");
        assert_ne!(a, b);
    }

    #[test]
    fn temp_repo_path_has_prefix() {
        let p = temp_repo_path("/ws");
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(SNAPSHOT_DIR_PREFIX));
    }

    #[test]
    fn stable_workspace_hash_is_stable_hex() {
        let a = stable_workspace_hash("/home/user/project");
        let b = stable_workspace_hash("/home/user/project");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, stable_workspace_hash("/home/user/other"));
    }

    #[test]
    fn snapshot_repo_path_uses_given_root() {
        let root = PathBuf::from("/data/file-snapshots");
        let p = snapshot_repo_path(&root, "/home/user/project");
        assert_eq!(p.parent(), Some(root.as_path()));
        assert!(p
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(SNAPSHOT_DIR_PREFIX));
    }

    #[test]
    fn try_reuse_accepts_marker_when_workdir_is_wrong() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("ws");
        let snapshot = tmp.path().join("snap");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("f.txt"), "orig").unwrap();
        init_snapshot_repo(&workspace, &snapshot).unwrap();

        let repo = Repository::open(&snapshot).unwrap();
        repo.set_workdir(&snapshot, false).unwrap();
        drop(repo);

        assert!(
            try_reuse_snapshot_repo(&workspace, &snapshot),
            "workspace marker must allow reuse when git2 workdir no longer matches"
        );
    }

    // -- index_operation / worktree_operation --

    #[test]
    fn index_operation_new() {
        assert_eq!(index_operation(Status::INDEX_NEW), Some(FileChangeOperation::Create));
    }

    #[test]
    fn index_operation_modified() {
        assert_eq!(
            index_operation(Status::INDEX_MODIFIED),
            Some(FileChangeOperation::Modify)
        );
    }

    #[test]
    fn index_operation_deleted() {
        assert_eq!(
            index_operation(Status::INDEX_DELETED),
            Some(FileChangeOperation::Delete)
        );
    }

    #[test]
    fn index_operation_none_for_wt() {
        assert_eq!(index_operation(Status::WT_NEW), None);
    }

    #[test]
    fn worktree_operation_new() {
        assert_eq!(worktree_operation(Status::WT_NEW), Some(FileChangeOperation::Create));
    }

    #[test]
    fn worktree_operation_modified() {
        assert_eq!(
            worktree_operation(Status::WT_MODIFIED),
            Some(FileChangeOperation::Modify)
        );
    }

    #[test]
    fn worktree_operation_deleted() {
        assert_eq!(
            worktree_operation(Status::WT_DELETED),
            Some(FileChangeOperation::Delete)
        );
    }

    #[test]
    fn worktree_operation_none_for_index() {
        assert_eq!(worktree_operation(Status::INDEX_NEW), None);
    }

    // -- resolve_workspace --

    #[test]
    fn resolve_workspace_not_found() {
        let err = resolve_workspace("/nonexistent/path/xyz123").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn resolve_workspace_success() {
        let tmp = std::env::temp_dir();
        let result = resolve_workspace(tmp.to_str().unwrap());
        assert!(result.is_ok());
    }

    // -- current_branch --

    #[test]
    fn current_branch_of_fresh_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        // Fresh repo with no commits -- HEAD is unborn
        assert!(current_branch(&repo).is_none());

        // Create an initial commit so HEAD points to a branch
        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        let branch = current_branch(&repo);
        assert!(branch.is_some());
        assert!(!branch.unwrap().is_empty());
    }

    // -- build_info --

    #[test]
    fn build_info_git_repo_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        let info = build_info(SnapshotMode::GitRepo, &repo);
        assert_eq!(info.mode, SnapshotMode::GitRepo);
        assert!(info.branch.is_some());
    }

    #[test]
    fn build_info_snapshot_mode_returns_no_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        let info = build_info(SnapshotMode::Snapshot, &repo);
        assert_eq!(info.mode, SnapshotMode::Snapshot);
        assert!(info.branch.is_none());
    }

    // -- read_baseline --

    #[test]
    fn read_baseline_no_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let result = read_baseline(&repo, "any.txt").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn read_baseline_tracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("hello.txt"), "Hello, world!").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("hello.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add hello", &tree, &[]).unwrap();

        let content = read_baseline(&repo, "hello.txt").unwrap();
        assert_eq!(content.as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn read_baseline_untracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        let content = read_baseline(&repo, "missing.txt").unwrap();
        assert!(content.is_none());
    }

    // -- parse_statuses --

    #[test]
    fn parse_statuses_clean_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert!(result.staged.is_empty());
        assert!(result.unstaged.is_empty());
    }

    #[test]
    fn parse_statuses_new_untracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("b.txt"), "b").unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert!(result.staged.is_empty());
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.unstaged[0].relative_path, "b.txt");
        assert_eq!(result.unstaged[0].operation, FileChangeOperation::Create);
    }

    #[test]
    fn parse_statuses_modified_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "modified").unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert!(result.staged.is_empty());
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.unstaged[0].operation, FileChangeOperation::Modify);
    }

    #[test]
    fn parse_statuses_deleted_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::remove_file(tmp.path().join("a.txt")).unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert!(result.staged.is_empty());
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.unstaged[0].operation, FileChangeOperation::Delete);
    }

    #[test]
    fn parse_statuses_staged_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("new.txt"), "new content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].relative_path, "new.txt");
        assert_eq!(result.staged[0].operation, FileChangeOperation::Create);
        assert!(result.unstaged.is_empty());
    }

    #[test]
    fn parse_statuses_staged_and_unstaged_mixed() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "staged change").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();

        std::fs::write(tmp.path().join("a.txt"), "unstaged change").unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].operation, FileChangeOperation::Modify);
        assert_eq!(result.unstaged.len(), 1);
        assert_eq!(result.unstaged[0].operation, FileChangeOperation::Modify);
    }

    // -- stage_all_with_deletions --

    #[test]
    fn stage_all_handles_deleted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        // Commit two files
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "b").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.add_path(Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        // Delete b.txt and modify a.txt
        std::fs::remove_file(tmp.path().join("b.txt")).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "modified").unwrap();

        stage_all_with_deletions(&repo).unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        // Both changes should be staged now
        assert_eq!(result.staged.len(), 2);
        assert!(result.unstaged.is_empty());

        let delete_entry = result
            .staged
            .iter()
            .find(|e| e.relative_path == "b.txt")
            .expect("b.txt should be staged");
        assert_eq!(delete_entry.operation, FileChangeOperation::Delete);
    }

    // -- stage_single_file --

    #[test]
    fn stage_single_file_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::remove_file(tmp.path().join("a.txt")).unwrap();
        stage_single_file(&repo, "a.txt").unwrap();

        let result = parse_statuses(&repo, tmp.path()).unwrap();
        assert_eq!(result.staged.len(), 1);
        assert_eq!(result.staged[0].operation, FileChangeOperation::Delete);
        assert!(result.unstaged.is_empty());
    }

    // -- discard_single_file --

    #[test]
    fn discard_created_file_deletes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("new.txt"), "new").unwrap();
        assert!(tmp.path().join("new.txt").exists());

        discard_single_file(&repo, tmp.path(), "new.txt", FileChangeOperation::Create).unwrap();

        assert!(!tmp.path().join("new.txt").exists());
    }

    #[test]
    fn discard_modified_file_restores_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "modified").unwrap();

        discard_single_file(&repo, tmp.path(), "a.txt", FileChangeOperation::Modify).unwrap();

        let content = std::fs::read_to_string(tmp.path().join("a.txt")).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn discard_deleted_file_restores_it() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

        std::fs::remove_file(tmp.path().join("a.txt")).unwrap();
        assert!(!tmp.path().join("a.txt").exists());

        discard_single_file(&repo, tmp.path(), "a.txt", FileChangeOperation::Delete).unwrap();

        assert!(tmp.path().join("a.txt").exists());
        let content = std::fs::read_to_string(tmp.path().join("a.txt")).unwrap();
        assert_eq!(content, "content");
    }

    // -- snapshot_guard --

    use std::time::Duration;

    /// A generous deadline so the walk completes; threshold logic is what we test.
    fn test_deadline() -> Duration {
        Duration::from_secs(30)
    }

    #[test]
    fn guard_refuses_when_file_count_exceeds_limit() {
        let tmp = tempfile::tempdir().unwrap();
        // 3 files, limit 2 -> refuse.
        for i in 0..3 {
            std::fs::write(tmp.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let canonical = std::fs::canonicalize(tmp.path()).unwrap();

        let reason = snapshot_guard_with_limits(&canonical, 2, u64::MAX, test_deadline());
        assert!(reason.is_some(), "should refuse a dir over the file-count limit");
    }

    #[test]
    fn guard_allows_when_under_file_count_limit() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..3 {
            std::fs::write(tmp.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let canonical = std::fs::canonicalize(tmp.path()).unwrap();

        // 3 files, limit 100 -> allow.
        let reason = snapshot_guard_with_limits(&canonical, 100, u64::MAX, test_deadline());
        assert!(reason.is_none(), "should allow a dir under the file-count limit: {reason:?}");
    }

    #[test]
    fn guard_refuses_when_byte_count_exceeds_limit() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big.bin"), vec![0u8; 4096]).unwrap();
        let canonical = std::fs::canonicalize(tmp.path()).unwrap();

        // 4096 bytes, byte limit 1024 -> refuse.
        let reason = snapshot_guard_with_limits(&canonical, usize::MAX, 1024, test_deadline());
        assert!(reason.is_some(), "should refuse a dir over the byte limit");
    }

    #[test]
    fn guard_excludes_node_modules_from_count() {
        let tmp = tempfile::tempdir().unwrap();
        // 1 real file + many files under node_modules/ (which is excluded).
        std::fs::write(tmp.path().join("index.js"), "1").unwrap();
        let nm = tmp.path().join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        for i in 0..50 {
            std::fs::write(nm.join(format!("dep{i}.js")), "x").unwrap();
        }
        let canonical = std::fs::canonicalize(tmp.path()).unwrap();

        // limit 5: would be exceeded if node_modules were counted, but it's excluded.
        let reason = snapshot_guard_with_limits(&canonical, 5, u64::MAX, test_deadline());
        assert!(
            reason.is_none(),
            "node_modules must be excluded from the pre-walk count: {reason:?}"
        );
    }

    #[test]
    fn guard_refuses_denylisted_system_temp_root() {
        // The system temp root itself is denylisted regardless of size.
        let temp_root = std::env::temp_dir();
        // Canonicalize to mirror what init does.
        let canonical = std::fs::canonicalize(&temp_root).unwrap_or(temp_root);

        let reason = snapshot_guard_with_limits(&canonical, usize::MAX, u64::MAX, test_deadline());
        assert!(reason.is_some(), "system temp root must be denylisted");
    }

    #[test]
    #[cfg(windows)]
    fn guard_refuses_drive_root() {
        let canonical = std::fs::canonicalize("C:\\").unwrap();
        let reason = snapshot_guard_with_limits(&canonical, usize::MAX, u64::MAX, test_deadline());
        assert!(reason.is_some(), "C:\\ drive root must be refused");
    }

    #[test]
    #[cfg(not(windows))]
    fn guard_refuses_fs_root() {
        let reason = snapshot_guard_with_limits(Path::new("/"), usize::MAX, u64::MAX, test_deadline());
        assert!(reason.is_some(), "/ fs root must be refused");
    }
}
