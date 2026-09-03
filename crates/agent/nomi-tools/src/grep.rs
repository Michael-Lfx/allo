use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

/// Safety-net wall-clock cap. Prefer early-exit after enough matches over
/// waiting this long — large trees should stop once the line budget is filled.
const GREP_TIMEOUT: Duration = Duration::from_secs(45);

/// Skip giant blobs that rarely contain useful source matches.
const GREP_MAX_FILESIZE: &str = "2M";

/// Stop reading (and kill rg) once we have this many output lines.
const GREP_MAX_LINES: usize = 250;

/// When the caller searches a broad root without a `glob`, restrict to common
/// source/text types so vendor/binary trees are not walked for every hit.
const DEFAULT_SOURCE_GLOBS: &[&str] = &[
    "*.rs", "*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs", "*.py", "*.go",
    "*.java", "*.kt", "*.swift", "*.cs", "*.cpp", "*.cc", "*.cxx", "*.c", "*.h",
    "*.hpp", "*.m", "*.mm", "*.rb", "*.php", "*.scala", "*.toml",
    "*.json", "*.jsonc", "*.yml", "*.yaml", "*.md", "*.mdx", "*.sql", "*.sh",
    "*.ps1", "*.css", "*.scss", "*.html", "*.vue", "*.svelte", "*.proto",
];

/// Default exclusions applied on every ripgrep search.
const DEFAULT_EXCLUDE_GLOBS: &[&str] = &[
    "!**/node_modules/**",
    "!**/.git/**",
    "!**/target/**",
    "!**/dist/**",
    "!**/.next/**",
    "!**/build/**",
    "!**/__pycache__/**",
    "!**/.turbo/**",
    "!**/vendor/**",
    "!**/coverage/**",
    "!**/.cache/**",
    "!**/Pods/**",
];

pub struct GrepTool {
    cwd: PathBuf,
}

impl GrepTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Searches file contents using regex patterns (powered by ripgrep).\n\n\
         IMPORTANT: ALWAYS use this Grep tool for content search. \
         NEVER run grep or rg as a Bash command.\n\n\
         - Supports full regex syntax (e.g., \"log.*Error\", \"fn\\\\s+\\\\w+\").\n\
         - Use the glob parameter to filter by file pattern (e.g., \"*.rs\").\n\
         - Prefer a narrow `path` (subdirectory) on large repos; searching the \
         workspace root without a glob auto-limits to common source file types.\n\
         - Matching lines are formatted as `path:line:hash: content` — the `line:hash` \
         part is an Edit anchor you can copy verbatim.\n\
         - All-lowercase patterns are case-insensitive unless `case_insensitive` is set \
         (so `minimax` matches `MiniMax`). Mixed-case patterns stay case-sensitive.\n\
         - Set context_lines (e.g. 2) to include surrounding lines for each match.\n\
         - Output stops after ~250 matching lines (process is killed early) — \
         refine path/glob rather than asking for more lines.\n\
         - Set case_insensitive to true for case-insensitive search."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: cwd). Prefer a subdirectory on large repos."
                },
                "glob": {
                    "type": "string",
                    "description": "File filter pattern, e.g. \"*.rs\" or \"*.{ts,tsx}\""
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Lines of context to show around each match (rg -C). Default 0."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search"
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

        let raw_path = input["path"].as_str().unwrap_or(".");
        let path = if std::path::Path::new(raw_path).is_relative() {
            self.cwd.join(raw_path).to_string_lossy().into_owned()
        } else {
            raw_path.to_owned()
        };

        tracing::debug!(cwd = %self.cwd.display(), resolved_path = %path, pattern = %pattern, "GrepTool searching");

        // Ensure managed/bundled rg exists before search (download on first need).
        if nomi_config::resolve_rg_executable().is_none() {
            let _ = nomi_config::ensure_runtime_dep(nomi_config::RuntimeDep::Ripgrep, true).await;
        }

        let glob_pattern = input["glob"].as_str();
        let case_insensitive = input["case_insensitive"]
            .as_bool()
            .unwrap_or_else(|| infer_case_insensitive(pattern));
        let context_lines = input["context_lines"].as_u64().unwrap_or(0) as usize;
        let auto_source_globs = glob_pattern.is_none() && is_broad_search_root(&path, &self.cwd);

        let result = try_ripgrep(
            pattern,
            &path,
            glob_pattern,
            auto_source_globs,
            case_insensitive,
            context_lines,
        )
        .await;

        match result {
            Ok(output) => output,
            Err(e) => {
                // Do NOT fall back to Windows `findstr /S` on large trees — it is
                // what made searches look "dumb" (minutes, no output). Prefer a
                // clear rg-missing error so the model installs/uses ripgrep.
                if cfg!(windows) {
                    ToolResult {
                        content: format!(
                            "ripgrep (rg) is required for Grep but was not found ({e}). \
                             Flowy normally ships or auto-installs rg into its data-dir bin; \
                             check network access or set NOMIFUN_AUTO_ENSURE_DEPS=1. \
                             Refusing slow findstr fallback on large workspaces."
                        ),
                        is_error: true,
                        images: Vec::new(),
                    }
                } else {
                    try_grep(pattern, &path, glob_pattern, case_insensitive, context_lines).await
                }
            }
        }
    }

    fn max_result_size(&self) -> usize {
        20_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let raw_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("Grep '{}' in {}", pattern, raw_path)
    }
}

