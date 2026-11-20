//! Lexical only: no `std::fs` calls (`metadata`, `exists`, `canonicalize`).

use std::path::{Path, PathBuf};

use url::Url;

const FORBIDDEN_CHARS: [char; 9] =
    ['\n', '\r', '\0', '"', '<', '>', '|', '*', '?'];

#[must_use]
pub(crate) fn path(target: &str) -> Option<PathBuf> {
    let target = strip_position(target);
    if target.is_empty() || target.contains(FORBIDDEN_CHARS) {
        return None;
    }
    if is_file_scheme(target) {
        return file_url_path(target);
    }
    if target.contains("://") || !colon_allowed(target) {
        return None;
    }
    let explicit = is_explicit(target);
    if !explicit && target.chars().any(char::is_whitespace) {
        return None;
    }
    if !explicit && !looks_like_file(target) {
        return None;
    }
    Some(PathBuf::from(target))
}

#[must_use]
pub(crate) fn resolve(
    target: &str,
    workspace: Option<&Path>,
) -> Option<PathBuf> {
    let file = path(target)?;
    if file.is_absolute() {
        return Some(file);
    }
    workspace.map(|root| root.join(file))
}

fn file_url_path(target: &str) -> Option<PathBuf> {
    let url = Url::parse(target).ok()?;
    if url.path().is_empty() {
        return None;
    }
    if let Some(host) = url.host_str()
        && !host.eq_ignore_ascii_case("localhost")
    {
        return None;
    }
    // `to_file_path` only maps URL segments; it never touches the filesystem.
    url.to_file_path().ok()
}

fn is_file_scheme(target: &str) -> bool {
    target
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
}

fn is_explicit(target: &str) -> bool {
    target.starts_with('/')
        || target.starts_with('\\')
        || target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with(".\\")
        || target.starts_with("..\\")
        || windows_drive(target)
}

const fn windows_drive(target: &str) -> bool {
    let bytes = target.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn colon_allowed(target: &str) -> bool {
    if !target.contains(':') {
        return true;
    }
    windows_drive(target) && !target[2..].contains(':')
}

fn looks_like_file(target: &str) -> bool {
    const NAMES: [&str; 4] = ["Dockerfile", "Makefile", "LICENSE", "README"];
    let name = target.rsplit(['/', '\\']).next().unwrap_or(target);
    if NAMES.contains(&name) {
        return true;
    }
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !extension.is_empty()
        && extension.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
        })
}

fn strip_position(target: &str) -> &str {
    let target = strip_hash_position(target);
    strip_colon_position(target)
}

fn strip_hash_position(target: &str) -> &str {
    if let Some((head, tail)) = target.rsplit_once("#L")
        && is_position(tail)
    {
        return head;
    }
    target
}

fn strip_colon_position(target: &str) -> &str {
    let Some((head, tail)) = target.rsplit_once(':') else {
        return target;
    };
    let drive_colon =
        head.len() == 1 && head.as_bytes()[0].is_ascii_alphabetic();
    if drive_colon || !is_positive(tail) {
        return target;
    }
    head
}

fn is_position(text: &str) -> bool {
    if let Some((line, column)) = text.split_once('C') {
        is_positive(line) && is_positive(column)
    } else {
        is_positive(text)
    }
}

