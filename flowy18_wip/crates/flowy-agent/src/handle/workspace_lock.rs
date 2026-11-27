//! Normalized directory-level read/write lease table for concurrent agent turns.
//!
//! Multiple turns may hold [`WorkspaceAccessMode::Read`] on overlapping
//! directories concurrently, whereas [`WorkspaceAccessMode::Write`] is strictly
//! exclusive against both writers and readers on the same canonicalized path
//! as well as any ancestor/descendant directory (preventing monorepo root vs.
//! sub-package write races). Independent directories (including distinct `Home`
//! session workspaces, distinct project roots, and sibling prefixes like
//! `proj` vs `proj-2`) never contend with one another.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::guards::resolve_deepest_ancestor;
use super::lock;

/// Access mode requested for a workspace directory during an agent turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceAccessMode {
    /// Shared read-only inspection (e.g. Router / read-only analysis turns).
    #[allow(dead_code)]
    Read,
    /// Exclusive write access (default for tool-capable agent turns).
    Write,
}

#[derive(Debug, Default)]
struct LockEntry {
    writer: Option<String>,
    readers: Vec<String>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: Mutex<HashMap<String, LockEntry>>,
    released_cv: Condvar,
}

/// Thread-safe leaf lock table tracking active workspace leases by normalized
/// directory path.
///
/// Lock ordering: always a leaf lock; never acquire another host mutex while
/// holding `entries`.
#[derive(Clone, Debug, Default)]
pub(super) struct WorkspaceLockTable {
    inner: Arc<Inner>,
}

/// RAII workspace lease that releases its directory lock automatically on drop
/// (including normal completion, cancellation, `WaitingForUser`, or panic).
#[derive(Debug)]
pub(super) struct WorkspaceLease {
    table: WorkspaceLockTable,
    key: String,
    mode: WorkspaceAccessMode,
    session_id: String,
    released: bool,
}

impl WorkspaceLease {
    /// Eagerly release this workspace lease before scope exit (idempotent).
    pub(super) fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        {
            let mut entries = lock(&self.table.inner.entries);
            if let Some(entry) = entries.get_mut(&self.key) {
                match self.mode {
                    WorkspaceAccessMode::Write => {
                        if entry.writer.as_deref()
                            == Some(self.session_id.as_str())
                        {
                            entry.writer = None;
                        }
                    }
                    WorkspaceAccessMode::Read => {
                        if let Some(pos) = entry
                            .readers
                            .iter()
                            .position(|holder| holder == &self.session_id)
                        {
                            entry.readers.swap_remove(pos);
                        }
                    }
                }
                if entry.writer.is_none() && entry.readers.is_empty() {
                    entries.remove(&self.key);
                }
            }
            drop(entries);
        }
        self.table.inner.released_cv.notify_all();
    }
}

impl Drop for WorkspaceLease {
    fn drop(&mut self) {
        self.release();
    }
}

