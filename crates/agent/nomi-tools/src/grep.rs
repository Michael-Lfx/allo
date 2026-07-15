use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

/// Wall-clock cap for a single Grep invocation (ripgrep / fallback).
/// Aligns with Cursor Shell's default foreground wait; Claude Code uses 20s for rg.
const GREP_TIMEOUT: Duration = Duration::from_secs(30);

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
         - Set context_lines (e.g. 2) to include surrounding lines for each match.\n\
         - Output is capped at 250 lines; when truncated, a notice reports the \
         true total so you can narrow the pattern or glob.\n\
         - Set case_insensitive to true for case-insensitive search.\n\
         - Searches time out after 30 seconds; if that happens, narrow `path` or `glob` \
         instead of retrying the same broad search."
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
                    "description": "Directory to search in (default: cwd)"
                },
                "glob": {
                    "type": "string",
                    "description": "File filter pattern, e.g. \"*.rs\""
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

        let glob_pattern = input["glob"].as_str();
        let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);
        let context_lines = input["context_lines"].as_u64().unwrap_or(0) as usize;

        // Try ripgrep first, fallback to grep
        let result = try_ripgrep(pattern, &path, glob_pattern, case_insensitive, context_lines).await;

        match result {
            Ok(output) => output,
            Err(_) => {
                // Fallback to grep (now also honours glob + context_lines on unix)
                try_grep(pattern, &path, glob_pattern, case_insensitive, context_lines).await
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

const GREP_MAX_LINES: usize = 250;

fn timeout_result() -> ToolResult {
    ToolResult {
        content: format!(
            "Grep timed out after {} seconds. The search may have matched files but did not \
             complete in time. Narrow the `path` or `glob` and try again.",
            GREP_TIMEOUT.as_secs()
        ),
        is_error: true,
        images: Vec::new(),
    }
}

enum TimedCommandError {
    Io(std::io::Error),
    TimedOut,
}

/// Spawn `cmd`, wait up to `timeout`, kill the child on cancel/timeout.
async fn run_timed_command(
    mut cmd: Command,
    timeout: Duration,
) -> Result<std::process::Output, TimedCommandError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = cmd.spawn().map_err(TimedCommandError::Io)?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(TimedCommandError::Io(e)),
        Err(_) => Err(TimedCommandError::TimedOut),
    }
}

/// Cap grep output to `max_lines`, appending a truncation notice with the true
/// total when exceeded — so the model knows results were cut and can narrow the
/// search, instead of silently losing matches.
fn format_grep_output(stdout: &str, max_lines: usize) -> String {
    let total = stdout.lines().count();
    if total <= max_lines {
        return stdout.trim_end().to_string();
    }
    let shown: Vec<&str> = stdout.lines().take(max_lines).collect();
    format!(
        "{}\n... [truncated: showing first {} of {} matching lines — narrow your pattern or set a `glob` filter]",
        shown.join("\n"),
        max_lines,
        total
    )
}

async fn try_ripgrep(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    case_insensitive: bool,
    context_lines: usize,
) -> Result<ToolResult, std::io::Error> {
    let mut cmd = Command::new("rg");
    cmd.arg(pattern).arg(path).arg("-n");

    if let Some(g) = glob_pattern {
        cmd.arg("--glob").arg(g);
    }
    if case_insensitive {
        cmd.arg("-i");
    }
    if context_lines > 0 {
        cmd.arg("-C").arg(context_lines.to_string());
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = match run_timed_command(cmd, GREP_TIMEOUT).await {
        Ok(output) => output,
        // Timeout is a completed tool error — do not fall back to findstr/grep
        // (that would also hang on the same huge tree).
        Err(TimedCommandError::TimedOut) => return Ok(timeout_result()),
        Err(TimedCommandError::Io(e)) => return Err(e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(1) && stdout.is_empty() {
        return Ok(ToolResult {
            content: "No matches found".to_string(),
            is_error: false,
            images: Vec::new(),
        });
    }

    if !output.status.success() && output.status.code() != Some(1) {
        return Ok(ToolResult {
            content: format!("rg error: {}", stderr),
            is_error: true,
            images: Vec::new(),
        });
    }

    Ok(ToolResult {
        content: format_grep_output(&stdout, GREP_MAX_LINES),
        is_error: false,
        images: Vec::new(),
    })
}

async fn try_grep(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    case_insensitive: bool,
    context_lines: usize,
) -> ToolResult {
    let mut cmd = if cfg!(windows) {
        // findstr has no glob-include or context-line support; those refinements
        // are silently unavailable on the Windows fallback path.
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
        // Honour the glob filter on the fallback path too (previously ignored,
        // so the model got matches from unintended file types).
        if let Some(g) = glob_pattern {
            c.arg(format!("--include={}", g));
        }
        if context_lines > 0 {
            c.arg("-C").arg(context_lines.to_string());
        }
        c
    };
    // CREATE_NO_WINDOW (covers the Windows `findstr` branch above).
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    match run_timed_command(cmd, GREP_TIMEOUT).await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.is_empty() {
                ToolResult {
                    content: "No matches found".to_string(),
                    is_error: false,
                    images: Vec::new(),
                }
            } else {
                ToolResult {
                    content: format_grep_output(&stdout, GREP_MAX_LINES),
                    is_error: false,
                    images: Vec::new(),
                }
            }
        }
        Err(TimedCommandError::TimedOut) => timeout_result(),
        Err(TimedCommandError::Io(e)) => ToolResult {
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
        // 250 shown lines + 1 notice line
        assert_eq!(out.lines().count(), 251);
    }

    #[test]
    fn format_grep_output_short_is_unchanged() {
        let out = super::format_grep_output("a\nb\nc\n", 250);
        assert_eq!(out, "a\nb\nc");
    }

    #[tokio::test]
    async fn grep_tool_finds_pattern_in_own_source() {
        let tool = GrepTool::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let input = json!({
            "pattern": "GrepTool",
            "path": env!("CARGO_MANIFEST_DIR")
        });
        let result = tool.execute(input).await;
        assert!(!result.is_error, "grep failed: {}", result.content);
        assert!(result.content.contains("GrepTool"));
    }

    #[tokio::test]
    async fn execute_uses_cwd_for_relative_path() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("searchable.txt"), "unique_grep_marker_xyz").unwrap();

        let tool = GrepTool::new(tmp.path().to_path_buf());
        let input = json!({"pattern": "unique_grep_marker_xyz", "path": "."});
        let result = tool.execute(input).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(
            result.content.contains("unique_grep_marker_xyz"),
            "should find pattern, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn timed_command_times_out_and_kills_child() {
        let cmd = if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            #[cfg(windows)]
            c.creation_flags(0x0800_0000);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };

        let started = std::time::Instant::now();
        let result = run_timed_command(cmd, Duration::from_millis(400)).await;
        let elapsed = started.elapsed();

        assert!(
            matches!(result, Err(TimedCommandError::TimedOut)),
            "expected TimedOut, got {:?}",
            result.err().map(|e| match e {
                TimedCommandError::TimedOut => "TimedOut".to_string(),
                TimedCommandError::Io(err) => format!("Io({err})"),
            })
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout should fire quickly, took {elapsed:?}"
        );
    }
}
