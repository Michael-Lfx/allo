//! Session-bound overflow store for oversized tool results.
//!
//! Large tool output is clipped at creation time and the full body is written
//! under `{cwd}/.flowy/content-refs/{id}` so the model can page it with
//! [`ReadContentRefTool`] (or ordinary Read/Grep, since the file sits inside
//! the workspace). Without a workspace cwd the store falls back to a process
//! temp directory so tests and headless callers still get a locator.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

/// Workspace-relative directory for persisted tool-result bodies.
pub const CONTENT_REFS_REL_DIR: &str = ".flowy/content-refs";

/// Default page size when `limit` is omitted.
const DEFAULT_READ_LIMIT: usize = crate::MAX_PROVIDER_TOOL_OUTPUT_BYTES;

/// Resolve the content-ref directory for a session working directory.
pub fn store_dir(cwd: &Path) -> PathBuf {
    cwd.join(".flowy").join("content-refs")
}

/// Persist `content` and return a model-visible locator.
///
/// `cwd` should be the session workspace. When `None`, the file is written
/// under a process temp directory (legacy / test fallback).
pub fn persist_content_reference(content: &str, cwd: Option<&Path>) -> String {
    let dir = match cwd {
        Some(cwd) => store_dir(cwd),
        None => std::env::temp_dir().join("nomi-content-refs"),
    };
    ensure_store_dir(&dir);
    let id = content_id(content);
    let path = dir.join(&id);
    let _ = std::fs::write(&path, content);
    let rel = format!("{CONTENT_REFS_REL_DIR}/{id}");
    format!(
        "[content_ref id={id} bytes={}] Full output is at `{rel}`. \
         Call ReadContentRef with this id (optional offset/limit in bytes) to page it.",
        content.len()
    )
}

/// Hex id derived from a short prefix hash plus length (stable for identical bodies).
pub fn content_id(content: &str) -> String {
    let mut hash = 0u64;
    for (i, b) in content.as_bytes().iter().take(4096).enumerate() {
        hash = hash
            .wrapping_mul(131)
            .wrapping_add(*b as u64)
            .wrapping_add(i as u64);
    }
    hash = hash.wrapping_add(content.len() as u64);
    format!("{hash:016x}")
}

pub fn is_valid_id(id: &str) -> bool {
    let len = id.len();
    (8..=64).contains(&len) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read a slice of a stored content-ref body.
pub fn read_stored(
    dir: &Path,
    id: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    if !is_valid_id(id) {
        return Err("id must be 8–64 hex characters".to_string());
    }
    let path = dir.join(id);
    let bytes = std::fs::read(&path).map_err(|e| {
        format!(
            "content_ref {id} not found at {} ({e})",
            path.display()
        )
    })?;
    let offset = offset.unwrap_or(0).min(bytes.len());
    let limit = limit.unwrap_or(DEFAULT_READ_LIMIT);
    let end = offset.saturating_add(limit).min(bytes.len());
    let slice = &bytes[offset..end];
    let text = String::from_utf8_lossy(slice);
    let mut out = format!(
        "[content_ref id={id} bytes={} offset={offset} showing={}/{}]\n{text}",
        bytes.len(),
        slice.len(),
        bytes.len().saturating_sub(offset)
    );
    if end < bytes.len() {
        out.push_str(&format!(
            "\n[truncated — call ReadContentRef again with offset={end} to continue]"
        ));
    }
    Ok(out)
}

fn ensure_store_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(gitignore, "*\n");
    }
}

/// First-class pager for bodies stored by [`persist_content_reference`].
pub struct ReadContentRefTool {
    dir: PathBuf,
}

impl ReadContentRefTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            dir: store_dir(&cwd),
        }
    }

    #[cfg(test)]
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }
}

#[async_trait]
impl Tool for ReadContentRefTool {
    fn name(&self) -> &str {
        "ReadContentRef"
    }

    fn description(&self) -> &str {
        "Page a truncated tool result that was stored as a content_ref. \
         Pass the `id` from `[content_ref id=…]` plus optional byte offset/limit. \
         Prefer this over guessing a filesystem path. The same body is also at \
         `.flowy/content-refs/{id}` if you need Grep."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The hex id from a [content_ref id=…] locator."
                },
                "offset": {
                    "type": "integer",
                    "description": "Byte offset to start reading (default 0)."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum bytes to return (default 32768)."
                }
            },
            "required": ["id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let id = match input.get("id").and_then(Value::as_str) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::error("id is required"),
        };
        let offset = input
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        match read_stored(&self.dir, id, offset, limit) {
            Ok(content) => ToolResult::text(content),
            Err(err) => ToolResult::error(err),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let id = input.get("id").and_then(Value::as_str).unwrap_or("?");
        format!("ReadContentRef {id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persist_and_read_roundtrip() {
        let dir = tempdir().unwrap();
        let locator = persist_content_reference("hello world overflow", Some(dir.path()));
        assert!(locator.contains("[content_ref id="));
        assert!(locator.contains("ReadContentRef"));
        let id = content_id("hello world overflow");
        let store = store_dir(dir.path());
        let page = read_stored(&store, &id, None, None).unwrap();
        assert!(page.contains("hello world overflow"));
    }

    #[test]
    fn rejects_path_traversal_id() {
        let dir = tempdir().unwrap();
        let err = read_stored(dir.path(), "../secret", None, None).unwrap_err();
        assert!(err.contains("hex"));
    }

    #[tokio::test]
    async fn tool_pages_with_offset() {
        let dir = tempdir().unwrap();
        let body = "abcdefghijklmnopqrstuvwxyz";
        persist_content_reference(body, Some(dir.path()));
        let tool = ReadContentRefTool::new(dir.path().to_path_buf());
        let id = content_id(body);
        let result = tool
            .execute(json!({ "id": id, "offset": 10, "limit": 5 }))
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("klmno"));
        assert!(result.content.contains("offset=10"));
    }
}
