use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};
use walkdir::{DirEntry, WalkDir};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

const MAX_RESULTS: usize = 100;

/// Stop walking after this many candidate paths so Glob cannot hang forever on
/// enormous source trees. Vendor dirs are skipped *before* descent, so this
/// cap no longer burns out on `node_modules`.
const MAX_WALKED: usize = 50_000;

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

fn should_skip_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| SKIP_DIR_NAMES.contains(&name))
}

fn relative_slash_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn glob_matches(pattern: &str, relative: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let relative = relative.replace('\\', "/");
    // Patterns without a path segment (`*.rs`, `Cargo.toml`) are root-only.
    // The `glob` crate otherwise lets `*` consume `/` on Windows.
    if !pattern.contains('/') && !pattern.contains("**") {
        if relative.contains('/') {
            return false;
        }
        return glob::Pattern::new(&pattern)
            .ok()
            .is_some_and(|matcher| matcher.matches(&relative));
    }
    let Ok(matcher) = glob::Pattern::new(&pattern) else {
        return false;
    };
    if matcher.matches(&relative) {
        return true;
    }
    let Some(rest) = pattern.strip_prefix("**/") else {
        return false;
    };
    let Ok(inner) = glob::Pattern::new(rest) else {
        return false;
    };
    inner.matches(&relative)
        || relative
            .rsplit('/')
            .next()
            .is_some_and(|name| inner.matches(name))
}

pub struct GlobTool {
    cwd: PathBuf,
}

