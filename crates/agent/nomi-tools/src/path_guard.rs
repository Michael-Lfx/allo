//! Write-root containment guard (design §3.6 "写根包含校验").
//!
//! An **opt-in** guardrail: when a write root is configured, the file-mutating
//! tools (Write / Edit / ApplyPatch) refuse to write outside it. Default is no
//! root → no containment, so existing behaviour is byte-for-byte unchanged.
//!
//! # Threat model (honest scope)
//!
//! This stops *accidental or buggy* out-of-workspace writes (a bad absolute
//! path, a `../../` traversal, or a symlink that escapes the root). It is **not**
//! a security sandbox against a determined agent: the same agent has `Bash`, so
//! a real boundary needs OS-level confinement (macOS Seatbelt / Linux
//! namespaces), which is a separate, runtime-verified piece. Scoping it this way
//! avoids a false sense of safety.
//!
//! # Symlink correctness
//!
//! Containment is checked against the **canonicalised** path, not the textual
//! one: we resolve the longest existing ancestor (which collapses `..` and
//! follows symlinks) and re-append the not-yet-existing tail. A symlink inside
//! the root that points outside therefore resolves outside and is rejected —
//! textual `starts_with` alone would be fooled by it.

use std::path::{Path, PathBuf};

/// Resolve `path` for containment checking: canonicalise the longest existing
/// ancestor (resolving symlinks and `..`), then re-append the remaining
/// not-yet-existing components. Returns `None` if no ancestor exists or the
/// path has no components.
fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    // Fast path: the whole path exists (existing file or dir).
    if let Ok(c) = path.canonicalize() {
        return Some(c);
    }
    // Walk up to the nearest existing ancestor, canonicalise it, then re-attach
    // the trailing components that do not exist yet.
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path;
    loop {
        let parent = cur.parent()?;
        tail.push(cur.file_name()?.to_os_string());
        if let Ok(c) = parent.canonicalize() {
            let mut resolved = c;
            for component in tail.iter().rev() {
                resolved.push(component);
            }
            return Some(resolved);
        }
        cur = parent;
    }
}

/// Whether `path` is contained within `root` after both are canonicalised.
/// A `root` that cannot be canonicalised (does not exist) yields `false` —
/// callers treat that as "cannot prove containment" → reject.
pub fn is_within_root(path: &Path, root: &Path) -> bool {
    let Ok(root_c) = root.canonicalize() else {
        return false;
    };
    match resolve_existing_prefix(path) {
        Some(target_c) => target_c.starts_with(&root_c),
        None => false,
    }
}

/// Resolve a model-supplied `file_path` for the file-mutating tools
/// (Write / Edit / ApplyPatch): a relative path is joined onto the session
/// working directory `cwd` (matching ReadTool / Grep / Glob / Bash); an
/// absolute path is returned unchanged. `cwd == None` leaves the path as-is, so
/// relative paths then resolve against the process cwd — the legacy behaviour.
///
/// This closes the read/write asymmetry: without it a relative path written by
/// the model lands against the Tauri process cwd rather than the conversation's
/// workspace, producing a truthful "Created …" while the file never appears in
/// the workspace the UI browses.
pub fn resolve_against_cwd(file_path: &str, cwd: Option<&Path>) -> String {
    match cwd {
        Some(cwd) if !Path::new(file_path).is_absolute() => {
            cwd.join(file_path).to_string_lossy().into_owned()
        }
        _ => file_path.to_owned(),
    }
}

/// Format a filesystem read failure for the model and logs.
///
/// The path is quoted so a Windows drive letter (`C:\...`) cannot be glued to
/// the OS error by the `path: error` convention — that concatenation is how
/// `C:\foo.ts: 系统找不到指定的文件 (os error 2)` gets misread as a broken path.
pub fn format_file_read_error(file_path: &str, err: &std::io::Error) -> String {
    if Path::new(file_path).is_dir() {
        format!("Failed to read file '{file_path}': it is a directory, not a file")
    } else {
        format!("Failed to read file '{file_path}': {err}")
    }
}

/// Guard a write to `file_path` against an optional `root`. Returns `Some(error)`
/// when the write must be rejected, `None` when allowed (no root, or contained).
///
/// When `coding_boundary` is true, the rejection uses the stable
/// `CODING_BOUNDARY:` prefix from `nomi-coding` so the model can recover.
pub fn ensure_within_root(file_path: &str, root: Option<&Path>) -> Option<String> {
    ensure_within_root_ex(file_path, root, false)
}