impl WorkspaceLockTable {
    #[must_use]
    pub(super) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        lock(&self.inner.entries).is_empty()
    }

    /// Attempt a non-blocking acquisition for `path` in `mode` on behalf of
    /// `session_id`. Returns `None` if an incompatible lease is currently held
    /// on the exact path or any ancestor/descendant directory.
    #[must_use]
    pub(super) fn try_acquire(
        &self,
        path: &Path,
        mode: WorkspaceAccessMode,
        session_id: &str,
    ) -> Option<WorkspaceLease> {
        let key = normalize_workspace_key(path);
        let acquired = {
            let mut entries = lock(&self.inner.entries);
            let ok = if has_conflict(&entries, &key, mode) {
                false
            } else {
                let entry = entries.entry(key.clone()).or_default();
                match mode {
                    WorkspaceAccessMode::Write => {
                        entry.writer = Some(session_id.to_owned());
                    }
                    WorkspaceAccessMode::Read => {
                        entry.readers.push(session_id.to_owned());
                    }
                }
                true
            };
            drop(entries);
            ok
        };
        acquired.then(|| WorkspaceLease {
            table: self.clone(),
            key,
            mode,
            session_id: session_id.to_owned(),
            released: false,
        })
    }

    /// Non-mutating check used by queue pre-filtering to skip receipts whose
    /// workspace (or any ancestor/descendant directory) would contend,
    /// preventing head-of-line blocking and attempt count inflation.
    #[must_use]
    pub(super) fn is_locked_for(
        &self,
        path: &Path,
        mode: WorkspaceAccessMode,
    ) -> bool {
        let key = normalize_workspace_key(path);
        has_conflict(&lock(&self.inner.entries), &key, mode)
    }

    /// Distinct session IDs currently holding a lease that conflicts with
    /// acquiring `path` in `mode`.
    #[must_use]
    pub(super) fn conflicting_holders(
        &self,
        path: &Path,
        mode: WorkspaceAccessMode,
    ) -> Vec<String> {
        let key = normalize_workspace_key(path);
        let entries = lock(&self.inner.entries);
        let mut holders = BTreeSet::new();
        for (existing_key, entry) in entries.iter() {
            if !paths_overlap(&key, existing_key) {
                continue;
            }
            if let Some(writer) = &entry.writer {
                holders.insert(writer.clone());
            }
            if mode == WorkspaceAccessMode::Write {
                for reader in &entry.readers {
                    holders.insert(reader.clone());
                }
            }
        }
        drop(entries);
        holders.into_iter().collect()
    }

    /// Wait up to `timeout` on the release condvar until `path` has no
    /// conflicting leases for `mode`. Returns `true` if now conflict-free.
    #[must_use]
    pub(super) fn wait_for_release_step(
        &self,
        path: &Path,
        mode: WorkspaceAccessMode,
        timeout: Duration,
    ) -> bool {
        let key = normalize_workspace_key(path);
        let deadline = Instant::now() + timeout;
        let mut guard = lock(&self.inner.entries);
        while has_conflict(&guard, &key, mode) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(guard);
                return false;
            }
            let (next, _) = self
                .inner
                .released_cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
        drop(guard);
        true
    }
}

fn has_conflict(
    entries: &HashMap<String, LockEntry>,
    key: &str,
    mode: WorkspaceAccessMode,
) -> bool {
    entries.iter().any(|(existing_key, entry)| {
        paths_overlap(key, existing_key)
            && match mode {
                WorkspaceAccessMode::Write => {
                    entry.writer.is_some() || !entry.readers.is_empty()
                }
                WorkspaceAccessMode::Read => entry.writer.is_some(),
            }
    })
}

fn paths_overlap(a: &str, b: &str) -> bool {
    a == b || is_ancestor_key(a, b) || is_ancestor_key(b, a)
}

fn is_ancestor_key(ancestor: &str, descendant: &str) -> bool {
    if ancestor.is_empty()
        || descendant.len() <= ancestor.len()
        || !descendant.starts_with(ancestor)
    {
        return false;
    }
    #[cfg(windows)]
    let sep = b'\\';
    #[cfg(not(windows))]
    let sep = b'/';
    ancestor.as_bytes().last() == Some(&sep)
        || descendant.as_bytes().get(ancestor.len()) == Some(&sep)
}

/// Normalize a workspace directory path into a canonical map key so equivalent
/// spellings (trailing slashes, `.` / `..`, Windows casing, `\\?\` / `\\?\UNC\`
/// extended prefixes, symlinks/junctions, and not-yet-created subdirectories)
/// always map to the exact same lock bucket.
#[must_use]
pub(super) fn normalize_workspace_key(path: &Path) -> String {
    // Windows may canonicalize `base\.` to an 8.3 short path while resolving
    // `base` to its long spelling. Removing CurDir first keeps both inputs on
    // the same deepest-existing-ancestor path without lexically resolving `..`
    // across junctions or symlinks.
    let without_cur_dir = strip_cur_dir_components(path);
    let resolved = resolve_deepest_ancestor(&without_cur_dir);
    let folded = fold_lexical_components(&resolved);
    let raw = folded.to_string_lossy();
    #[cfg(windows)]
    {
        let lowered = raw.replace('/', "\\").to_lowercase();
        let mut normalized =
            if let Some(unc) = lowered.strip_prefix(r"\\?\unc\") {
                format!(r"\\{unc}")
            } else if let Some(rest) = lowered.strip_prefix(r"\\?\") {
                rest.to_owned()
            } else {
                lowered
            };
        while normalized.len() > 3 && normalized.ends_with('\\') {
            normalized.pop();
        }
        normalized
    }
    #[cfg(not(windows))]
    {
        let mut normalized = raw.into_owned();
        while normalized.len() > 1 && normalized.ends_with('/') {
            normalized.pop();
        }
        normalized
    }
}

