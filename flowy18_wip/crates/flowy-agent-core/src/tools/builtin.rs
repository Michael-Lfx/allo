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
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::tools::matcher::glob_match;
use crate::tools::registry::{ToolDefinition, ToolRegistry};
use crate::types::{
    TodoItem, TodoStatus, ToolCallArgs, ToolExecutionResult, ToolRunContext,
};

/// Maximum file bytes returned by `read_file` or scanned by `grep`.
const MAX_READ_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Maximum file bytes allowed for in-memory `edit_file`.
const MAX_EDIT_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_GLOB_PATTERN_CHARS: usize = 4_096;
const MAX_WALK_DEPTH: usize = 16;
const MAX_WALK_ENTRIES: usize = 5_000;
const READ_TRUNCATION_SUFFIX: &str =
    "\n...[truncated: file exceeds 2 MiB host read limit]";

#[must_use]
pub fn is_windows_reserved_device_name(name: &str) -> bool {
    let trimmed = name.trim_end_matches([' ', '.']);
    // Split on both `.` and `:` so `CON:a` / `NUL:stream` cannot bypass
    // Win32 DOS device resolution (`RtlIsDosDeviceName_U`).
    let stem = trimmed
        .split(['.', ':'])
        .next()
        .unwrap_or(trimmed)
        .trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "CONIN$"
            | "CONOUT$"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    )
}

#[must_use]
pub fn has_reserved_device_component(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    path.components().any(|component| match component {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::DeviceNS(_) => true,
            Prefix::Verbatim(name) => {
                name.to_str().is_none_or(is_windows_reserved_device_name)
            }
            _ => false,
        },
        Component::Normal(part) => part.to_str().is_none_or(|s| {
            is_windows_reserved_device_name(s)
                || (cfg!(windows) && s.contains(':'))
        }),
        _ => false,
    })
}

#[cfg(windows)]
#[must_use]
pub fn is_windows_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
#[must_use]
pub const fn is_windows_reparse_point(_meta: &std::fs::Metadata) -> bool {
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
    #[serde(default)]
    offset_line: Option<usize>,
    #[serde(default)]
    max_lines: Option<usize>,
}

struct SlicedRead {
    content: String,
    returned_lines: usize,
    has_more: bool,
    byte_capped: bool,
}

async fn read_file_slice(
    file: tokio::fs::File,
    start_line: usize,
    max_lines: Option<usize>,
    max_bytes: usize,
) -> Result<SlicedRead, String> {
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut skip_remaining = start_line.saturating_sub(1);
    while skip_remaining > 0 {
        let chunk = reader.fill_buf().await.map_err(|err| err.to_string())?;
        if chunk.is_empty() {
            return Ok(SlicedRead {
                content: String::new(),
                returned_lines: 0,
                has_more: false,
                byte_capped: false,
            });
        }
        let mut consumed = chunk.len();
        for (idx, &byte) in chunk.iter().enumerate() {
            if byte == b'\n' {
                skip_remaining -= 1;
                if skip_remaining == 0 {
                    consumed = idx + 1;
                    break;
                }
            }
        }
        reader.consume(consumed);
    }

    let line_limit = max_lines.unwrap_or(usize::MAX);
    let cap = max_bytes.saturating_add(1);
    let mut buf = Vec::new();
    let mut completed_lines = 0_usize;
    let mut byte_capped = false;
    let mut hit_line_limit = false;

    loop {
        if completed_lines >= line_limit {
            hit_line_limit = true;
            break;
        }
        if buf.len() >= cap {
            byte_capped = true;
            break;
        }
        let chunk = reader.fill_buf().await.map_err(|err| err.to_string())?;
        if chunk.is_empty() {
            break;
        }
        let remaining_budget = cap - buf.len();
        let take_limit = chunk.len().min(remaining_budget);
        let mut consumed = take_limit;
        for (idx, &byte) in chunk[..take_limit].iter().enumerate() {
            if byte == b'\n' {
                completed_lines += 1;
                if completed_lines >= line_limit {
                    consumed = idx + 1;
                    hit_line_limit = true;
                    break;
                }
            }
        }
        buf.extend_from_slice(&chunk[..consumed]);
        reader.consume(consumed);
        if buf.len() >= cap {
            byte_capped = true;
            break;
        }
        if hit_line_limit {
            break;
        }
    }

    let has_more = if byte_capped {
        true
    } else if hit_line_limit {
        let next = reader.fill_buf().await.map_err(|err| err.to_string())?;
        !next.is_empty()
    } else {
        false
    };

    if byte_capped {
        buf.truncate(max_bytes);
    }

    if let Err(err) = std::str::from_utf8(&buf) {
        if byte_capped && err.error_len().is_none() {
            buf.truncate(err.valid_up_to());
        } else {
            return Err(err.to_string());
        }
    }

    let mut content = String::from_utf8(buf).map_err(|err| err.to_string())?;
    let returned_lines = content.lines().count();
    if byte_capped {
        content.push_str(READ_TRUNCATION_SUFFIX);
    }

    Ok(SlicedRead {
        content,
        returned_lines,
        has_more,
        byte_capped,
    })
}

