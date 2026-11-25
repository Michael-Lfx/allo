//! UI-side extensible adapter ports, dirty-cached board snapshots, local
//! environment checks, and redacted trace projections.
//!
//! Boundary invariant: this module only reads through `flowy_agent::AgentHandle`
//! public APIs and `crate::projects::Project`. It never modifies kernel crates
//! or parses kernel disk files (`sessions/*.json`, `delivery/*.jsonl`,
//! `acceptance/journal.jsonl`) directly.

mod board;
mod local_env;
mod trace;

use std::path::{Component, Path, PathBuf, Prefix};

pub use board::{
    BoardDataSource, BoardKpiSummary, BranchPhase, DeliverableFormat,
    SessionBoardGroup, SkillChip, SkillLabel, UNASSIGNED_QUEUE_SESSION_ID,
    WorkBoardSnapshot, WorkBranchSummary, WorkContractProgress,
};
pub use local_env::{
    EnvSeverity, LocalEnvCheck, LocalEnvDetail, LocalEnvSnapshot,
    probe_local_env,
};
pub use trace::{
    RedactedTraceEntry, TraceLevel, collect_session_traces,
    format_traces_jsonl, redact_trace_text,
};

/// Strips Windows extended-length path prefixes (`\\?\` / `\\?\UNC\`) for clean
/// UI rendering across breadcrumbs, composer chips, approval cards, and drawers.
#[must_use]
pub fn display_path(path: &Path) -> String {
    display_path_str(&path.to_string_lossy())
}

/// String variant of [`display_path`] for persisted artifact or tool paths.
#[must_use]
pub fn display_path_str(raw: &str) -> String {
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_owned()
    } else {
        raw.to_owned()
    }
}

/// Last few characters of an id, by **character** (never by byte) so
/// human-edited UTF-8 journal ids cannot panic the UI.
#[must_use]
pub(crate) fn short_id(id: &str) -> String {
    let count = id.chars().count();
    id.chars().skip(count.saturating_sub(8)).collect()
}

/// Rejects relative paths, NUL bytes, URI schemes, UNC/verbatim-UNC/device/
/// non-disk verbatim namespace prefixes, and Windows reserved device names
/// BEFORE any filesystem syscall (preventing SMB/NTLM credential leaks,
/// `\\.\PIPE` hangs, and `ShellExecute` protocol dispatch).
#[must_use]
pub(crate) fn is_unsafe_local_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() || raw.contains('\0') {
        return true;
    }
    if raw.contains("://") {
        return true;
    }
    // Block UNC, device, and non-disk verbatim namespaces (`\\?\unc\...`,
    // `\\?\PIPE\...`, `\\?\GLOBALROOT\...`, `\\?\Volume{...}\...`) on raw
    // slashes BEFORE any syscall.
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        let is_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        if !is_drive {
            return true;
        }
    } else if raw.starts_with(r"\\") || raw.starts_with("//") {
        return true;
    }
    if !path.is_absolute() {
        return true;
    }
    if flowy_agent::has_reserved_device_component(path) {
        return true;
    }
    path.components().any(|component| match component {
        Component::Prefix(prefix) => {
            !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        }
        _ => false,
    })
}

/// Strips trailing path separators while preserving filesystem roots (`/` and
/// Windows drive roots like `C:\` / `\\?\C:\`), so a trailing slash on a
/// Windows directory junction (`C:\base\junction\`) cannot bypass leaf reparse
/// inspection.
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