fn is_positive(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.bytes().any(|byte| byte != b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_target_accepts_explicit_paths() {
        for target in ["./src/lib.rs", "../crates/flowy-studio/src/lib.rs"] {
            assert_eq!(path(target), Some(PathBuf::from(target)), "{target}");
        }
    }

    #[test]
    fn file_target_accepts_absolute_path() {
        assert_eq!(
            path("/home/user/report.md"),
            Some(PathBuf::from("/home/user/report.md"))
        );
    }

    #[test]
    fn file_target_accepts_windows_drive_paths() {
        for target in [r"C:\work\notes.txt", r"D:/work/notes.txt"] {
            assert_eq!(path(target), Some(PathBuf::from(target)), "{target}");
        }
    }

    #[test]
    fn file_target_accepts_file_urls() {
        for target in [
            "file:///C:/work/report.md",
            "file://localhost/C:/work/report.md",
            "file:C:/work/report.md",
        ] {
            assert!(
                path(target)
                    .is_some_and(|candidate| candidate.ends_with("report.md")),
                "{target}"
            );
        }
    }

    #[test]
    fn file_target_strips_hash_line_suffix() {
        for target in ["src/main.rs#L12", "src/main.rs#L12C3"] {
            assert_eq!(
                path(target),
                Some(PathBuf::from("src/main.rs")),
                "{target}"
            );
        }
    }

    #[test]
    fn file_target_strips_colon_line_suffix() {
        assert_eq!(path("src/main.rs:12"), Some(PathBuf::from("src/main.rs")));
    }

    #[test]
    fn file_target_accepts_bare_file_name() {
        assert_eq!(path("README.md"), Some(PathBuf::from("README.md")));
    }

    #[test]
    fn file_target_accepts_known_extensionless_names() {
        for target in ["Dockerfile", "Makefile", "LICENSE", "README"] {
            assert_eq!(path(target), Some(PathBuf::from(target)), "{target}");
        }
    }

    #[test]
    fn file_target_allows_spaces_in_explicit_path() {
        assert_eq!(
            path("./notes/my report.md"),
            Some(PathBuf::from("./notes/my report.md"))
        );
    }

    #[test]
    fn file_target_resolves_relative_path_with_workspace() {
        let resolved = resolve("./src/main.rs", Some(Path::new("/work")));
        assert!(
            resolved
                .is_some_and(|candidate| candidate.ends_with("src/main.rs"))
        );
    }

    #[test]
    fn file_target_resolves_file_url_without_workspace() {
        assert!(resolve("file:///C:/work/report.md", None).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn file_target_resolves_absolute_path_without_workspace() {
        assert_eq!(
            resolve("/var/log/report.md", None),
            Some(PathBuf::from("/var/log/report.md"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn file_target_resolves_drive_path_without_workspace() {
        assert_eq!(
            resolve(r"C:\work\report.md", None),
            Some(PathBuf::from(r"C:\work\report.md"))
        );
    }

    #[test]
    fn file_target_rejects_empty_target() {
        assert_eq!(path(""), None);
        assert_eq!(path("   "), None);
    }

    #[test]
    fn file_target_rejects_shell_command() {
        assert_eq!(path("cargo test --workspace"), None);
    }

    #[test]
    fn file_target_rejects_https_url() {
        assert_eq!(path("https://example.com/src/main.rs"), None);
    }

    #[test]
    fn file_target_rejects_pipe_in_explicit_path() {
        assert_eq!(path("./src/lib.rs | head"), None);
    }

    #[test]
    fn file_target_rejects_multiline_text() {
        assert_eq!(path("src/main.rs\nREADME.md"), None);
    }

    #[test]
    fn file_target_rejects_bare_word() {
        assert_eq!(path("hello"), None);
        assert_eq!(path("src/main"), None);
    }

    #[test]
    fn file_target_rejects_non_local_file_host() {
        assert_eq!(path("file://server/share/report.md"), None);
    }

    #[test]
    fn file_target_rejects_quoted_path() {
        assert_eq!(path(r#""C:\work\notes.txt""#), None);
    }

    #[test]
    fn file_target_rejects_glob_characters() {
        assert_eq!(path("./src/*.rs"), None);
        assert_eq!(path("./src/main?.rs"), None);
    }

    #[test]
    fn file_target_rejects_hidden_file_name() {
        assert_eq!(path(".env"), None);
    }

    #[test]
    fn file_target_rejects_trailing_dot() {
        assert_eq!(path("notes."), None);
    }

    #[test]
    fn file_target_rejects_non_numeric_colon_suffix() {
        assert_eq!(path("notes.txt:tag"), None);
    }

    #[test]
    fn file_target_rejects_zero_line_suffix() {
        assert_eq!(path("src/main.rs#L0"), None);
        assert_eq!(path("src/main.rs:0"), None);
    }

    #[test]
    fn file_target_rejects_bare_name_with_space() {
        assert_eq!(path("my notes.txt"), None);
    }

    #[test]
    fn file_target_rejects_relative_resolve_without_workspace() {
        assert_eq!(resolve("./src/main.rs", None), None);
    }

    #[test]
    fn file_target_rejects_redirect() {
        assert_eq!(path("> out.txt"), None);
    }
}
