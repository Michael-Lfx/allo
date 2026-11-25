//! Built-in host tools: `read_file`, `write_file`, `edit_file`, `glob`, `grep`,
//! and `todo_write`.
//!
//! Ported from the MIT-licensed `dsh-rs` tool set; the kernel dependency is
//! gone. `bash` is not here: it comes from pi-rust's `rpi-tools` (see
//! [`crate::tools::pi`]). `todo_write` reports through a host-owned
//! [`crate::types::TodoSink`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::tools::matcher::glob_match;
use crate::tools::registry::{ToolDefinition, ToolRegistry};
use crate::types::{
    TodoItem, TodoStatus, ToolCallArgs, ToolExecutionResult, ToolRunContext,
};

/// Maximum file bytes returned by `read_file` or scanned by `grep`.
const MAX_READ_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 16;
const MAX_WALK_ENTRIES: usize = 5_000;
const READ_TRUNCATION_SUFFIX: &str =
    "\n...[truncated: file exceeds 2 MiB host read limit]";

#[cfg(windows)]
fn is_windows_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_windows_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Register every built-in tool on the registry.
pub fn register_builtin_tools(registry: &ToolRegistry) {
    registry.register(read_file_tool());
    registry.register(write_file_tool());
    registry.register(edit_file_tool());
    registry.register(glob_tool());
    registry.register(grep_tool());
    registry.register(todo_write_tool());
}

/// The `todo_write` tool: replaces the calling agent's whole todo list through
/// the host-provided [`crate::types::TodoSink`].
fn todo_write_tool() -> Arc<ToolDefinition> {
    let parameters = serde_json::json!({
        "type": "object",
        "properties": {
            "todos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                    },
                    "required": ["content"]
                }
            }
        },
        "required": ["todos"]
    });
    Arc::new(ToolDefinition::new(
        "todo_write",
        "Replace the agent's whole todo list. Use to track multi-step work; the list is overwritten on every call.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: serde_json::Value =
                    match serde_json::from_value(args.arguments.clone()) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let Some(agent_id) = run_ctx.agent_id.clone() else {
                    return ToolExecutionResult::error(
                        "NO_AGENT",
                        "tool called without an agent",
                    );
                };
                let Some(sink) = run_ctx.todos.clone() else {
                    return ToolExecutionResult::error(
                        "NO_TODOS",
                        "todo store unavailable",
                    );
                };
                let todos: Vec<TodoItem> = parsed
                    .get("todos")
                    .and_then(|t| t.as_array())
                    .map_or_default(|arr| {
                        arr.iter()
                            .map(|entry| TodoItem {
                                content: entry
                                    .get("content")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                status: match entry
                                    .get("status")
                                    .and_then(|s| s.as_str())
                                {
                                    Some("in_progress") => {
                                        TodoStatus::InProgress
                                    }
                                    Some("completed") => TodoStatus::Completed,
                                    _ => TodoStatus::Pending,
                                },
                            })
                            .collect()
                    });
                sink.replace_todos(Some(&agent_id), todos.clone());
                ToolExecutionResult::success_text(
                    format!("todo list updated ({} items)", todos.len()),
                    serde_json::json!({ "todos": todos }),
                )
            })
        },
    ))
}

fn resolve_path(run_ctx: &ToolRunContext, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = &run_ctx.cwd {
        Path::new(cwd).join(path)
    } else {
        path.to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// read_file
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
}

fn read_file_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": { "path": { "type": "string", "description": "Path of the file to read." } },
        "required": ["path"]
    });
    Arc::new(ToolDefinition::new(
        "read_file",
        "Read a text file and return its full contents.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: ReadArgs =
                    match serde_json::from_value(args.arguments) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let path = resolve_path(&run_ctx, &parsed.path);
                let mut file = match tokio::fs::File::open(&path).await {
                    Ok(file) => file,
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        );
                    }
                };
                let mut buf = Vec::new();
                if let Err(err) = (&mut file)
                    .take(MAX_READ_FILE_BYTES.saturating_add(1))
                    .read_to_end(&mut buf)
                    .await
                {
                    return ToolExecutionResult::error(
                        "READ_FAILED",
                        format!("cannot read {}: {err}", path.display()),
                    );
                }
                let truncated = (buf.len() as u64) > MAX_READ_FILE_BYTES;
                if truncated {
                    buf.truncate(
                        usize::try_from(MAX_READ_FILE_BYTES)
                            .unwrap_or(usize::MAX),
                    );
                    let mut content =
                        String::from_utf8_lossy(&buf).into_owned();
                    content.push_str(READ_TRUNCATION_SUFFIX);
                    let value = json!({
                        "path": path.display().to_string(),
                        "content": content,
                        "truncated": true,
                        "max_bytes": MAX_READ_FILE_BYTES,
                    });
                    ToolExecutionResult::success_text(content, value)
                } else {
                    match String::from_utf8(buf) {
                        Ok(content) => {
                            let value = json!({
                                "path": path.display().to_string(),
                                "content": content,
                                "truncated": false,
                            });
                            ToolExecutionResult::success_text(content, value)
                        }
                        Err(err) => ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        ),
                    }
                }
            })
        },
    ))
}