impl GlobTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Fast OS-agnostic file pattern matching tool that works with any codebase size.\n\n\
         - Supports glob patterns like \"**/*.rs\" or \"src/**/*.ts\".\n\
         - Returns matching file paths sorted by modification time (newest first).\n\
         - Returns at most 100 results. Only returns files, not directories.\n\
         - Skips common vendor/build directories during the walk (node_modules, target, dist, …) \
         rather than scanning them and filtering afterwards.\n\
         - The path parameter defaults to the current working directory.\n\
         - Use this OS-agnostic tool to list files in the current directory or workspace on every operating system: \"*\" lists top-level files and \"**/*\" lists files recursively.\n\
         - Use this tool when you need to find files by name or extension patterns, and prefer it over Bash for directory file listings."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern, e.g. \"**/*.rs\""
                },
                "path": {
                    "type": "string",
                    "description": "Root directory (default: cwd)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(pattern) = input["pattern"].as_str() else {
            return ToolResult {
                content: "Missing required parameter: pattern".to_string(),
                is_error: true,
                images: Vec::new(),
            };
        };

        let root = input["path"].as_str().unwrap_or(".");
        let root_path = if Path::new(root).is_relative() {
            self.cwd.join(root)
        } else {
            PathBuf::from(root)
        };

        tracing::debug!(cwd = %self.cwd.display(), resolved_root = %root_path.display(), pattern = %pattern, "GlobTool scanning");

        let pattern = pattern.replace('\\', "/");
        if glob::Pattern::new(&pattern).is_err() {
            return ToolResult {
                content: format!("Invalid glob pattern: {pattern}"),
                is_error: true,
                images: Vec::new(),
            };
        }

        let mut files: Vec<(std::time::SystemTime, String)> = Vec::new();
        let mut total_matched = 0usize;
        let mut walked = 0usize;
        let mut walk_capped = false;

        let walker = WalkDir::new(&root_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !should_skip_dir(entry));

        for entry in walker {
            walked += 1;
            if walked > MAX_WALKED {
                walk_capped = true;
                break;
            }
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let relative = relative_slash_path(path, &root_path);
            if !glob_matches(&pattern, &relative) {
                continue;
            }
            total_matched += 1;
            if files.len() >= MAX_RESULTS {
                continue;
            }

            let mtime = path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            files.push((mtime, relative));
        }

        // Sort by modification time, newest first
        files.sort_by_key(|f| std::cmp::Reverse(f.0));

        if files.is_empty() {
            return ToolResult {
                content: if walk_capped {
                    format!(
                        "No files matched the pattern before the walk limit ({MAX_WALKED} paths). \
                         Narrow `path` or `pattern` (and avoid scanning vendor trees)."
                    )
                } else {
                    "No files matched the pattern".to_string()
                },
                is_error: false,
                images: Vec::new(),
            };
        }

        let mut result: Vec<String> = files.into_iter().map(|(_, path)| path).collect();
        if total_matched > MAX_RESULTS {
            result.push(format!(
                "... [showing {} of {} matching files — refine the pattern or path]",
                MAX_RESULTS, total_matched
            ));
        } else if walk_capped {
            result.push(format!(
                "... [walk stopped after {MAX_WALKED} paths — refine the pattern or path]"
            ));
        }
        ToolResult {
            content: result.join("\n"),
            is_error: false,
            images: Vec::new(),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
        format!("Search for {}", pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    use nomi_types::tool::ToolResult;

    async fn run_glob(pattern: &str, path: &str) -> ToolResult {
        let tool = GlobTool::new(PathBuf::from(path));
        let input = json!({ "pattern": pattern, "path": path });
        tool.execute(input).await
    }

    #[tokio::test]
    async fn glob_reports_truncation_with_true_total() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        let n = super::MAX_RESULTS + 5;
        for i in 0..n {
            fs::write(base.join(format!("f{i}.rs")), "x").unwrap();
        }
        let result = run_glob("*.rs", base.to_str().unwrap()).await;
        assert!(!result.is_error, "glob should succeed: {}", result.content);
        assert!(
            result.content.contains(&n.to_string()),
            "must report the true total {n}, got: {}",
            result.content
        );
        assert!(
            result.content.to_lowercase().contains("truncat")
                || result.content.contains("showing"),
            "must announce truncation, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_glob_matches_pattern() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        fs::write(base.join("main.rs"), "fn main() {}").unwrap();
        fs::write(base.join("lib.rs"), "pub mod lib;").unwrap();
        fs::write(base.join("notes.txt"), "some notes").unwrap();
        fs::write(base.join("readme.md"), "# Readme").unwrap();

        let result = run_glob("*.rs", base.to_str().unwrap()).await;

        assert!(!result.is_error, "glob should succeed");
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 2, "should match exactly 2 .rs files");
        for line in &lines {
            assert!(
                line.ends_with(".rs"),
                "each match should be a .rs file, got: {}",
                line
            );
        }
        assert!(
            !result.content.contains("notes.txt"),
            "should not include .txt files"
        );
        assert!(
            !result.content.contains("readme.md"),
            "should not include .md files"
        );
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        fs::write(base.join("main.rs"), "fn main() {}").unwrap();
        fs::write(base.join("lib.rs"), "pub mod lib;").unwrap();

        let result = run_glob("*.xyz", base.to_str().unwrap()).await;

        assert!(!result.is_error, "no-match glob should not be an error");
        assert_eq!(result.content, "No files matched the pattern");
    }

    #[tokio::test]
    async fn test_glob_with_limit() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        for i in 0..5 {
            fs::write(
                base.join(format!("file_{}.txt", i)),
                format!("content {}", i),
            )
            .unwrap();
        }

        let result = run_glob("*.txt", base.to_str().unwrap()).await;

        assert!(!result.is_error, "glob should succeed");
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 5, "all 5 files should be returned");
    }

    #[tokio::test]
    async fn test_glob_recursive() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create nested directory structure
        let sub_a = base.join("a");
        let sub_b = base.join("a").join("b");
        fs::create_dir_all(&sub_b).unwrap();

        fs::write(base.join("root.txt"), "root level").unwrap();
        fs::write(sub_a.join("mid.txt"), "middle level").unwrap();
        fs::write(sub_b.join("deep.txt"), "deep level").unwrap();
        // Non-matching file
        fs::write(sub_a.join("skip.rs"), "not a txt").unwrap();

        let result = run_glob("**/*.txt", base.to_str().unwrap()).await;

        assert!(!result.is_error, "recursive glob should succeed");
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 3, "should find 3 .txt files across all levels");
        assert!(
            result.content.contains("root.txt"),
            "should include root-level file"
        );
        assert!(
            result.content.contains("mid.txt"),
            "should include mid-level file"
        );
        assert!(
            result.content.contains("deep.txt"),
            "should include deep-level file"
        );
        assert!(
            !result.content.contains("skip.rs"),
            "should not include .rs files"
        );
    }

    #[tokio::test]
    async fn execute_uses_cwd_for_relative_path() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("marker.txt"), "hello").unwrap();

        let tool = GlobTool::new(tmp.path().to_path_buf());
        let input = json!({"pattern": "marker.txt"});
        let result = tool.execute(input).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(
            result.content.contains("marker.txt"),
            "should find marker.txt, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn glob_does_not_descend_into_node_modules() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        let vendor = base.join("node_modules").join("pkg");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("icons.generated.ts"), "vendor").unwrap();
        fs::write(base.join("icons.generated.ts"), "ok").unwrap();

        let started = std::time::Instant::now();
        let result = run_glob("**/icons.generated.ts", base.to_str().unwrap()).await;
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "vendor skip should keep glob fast, took {:?}",
            started.elapsed()
        );
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(
            result.content.contains("icons.generated.ts"),
            "should find the workspace file, got: {}",
            result.content
        );
        assert!(
            !result.content.contains("node_modules"),
            "must not return vendor hits, got: {}",
            result.content
        );
    }

    #[test]
    fn glob_matches_starstar_at_root() {
        assert!(glob_matches("**/*.txt", "root.txt"));
        assert!(glob_matches("**/*.txt", "a/b.txt"));
        assert!(!glob_matches("*.txt", "a/b.txt"));
        assert!(glob_matches("**/icons.generated.ts", "apps/desktop/icons.generated.ts"));
    }
}