fn read_file_tool() -> Arc<ToolDefinition> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path of the file to read." },
            "offset_line": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional 1-based line number to start reading from."
            },
            "max_lines": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional maximum number of lines to return from offset_line."
            }
        },
        "required": ["path"]
    });
    Arc::new(ToolDefinition::new(
        "read_file",
        "Read a text file and return its full contents, or a 1-based line slice via offset_line and max_lines.",
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
                if parsed.offset_line == Some(0) || parsed.max_lines == Some(0)
                {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        "offset_line and max_lines must be >= 1",
                    );
                }
                let path = resolve_path(&run_ctx, &parsed.path);
                if has_reserved_device_component(&path) {
                    return ToolExecutionResult::error(
                        "INVALID_PATH",
                        format!(
                            "reserved device path refused: {}",
                            path.display()
                        ),
                    );
                }
                match tokio::fs::symlink_metadata(&path).await {
                    Ok(meta)
                        if !meta.is_file()
                            || meta.file_type().is_symlink()
                            || is_windows_reparse_point(&meta) =>
                    {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("not a regular file: {}", path.display()),
                        );
                    }
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        );
                    }
                    Ok(_) => {}
                }
                let mut file = match tokio::fs::File::open(&path).await {
                    Ok(file) => file,
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        );
                    }
                };
                if parsed.offset_line.is_some() || parsed.max_lines.is_some() {
                    let start_line = parsed.offset_line.unwrap_or(1);
                    let max_bytes = usize::try_from(MAX_READ_FILE_BYTES)
                        .unwrap_or(usize::MAX);
                    return match read_file_slice(
                        file,
                        start_line,
                        parsed.max_lines,
                        max_bytes,
                    )
                    .await
                    {
                        Ok(sliced) => {
                            let next_offset_line = sliced.has_more.then(|| {
                                start_line.saturating_add(sliced.returned_lines)
                            });
                            let mut value = json!({
                                "path": path.display().to_string(),
                                "content": &sliced.content,
                                "offset_line": start_line,
                                "returned_lines": sliced.returned_lines,
                                "has_more": sliced.has_more,
                                "next_offset_line": next_offset_line,
                                "truncated": sliced.has_more || sliced.byte_capped,
                            });
                            if sliced.byte_capped
                                && let Some(obj) = value.as_object_mut()
                            {
                                obj.insert(
                                    "max_bytes".to_owned(),
                                    json!(MAX_READ_FILE_BYTES),
                                );
                            }
                            ToolExecutionResult::success_text(
                                sliced.content,
                                value,
                            )
                        }
                        Err(err) => ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        ),
                    };
                }
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
                if has_reserved_device_component(&path) {
                    return ToolExecutionResult::error(
                        "INVALID_PATH",
                        format!(
                            "reserved device path refused: {}",
                            path.display()
                        ),
                    );
                }
                if let Ok(meta) = tokio::fs::symlink_metadata(&path).await
                    && (!meta.is_file()
                        || meta.file_type().is_symlink()
                        || is_windows_reparse_point(&meta))
                {
                    return ToolExecutionResult::error(
                        "WRITE_FAILED",
                        format!(
                            "refusing to write non-regular file: {}",
                            path.display()
                        ),
                    );
                }
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
                if parsed.old_string.is_empty() {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        "old_string must not be empty",
                    );
                }
                let path = resolve_path(&run_ctx, &parsed.path);
                if has_reserved_device_component(&path) {
                    return ToolExecutionResult::error(
                        "INVALID_PATH",
                        format!(
                            "reserved device path refused: {}",
                            path.display()
                        ),
                    );
                }
                match tokio::fs::symlink_metadata(&path).await {
                    Ok(meta)
                        if !meta.is_file()
                            || meta.file_type().is_symlink()
                            || is_windows_reparse_point(&meta) =>
                    {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("not a regular file: {}", path.display()),
                        );
                    }
                    Ok(meta) if meta.len() > MAX_EDIT_FILE_BYTES => {
                        return ToolExecutionResult::error(
                            "FILE_TOO_LARGE",
                            format!(
                                "cannot edit {}: exceeds {} MiB host edit limit",
                                path.display(),
                                MAX_EDIT_FILE_BYTES / (1024 * 1024)
                            ),
                        );
                    }
                    Err(err) => {
                        return ToolExecutionResult::error(
                            "READ_FAILED",
                            format!("cannot read {}: {err}", path.display()),
                        );
                    }
                    Ok(_) => {}
                }
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
                    let occurrences =
                        content.matches(&parsed.old_string).count();
                    if occurrences == 0 {
                        return ToolExecutionResult::error(
                            "NOT_FOUND",
                            format!(
                                "old_string not found in {}",
                                path.display()
                            ),
                        );
                    }
                    let grown = (content.len() as u64)
                        .saturating_sub(
                            (occurrences as u64)
                                .saturating_mul(parsed.old_string.len() as u64),
                        )
                        .saturating_add(
                            (occurrences as u64)
                                .saturating_mul(parsed.new_string.len() as u64),
                        );
                    if grown > MAX_EDIT_FILE_BYTES {
                        return ToolExecutionResult::error(
                            "FILE_TOO_LARGE",
                            format!(
                                "cannot edit {}: replacement output exceeds {} MiB host edit limit",
                                path.display(),
                                MAX_EDIT_FILE_BYTES / (1024 * 1024)
                            ),
                        );
                    }
                    content.replace(&parsed.old_string, &parsed.new_string)
                } else {
                    match content.find(&parsed.old_string) {
                        Some(_) => {
                            let grown = (content.len() as u64)
                                .saturating_sub(parsed.old_string.len() as u64)
                                .saturating_add(parsed.new_string.len() as u64);
                            if grown > MAX_EDIT_FILE_BYTES {
                                return ToolExecutionResult::error(
                                    "FILE_TOO_LARGE",
                                    format!(
                                        "cannot edit {}: replacement output exceeds {} MiB host edit limit",
                                        path.display(),
                                        MAX_EDIT_FILE_BYTES / (1024 * 1024)
                                    ),
                                );
                            }
                            content.replacen(
                                &parsed.old_string,
                                &parsed.new_string,
                                1,
                            )
                        }
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
                if pattern.chars().count() > MAX_GLOB_PATTERN_CHARS {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        format!(
                            "glob pattern exceeds {MAX_GLOB_PATTERN_CHARS} character limit"
                        ),
                    );
                }
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
    if let Ok(meta) = tokio::fs::symlink_metadata(base).await
        && (meta.file_type().is_symlink() || is_windows_reparse_point(&meta))
    {
        return out;
    }
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
                if let Some(include) = &parsed.include
                    && include.chars().count() > MAX_GLOB_PATTERN_CHARS
                {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        format!(
                            "include glob exceeds {MAX_GLOB_PATTERN_CHARS} character limit"
                        ),
                    );
                }
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
                assert_eq!(code, "INVALID_ARGS");
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
                assert_eq!(code, "NO_TODOS");
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
        match result {
            ToolExecutionResult::Success { value, .. } => {
                assert_eq!(value["truncated"], true);
                assert_eq!(value["max_bytes"], super::MAX_READ_FILE_BYTES);
                assert!(value.get("offset_line").is_none());
                assert!(value.get("returned_lines").is_none());
                assert!(value.get("has_more").is_none());
                assert!(value.get("next_offset_line").is_none());
            }
            ToolExecutionResult::Error { .. } => {
                panic!("expected success")
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "flowy-builtin-{tag}-{}-{id}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn unwrap_success_value(result: ToolExecutionResult) -> serde_json::Value {
        match result {
            ToolExecutionResult::Success { value, .. } => value,
            ToolExecutionResult::Error { code, message, .. } => {
                panic!("expected success, got error {code}: {message}")
            }
        }
    }

    #[tokio::test]
    async fn read_file_legacy_full_read_json_shape_unchanged() {
        let dir = TempDir::new("legacy-shape");
        let file = dir.0.join("plain.txt");
        std::fs::write(&file, "hello\nworld\n").unwrap();

        let value = unwrap_success_value(
            registry()
                .execute(
                    "c-legacy".into(),
                    "read_file".into(),
                    json!({ "path": file.display().to_string() }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(value["content"], "hello\nworld\n");
        assert_eq!(value["truncated"], false);
        assert!(value.get("max_bytes").is_none());
        assert!(value.get("offset_line").is_none());
        assert!(value.get("returned_lines").is_none());
        assert!(value.get("has_more").is_none());
        assert!(value.get("next_offset_line").is_none());
    }

    #[tokio::test]
    async fn read_file_rejects_zero_offset_or_max_lines() {
        let dir = TempDir::new("zero-args");
        for args in [
            json!({ "path": "missing.txt", "offset_line": 0 }),
            json!({ "path": "missing.txt", "max_lines": 0 }),
            json!({ "path": "missing.txt", "offset_line": 0, "max_lines": 2 }),
        ] {
            let result = registry()
                .execute(
                    "c-zero".into(),
                    "read_file".into(),
                    args,
                    ctx(dir.0.clone()),
                )
                .await;
            match result {
                ToolExecutionResult::Error { code, .. } => {
                    assert_eq!(code, "INVALID_ARGS");
                }
                ToolExecutionResult::Success { .. } => {
                    panic!("zero offset_line or max_lines must fail")
                }
            }
        }
    }

    #[tokio::test]
    async fn read_file_slice_paginates_multiline_file() {
        let dir = TempDir::new("paginate");
        let file = dir.0.join("lines.txt");
        std::fs::write(&file, "line-1\nline-2\nline-3\nline-4\nline-5")
            .unwrap();

        let p1 = unwrap_success_value(
            registry()
                .execute(
                    "p1".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 1,
                        "max_lines": 2
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(p1["content"], "line-1\nline-2\n");
        assert_eq!(p1["offset_line"], 1);
        assert_eq!(p1["returned_lines"], 2);
        assert_eq!(p1["has_more"], true);
        assert_eq!(p1["next_offset_line"], 3);
        assert_eq!(p1["truncated"], true);
        assert!(p1.get("max_bytes").is_none());

        let p2 = unwrap_success_value(
            registry()
                .execute(
                    "p2".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 3,
                        "max_lines": 2
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(p2["content"], "line-3\nline-4\n");
        assert_eq!(p2["offset_line"], 3);
        assert_eq!(p2["returned_lines"], 2);
        assert_eq!(p2["has_more"], true);
        assert_eq!(p2["next_offset_line"], 5);
        assert_eq!(p2["truncated"], true);

        let p3 = unwrap_success_value(
            registry()
                .execute(
                    "p3".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 5,
                        "max_lines": 2
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(p3["content"], "line-5");
        assert_eq!(p3["offset_line"], 5);
        assert_eq!(p3["returned_lines"], 1);
        assert_eq!(p3["has_more"], false);
        assert!(p3["next_offset_line"].is_null());
        assert_eq!(p3["truncated"], false);

        // Trailing newline at EOF with exact max_lines has no remaining bytes.
        let trailing = dir.0.join("trailing.txt");
        std::fs::write(&trailing, "a\nb\n").unwrap();
        let exact = unwrap_success_value(
            registry()
                .execute(
                    "p-exact".into(),
                    "read_file".into(),
                    json!({
                        "path": trailing.display().to_string(),
                        "max_lines": 2
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(exact["content"], "a\nb\n");
        assert_eq!(exact["offset_line"], 1);
        assert_eq!(exact["returned_lines"], 2);
        assert_eq!(exact["has_more"], false);
        assert!(exact["next_offset_line"].is_null());
        assert_eq!(exact["truncated"], false);

        // offset_line only (without max_lines) returns tail from start_line.
        let tail = unwrap_success_value(
            registry()
                .execute(
                    "p-tail".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 4
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(tail["content"], "line-4\nline-5");
        assert_eq!(tail["offset_line"], 4);
        assert_eq!(tail["returned_lines"], 2);
        assert_eq!(tail["has_more"], false);
        assert!(tail["next_offset_line"].is_null());
    }

    #[tokio::test]
    async fn read_file_slice_offset_beyond_eof_returns_empty() {
        let dir = TempDir::new("beyond-eof");
        let file = dir.0.join("short.txt");
        std::fs::write(&file, "alpha\nbeta\n").unwrap();

        for offset in [3_usize, 10_usize] {
            let value = unwrap_success_value(
                registry()
                    .execute(
                        "eof".into(),
                        "read_file".into(),
                        json!({
                            "path": file.display().to_string(),
                            "offset_line": offset,
                            "max_lines": 5
                        }),
                        ctx(dir.0.clone()),
                    )
                    .await,
            );
            assert_eq!(value["content"], "");
            assert_eq!(value["offset_line"], offset);
            assert_eq!(value["returned_lines"], 0);
            assert_eq!(value["has_more"], false);
            assert!(value["next_offset_line"].is_null());
            assert_eq!(value["truncated"], false);
        }
    }

    #[tokio::test]
    async fn read_file_slice_handles_cjk_utf8_and_crlf() {
        let dir = TempDir::new("cjk-utf8");
        let file = dir.0.join("cjk.txt");
        std::fs::write(&file, "第一行\r\n第二行 🚀\n第三行中文\n").unwrap();

        let value = unwrap_success_value(
            registry()
                .execute(
                    "cjk".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 2,
                        "max_lines": 1
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(value["content"], "第二行 🚀\n");
        assert_eq!(value["offset_line"], 2);
        assert_eq!(value["returned_lines"], 1);
        assert_eq!(value["has_more"], true);
        assert_eq!(value["next_offset_line"], 3);
        assert_eq!(value["truncated"], true);

        let first = unwrap_success_value(
            registry()
                .execute(
                    "crlf".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 1,
                        "max_lines": 1
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(first["content"], "第一行\r\n");
        assert_eq!(first["returned_lines"], 1);
        assert_eq!(first["has_more"], true);
    }

    #[tokio::test]
    async fn read_file_slice_caps_giant_single_line_without_oom() {
        let dir = TempDir::new("giant-line");
        let file = dir.0.join("giant.txt");
        // Shift by 1 ASCII byte so the 2 MiB boundary splits a 4-byte emoji.
        let mut payload = String::from("x");
        payload.push_str(&"🚀".repeat(524_295));
        assert!(payload.len() > super::MAX_READ_FILE_BYTES as usize);
        std::fs::write(&file, &payload).unwrap();

        let value = unwrap_success_value(
            registry()
                .execute(
                    "giant".into(),
                    "read_file".into(),
                    json!({
                        "path": file.display().to_string(),
                        "offset_line": 1,
                        "max_lines": 10
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(value["offset_line"], 1);
        assert_eq!(value["returned_lines"], 1);
        assert_eq!(value["has_more"], true);
        assert_eq!(value["next_offset_line"], 2);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["max_bytes"], super::MAX_READ_FILE_BYTES);
        let content = value["content"].as_str().unwrap();
        assert!(content.ends_with(super::READ_TRUNCATION_SUFFIX));
        let prefix =
            content.strip_suffix(super::READ_TRUNCATION_SUFFIX).unwrap();
        assert!(prefix.len() <= super::MAX_READ_FILE_BYTES as usize);
        assert!(prefix.len() >= (super::MAX_READ_FILE_BYTES as usize) - 3);
        assert!(prefix.starts_with("x🚀"));

        // Also verify skipping over a >2 MiB line to read line 2 works cleanly.
        let two_lines = dir.0.join("two-lines-giant.txt");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&two_lines).unwrap();
            let chunk = vec![b'a'; 64 * 1024];
            for _ in 0..48 {
                f.write_all(&chunk).unwrap();
            }
            f.write_all(b"\nline-after-giant\n").unwrap();
        }
        let line2 = unwrap_success_value(
            registry()
                .execute(
                    "skip-giant".into(),
                    "read_file".into(),
                    json!({
                        "path": two_lines.display().to_string(),
                        "offset_line": 2,
                        "max_lines": 1
                    }),
                    ctx(dir.0.clone()),
                )
                .await,
        );
        assert_eq!(line2["content"], "line-after-giant\n");
        assert_eq!(line2["offset_line"], 2);
        assert_eq!(line2["returned_lines"], 1);
        assert_eq!(line2["has_more"], false);
        assert!(line2["next_offset_line"].is_null());
        assert_eq!(line2["truncated"], false);
    }

    #[tokio::test]
    async fn read_file_slice_rejects_invalid_utf8_binary() {
        let dir = TempDir::new("invalid-utf8");
        let small_bin = dir.0.join("small.bin");
        std::fs::write(&small_bin, [0xff_u8, 0xfe, 0x00, 0x01]).unwrap();
        let res1 = registry()
            .execute(
                "bin1".into(),
                "read_file".into(),
                json!({
                    "path": small_bin.display().to_string(),
                    "offset_line": 1,
                    "max_lines": 1
                }),
                ctx(dir.0.clone()),
            )
            .await;
        match res1 {
            ToolExecutionResult::Error { code, .. } => {
                assert_eq!(code, "READ_FAILED");
            }
            ToolExecutionResult::Success { .. } => {
                panic!("binary file must fail UTF-8 validation")
            }
        }

        let large_bin = dir.0.join("large.bin");
        let mut bytes = vec![0xff_u8, 0xfe, 0x00];
        bytes.resize((super::MAX_READ_FILE_BYTES as usize) + 64, b'a');
        std::fs::write(&large_bin, &bytes).unwrap();
        let res2 = registry()
            .execute(
                "bin2".into(),
                "read_file".into(),
                json!({
                    "path": large_bin.display().to_string(),
                    "offset_line": 1,
                    "max_lines": 1
                }),
                ctx(dir.0.clone()),
            )
            .await;
        match res2 {
            ToolExecutionResult::Error { code, .. } => {
                assert_eq!(code, "READ_FAILED");
            }
            ToolExecutionResult::Success { .. } => {
                panic!("oversized binary file must fail UTF-8 validation")
            }
        }
    }
}