fn infer_case_insensitive(pattern: &str) -> bool {
    let has_letter = pattern.chars().any(|c| c.is_ascii_alphabetic());
    has_letter && !pattern.chars().any(|c| c.is_ascii_uppercase())
}

fn is_broad_search_root(path: &str, cwd: &Path) -> bool {
    let p = Path::new(path);
    if path == "." || path.is_empty() {
        return true;
    }
    let Ok(path_canon) = p.canonicalize() else {
        // Unresolved absolute/relative root of the session is still "broad".
        return p == cwd || cwd.join(path) == *cwd;
    };
    let Ok(cwd_canon) = cwd.canonicalize() else {
        return false;
    };
    path_canon == cwd_canon
}

fn format_grep_output(stdout: &str, max_lines: usize) -> String {
    let mut formatted = String::new();
    let mut total = 0usize;
    for line in stdout.lines() {
        total += 1;
        if total > max_lines {
            continue;
        }
        // ripgrep default: path:line:content  or path-line-content for context
        let enriched = enrich_grep_line_with_anchor(line);
        if !formatted.is_empty() {
            formatted.push('\n');
        }
        formatted.push_str(&enriched);
    }
    if total <= max_lines {
        return formatted;
    }
    format!(
        "{formatted}\n... [truncated: showing first {max_lines} of at least {total} matching lines — narrow your pattern, path, or glob]"
    )
}

/// Inject `line:hash` into ripgrep content lines so Edit can use anchors.
fn enrich_grep_line_with_anchor(line: &str) -> String {
    // Match `path:lineno:rest` (content mode) — avoid Windows drive `C:`
    let bytes = line.as_bytes();
    let mut first_colon = None;
    let mut second_colon = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' {
            if first_colon.is_none() {
                // Skip drive letter `X:`
                if i == 1 && bytes[0].is_ascii_alphabetic() {
                    continue;
                }
                first_colon = Some(i);
            } else {
                second_colon = Some(i);
                break;
            }
        }
    }
    let (Some(c1), Some(c2)) = (first_colon, second_colon) else {
        return line.to_string();
    };
    let line_no = &line[c1 + 1..c2];
    if !line_no.chars().all(|c| c.is_ascii_digit()) || line_no.is_empty() {
        return line.to_string();
    }
    let content = &line[c2 + 1..];
    // Strip optional leading space from rg
    let content_trim = content.strip_prefix(' ').unwrap_or(content);
    let hash = crate::anchors::anchor_line_hash(content_trim);
    format!("{}:{}:{}: {}", &line[..c1], line_no, hash, content_trim)
}

fn apply_default_excludes(cmd: &mut Command) {
    for glob in DEFAULT_EXCLUDE_GLOBS {
        cmd.arg("--glob").arg(glob);
    }
}

fn apply_default_source_globs(cmd: &mut Command) {
    for glob in DEFAULT_SOURCE_GLOBS {
        cmd.arg("--glob").arg(glob);
    }
}

enum CollectOutcome {
    Complete(String),
    /// Hit the line budget and killed the searcher early (success for the agent).
    EarlyCap(String),
    /// Timed out but captured partial stdout (still useful — not a hard failure).
    TimedOutPartial(String),
    TimedOutEmpty,
    Io(std::io::Error),
}

