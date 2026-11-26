//! Local filesystem path validation, top-down Windows reparse-point safety,
//! and UI path display helpers.
//!
//! Security invariants:
//! - Rejects UNC, verbatim-UNC, device (`\\.\`), non-disk verbatim namespaces,
//!   URLs, relative paths (including drive-relative `C:foo`), and Windows
//!   reserved device names BEFORE any filesystem syscall.
//! - Walks ancestor directories **top-down** (root to leaf) with bounded
//!   reparse hop resolution (`MAX_REPARSE_HOPS = 16`), so an ancestor junction
//!   or chained junction pointing at `\\attacker\share` is caught via local
//!   `read_link` before the OS can ever trigger an outbound SMB/NTLM probe.
//! - Distinguishes link reparse points (symlinks & NTFS junctions readable by
//!   `read_link`) from non-link cloud placeholders (Windows `OneDrive`), keeping
//!   leaf symlinks/junctions blocked while allowing `OneDrive` files/folders.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf, Prefix};

const MAX_REPARSE_HOPS: usize = 16;

/// Strips Windows extended-length path prefixes (`\\?\` / `\\?\UNC\`) for clean
/// UI rendering across breadcrumbs, composer chips, approval cards, and drawers.
#[must_use]
pub fn display_path(path: &Path) -> String {
    display_path_str(&path.to_string_lossy())
}

fn strip_windows_unc_prefix(raw: &str) -> Option<&str> {
    let prefix = raw.get(..8)?;
    prefix.eq_ignore_ascii_case(r"\\?\unc\").then(|| &raw[8..])
}

/// String variant of [`display_path`] for persisted artifact or tool paths.
#[must_use]
pub fn display_path_str(raw: &str) -> String {
    if let Some(unc_tail) = strip_windows_unc_prefix(raw) {
        format!(r"\\{unc_tail}")
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_owned()
    } else {
        raw.to_owned()
    }
}

/// Rejects relative paths (including Windows drive-relative `C:foo`), NUL
/// bytes, URI schemes, UNC/verbatim-UNC/device/non-disk verbatim namespace
/// prefixes, and Windows reserved device names BEFORE any filesystem syscall.
#[must_use]
pub fn is_unsafe_local_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() || raw.contains('\0') || raw.contains("://") {
        return true;
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        if !has_disk_drive_root(rest) {
            return true;
        }
    } else if raw.starts_with(r"\\") || raw.starts_with("//") {
        return true;
    }
    if !path.is_absolute() || flowy_agent::has_reserved_device_component(path) {
        return true;
    }
    let mut saw_disk_prefix = false;
    let mut saw_root_after_prefix = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                if !matches!(
                    prefix.kind(),
                    Prefix::Disk(_) | Prefix::VerbatimDisk(_)
                ) {
                    return true;
                }
                saw_disk_prefix = true;
            }
            Component::RootDir if saw_disk_prefix => {
                saw_root_after_prefix = true;
            }
            _ => {}
        }
    }
    saw_disk_prefix && !saw_root_after_prefix
}

const fn has_disk_drive_root(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// Strips trailing path separators while preserving filesystem roots (`/` and
/// Windows drive roots like `C:\` / `\\?\C:\`), so a trailing slash on a
/// Windows directory junction (`C:\base\junction\`) cannot bypass leaf checks.
fn trim_trailing_separators(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return path.to_path_buf();
    }
    if trimmed.ends_with(':') {
        return PathBuf::from(format!("{trimmed}\\"));
    }
    PathBuf::from(trimmed)
}

#[derive(Debug)]
enum ReparseKind {
    Regular,
    CloudPlaceholder,
    Link(PathBuf),
    Unsafe,
}

fn classify_reparse(path: &Path, meta: &std::fs::Metadata) -> ReparseKind {
    let is_symlink = meta.file_type().is_symlink();
    #[cfg(windows)]
    let is_reparse = is_symlink || flowy_agent::is_windows_reparse_point(meta);
    #[cfg(not(windows))]
    let is_reparse = is_symlink;

    if !is_reparse {
        return ReparseKind::Regular;
    }
    match std::fs::read_link(path) {
        Ok(target) => ReparseKind::Link(target),
        Err(err) => {
            if is_symlink || err.kind() == std::io::ErrorKind::PermissionDenied
            {
                ReparseKind::Unsafe
            } else {
                // Non-link cloud placeholder reparse tags (e.g. OneDrive) are
                // not readable by `read_link` and remain regular files/dirs.
                ReparseKind::CloudPlaceholder
            }
        }
    }
}