fn strip_cur_dir_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        if !matches!(component, Component::CurDir) {
            out.push(component.as_os_str());
        }
    }
    out
}

fn fold_lexical_components(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last(), Some(Component::Normal(_))) {
                    parts.pop();
                } else {
                    parts.push(component);
                }
            }
            other => parts.push(other),
        }
    }
    let mut out = PathBuf::new();
    for part in parts {
        out.push(part.as_os_str());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::next_stamp;

    #[test]
    fn same_workspace_exclusive_writes_serialize() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-wlock-write-{}", next_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let table = WorkspaceLockTable::new();

        let lease_a = table
            .try_acquire(&dir, WorkspaceAccessMode::Write, "s-a")
            .expect("first writer acquires");
        assert!(table.is_locked_for(&dir, WorkspaceAccessMode::Write));
        assert!(table.is_locked_for(&dir, WorkspaceAccessMode::Read));
        assert!(
            table
                .try_acquire(&dir, WorkspaceAccessMode::Write, "s-b")
                .is_none(),
            "second writer on same workspace must be refused"
        );
        assert!(
            table
                .try_acquire(&dir, WorkspaceAccessMode::Read, "s-r")
                .is_none(),
            "reader must be refused while writer holds lock"
        );

        drop(lease_a);
        assert!(!table.is_locked_for(&dir, WorkspaceAccessMode::Write));
        let lease_b = table
            .try_acquire(&dir, WorkspaceAccessMode::Write, "s-b")
            .expect("second writer acquires after first drops");
        drop(lease_b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_workspace_concurrent_reads_succeed_and_block_writer() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-wlock-read-{}", next_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let table = WorkspaceLockTable::new();

        let r1 = table
            .try_acquire(&dir, WorkspaceAccessMode::Read, "s-r1")
            .expect("reader 1 acquires");
        let r2 = table
            .try_acquire(&dir, WorkspaceAccessMode::Read, "s-r2")
            .expect("reader 2 shares read lock");
        assert!(!table.is_locked_for(&dir, WorkspaceAccessMode::Read));
        assert!(table.is_locked_for(&dir, WorkspaceAccessMode::Write));
        assert!(
            table
                .try_acquire(&dir, WorkspaceAccessMode::Write, "s-w")
                .is_none(),
            "writer must wait until all readers release"
        );

        drop(r1);
        assert!(
            table.is_locked_for(&dir, WorkspaceAccessMode::Write),
            "remaining reader still blocks writer"
        );
        drop(r2);
        assert!(!table.is_locked_for(&dir, WorkspaceAccessMode::Write));
        let writer = table
            .try_acquire(&dir, WorkspaceAccessMode::Write, "s-w")
            .expect("writer acquires once all readers drop");
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_workspaces_acquire_concurrent_write_leases() {
        let base = std::env::temp_dir()
            .join(format!("flowy-wlock-diff-{}", next_stamp()));
        let dir_a = base.join("proj-a");
        let dir_b = base.join("proj-b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let table = WorkspaceLockTable::new();

        let lease_a = table
            .try_acquire(&dir_a, WorkspaceAccessMode::Write, "s-a")
            .expect("writer A acquires");
        let lease_b = table
            .try_acquire(&dir_b, WorkspaceAccessMode::Write, "s-b")
            .expect("writer B acquires independent directory concurrently");
        assert!(table.is_locked_for(&dir_a, WorkspaceAccessMode::Write));
        assert!(table.is_locked_for(&dir_b, WorkspaceAccessMode::Write));

        drop(lease_a);
        assert!(!table.is_locked_for(&dir_a, WorkspaceAccessMode::Write));
        assert!(table.is_locked_for(&dir_b, WorkspaceAccessMode::Write));
        drop(lease_b);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parent_and_child_workspaces_serialize_writes_while_sibling_prefixes_run_concurrently()
     {
        let base = std::env::temp_dir()
            .join(format!("flowy-wlock-nested-{}", next_stamp()));
        let parent = base.join("proj");
        let child = parent.join("packages").join("app");
        let sibling = base.join("proj-2");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let table = WorkspaceLockTable::new();

        // Parent write blocks child write & read, but NOT sibling `proj-2`.
        let parent_write = table
            .try_acquire(&parent, WorkspaceAccessMode::Write, "s-parent")
            .expect("parent writer");
        assert!(table.is_locked_for(&child, WorkspaceAccessMode::Write));
        assert!(table.is_locked_for(&child, WorkspaceAccessMode::Read));
        assert_eq!(
            table.conflicting_holders(&child, WorkspaceAccessMode::Write),
            vec!["s-parent".to_owned()]
        );
        assert!(
            table
                .try_acquire(&child, WorkspaceAccessMode::Write, "s-child")
                .is_none(),
            "child write must be blocked by parent write"
        );
        let sibling_write = table
            .try_acquire(&sibling, WorkspaceAccessMode::Write, "s-sib")
            .expect(
                "sibling prefix `proj-2` must not false-conflict with `proj`",
            );
        drop(sibling_write);
        drop(parent_write);

        // Symmetric: child write blocks parent write!
        let child_write = table
            .try_acquire(&child, WorkspaceAccessMode::Write, "s-child")
            .expect("child writer");
        assert!(table.is_locked_for(&parent, WorkspaceAccessMode::Write));
        assert!(
            table
                .try_acquire(&parent, WorkspaceAccessMode::Write, "s-parent")
                .is_none(),
            "parent write must be blocked by active child write"
        );
        drop(child_write);

        // Nested reads share cleanly and block nested writes until both drop.
        let r_parent = table
            .try_acquire(&parent, WorkspaceAccessMode::Read, "s-rp")
            .expect("parent read");
        let r_child = table
            .try_acquire(&child, WorkspaceAccessMode::Read, "s-rc")
            .expect("child read shares with parent read");
        assert!(
            table
                .try_acquire(&child, WorkspaceAccessMode::Write, "s-w")
                .is_none()
        );
        assert!(
            table
                .try_acquire(&parent, WorkspaceAccessMode::Write, "s-w")
                .is_none()
        );
        drop(r_parent);
        assert!(table.is_locked_for(&parent, WorkspaceAccessMode::Write));
        drop(r_child);
        assert!(!table.is_locked_for(&parent, WorkspaceAccessMode::Write));
        assert!(table.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn drop_releases_entry_even_when_unwinding_from_panic() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-wlock-panic-{}", next_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let table = WorkspaceLockTable::new();
        let table_clone = table.clone();
        let dir_clone = dir.clone();

        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _lease = table_clone
                    .try_acquire(&dir_clone, WorkspaceAccessMode::Write, "s-p")
                    .expect("acquire");
                panic!("simulated worker unwind");
            }));
        assert!(outcome.is_err());
        assert!(
            !table.is_locked_for(&dir, WorkspaceAccessMode::Write),
            "RAII Drop must release lease on panic unwind"
        );
        assert!(table.is_empty(), "released lock bucket must be pruned");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn equivalent_spellings_and_trailing_slashes_normalize_to_same_key() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-wlock-norm-{}", next_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let with_dot = dir.join(".");
        let with_parent = dir.join("sub").join("..");

        let key_plain = normalize_workspace_key(&dir);
        let key_dot = normalize_workspace_key(&with_dot);
        let key_parent = normalize_workspace_key(&with_parent);
        assert_eq!(key_plain, key_dot);
        assert_eq!(key_plain, key_parent);

        #[cfg(windows)]
        {
            let upper = PathBuf::from(format!(
                "{}\\",
                dir.to_string_lossy().to_uppercase().replace('\\', "/")
            ));
            let verbatim = PathBuf::from(format!(r"\\?\{}", dir.display()));
            assert_eq!(key_plain, normalize_workspace_key(&upper));
            assert_eq!(key_plain, normalize_workspace_key(&verbatim));
            // Case-insensitive `\\?\UNC\` / `\\?\unc\` normalization.
            let unc_upper = PathBuf::from(r"\\?\UNC\Server\Share\Repo\");
            let unc_lower = PathBuf::from(r"\\?\unc\server\share\repo");
            let unc_plain = PathBuf::from(r"\\SERVER\SHARE\Repo/");
            assert_eq!(
                normalize_workspace_key(&unc_upper),
                normalize_workspace_key(&unc_lower)
            );
            assert_eq!(
                normalize_workspace_key(&unc_upper),
                normalize_workspace_key(&unc_plain)
            );
            assert!(paths_overlap(r"c:\", r"c:\work\repo"));
            assert!(!paths_overlap(r"c:\", r"d:\work\repo"));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonexistent_subdirectories_resolve_through_deepest_existing_ancestor() {
        let base = std::env::temp_dir()
            .join(format!("flowy-wlock-ancestor-{}", next_stamp()));
        std::fs::create_dir_all(&base).unwrap();
        let future_a = base.join("future").join("nested");
        let future_b = base
            .join(".")
            .join("future")
            .join("x")
            .join("..")
            .join("nested");
        assert_eq!(
            normalize_workspace_key(&future_a),
            normalize_workspace_key(&future_b)
        );

        let table = WorkspaceLockTable::new();
        let lease = table
            .try_acquire(&future_a, WorkspaceAccessMode::Write, "s-1")
            .expect("acquire non-existent path");
        assert!(table.is_locked_for(&future_b, WorkspaceAccessMode::Write));
        // Creating the directory on disk afterwards must still match the same key.
        std::fs::create_dir_all(&future_a).unwrap();
        assert_eq!(
            normalize_workspace_key(&future_a),
            lease.key,
            "key before and after mkdir must remain identical"
        );
        drop(lease);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn nonexistent_dotdot_suffix_still_canonicalizes_ancestor_junction() {
        let base = std::env::temp_dir()
            .join(format!("flowy-wlock-junc-{}", next_stamp()));
        let real = base.join("real");
        let link = base.join("link");
        std::fs::create_dir_all(&real).unwrap();

        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .status()
            .expect("invoke cmd mklink /J");
        assert!(status.success(), "mklink /J must succeed");

        let link_future = link
            .join(".")
            .join("future")
            .join("x")
            .join("..")
            .join("nested");
        let real_future = real.join("future").join("nested");
        assert_eq!(
            normalize_workspace_key(&link_future),
            normalize_workspace_key(&real_future)
        );

        let table = WorkspaceLockTable::new();
        let _lease = table
            .try_acquire(&real_future, WorkspaceAccessMode::Write, "s-real")
            .expect("acquire real future path");
        assert!(
            table.is_locked_for(&link_future, WorkspaceAccessMode::Write),
            "dotdot suffix under ancestor junction must contend with canonical target"
        );

        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nonexistent_dotdot_back_to_existing_sibling_re_enables_canonicalization()
    {
        let base = std::env::temp_dir()
            .join(format!("flowy-wlock-reenter-{}", next_stamp()));
        let sub_real = base.join("sub_real");
        std::fs::create_dir_all(&sub_real).unwrap();

        #[cfg(windows)]
        let sub_alias = {
            let alias = base.join("sub_link");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&alias)
                .arg(&sub_real)
                .status()
                .expect("invoke cmd mklink /J");
            assert!(status.success(), "mklink /J must succeed");
            alias
        };
        #[cfg(not(windows))]
        let sub_alias = sub_real.clone();

        let reentered = base
            .join("nonexistent")
            .join("..")
            .join(sub_alias.file_name().unwrap());
        assert_eq!(
            normalize_workspace_key(&reentered),
            normalize_workspace_key(&sub_real)
        );

        #[cfg(windows)]
        let _ = std::fs::remove_dir(&sub_alias);
        let _ = std::fs::remove_dir_all(&base);
    }
}