/// Stream stdout and stop as soon as we have enough lines, killing the child.
async fn collect_capped_output(
    mut child: tokio::process::Child,
    max_lines: usize,
    timeout: Duration,
) -> CollectOutcome {
    use std::time::Instant;

    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        return CollectOutcome::Io(std::io::Error::other("missing stdout pipe"));
    };

    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut tmp = [0u8; 8192];
    let mut lines = 0usize;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let text = String::from_utf8_lossy(&buf).into_owned();
            return if text.trim().is_empty() {
                CollectOutcome::TimedOutEmpty
            } else {
                CollectOutcome::TimedOutPartial(text)
            };
        }

        match tokio::time::timeout(remaining, stdout.read(&mut tmp)).await {
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                let text = String::from_utf8_lossy(&buf).into_owned();
                return if text.trim().is_empty() {
                    CollectOutcome::TimedOutEmpty
                } else {
                    CollectOutcome::TimedOutPartial(text)
                };
            }
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                for &b in &tmp[..n] {
                    if b == b'\n' {
                        lines += 1;
                    }
                }
                buf.extend_from_slice(&tmp[..n]);
                if lines >= max_lines {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    return CollectOutcome::EarlyCap(text);
                }
            }
            Ok(Err(e)) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return CollectOutcome::Io(e);
            }
        }
    }

    let status = child.wait().await;
    let text = String::from_utf8_lossy(&buf).into_owned();
    match status {
        Ok(_) => CollectOutcome::Complete(text),
        Err(e) => {
            if text.is_empty() {
                CollectOutcome::Io(e)
            } else {
                CollectOutcome::Complete(text)
            }
        }
    }
}

fn tool_result_from_stdout(stdout: &str, note: Option<&str>) -> ToolResult {
    if stdout.trim().is_empty() {
        return ToolResult {
            content: "No matches found".to_string(),
            is_error: false,
            images: Vec::new(),
        };
    }
    let mut content = format_grep_output(stdout, GREP_MAX_LINES);
    if let Some(note) = note {
        content.push('\n');
        content.push_str(note);
    }
    ToolResult {
        content,
        is_error: false,
        images: Vec::new(),
    }
}

async fn try_ripgrep(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    auto_source_globs: bool,
    case_insensitive: bool,
    context_lines: usize,
) -> Result<ToolResult, std::io::Error> {
    let rg_bin = nomi_config::resolve_rg_executable().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "rg executable not found (managed bin, app bundle, or PATH)",
        )
    })?;

    // CRITICAL: all flags MUST precede PATTERN and PATH.
    let mut cmd = Command::new(&rg_bin);
    cmd.arg("--color=never")
        .arg("-n")
        .arg("--no-heading")
        .arg("--max-filesize")
        .arg(GREP_MAX_FILESIZE)
        .arg("--max-count")
        .arg("20");

    apply_default_excludes(&mut cmd);

    if let Some(g) = glob_pattern {
        for piece in expand_brace_glob(g) {
            cmd.arg("--glob").arg(piece);
        }
    } else if auto_source_globs {
        apply_default_source_globs(&mut cmd);
        cmd.arg("--max-depth").arg("12");
    }

    if case_insensitive {
        cmd.arg("-i");
    }
    if context_lines > 0 {
        cmd.arg("-C").arg(context_lines.to_string());
    }

    cmd.arg("--")
        .arg(pattern)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(e),
    };

    match collect_capped_output(child, GREP_MAX_LINES, GREP_TIMEOUT).await {
        CollectOutcome::Complete(stdout) | CollectOutcome::EarlyCap(stdout) => {
            let note = if auto_source_globs {
                Some(
                    "[note: searched common source globs only because `path` is the workspace root \
                     and no `glob` was set — pass an explicit `glob` or a subdirectory `path` to widen/narrow]",
                )
            } else {
                None
            };
            Ok(tool_result_from_stdout(&stdout, note))
        }
        CollectOutcome::TimedOutPartial(stdout) => Ok(tool_result_from_stdout(
            &stdout,
            Some(
                "[note: search stopped early (time budget) with partial results — \
                 narrow `path` or `glob` and retry]",
            ),
        )),
        CollectOutcome::TimedOutEmpty => Ok(ToolResult {
            content: format!(
                "No matches returned within {}s of scanning `{}`. \
                 Treat as no useful hits for this query, then narrow `path` \
                 (e.g. a single package) or simplify the pattern — do not retry identically.",
                GREP_TIMEOUT.as_secs(),
                path
            ),
            is_error: false,
            images: Vec::new(),
        }),
        CollectOutcome::Io(e) => Err(e),
    }
}

/// Expand a single glob that may contain one `{a,b,c}` brace group into
/// concrete globs. Globs without braces are returned unchanged.
fn expand_brace_glob(glob: &str) -> Vec<String> {
    let Some(start) = glob.find('{') else {
        return vec![glob.to_string()];
    };
    let Some(end_rel) = glob[start + 1..].find('}') else {
        return vec![glob.to_string()];
    };
    let end = start + 1 + end_rel;
    let prefix = &glob[..start];
    let suffix = &glob[end + 1..];
    let inner = &glob[start + 1..end];
    if inner.is_empty() || inner.contains('{') {
        return vec![glob.to_string()];
    }
    let parts: Vec<String> = inner
        .split(',')
        .map(|part| format!("{prefix}{}{suffix}", part.trim()))
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        vec![glob.to_string()]
    } else {
        parts
    }
}

