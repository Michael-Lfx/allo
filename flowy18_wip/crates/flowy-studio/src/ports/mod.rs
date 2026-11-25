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

/// Rejects relative paths, NUL bytes, URI schemes, UNC/verbatim-UNC/device
/// namespace prefixes, and Windows reserved device names BEFORE any filesystem
/// syscall (preventing SMB/NTLM credential leaks, `\\.\PIPE` hangs, and
/// `ShellExecute` protocol dispatch).
#[must_use]
pub(crate) fn is_unsafe_local_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() || raw.contains('\0') {
        return true;
    }
    if raw.contains("://") {
        return true;
    }
    // Block UNC and Windows device namespaces even on raw slashes before parsing.
    if raw.starts_with(r"\\") && !raw.starts_with(r"\\?\") {
        return true;
    }
    if raw.starts_with(r"\\?\UNC\") || raw.starts_with("//") {
        return true;
    }
    if !path.is_absolute() {
        return true;
    }
    if flowy_agent::has_reserved_device_component(path) {
        return true;
    }
    path.components().any(|component| match component {
        Component::Prefix(prefix) => matches!(
            prefix.kind(),
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(_)
        ),
        _ => false,
    })
}

fn has_symlink_or_reparse_chain(path: &Path) -> bool {
    let Ok(leaf_meta) = std::fs::symlink_metadata(path) else {
        return true;
    };
    if leaf_meta.file_type().is_symlink()
        || flowy_agent::is_windows_reparse_point(&leaf_meta)
    {
        return true;
    }
    #[cfg(windows)]
    {
        for ancestor in path.ancestors().skip(1) {
            if ancestor.parent().is_none() {
                break;
            }
            let Ok(meta) = std::fs::symlink_metadata(ancestor) else {
                return true;
            };
            if meta.file_type().is_symlink()
                || flowy_agent::is_windows_reparse_point(&meta)
            {
                return true;
            }
        }
    }
    false
}

fn strip_windows_verbatim_local(path: &Path) -> Option<PathBuf> {
    let raw = path.to_string_lossy();
    if raw.starts_with(r"\\?\UNC\") {
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

/// Validates and canonicalizes a local regular file path for external open or
/// preview, rejecting UNC shares, device namespaces, URLs, relative paths, and
/// symlinks / NTFS reparse points.
#[must_use]
pub(crate) fn resolve_safe_local_file(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    if is_unsafe_local_path(path) {
        return None;
    }
    if has_symlink_or_reparse_chain(path) {
        return None;
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let canon = std::fs::canonicalize(path).ok()?;
    strip_windows_verbatim_local(&canon)
}

/// Validates and canonicalizes a local directory path for OS reveal or project
/// registration, rejecting executables, URIs, UNC shares, device namespaces,
/// and symlinks / NTFS reparse points.
#[must_use]
pub(crate) fn resolve_safe_local_dir(path: &Path) -> Option<PathBuf> {
    if is_unsafe_local_path(path) {
        return None;
    }
    if has_symlink_or_reparse_chain(path) {
        return None;
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let canon = std::fs::canonicalize(path).ok()?;
    strip_windows_verbatim_local(&canon)
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
}