// ---------------------------------------------------------------------------
// write_file
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

fn write_file_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path of the file to write." },
            "content": { "type": "string", "description": "Full new content of the file." }
        },
        "required": ["path", "content"]
    });
    Arc::new(ToolDefinition::new(
        "write_file",
        "Write a file with the given content, creating parent directories.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: WriteArgs =
                    match serde_json::from_value(args.arguments) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let path = resolve_path(&run_ctx, &parsed.path);
                if let Some(parent) = path.parent()
                    && let Err(err) = tokio::fs::create_dir_all(parent).await
                {
                    return ToolExecutionResult::error(
                        "MKDIR_FAILED",
                        format!("cannot create {}: {err}", parent.display()),
                    );
                }
                match tokio::fs::write(&path, &parsed.content).await {
                    Ok(()) => {
                        let value = json!({ "path": path.display().to_string(), "bytes": parsed.content.len() });
                        ToolExecutionResult::success_text(
                            format!(
                                "wrote {} bytes to {}",
                                parsed.content.len(),
                                path.display()
                            ),
                            value,
                        )
                    }
                    Err(err) => ToolExecutionResult::error(
                        "WRITE_FAILED",
                        format!("cannot write {}: {err}", path.display()),
                    ),
                }
            })
        },
    ))
}

// ---------------------------------------------------------------------------
// edit_file
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

fn edit_file_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path of the file to edit." },
            "old_string": { "type": "string", "description": "Text to find." },
            "new_string": { "type": "string", "description": "Replacement text." },
            "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." }
        },
        "required": ["path", "old_string", "new_string"]
    });
    Arc::new(ToolDefinition::new(
        "edit_file",
        "Replace old_string with new_string in a file. Fails when old_string is absent.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: EditArgs =
                    match serde_json::from_value(args.arguments) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let path = resolve_path(&run_ctx, &parsed.path);
                let content = match tokio::fs::read_to_string(&path).await {
                    Ok(content) => content,
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        );
                    }
                };
                let updated = if parsed.replace_all {
                    if !content.contains(&parsed.old_string) {
                        return ToolExecutionResult::error(
                            "NOT_FOUND",
                            format!(
                                "old_string not found in {}",
                                path.display()
                            ),
                        );
                    }
                    content.replace(&parsed.old_string, &parsed.new_string)
                } else {
                    match content.find(&parsed.old_string) {
                        Some(_) => content.replacen(
                            &parsed.old_string,
                            &parsed.new_string,
                            1,
                        ),
                        None => {
                            return ToolExecutionResult::error(
                                "NOT_FOUND",
                                format!(
                                    "old_string not found in {}",
                                    path.display()
                                ),
                            );
                        }
                    }
                };
                match tokio::fs::write(&path, &updated).await {
                    Ok(()) => {
                        let value = json!({ "path": path.display().to_string(), "bytes": updated.len() });
                        ToolExecutionResult::success_text(
                            format!("edited {}", path.display()),
                            value,
                        )
                    }
                    Err(err) => ToolExecutionResult::error(
                        "WRITE_FAILED",
                        err.to_string(),
                    ),
                }
            })
        },
    ))
}

// ---------------------------------------------------------------------------
// glob
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize)]
struct GlobResult {
    matches: Vec<String>,
    count: usize,
}

fn glob_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Glob pattern; ** crosses directories." },
            "path": { "type": "string", "description": "Base directory (default: cwd)." }
        },
        "required": ["pattern"]
    });
    Arc::new(ToolDefinition::new(
        "glob",
        "List files matching a glob pattern under a directory.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: GlobArgs =
                    match serde_json::from_value(args.arguments) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let base = parsed
                    .path
                    .map(|p| resolve_path(&run_ctx, &p))
                    .or_else(|| run_ctx.cwd.clone().map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("."));
                let pattern = parsed.pattern.clone();
                let matches = walk_and_match(&base, &pattern).await;
                let relative: Vec<String> = matches
                    .iter()
                    .map(|m| {
                        let rel = m.strip_prefix(&base).unwrap_or(m);
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        rel.trim_start_matches('/').to_string()
                    })
                    .collect();
                let value = serde_json::to_value(&GlobResult {
                    count: relative.len(),
                    matches: relative.clone(),
                })
                .unwrap_or(Value::Null);
                let text = if relative.is_empty() {
                    format!("no files match {pattern}")
                } else {
                    relative.join("\n")
                };
                ToolExecutionResult::success_text(text, value)
            })
        },
    ))
}

