//! Best-effort "reported" file outputs from Write/Edit-style tools.
//!
//! These are **not** verified [`crate::types::TraceArtifactMeta`] receipts from
//! `PersistedArtifact`. They exist so R&D Trace can list workspace scripts /
//! docs that never enter the media artifact delivery pipeline.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::types::TraceArtifactMeta;

const MAX_REPORTED_PATHS_PER_CALL: usize = 16;
const MAX_PATH_CHARS: usize = 512;

/// Compacted lowercase leaf names that imply a successful run mutated files.
const FILE_MUTATION_TOOL_NAMES: &[&str] = &[
    "write",
    "edit",
    "applypatch",
    "multiedit",
    "patch",
    "replace",
    "writefile",
];

/// Whether `name` is a native file-mutation tool (MCP names are excluded).
pub fn is_file_mutation_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.starts_with("mcp__") {
        return false;
    }
    let compacted: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .flat_map(|c| c.to_lowercase())
        .collect();
    FILE_MUTATION_TOOL_NAMES.contains(&compacted.as_str())
}

/// Normalize a tool-declared path into a safe display / relative locator.
///
/// Absolute host paths collapse to their basename (no workspace root available
/// in this crate). Traversal segments (`..`) are rejected.
pub fn normalize_reported_path(raw: &str) -> Option<String> {
    let mut path = raw.trim().replace('\\', "/");
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if let Some(stripped) = path.strip_prefix("./") {
        path = stripped.to_owned();
    }
    if path.is_empty() || path == "/" || looks_like_drive_root(&path) {
        return None;
    }
    if path.len() > MAX_PATH_CHARS {
        return None;
    }
    if path.split('/').any(|seg| seg == "..") {
        return None;
    }
    if is_absolute_path(&path) {
        let base = path.rsplit('/').next().unwrap_or("").trim();
        if base.is_empty() || base == ".." {
            return None;
        }
        return Some(base.to_owned());
    }
    Some(path)
}

/// Collect target paths from tool args (`file_path` / `path` / `files[]`).
pub fn collect_paths_from_tool_args(args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    push_path_string(&mut out, obj.get("file_path"));
    push_path_string(&mut out, obj.get("path"));
    if let Some(files) = obj.get("files").and_then(Value::as_array) {
        for entry in files {
            let Some(record) = entry.as_object() else {
                continue;
            };
            if record.get("delete").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            push_path_string(&mut out, record.get("file_path"));
            push_path_string(&mut out, record.get("path"));
        }
    }
    dedupe_normalize(out)
}

