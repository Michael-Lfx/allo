use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use ignore::{DirEntry, WalkBuilder};
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

const MAX_RESULTS: usize = 100;

/// Hard stop on visited entries so Glob cannot hang on enormous trees. Ignored
/// directories are pruned before descent, so this only trips on genuinely
/// large, non-ignored trees.
const MAX_WALKED: usize = 50_000;

const MAX_WALK_TIME: Duration = Duration::from_secs(10);
const TIME_CHECK_INTERVAL: usize = 512;
const GLOB_META: &[char] = &['*', '?', '[', ']', '{', '}'];

const SKIP_DIR_NAMES: &[&str] = &["node_modules", ".git", "target", "__pycache__"];

fn should_skip_dir(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return false;
    }
    entry
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

/// Leading directory segments of `pattern` that contain no glob syntax. The walk
/// can start there instead of at the search root.
fn literal_dir_prefix(pattern: &str) -> Option<String> {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let segments = pattern.split('/').collect::<Vec<_>>();
    let (_, dirs) = segments.split_last()?;
    let literal = dirs
        .iter()
        .take_while(|segment| !segment.is_empty() && !segment.contains(GLOB_META))
        .copied()
        .collect::<Vec<_>>();
    if literal.is_empty() || literal.contains(&"..") {
        return None;
    }
    Some(literal.join("/"))
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum WalkStop {
    Completed,
    EntryLimit,
    TimeLimit,
}

struct WalkOutcome {
    matches: Vec<(SystemTime, String)>,
    stop: WalkStop,
    heaviest_top_level: Option<(String, usize)>,
}

fn collect_matches(
    root: &Path,
    walk_root: &Path,
    pattern: &str,
    max_walked: usize,
) -> WalkOutcome {
    let started = Instant::now();
    let mut matches = Vec::new();
    let mut walked = 0usize;
    let mut stop = WalkStop::Completed;
    let mut per_top_level: HashMap<String, usize> = HashMap::new();

    let walker = WalkBuilder::new(walk_root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| !should_skip_dir(entry))
        .build();

    for entry in walker {
        walked += 1;
        if walked > max_walked {
            stop = WalkStop::EntryLimit;
            break;
        }
        if walked.is_multiple_of(TIME_CHECK_INTERVAL) && started.elapsed() > MAX_WALK_TIME {
            stop = WalkStop::TimeLimit;
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let relative = relative_slash_path(path, root);
        if let Some(top) = relative.split('/').next().filter(|top| *top != relative) {
            *per_top_level.entry(top.to_owned()).or_default() += 1;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if !glob_matches(pattern, &relative) {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        matches.push((mtime, relative));
    }

    let heaviest_top_level = per_top_level.into_iter().max_by_key(|(_, count)| *count);
    WalkOutcome {
        matches,
        stop,
        heaviest_top_level,
    }
}

fn stop_hint(outcome: &WalkOutcome) -> String {
    let reason = match outcome.stop {
        WalkStop::Completed => return String::new(),
        WalkStop::EntryLimit => format!("walk stopped after {MAX_WALKED} paths"),
        WalkStop::TimeLimit => format!("walk stopped after {}s", MAX_WALK_TIME.as_secs()),
    };
    let heavy = outcome
        .heaviest_top_level
        .as_ref()
        .map(|(name, count)| format!("; most paths were under `{name}/` ({count})"))
        .unwrap_or_default();
    format!("{reason}{heavy} — narrow `path` or put a literal directory prefix in `pattern`")
}

pub struct GlobTool {
    cwd: PathBuf,
}

impl GlobTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

fn error_result(content: String) -> ToolResult {
    ToolResult {
        content,
        is_error: true,
        images: Vec::new(),
    }
}

fn ok_result(content: String) -> ToolResult {
    ToolResult {
        content,
        is_error: false,
        images: Vec::new(),
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
         - Respects .gitignore, and always skips .git, node_modules, target and __pycache__. Files in ignored \
         directories are not listed; pass `path` explicitly to search inside one.\n\
         - A literal directory prefix in the pattern (\"docs/release/**\") narrows the walk to that directory.\n\
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
            return error_result("Missing required parameter: pattern".to_string());
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
            return error_result(format!("Invalid glob pattern: {pattern}"));
        }
        if !root_path.is_dir() {
            return error_result(format!(
                "Search path is not an existing directory: {}",
                root_path.display()
            ));
        }

        let walk_root = match literal_dir_prefix(&pattern) {
            Some(prefix) => {
                let candidate = root_path.join(&prefix);
                if !candidate.is_dir() {
                    return ok_result(format!(
                        "No files matched the pattern: directory `{prefix}` does not exist under {}",
                        root_path.display()
                    ));
                }
                candidate
            }
            None => root_path.clone(),
        };

        let walk_pattern = pattern.clone();
        let walk_base = root_path.clone();
        let outcome = match tokio::task::spawn_blocking(move || {
            collect_matches(&walk_base, &walk_root, &walk_pattern, MAX_WALKED)
        })
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => return error_result(format!("Glob walk failed: {error}")),
        };

        let hint = stop_hint(&outcome);
        let total_matched = outcome.matches.len();
        let mut files = outcome.matches;
        files.sort_by_key(|file| std::cmp::Reverse(file.0));

        if files.is_empty() {
            return ok_result(if hint.is_empty() {
                "No files matched the pattern".to_string()
            } else {
                format!("No files matched the pattern before the walk stopped ({hint}).")
            });
        }

        files.truncate(MAX_RESULTS);
        let mut result: Vec<String> = files.into_iter().map(|(_, path)| path).collect();
        if total_matched > MAX_RESULTS {
            result.push(format!(
                "... [showing {MAX_RESULTS} of {total_matched} matching files — refine the pattern or path]"
            ));
        }
        if !hint.is_empty() {
            result.push(format!("... [{hint}]"));
        }
        ok_result(result.join("\n"))
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

    fn lines(result: &ToolResult) -> Vec<&str> {
        result.content.lines().collect()
    }

    #[tokio::test]
    async fn glob_respects_gitignore_without_a_git_repo() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join(".gitignore"), "generated/\n").unwrap();
        fs::create_dir_all(base.join("generated")).unwrap();
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("generated").join("out.txt"), "x").unwrap();
        fs::write(base.join("src").join("keep.txt"), "x").unwrap();

        let result = run_glob("**/*.txt", base.to_str().unwrap()).await;

        assert_eq!(lines(&result), vec!["src/keep.txt"], "{}", result.content);
    }

    #[tokio::test]
    async fn glob_searches_directories_named_build_dist_and_vendor() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        for name in ["build", "dist", "vendor"] {
            fs::create_dir_all(base.join(name)).unwrap();
            fs::write(base.join(name).join("script.sh"), "x").unwrap();
        }

        let result = run_glob("**/*.sh", base.to_str().unwrap()).await;

        let mut listed = lines(&result);
        listed.sort_unstable();
        assert_eq!(
            listed,
            vec!["build/script.sh", "dist/script.sh", "vendor/script.sh"],
            "{}",
            result.content
        );
    }

    #[tokio::test]
    async fn explicit_path_can_search_inside_a_skipped_directory() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        let target = base.join("target").join("debug");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("app.exe"), "x").unwrap();

        let result = run_glob("*.exe", target.to_str().unwrap()).await;

        assert_eq!(lines(&result), vec!["app.exe"], "{}", result.content);
    }

    #[tokio::test]
    async fn literal_prefix_narrows_the_walk_and_keeps_relative_paths() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("docs").join("release")).unwrap();
        fs::create_dir_all(base.join("other")).unwrap();
        fs::write(base.join("docs").join("release").join("a.md"), "x").unwrap();
        fs::write(base.join("docs").join("b.md"), "x").unwrap();
        fs::write(base.join("other").join("c.md"), "x").unwrap();

        let result = run_glob("docs/release/**", base.to_str().unwrap()).await;

        assert_eq!(lines(&result), vec!["docs/release/a.md"], "{}", result.content);
    }

    #[tokio::test]
    async fn missing_literal_directory_is_reported_distinctly() {
        let dir = tempdir().unwrap();

        let result = run_glob("docs/release/**", dir.path().to_str().unwrap()).await;

        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("`docs/release` does not exist"), "{}", result.content);
    }

    #[tokio::test]
    async fn missing_search_path_is_an_error() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");

        let result = run_glob("*.rs", missing.to_str().unwrap()).await;

        assert!(result.is_error, "{}", result.content);
        assert!(result.content.contains("not an existing directory"), "{}", result.content);
    }

    #[tokio::test]
    async fn results_are_the_newest_matches_not_the_first_walked() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        let total = MAX_RESULTS + 20;
        for index in 0..total {
            let file = fs::File::create(base.join(format!("f{index:04}.rs"))).unwrap();
            let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + index as u64);
            file.set_modified(mtime).unwrap();
        }

        let result = run_glob("*.rs", base.to_str().unwrap()).await;

        let listed = lines(&result);
        assert_eq!(listed[0], format!("f{:04}.rs", total - 1));
        assert!(!listed.contains(&"f0000.rs"), "oldest file must be cut: {}", result.content);
    }

    #[test]
    fn entry_limit_names_the_heaviest_top_level_directory() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("big")).unwrap();
        fs::create_dir_all(base.join("small")).unwrap();
        for index in 0..40 {
            fs::write(base.join("big").join(format!("{index}.log")), "x").unwrap();
        }
        fs::write(base.join("small").join("a.log"), "x").unwrap();

        let outcome = collect_matches(base, base, "**/*.nothing", 20);

        assert!(outcome.stop == WalkStop::EntryLimit);
        assert_eq!(outcome.heaviest_top_level.as_ref().map(|(name, _)| name.as_str()), Some("big"));
        let hint = stop_hint(&outcome);
        assert!(hint.contains("`big/`"), "{hint}");
    }

    #[test]
    fn literal_dir_prefix_stops_at_the_first_glob_segment() {
        assert_eq!(literal_dir_prefix("docs/release/**").as_deref(), Some("docs/release"));
        assert_eq!(literal_dir_prefix("./docs/*.md").as_deref(), Some("docs"));
        assert_eq!(literal_dir_prefix("docs/a/b.md").as_deref(), Some("docs/a"));
        assert_eq!(literal_dir_prefix("**/*.rs"), None);
        assert_eq!(literal_dir_prefix("*.rs"), None);
        assert_eq!(literal_dir_prefix("../x/*.rs"), None);
        assert_eq!(literal_dir_prefix("src/{a,b}/*.rs").as_deref(), Some("src"));
    }

    #[test]
    fn glob_matches_starstar_at_root() {
        assert!(glob_matches("**/*.txt", "root.txt"));
        assert!(glob_matches("**/*.txt", "a/b.txt"));
        assert!(!glob_matches("*.txt", "a/b.txt"));
        assert!(glob_matches("**/icons.generated.ts", "apps/desktop/icons.generated.ts"));
    }
}