async fn walk_and_match(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut visited = 0_usize;
    let mut stack = vec![(base.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH || visited >= MAX_WALK_ENTRIES {
            continue;
        }
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        let mut collected = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            collected.push(entry);
        }
        // Sort for determinism.
        collected.sort_by_key(tokio::fs::DirEntry::file_name);
        for entry in collected {
            if visited >= MAX_WALK_ENTRIES {
                break;
            }
            visited = visited.saturating_add(1);
            let path = entry.path();
            let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                continue;
            };
            if meta.file_type().is_symlink() || is_windows_reparse_point(&meta)
            {
                continue;
            }
            if meta.is_dir() && depth < MAX_WALK_DEPTH {
                stack.push((path.clone(), depth + 1));
            }
            if let Ok(relative) = path.strip_prefix(base) {
                let rel = relative.to_string_lossy().replace('\\', "/");
                if glob_match(pattern, &rel) {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// grep
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
}

#[derive(Serialize)]
struct GrepMatch {
    path: String,
    line: u64,
    text: String,
}

fn grep_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Regular expression to search for." },
            "path": { "type": "string", "description": "File or directory to search (default: cwd)." },
            "include": { "type": "string", "description": "Optional glob filter over file names." }
        },
        "required": ["pattern"]
    });
    Arc::new(ToolDefinition::new(
        "grep",
        "Search files for lines matching a regular expression, returning matches with line numbers.",
        parameters,
        |args: ToolCallArgs, run_ctx: ToolRunContext| {
            Box::pin(async move {
                let parsed: GrepArgs =
                    match serde_json::from_value(args.arguments) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                err.to_string(),
                            );
                        }
                    };
                let re = match regex::Regex::new(&parsed.pattern) {
                    Ok(re) => re,
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "BAD_REGEX",
                            err.to_string(),
                        );
                    }
                };
                let base = parsed
                    .path
                    .map(|p| resolve_path(&run_ctx, &p))
                    .or_else(|| run_ctx.cwd.clone().map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("."));
                let files = collect_files(&base).await;
                let mut matches: Vec<GrepMatch> = Vec::new();
                for file in files {
                    if let Some(include) = &parsed.include {
                        let name = file.file_name().map_or_default(|n| {
                            n.to_string_lossy().to_string()
                        });
                        if !glob_match(include, &name) {
                            continue;
                        }
                    }
                    if let Ok(meta) = tokio::fs::symlink_metadata(&file).await
                        && (meta.len() > MAX_READ_FILE_BYTES
                            || meta.file_type().is_symlink()
                            || is_windows_reparse_point(&meta))
                    {
                        continue;
                    }
                    let Ok(content) = tokio::fs::read_to_string(&file).await
                    else {
                        continue;
                    };
                    for (idx, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            matches.push(GrepMatch {
                                path: file.display().to_string(),
                                line: (idx + 1) as u64,
                                text: line.chars().take(300).collect(),
                            });
                            if matches.len() >= 500 {
                                break;
                            }
                        }
                    }
                    if matches.len() >= 500 {
                        break;
                    }
                }
                let value =
                    serde_json::to_value(&matches).unwrap_or(Value::Null);
                let text = if matches.is_empty() {
                    format!("no matches for {}", parsed.pattern)
                } else {
                    matches
                        .iter()
                        .map(|m| format!("{}:{}:{}", m.path, m.line, m.text))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                ToolExecutionResult::success_text(text, value)
            })
        },
    ))
}