async fn try_grep(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    case_insensitive: bool,
    context_lines: usize,
) -> ToolResult {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("findstr");
        c.arg("/S")
            .arg("/N")
            .arg("/R")
            .arg(pattern)
            .arg(format!("{}\\*", path.trim_end_matches(['\\', '/'])));
        if case_insensitive {
            c.arg("/I");
        }
        c
    } else {
        let mut c = Command::new("grep");
        c.arg("-rn").arg(pattern).arg(path);
        if case_insensitive {
            c.arg("-i");
        }
        if let Some(g) = glob_pattern {
            c.arg(format!("--include={}", g));
        }
        for dir in [
            "node_modules",
            ".git",
            "target",
            "dist",
            ".next",
            "build",
            "__pycache__",
            ".turbo",
            "vendor",
        ] {
            c.arg("--exclude-dir").arg(dir);
        }
        if context_lines > 0 {
            c.arg("-C").arg(context_lines.to_string());
        }
        c
    };
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                content: format!("grep failed: {}", e),
                is_error: true,
                images: Vec::new(),
            };
        }
    };

    match collect_capped_output(child, GREP_MAX_LINES, GREP_TIMEOUT).await {
        CollectOutcome::Complete(stdout) | CollectOutcome::EarlyCap(stdout) => {
            tool_result_from_stdout(&stdout, None)
        }
        CollectOutcome::TimedOutPartial(stdout) => tool_result_from_stdout(
            &stdout,
            Some("[note: search stopped early with partial results — narrow path/glob]"),
        ),
        CollectOutcome::TimedOutEmpty => ToolResult {
            content: format!(
                "Grep hit the {}s safety budget with no lines collected yet. Narrow `path` or `glob`.",
                GREP_TIMEOUT.as_secs()
            ),
            is_error: true,
            images: Vec::new(),
        },
        CollectOutcome::Io(e) => ToolResult {
            content: format!("grep failed: {}", e),
            is_error: true,
            images: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_grep_output_appends_truncation_notice_with_total() {
        let lines: String = (0..300).map(|i| format!("line{i}\n")).collect();
        let out = super::format_grep_output(&lines, 250);
        assert!(out.contains("truncated"), "must announce truncation: {out}");
        assert!(out.contains("300"), "must report the true total match count");
        assert_eq!(out.lines().count(), 251);
    }

    #[test]
    fn format_grep_output_passthrough_when_under_limit() {
        let out = super::format_grep_output("a\nb\n", 250);
        assert_eq!(out, "a\nb");
    }

    #[test]
    fn broad_root_detection_for_dot_and_cwd() {
        let cwd = std::env::temp_dir();
        assert!(is_broad_search_root(".", &cwd));
        assert!(is_broad_search_root(cwd.to_str().unwrap_or("."), &cwd));
    }

    #[test]
    fn expand_brace_glob_splits_extensions() {
        assert_eq!(
            expand_brace_glob("*.{ts,tsx}"),
            vec!["*.ts".to_string(), "*.tsx".to_string()]
        );
        assert_eq!(expand_brace_glob("*.rs"), vec!["*.rs".to_string()]);
    }

    #[tokio::test]
    async fn execute_missing_pattern_is_error() {
        let tool = GrepTool::new(std::env::temp_dir());
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn ripgrep_finds_matches_quickly_when_available() {
        let Some(_rg) = nomi_config::dep_check::resolve_rg_executable() else {
            eprintln!("skip: rg not resolvable");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hit.ts");
        std::fs::write(&file, "const provider = 'deepseek';\n").unwrap();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let started = std::time::Instant::now();
        let result = tool
            .execute(serde_json::json!({
                "pattern": "deepseek|zhipu",
                "path": dir.path().to_string_lossy(),
                "glob": "*.{ts,tsx}",
                "case_insensitive": true
            }))
            .await;
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "grep took too long: {:?}",
            started.elapsed()
        );
        assert!(!result.is_error, "{}", result.content);
        assert!(
            result.content.to_ascii_lowercase().contains("deepseek"),
            "got: {}",
            result.content
        );
    }

    #[test]
    fn lowercase_patterns_infer_case_insensitive() {
        assert!(infer_case_insensitive("minimax"));
        assert!(infer_case_insensitive("deepseek|zhipu"));
        assert!(!infer_case_insensitive("MiniMax"));
        assert!(!infer_case_insensitive("[A-Z]+"));
        assert!(!infer_case_insensitive("123"));
    }
}