#[cfg(windows)]
fn is_unsafe_reparse_target(target: &Path) -> bool {
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
        return true;
    }
    if let Some(rest) = raw
        .strip_prefix(r"\??\")
        .or_else(|| raw.strip_prefix(r"\\?\"))
    {
        let bytes = rest.as_bytes();
        let is_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        if !is_drive {
            return true;
        }
        return is_unsafe_local_path(Path::new(rest));
    }
    if raw.starts_with(r"\\") {
        return true;
    }
    flowy_agent::has_reserved_device_component(target)
        || target.components().any(|component| match component {
            Component::Prefix(prefix) => !matches!(
                prefix.kind(),
                Prefix::Disk(_) | Prefix::VerbatimDisk(_)
            ),
            _ => false,
        })
}

/// Walks ancestor directories on Windows BEFORE inspecting the leaf or calling
/// `std::fs::canonicalize`: reads any ancestor reparse target locally via
/// `std::fs::read_link` (no outbound SMB probe) and rejects targets pointing
/// to UNC/device/non-local namespaces while allowing legitimate local folder
/// redirections (`AppData\Roaming\Flowy`, `OneDrive`, local NTFS junctions).
#[cfg(windows)]
fn has_unsafe_ancestor_reparse(path: &Path) -> bool {
    for ancestor in path.ancestors().skip(1) {
        if ancestor.parent().is_none() {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(ancestor) else {
            return true;
        };
        if flowy_agent::is_windows_reparse_point(&meta) {
            match std::fs::read_link(ancestor) {
                Ok(target) => {
                    if is_unsafe_reparse_target(&target) {
                        return true;
                    }
                }
                Err(_) => {
                    // Non-link cloud placeholder reparse tags (e.g. OneDrive)
                    // are not readable by `read_link`; allow them here and
                    // still enforce canonical validation below. Any actual
                    // symlink whose target cannot be read is rejected.
                    if meta.file_type().is_symlink() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn strip_windows_verbatim_local(path: &Path) -> Option<PathBuf> {
    let raw = path.to_string_lossy();
    if raw.to_ascii_lowercase().starts_with(r"\\?\unc\") {
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

fn resolve_safe_local(path: &Path, expect_dir: bool) -> Option<PathBuf> {
    if is_unsafe_local_path(path) {
        return None;
    }
    let normalized = trim_trailing_separators(path);
    if is_unsafe_local_path(&normalized) {
        return None;
    }
    #[cfg(windows)]
    if has_unsafe_ancestor_reparse(&normalized) {
        return None;
    }
    let leaf_meta = std::fs::symlink_metadata(&normalized).ok()?;
    if leaf_meta.file_type().is_symlink()
        || flowy_agent::is_windows_reparse_point(&leaf_meta)
    {
        return None;
    }
    if expect_dir {
        if !leaf_meta.is_dir() {
            return None;
        }
    } else if !leaf_meta.is_file() {
        return None;
    }
    let canon = std::fs::canonicalize(&normalized).ok()?;
    let safe = strip_windows_verbatim_local(&canon)?;
    let canon_meta = std::fs::symlink_metadata(&safe).ok()?;
    if canon_meta.file_type().is_symlink()
        || flowy_agent::is_windows_reparse_point(&canon_meta)
    {
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
/// unsafe ancestor reparse targets, and leaf symlinks / NTFS reparse points.
#[must_use]
pub(crate) fn resolve_safe_local_file(raw: &str) -> Option<PathBuf> {
    resolve_safe_local(Path::new(raw), false)
}

/// Validates and canonicalizes a local directory path for OS reveal or project
/// registration, rejecting executables, URIs, UNC shares, device namespaces,
/// unsafe ancestor reparse targets, and leaf symlinks / NTFS reparse points.
#[must_use]
pub(crate) fn resolve_safe_local_dir(path: &Path) -> Option<PathBuf> {
    resolve_safe_local(path, true)
}

/// Resolves the validated parent directory of a file path (rejecting lexical
/// tricks like `C:\work\payload.exe\x` where `parent()` is an executable).
#[must_use]
pub(crate) fn resolve_safe_parent_dir(file_path: &str) -> Option<PathBuf> {
    let path = Path::new(file_path);
    if is_unsafe_local_path(path) {
        return None;
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty())?;
    resolve_safe_local_dir(parent)
}

/// Returns `true` when `candidate` is equal to, nested inside, or an ancestor
/// of `protected` (e.g. preventing `store_root`, `store_root/sessions/cwd`, or
/// a drive root containing `store_root` from being promoted to a user project).
#[must_use]
pub(crate) fn is_same_or_overlapping_dir(
    candidate: &Path,
    protected: &Path,
) -> bool {
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

#[must_use]
pub(crate) fn basename(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

pub(super) fn clip_label(text: &str, max_chars: usize) -> String {
    let collapsed: String =
        text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}\u{2026}")
    } else {
        head
    }
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
        assert_eq!(display_path_str("/tmp/demo.docx"), "/tmp/demo.docx");
    }

    #[test]
    fn short_id_never_splits_a_character() {
        assert_eq!(short_id("accepted-1a208c0bc80-0"), "c0bc80-0");
        assert_eq!(short_id("short"), "short");
        assert_eq!(short_id(""), "");
        assert_eq!(short_id("中文中文"), "中文中文");
        assert_eq!(short_id("排队中排队中中文测试"), "中排队中中文测试");
        assert_eq!(short_id("短"), "短");
    }

    #[test]
    fn rejects_unsafe_raw_paths_and_canonical_unc_targets() {
        for raw in [
            "",
            "relative/report.txt",
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
            .is_none(),
            "canonical UNC targets must remain blocked"
        );
        assert!(
            strip_windows_verbatim_local(Path::new(
                r"\\?\unc\fileserver\share\report.txt"
            ))
            .is_none(),
            "lowercase canonical UNC targets must remain blocked"
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
    fn windows_reparse_target_guard_blocks_unc_and_device_targets() {
        for bad in [
            r"\??\UNC\fileserver\share",
            r"\\?\unc\fileserver\share",
            r"\\fileserver\share",
            r"\\.\PIPE\flowy",
            r"\??\PIPE\flowy",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1",
            r"C:\tmp\CON",
        ] {
            assert!(
                is_unsafe_reparse_target(Path::new(bad)),
                "expected reparse target rejection for {bad:?}"
            );
        }
        for good in [r"C:\Users\alice\AppData", r"\??\C:\Users\alice\OneDrive"]
        {
            assert!(
                !is_unsafe_reparse_target(Path::new(good)),
                "expected local reparse target to be allowed: {good:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_allows_redirected_ancestor_junction_and_rejects_leaf_junction() {
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
        let file = nested.join("report.txt");
        let junction = base.join("redirected");

        std::fs::create_dir_all(&nested).expect("create test dirs");
        std::fs::write(&file, b"ok").expect("create test file");

        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&real_root)
            .output()
            .expect("invoke mklink /J");
        assert!(
            status.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let via_junction_dir = junction.join("nested");
        let via_junction_file = via_junction_dir.join("report.txt");
        let via_junction_file_str = via_junction_file.to_string_lossy();

        // Regular dir and file nested under a Windows directory junction
        // ancestor must resolve cleanly (mirrors AppData/OneDrive redirection).
        assert!(
            resolve_safe_local_dir(&via_junction_dir).is_some(),
            "regular directory inside a Windows junction ancestor must open"
        );
        assert!(
            resolve_safe_local_file(&via_junction_file_str).is_some(),
            "regular file inside a Windows junction ancestor must open"
        );
        assert!(
            resolve_safe_parent_dir(&via_junction_file_str).is_some(),
            "parent directory inside a Windows junction ancestor must open"
        );

        // The junction itself as a leaf target stays strictly blocked —
        // including with trailing slashes.
        assert!(
            resolve_safe_local_dir(&junction).is_none(),
            "leaf NTFS junction must still be rejected"
        );
        let junction_trailing_backslash =
            PathBuf::from(format!("{}\\", junction.display()));
        let junction_trailing_slash =
            PathBuf::from(format!("{}/", junction.display()));
        assert!(
            resolve_safe_local_dir(&junction_trailing_backslash).is_none(),
            "trailing backslash must not bypass leaf junction rejection"
        );
        assert!(
            resolve_safe_local_dir(&junction_trailing_slash).is_none(),
            "trailing slash must not bypass leaf junction rejection"
        );

        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir_all(&base);
    }
}
