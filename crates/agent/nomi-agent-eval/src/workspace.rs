//! Isolated-workspace helpers for live evaluation cases.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::corpus::CorpusError;

/// Resolve `relative` under `root`, rejecting absolute paths and `..` segments.
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, CorpusError> {
    let rel = Path::new(relative);
    if rel.is_absolute() {
        return Err(CorpusError::Invalid(format!(
            "workspace path must be relative: {relative}"
        )));
    }
    for component in rel.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => {
                return Err(CorpusError::Invalid(format!(
                    "workspace path must not contain '..' or prefixes: {relative}"
                )));
            }
        }
    }
    Ok(root.join(rel))
}

/// Write `files` into `root` (creating parent directories as needed).
pub fn materialize_files(
    root: &Path,
    files: &BTreeMap<String, String>,
) -> Result<(), CorpusError> {
    fs::create_dir_all(root)?;
    for (relative, contents) in files {
        let path = safe_join(root, relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(())
}

const ARTIFACT_PREVIEW_CHARS: usize = 2000;
const MAX_ARTIFACTS: usize = 40;
const MAX_WALK_DEPTH: usize = 4;

/// List workspace files for eval UI (relative paths only, text preview truncated).
pub fn collect_workspace_artifacts(root: &Path) -> Vec<crate::types::EvalArtifactMeta> {
    let mut out = Vec::new();
    collect_artifacts_inner(root, root, 0, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.truncate(MAX_ARTIFACTS);
    out
}

fn collect_artifacts_inner(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<crate::types::EvalArtifactMeta>,
) {
    if depth > MAX_WALK_DEPTH || out.len() >= MAX_ARTIFACTS {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_ARTIFACTS {
            return;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_artifacts_inner(root, &path, depth + 1, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let relative = match path.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        if should_skip_artifact(&relative) {
            continue;
        }
        let kind = if is_text_artifact(&relative) {
            "text"
        } else {
            "binary"
        };
        let preview = if kind == "text" {
            fs::read_to_string(&path).ok().map(|text| truncate_preview(&text, ARTIFACT_PREVIEW_CHARS))
        } else {
            None
        };
        out.push(crate::types::EvalArtifactMeta {
            path: relative,
            size_bytes: meta.len(),
            kind: kind.into(),
            preview,
        });
    }
}

fn should_skip_artifact(relative: &str) -> bool {
    relative == "_eval_check.py"
        || relative.ends_with("/_eval_check.py")
        || relative.ends_with(".eval-trace.json")
        || relative.contains("/__pycache__/")
        || relative.ends_with(".pyc")
}

fn is_text_artifact(relative: &str) -> bool {
    matches!(
        Path::new(relative)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "txt" | "csv" | "tsv" | "json" | "html" | "xml" | "py" | "rs" | "ts" | "js" | "css")
    )
}

fn truncate_preview(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let clipped: String = text.chars().take(max).collect();
        format!("{clipped}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_parent_and_absolute() {
        let root = Path::new("/tmp/eval");
        assert!(safe_join(root, "../secret").is_err());
        assert!(safe_join(root, "ok/../../x").is_err());
        #[cfg(windows)]
        assert!(safe_join(root, r"C:\Windows\notepad.exe").is_err());
        #[cfg(not(windows))]
        assert!(safe_join(root, "/etc/passwd").is_err());
    }

    #[test]
    fn writes_nested_relative_files() {
        let dir = tempdir().unwrap();
        let mut files = BTreeMap::new();
        files.insert("a/b.txt".into(), "hello".into());
        materialize_files(dir.path(), &files).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a/b.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn lists_text_artifacts_with_relative_paths() {
        let dir = tempdir().unwrap();
        let mut files = BTreeMap::new();
        files.insert("memo.md".into(), "hello MEMO_OK".into());
        files.insert("_eval_check.py".into(), "assert False".into());
        materialize_files(dir.path(), &files).unwrap();
        let artifacts = collect_workspace_artifacts(dir.path());
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "memo.md");
        assert_eq!(artifacts[0].kind, "text");
        assert!(artifacts[0]
            .preview
            .as_deref()
            .unwrap()
            .contains("MEMO_OK"));
    }
}
