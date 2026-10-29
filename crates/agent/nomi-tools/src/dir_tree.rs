//! Lightweight directory tree listing for coding orientation.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

const DEFAULT_MAX_DEPTH: usize = 3;
const DEFAULT_MAX_ENTRIES: usize = 200;

const SKIP_DIR_NAMES: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    ".next",
    "build",
    "__pycache__",
    ".turbo",
    "vendor",
];

pub struct DirTreeTool {
    cwd: PathBuf,
    extra_roots: Vec<PathBuf>,
}

impl DirTreeTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            extra_roots: Vec::new(),
        }
    }

    pub fn with_extra_roots(mut self, extra_roots: Vec<PathBuf>) -> Self {
        self.extra_roots = extra_roots;
        self
    }
}

fn should_skip(name: &str) -> bool {
    SKIP_DIR_NAMES.contains(&name) || name.starts_with('.')
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    remaining: &mut usize,
    out: &mut Vec<String>,
) -> std::io::Result<()> {
    if depth > max_depth || *remaining == 0 {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *remaining == 0 {
            out.push(format!("{prefix}… (truncated: entry budget reached)"));
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_skip(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        *remaining = remaining.saturating_sub(1);
        if is_dir {
            out.push(format!("{prefix}{name}/"));
            walk(
                &path,
                &format!("{prefix}  "),
                depth + 1,
                max_depth,
                remaining,
                out,
            )?;
        } else {
            out.push(format!("{prefix}{name}"));
        }
    }
    Ok(())
}

#[async_trait]
impl Tool for DirTreeTool {
    fn name(&self) -> &str {
        "DirTree"
    }

    fn description(&self) -> &str {
        "Show a compact directory tree for orientation. Prefer this over shell `find`/`tree` \
         and over broad Glob when you need a high-level layout. Skips common heavy dirs \
         (node_modules, .git, target, …). Use a subdirectory `path` and small `max_depth`. \
         Absolute paths must stay in the session workspace or a skill tree."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (relative to cwd or absolute). Default: cwd."
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Max directory depth (default 3)."
                },
                "max_entries": {
                    "type": "integer",
                    "description": "Max entries to print (default 200)."
                }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let rel = input["path"].as_str().unwrap_or(".");
        let root = match crate::path_guard::resolve_explore_path(
            rel,
            self.cwd.as_path(),
            &self.extra_roots,
        ) {
            Ok(root) => root,
            Err(message) => return ToolResult::error(message),
        };
        let path = PathBuf::from(&root);
        if !path.is_dir() {
            return ToolResult::error(format!("Not a directory: {root}"));
        }
        let max_depth = input["max_depth"]
            .as_u64()
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .clamp(1, 8);
        let mut remaining = input["max_entries"]
            .as_u64()
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_ENTRIES)
            .clamp(10, 2000);

        let mut out = vec![format!("{}/", path.file_name().and_then(|s| s.to_str()).unwrap_or("."))];
        if let Err(e) = walk(&path, "", 1, max_depth, &mut remaining, &mut out) {
            return ToolResult::error(format!("DirTree failed: {e}"));
        }
        ToolResult {
            content: out.join("\n"),
            is_error: false,
            images: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn lists_nested() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "x").unwrap();
        let tool = DirTreeTool::new(dir.path().to_path_buf());
        let res = tool.execute(json!({"path": ".", "max_depth": 2})).await;
        assert!(!res.is_error);
        assert!(res.content.contains("src/"));
        assert!(res.content.contains("a.rs"));
    }

    #[tokio::test]
    async fn rejects_absolute_path_outside_workspace() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        std::fs::write(other.path().join("secret"), "x").unwrap();
        let tool = DirTreeTool::new(dir.path().to_path_buf());
        let res = tool
            .execute(json!({"path": other.path().to_string_lossy()}))
            .await;
        assert!(res.is_error);
        assert!(res.content.contains("outside the session workspace"));
    }

    #[tokio::test]
    async fn allows_configured_skill_tree() {
        let workspace = tempdir().unwrap();
        let skills = tempdir().unwrap();
        std::fs::write(skills.path().join("SKILL.md"), "# skill").unwrap();
        let tool = DirTreeTool::new(workspace.path().to_path_buf())
            .with_extra_roots(vec![skills.path().to_path_buf()]);
        let res = tool
            .execute(json!({"path": skills.path().to_string_lossy(), "max_depth": 1}))
            .await;
        assert!(!res.is_error, "{}", res.content);
        assert!(res.content.contains("SKILL.md"));
    }
}