async fn collect_files(base: &Path) -> Vec<PathBuf> {
    if let Ok(meta) = tokio::fs::symlink_metadata(base).await {
        if meta.file_type().is_symlink() || is_windows_reparse_point(&meta) {
            return Vec::new();
        }
        if meta.is_file() {
            return vec![base.to_path_buf()];
        }
    }
    let mut out = Vec::new();
    let mut visited = 0_usize;
    let mut stack = vec![(base.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH || visited >= MAX_WALK_ENTRIES {
            continue;
        }
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        let mut collected = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            collected.push(entry);
        }
        collected.sort_by_key(tokio::fs::DirEntry::file_name);
        for entry in collected {
            if visited >= MAX_WALK_ENTRIES {
                break;
            }
            visited = visited.saturating_add(1);
            let path = entry.path();
            let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                continue;
            };
            if meta.file_type().is_symlink() || is_windows_reparse_point(&meta)
            {
                continue;
            }
            if meta.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if meta.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::{ToolRegistry, register_builtin_tools};
    use crate::types::{
        CancelToken, TodoItem, TodoSink, TodoStatus, ToolExecutionResult,
        ToolRunContext,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct RecordingTodos(Mutex<Vec<(Option<String>, Vec<TodoItem>)>>);

    impl TodoSink for RecordingTodos {
        fn replace_todos(&self, agent_id: Option<&str>, todos: Vec<TodoItem>) {
            self.0
                .lock()
                .unwrap()
                .push((agent_id.map(str::to_owned), todos));
        }
    }

    fn ctx(cwd: PathBuf) -> ToolRunContext {
        ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("agent-1".into()),
            cwd: Some(cwd.display().to_string()),
            todos: None,
            submission_id: None,
            session_origin: crate::session::SessionOrigin::Unknown,
            output_root: None,
            artifact_delivery_available: false,
        }
    }

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn registry() -> ToolRegistry {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry);
        registry
    }

    #[tokio::test]
    async fn read_file_returns_file_content() {
        let result = registry()
            .execute(
                "c1".into(),
                "read_file".into(),
                json!({"path": manifest().join("Cargo.toml").display().to_string()}),
                ctx(manifest()),
            )
            .await;
        assert!(!result.is_error(), "read_file failed: {result:?}");
        let text = result
            .content()
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<String>();
        assert!(text.contains("flowy-agent-core"));
    }

    #[tokio::test]
    async fn malformed_arguments_are_rejected_before_touching_the_filesystem() {
        let result = registry()
            .execute(
                "c2".into(),
                "read_file".into(),
                json!({}),
                ctx(manifest()),
            )
            .await;
        match result {
            ToolExecutionResult::Error { code, .. } => {
                assert_eq!(code, "INVALID_ARGS")
            }
            ToolExecutionResult::Success { .. } => {
                panic!("missing path must not succeed")
            }
        }
    }

    #[tokio::test]
    async fn glob_returns_relative_matches_under_cwd() {
        let result = registry()
            .execute(
                "c3".into(),
                "glob".into(),
                json!({"pattern": "src/*.rs"}),
                ctx(manifest()),
            )
            .await;
        assert!(!result.is_error(), "glob failed: {result:?}");
        let text = result
            .content()
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<String>();
        assert!(text.contains("src/types.rs"), "unexpected matches: {text}");
    }

    #[tokio::test]
    async fn todo_write_reports_to_the_host_sink() {
        let sink = Arc::new(RecordingTodos(Mutex::new(Vec::new())));
        let mut run_ctx = ctx(manifest());
        run_ctx.todos = Some(sink.clone());
        let result = registry()
            .execute(
                "c4".into(),
                "todo_write".into(),
                json!({"todos":[{"content":"ship core","status":"in_progress"}]}),
                run_ctx,
            )
            .await;
        assert!(!result.is_error(), "todo_write failed: {result:?}");
        let recorded = sink.0.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0.as_deref(), Some("agent-1"));
        assert_eq!(recorded[0].1[0].status, TodoStatus::InProgress);
        assert_eq!(recorded[0].1[0].content, "ship core");
    }

    #[tokio::test]
    async fn todo_write_without_host_sink_fails_explicitly() {
        let result = registry()
            .execute(
                "c5".into(),
                "todo_write".into(),
                json!({"todos":[]}),
                ctx(manifest()),
            )
            .await;
        match result {
            ToolExecutionResult::Error { code, .. } => {
                assert_eq!(code, "NO_TODOS")
            }
            ToolExecutionResult::Success { .. } => {
                panic!("missing todo sink must not succeed")
            }
        }
    }

    #[tokio::test]
    async fn read_file_truncates_files_above_two_mib_cap() {
        let dir = std::env::temp_dir()
            .join(format!("flowy-builtin-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("huge.log");
        let payload = "a".repeat((super::MAX_READ_FILE_BYTES as usize) + 4096);
        std::fs::write(&big, &payload).unwrap();

        let result = registry()
            .execute(
                "c-big".into(),
                "read_file".into(),
                json!({ "path": big.display().to_string() }),
                ctx(dir.clone()),
            )
            .await;
        assert!(!result.is_error(), "oversized read must truncate cleanly");
        let text = result
            .content()
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<String>();
        assert!(text.contains("exceeds 2 MiB host read limit"));
        assert!(text.len() < payload.len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