/// Returns `true` when `path` is a symlink, readable NTFS junction/mount point,
/// or unreadable/permission-denied symlink (allowing `OneDrive` cloud
/// placeholders through).
#[must_use]
pub fn is_link_or_unsafe_reparse(
    path: &Path,
    meta: &std::fs::Metadata,
) -> bool {
    matches!(
        classify_reparse(path, meta),
        ReparseKind::Link(_) | ReparseKind::Unsafe
    )
}

fn normalize_reparse_target(
    link_path: &Path,
    target: &Path,
) -> Option<PathBuf> {
    let raw = target.to_string_lossy();
    let lower = raw.to_ascii_lowercase();
    if raw.is_empty()
        || raw.contains('\0')
        || raw.contains("://")
        || lower.starts_with(r"\??\unc\")
        || lower.starts_with(r"\\?\unc\")
        || lower.starts_with(r"\\.\")
        || raw.starts_with("//")
    {
        return None;
    }
    if let Some(rest) = raw
        .strip_prefix(r"\??\")
        .or_else(|| raw.strip_prefix(r"\\?\"))
    {
        if !has_disk_drive_root(rest) {
            return None;
        }
        let local = PathBuf::from(rest);
        return (!is_unsafe_local_path(&local)).then_some(local);
    }
    if raw.starts_with(r"\\") {
        return None;
    }
    if flowy_agent::has_reserved_device_component(target) {
        return None;
    }
    if target.is_absolute() {
        return (!is_unsafe_local_path(target)).then(|| target.to_path_buf());
    }
    // Reject Windows drive-relative targets like `C:foo`.
    if target
        .components()
        .any(|c| matches!(c, Component::Prefix(_)))
    {
        return None;
    }
    let parent = link_path.parent()?;
    let joined = parent.join(target);
    (!is_unsafe_local_path(&joined)).then_some(joined)
}

fn strip_windows_verbatim_local(path: &Path) -> Option<PathBuf> {
    let raw = path.to_string_lossy();
    if strip_windows_unc_prefix(&raw).is_some() {
        return None;
    }
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        let local = PathBuf::from(stripped);
        (!is_unsafe_local_path(&local)).then_some(local)
    } else if !is_unsafe_local_path(path) {
        Some(path.to_path_buf())
    } else {
        None
    }
}

/// Walks `path` strictly top-down (root -> leaf), following safe local
/// ancestor junctions/symlinks up to `MAX_REPARSE_HOPS` while blocking UNC /
/// device / chained-UNC targets before any OS traversal can probe the network,
/// and strictly rejecting leaf symlinks / leaf junctions.
fn resolve_safe_local(path: &Path, expect_dir: bool) -> Option<PathBuf> {
    if is_unsafe_local_path(path) {
        return None;
    }
    let normalized = trim_trailing_separators(path);
    if is_unsafe_local_path(&normalized) {
        return None;
    }

    let mut pending: VecDeque<PathBuf> = normalized
        .components()
        .map(|c| PathBuf::from(c.as_os_str()))
        .collect();
    let mut resolved = PathBuf::new();
    let mut hops: usize = 0;

    while let Some(part) = pending.pop_front() {
        match part.components().next()? {
            Component::Prefix(_) | Component::RootDir => {
                resolved.push(&part);
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                if !resolved.pop() {
                    return None;
                }
                continue;
            }
            Component::Normal(name) => {
                if flowy_agent::has_reserved_device_component(Path::new(name)) {
                    return None;
                }
                resolved.push(name);
            }
        }

        let meta = std::fs::symlink_metadata(&resolved).ok()?;
        let is_leaf = pending.is_empty();
        match classify_reparse(&resolved, &meta) {
            ReparseKind::Unsafe => return None,
            ReparseKind::Link(target) => {
                if is_leaf {
                    return None;
                }
                hops += 1;
                if hops > MAX_REPARSE_HOPS {
                    return None;
                }
                let next_base = normalize_reparse_target(&resolved, &target)?;
                let mut next_parts: VecDeque<PathBuf> = next_base
                    .components()
                    .map(|c| PathBuf::from(c.as_os_str()))
                    .collect();
                while let Some(tail) = pending.pop_front() {
                    next_parts.push_back(tail);
                }
                pending = next_parts;
                resolved.clear();
            }
            ReparseKind::Regular | ReparseKind::CloudPlaceholder => {
                if is_leaf {
                    if expect_dir && !meta.is_dir() {
                        return None;
                    }
                    if !expect_dir && !meta.is_file() {
                        return None;
                    }
                }
            }
        }
    }

    if resolved.as_os_str().is_empty() {
        return None;
    }
    let canon = std::fs::canonicalize(&resolved).ok()?;
    let safe = strip_windows_verbatim_local(&canon)?;
    // Inspect `&canon` (extended-length safe on Windows >260 chars) to close
    // TOCTOU swaps between walk and canonicalize.
    let canon_meta = std::fs::symlink_metadata(&canon).ok()?;
    if is_link_or_unsafe_reparse(&canon, &canon_meta) {
        return None;
    }
    if expect_dir {
        canon_meta.is_dir().then_some(safe)
    } else {
        canon_meta.is_file().then_some(safe)
    }
}

/// Validates and canonicalizes a local regular file path for external open or
/// preview, rejecting UNC shares, device namespaces, URLs, relative paths,
/// unsafe ancestor reparse targets, and leaf symlinks / NTFS junctions.
#[must_use]
pub fn resolve_safe_local_file(raw: &str) -> Option<PathBuf> {
    resolve_safe_local(Path::new(raw), false)
}

/// Validates and canonicalizes a local directory path for OS reveal or project
/// registration, rejecting executables, URIs, UNC shares, device namespaces,
/// unsafe ancestor reparse targets, and leaf symlinks / NTFS junctions.
#[must_use]
pub fn resolve_safe_local_dir(path: &Path) -> Option<PathBuf> {
    resolve_safe_local(path, true)
}

/// Resolves the validated parent directory of a file path (rejecting lexical
/// tricks like `C:\work\payload.exe\x` while allowing files located directly
/// inside a redirected local junction by resolving the canonical file first).
#[must_use]
pub fn resolve_safe_parent_dir(file_path: &str) -> Option<PathBuf> {
    if let Some(safe_file) = resolve_safe_local_file(file_path) {
        let parent =
            safe_file.parent().filter(|p| !p.as_os_str().is_empty())?;
        return resolve_safe_local_dir(parent);
    }
    let path = Path::new(file_path);
    if is_unsafe_local_path(path) {
        return None;
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty())?;
    resolve_safe_local_dir(parent)
}

/// Returns `true` when `candidate` is equal to, nested inside, or an ancestor
/// of `protected`.
#[must_use]
pub fn is_same_or_overlapping_dir(candidate: &Path, protected: &Path) -> bool {
    let norm = |p: &Path| {
        let canon =
            std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let clean = display_path(&canon);
        if cfg!(windows) {
            PathBuf::from(clean.to_ascii_lowercase())
        } else {
            PathBuf::from(clean)
        }
    };
    let a = norm(candidate);
    let b = norm(protected);
    a.starts_with(&b) || b.starts_with(&a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_strips_windows_extended_prefixes() {
        assert_eq!(
            display_path_str(r"\\?\C:\Users\RiKa\Docs\Q3.xlsx"),
            r"C:\Users\RiKa\Docs\Q3.xlsx"
        );
        assert_eq!(
            display_path_str(r"\\?\UNC\fileserver\share\report.pptx"),
            r"\\fileserver\share\report.pptx"
        );
        assert_eq!(
            display_path_str(r"\\?\unc\fileserver\share\report.pptx"),
            r"\\fileserver\share\report.pptx"
        );
        assert_eq!(display_path_str("/tmp/demo.docx"), "/tmp/demo.docx");
    }

    #[test]
    fn rejects_unsafe_raw_paths_and_canonical_unc_targets() {
        for raw in [
            "",
            "relative/report.txt",
            r"C:relative.txt",
            r"\\fileserver\share\report.txt",
            r"\\?\UNC\fileserver\share\report.txt",
            r"\\?\unc\fileserver\share\report.txt",
            r"\\?\PIPE\flowy",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\a.txt",
            r"\\.\PIPE\flowy",
            "//fileserver/share/report.txt",
            "file:///C:/Users/alice/report.txt",
            "https://example.com/report.txt",
            r"C:\tmp\CON",
            r"C:\tmp\NUL.txt",
        ] {
            assert!(
                is_unsafe_local_path(Path::new(raw)),
                "expected unsafe path rejection for {raw:?}"
            );
            assert!(resolve_safe_local_file(raw).is_none());
            assert!(resolve_safe_local_dir(Path::new(raw)).is_none());
            assert!(resolve_safe_parent_dir(raw).is_none());
        }
        assert!(
            strip_windows_verbatim_local(Path::new(
                r"\\?\UNC\fileserver\share\report.txt"
            ))
            .is_none()
        );
        assert!(
            strip_windows_verbatim_local(Path::new(
                r"\\?\unc\fileserver\share\report.txt"
            ))
            .is_none()
        );
        assert_eq!(
            trim_trailing_separators(Path::new(r"C:\")),
            PathBuf::from(r"C:\")
        );
        assert_eq!(
            trim_trailing_separators(Path::new(r"C:\base\junction\")),
            PathBuf::from(r"C:\base\junction")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_target_guard_blocks_unc_device_and_drive_relative() {
        let dummy = Path::new(r"C:\base\link");
        for bad in [
            r"\??\UNC\fileserver\share",
            r"\\?\unc\fileserver\share",
            r"\\fileserver\share",
            r"\\.\PIPE\flowy",
            r"\??\PIPE\flowy",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1",
            r"C:\tmp\CON",
            r"C:relative_dir",
        ] {
            assert!(
                normalize_reparse_target(dummy, Path::new(bad)).is_none(),
                "expected reparse target rejection for {bad:?}"
            );
        }
        for good in [r"C:\Users\alice\AppData", r"\??\C:\Users\alice\OneDrive"]
        {
            assert!(
                normalize_reparse_target(dummy, Path::new(good)).is_some(),
                "expected local reparse target to be allowed: {good:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_top_down_junction_allows_direct_parent_and_chained_local_and_blocks_leaf()
     {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "flowy-reparse-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        let real_root = base.join("real_root");
        let nested = real_root.join("nested");
        let direct_file = real_root.join("direct.txt");
        let nested_file = nested.join("report.txt");
        let junction = base.join("redirected");
        let chained = base.join("chained");

        std::fs::create_dir_all(&nested).expect("create test dirs");
        std::fs::write(&direct_file, b"direct").expect("write direct file");
        std::fs::write(&nested_file, b"nested").expect("write nested file");

        for (link, target) in [(&junction, &real_root), (&chained, &junction)] {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .expect("invoke mklink /J");
            assert!(
                status.status.success(),
                "mklink /J failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }

        let via_junction_dir = junction.join("nested");
        let via_junction_file = via_junction_dir.join("report.txt");
        let direct_via_junction = junction.join("direct.txt");
        let chained_file = chained.join("nested").join("report.txt");

        assert!(resolve_safe_local_dir(&via_junction_dir).is_some());
        assert!(
            resolve_safe_local_file(&via_junction_file.to_string_lossy())
                .is_some()
        );
        assert!(
            resolve_safe_parent_dir(&via_junction_file.to_string_lossy())
                .is_some()
        );
        // Direct child of a redirected local junction must also resolve its
        // canonical parent directory for OS folder reveal.
        assert!(
            resolve_safe_parent_dir(&direct_via_junction.to_string_lossy())
                .is_some(),
            "parent dir of file directly inside a local junction must resolve"
        );
        // Chained local junctions resolve top-down cleanly.
        assert!(
            resolve_safe_local_file(&chained_file.to_string_lossy()).is_some(),
            "chained local junction file must resolve"
        );

        // Leaf junction stays strictly blocked, including with trailing slashes.
        assert!(resolve_safe_local_dir(&junction).is_none());
        assert!(
            resolve_safe_local_dir(&PathBuf::from(format!(
                "{}\\",
                junction.display()
            )))
            .is_none()
        );
        assert!(
            resolve_safe_local_dir(&PathBuf::from(format!(
                "{}/",
                junction.display()
            )))
            .is_none()
        );

        let _ = std::fs::remove_dir(&chained);
        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn multibyte_utf8_paths_do_not_panic_on_unc_prefix_checks() {
        assert_eq!(display_path_str(r"C:\项目\报告.md"), r"C:\项目\报告.md");
        assert_eq!(
            display_path_str(r"\\?\C:\项目\报告.md"),
            r"C:\项目\报告.md"
        );
        assert_eq!(
            display_path_str(r"\\?\UNC\server\共享\报告.md"),
            r"\\server\共享\报告.md"
        );
        assert_eq!(
            display_path_str(r"\\?\unc\server\共享\报告.md"),
            r"\\server\共享\报告.md"
        );
        assert_eq!(display_path_str("报告"), "报告");
        assert_eq!(
            display_path(Path::new(r"C:\项目\报告.md")),
            r"C:\项目\报告.md"
        );

        assert_eq!(
            strip_windows_verbatim_local(Path::new(r"C:\项目\报告.md")),
            Some(PathBuf::from(r"C:\项目\报告.md"))
        );
        assert_eq!(
            strip_windows_verbatim_local(Path::new(r"\\?\C:\项目\报告.md")),
            Some(PathBuf::from(r"C:\项目\报告.md"))
        );
        assert!(
            strip_windows_verbatim_local(Path::new(
                r"\\?\UNC\server\共享\报告.md"
            ))
            .is_none()
        );
        assert!(
            strip_windows_verbatim_local(Path::new(
                r"\\?\unc\server\共享\报告.md"
            ))
            .is_none()
        );
        assert!(strip_windows_verbatim_local(Path::new("报告")).is_none());
    }
}