/// Recover paths from Write/Edit tool output text when args were overwritten.
///
/// Recognizes:
/// - `Created {path} (N lines)`
/// - `Updated {path} (N lines)`
/// - `Updated {path} (rename failed: …)`
/// - `Edited {path}: …`
pub fn parse_paths_from_tool_output(output: &str) -> Vec<String> {
    let mut raw = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line
            .strip_prefix("Created ")
            .or_else(|| line.strip_prefix("Updated "))
        {
            let path = rest
                .split(" (")
                .next()
                .unwrap_or(rest)
                .trim();
            if !path.is_empty() {
                raw.push(path.to_owned());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("Edited ") {
            let path = rest.split(':').next().unwrap_or(rest).trim();
            if !path.is_empty() {
                raw.push(path.to_owned());
            }
        }
    }
    dedupe_normalize(raw)
}

/// Build metadata-only reported artifacts for a completed file-mutation tool.
pub fn reported_artifacts_from_tool_call(
    call_id: &str,
    tool_name: &str,
    args: &Value,
    output: Option<&str>,
) -> Vec<TraceArtifactMeta> {
    if !is_file_mutation_tool_name(tool_name) {
        return Vec::new();
    }
    let mut paths = collect_paths_from_tool_args(args);
    if paths.is_empty() {
        if let Some(out) = output {
            paths = parse_paths_from_tool_output(out);
        }
    }
    paths
        .into_iter()
        .take(MAX_REPORTED_PATHS_PER_CALL)
        .map(|relative_path| {
            reported_artifact_meta(call_id, tool_name, relative_path)
        })
        .collect()
}

/// Recover reported artifacts from a historical tool span (attrs / preview).
pub fn reported_artifacts_from_tool_span(
    tool_name: &str,
    call_id: Option<&str>,
    preview: Option<&str>,
    attributes_artifacts: &[TraceArtifactMeta],
) -> Vec<TraceArtifactMeta> {
    if !attributes_artifacts.is_empty() {
        return attributes_artifacts.to_vec();
    }
    if !is_file_mutation_tool_name(tool_name) {
        return Vec::new();
    }
    let call = call_id.unwrap_or("unknown");
    let paths = preview
        .map(parse_paths_from_tool_output)
        .unwrap_or_default();
    paths
        .into_iter()
        .take(MAX_REPORTED_PATHS_PER_CALL)
        .map(|relative_path| reported_artifact_meta(call, tool_name, relative_path))
        .collect()
}

fn reported_artifact_meta(call_id: &str, tool_name: &str, relative_path: String) -> TraceArtifactMeta {
    let kind = kind_from_path(&relative_path).to_owned();
    TraceArtifactMeta {
        id: format!("reported:{call_id}:{relative_path}"),
        kind,
        mime_type: String::new(),
        relative_path,
        size_bytes: 0,
        sha256: String::new(),
        call_id: Some(call_id.to_owned()),
        tool_name: Some(tool_name.to_owned()),
        source: Some("reported".into()),
    }
}

fn kind_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image",
        "mp3" | "wav" | "ogg" | "flac" | "m4a" => "audio",
        "mp4" | "webm" | "mov" | "mkv" => "video",
        "py" | "rs" | "ts" | "tsx" | "js" | "jsx" | "md" | "txt" | "json" | "yaml" | "yml"
        | "toml" | "csv" | "html" | "css" | "sh" | "ps1" | "sql" | "xml" => "text",
        _ => "file",
    }
}

fn dedupe_normalize(raw: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in raw {
        let Some(path) = normalize_reported_path(&item) else {
            continue;
        };
        if seen.insert(path.clone()) {
            out.push(path);
        }
        if out.len() >= MAX_REPORTED_PATHS_PER_CALL {
            break;
        }
    }
    out
}

fn push_path_string(out: &mut Vec<String>, value: Option<&Value>) {
    if let Some(Value::String(s)) = value {
        if !s.trim().is_empty() {
            out.push(s.clone());
        }
    }
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/') || looks_like_windows_abs(path)
}

fn looks_like_windows_abs(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn looks_like_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!(bytes, [a, b':'] | [a, b':', b'/'] if a.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_mutation_tool_names() {
        assert!(is_file_mutation_tool_name("Write"));
        assert!(is_file_mutation_tool_name("apply_patch"));
        assert!(is_file_mutation_tool_name("MultiEdit"));
        assert!(!is_file_mutation_tool_name("Bash"));
        assert!(!is_file_mutation_tool_name("mcp__fs__write"));
        assert!(!is_file_mutation_tool_name("Read"));
    }

    #[test]
    fn collects_args_and_normalizes_absolute() {
        let args = serde_json::json!({
            "file_path": r"C:\Users\me\ws\scripts\app.py",
            "content": "print(1)"
        });
        let paths = collect_paths_from_tool_args(&args);
        assert_eq!(paths, vec!["app.py".to_owned()]);
    }

    #[test]
    fn parses_write_output_lines() {
        let out = "Created scripts/hello.py (12 lines)\nUpdated docs/readme.md (3 lines)";
        let paths = parse_paths_from_tool_output(out);
        assert_eq!(
            paths,
            vec!["scripts/hello.py".to_owned(), "docs/readme.md".to_owned()]
        );
    }

    #[test]
    fn reported_artifacts_from_write_args() {
        let arts = reported_artifacts_from_tool_call(
            "c1",
            "Write",
            &serde_json::json!({"file_path": "src/main.py"}),
            Some("Created src/main.py (1 lines)"),
        );
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].relative_path, "src/main.py");
        assert_eq!(arts[0].kind, "text");
        assert_eq!(arts[0].source.as_deref(), Some("reported"));
        assert!(arts[0].id.starts_with("reported:c1:"));
    }
}