/// Same as [`ensure_within_root`], with optional coding-boundary copy.
pub fn ensure_within_root_ex(
    file_path: &str,
    root: Option<&Path>,
    coding_boundary: bool,
) -> Option<String> {
    let root = root?;
    if is_within_root(Path::new(file_path), root) {
        None
    } else if coding_boundary {
        Some(nomi_coding::format_write_root_rejection(
            file_path,
            &root.display().to_string(),
        ))
    } else {
        Some(format!(
            "Write rejected: {} is outside the allowed write root {}. \
             Recovery: use a path under that root (relative to the session working directory). \
             Bash is not blocked by this guard — prefer Write/Edit/ApplyPatch for file changes. \
             (Only disable tools.write_root if you intentionally need unrestricted writes.)",
            file_path,
            root.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn allows_existing_file_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        assert!(is_within_root(&f, dir.path()));
    }

    #[test]
    fn allows_new_file_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("sub/new.txt"); // sub/ may not exist yet
        // parent sub/ does not exist; resolve_existing_prefix walks to dir.
        assert!(is_within_root(&f, dir.path()));
    }

    #[test]
    fn rejects_absolute_path_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let f = other.path().join("escape.txt");
        assert!(!is_within_root(&f, dir.path()));
    }

    #[test]
    fn rejects_parent_traversal_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        // root/../sibling.txt resolves to dir/sibling.txt — outside root.
        let escape = root.join("../sibling.txt");
        assert!(!is_within_root(&escape, &root));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escaping_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        // root/link -> outside ; a write to root/link/file actually lands outside.
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let via_link = root.join("link/file.txt");
        assert!(
            !is_within_root(&via_link, &root),
            "a symlink escaping the root must be rejected (textual check would pass)"
        );
    }

    #[test]
    fn resolve_against_cwd_joins_relative_and_keeps_absolute() {
        // Use real absolute dirs so the test is platform-agnostic (a leading "/"
        // is NOT absolute on Windows, so hardcoded unix paths would be wrong here).
        let cwd = std::env::temp_dir();
        // Relative → joined onto cwd (expected computed with the same join so the
        // separator is platform-correct).
        assert_eq!(
            resolve_against_cwd("notes.txt", Some(&cwd)),
            cwd.join("notes.txt").to_string_lossy().into_owned()
        );
        assert_eq!(
            resolve_against_cwd("a/b.txt", Some(&cwd)),
            cwd.join("a/b.txt").to_string_lossy().into_owned()
        );
        // Absolute input → returned unchanged.
        let abs = cwd.join("already_absolute.txt");
        let abs_str = abs.to_str().unwrap();
        assert_eq!(resolve_against_cwd(abs_str, Some(&cwd)), abs_str);
        // No cwd → unchanged (legacy: relative resolves against the process cwd).
        assert_eq!(resolve_against_cwd("notes.txt", None), "notes.txt");
    }

    #[test]
    fn ensure_within_root_is_noop_without_a_root() {
        // No configured root → never rejects (default behaviour unchanged).
        assert!(ensure_within_root("/anywhere/at/all.txt", None).is_none());
    }

    #[test]
    fn ensure_within_root_rejects_outside() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let outside = other.path().join("x.txt");
        assert!(ensure_within_root(outside.to_str().unwrap(), Some(dir.path())).is_some());
        let inside = dir.path().join("x.txt");
        assert!(ensure_within_root(inside.to_str().unwrap(), Some(dir.path())).is_none());
    }

    #[test]
    fn coding_boundary_rejection_uses_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let outside = other.path().join("x.txt");
        let msg = ensure_within_root_ex(outside.to_str().unwrap(), Some(dir.path()), true)
            .expect("reject");
        assert!(msg.starts_with(nomi_coding::BOUNDARY_PREFIX));
    }

    #[test]
    fn format_file_read_error_quotes_path_away_from_os_error() {
        let path = r"C:\flowy-workspace\code\open-vetta\packages\ai\scripts\generate-models.ts";
        let err = std::io::Error::from_raw_os_error(2);
        let msg = format_file_read_error(path, &err);
        assert!(
            msg.starts_with("Failed to read file 'C:\\flowy-workspace\\"),
            "{msg}"
        );
        assert!(msg.contains("generate-models.ts': "), "{msg}");
        assert!(
            !msg.contains("generate-models.ts:"),
            "path must not glue to the OS error: {msg}"
        );
    }

    #[test]
    fn format_file_read_error_names_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let err = std::io::Error::other("should be ignored for directories");
        let msg = format_file_read_error(path, &err);
        assert!(
            msg.contains("it is a directory, not a file"),
            "{msg}"
        );
        assert!(msg.contains(&format!("'{path}'")), "{msg}");
    }
}
