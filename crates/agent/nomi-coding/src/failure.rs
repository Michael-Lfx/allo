//! Structured tool-failure context for the next model turn.

use crate::edit_hints::{EditFailureKind, infer_edit_failure_kind};

/// Coarse failure class shown to the model with a recommended next action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFailureClass {
    Schema,
    NotFound,
    Timeout,
    Permission,
    StaleAnchor,
    VerifyFail,
    Other,
}

impl ToolFailureClass {
    pub fn from_tool(name: &str, error: &str) -> Self {
        let lower = error.to_ascii_lowercase();
        if looks_like_schema_error(&lower) {
            return Self::Schema;
        }
        if looks_like_not_found(name, &lower) {
            return Self::NotFound;
        }
        if looks_like_timeout(&lower) {
            return Self::Timeout;
        }
        if lower.contains("permission") || lower.contains("eacces") || lower.contains("access is denied")
        {
            return Self::Permission;
        }
        if matches!(name, "Edit" | "Write" | "ApplyPatch") {
            match infer_edit_failure_kind(error) {
                EditFailureKind::StaleAfterRead | EditFailureKind::OldStringNotFound => {
                    return Self::StaleAnchor;
                }
                EditFailureKind::MustReadFirst => return Self::StaleAnchor,
                _ => {}
            }
        }
        if matches!(name, "Bash" | "exec_command")
            && (lower.contains("failed") || lower.contains("error"))
        {
            return Self::VerifyFail;
        }
        Self::Other
    }

    pub fn recommended_action(self) -> &'static str {
        match self {
            Self::Schema => {
                "Fix the tool arguments against the schema. Do not re-Read the same file range."
            }
            Self::NotFound => {
                "Check the path with Glob/Grep. Do not retry the identical Read."
            }
            Self::Timeout => {
                "Retry with a longer timeout or exec_command polling. Do not re-Read the whole file."
            }
            Self::Permission => {
                "Stay inside the write root / cwd. Ask the user if a wider path is required."
            }
            Self::StaleAnchor => {
                "Retry Edit with the fresh line:hash anchors from the failed result. Do not re-Read the whole file."
            }
            Self::VerifyFail => {
                "Inspect the failing command output, fix the code, and rerun the same narrow verify command. Cap retries at 3."
            }
            Self::Other => "Inspect the error, change strategy, and do not repeat the identical call.",
        }
    }
}

fn looks_like_schema_error(lower: &str) -> bool {
    lower.contains("missing required parameter")
        || lower.contains("invalid argument")
        || lower.contains("json schema")
        || lower.contains("schema validation")
        || lower.contains("failed json-schema")
}

fn looks_like_not_found(name: &str, lower: &str) -> bool {
    if lower.contains("no such file")
        || lower.contains("cannot find the path")
        || lower.contains("file not found")
        || lower.contains("directory not found")
    {
        return true;
    }
    matches!(name, "Read" | "Glob" | "Grep" | "DirTree" | "Write" | "Edit")
        && (lower.contains("not found") || lower.contains("does not exist"))
}

fn looks_like_timeout(lower: &str) -> bool {
    if lower.contains("skipped because a previous tool") {
        return false;
    }
    lower.contains("timed out") || lower.contains("command timed out")
}

/// True when an extra `Coding tool-failure` user message would mislead more
/// than it helps. The original tool result is still in the transcript.
pub fn skip_failure_nudge(name: &str, error: &str) -> bool {
    if name.eq_ignore_ascii_case("write_stdin") && error.contains("unknown or finished session_id")
    {
        return true;
    }
    matches!(name, "Bash" | "exec_command") && nonzero_exit_with_success_stdout(error)
}

fn nonzero_exit_with_success_stdout(error: &str) -> bool {
    let has_fail_exit = error.contains("Exit code: 1") || error.contains("exit_code=1");
    if !has_fail_exit {
        return false;
    }
    let stdout = stdout_section(error);
    let stderr = stderr_section(error);
    !stdout.trim().is_empty() && !stderr_has_meaningful_text(stderr)
}

fn stdout_section(error: &str) -> &str {
    let Some(rest) = error.splitn(2, "STDOUT:").nth(1) else {
        return "";
    };
    rest.split("STDERR:").next().unwrap_or(rest)
}

fn stderr_section(error: &str) -> &str {
    error.splitn(2, "STDERR:").nth(1).unwrap_or("")
}

fn stderr_has_meaningful_text(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed
                .chars()
                .all(|c| c == '+' || c == '~' || c == '^' || c.is_whitespace())
    })
}

pub fn failure_nudge(name: &str, error: &str) -> String {
    let class = ToolFailureClass::from_tool(name, error);
    format!(
        "Coding tool-failure ({name}, {class:?}): {}. {}",
        error.chars().take(240).collect::<String>(),
        class.recommended_action()
    )
}

pub fn failure_nudge_if_useful(name: &str, error: &str) -> Option<String> {
    if skip_failure_nudge(name, error) {
        None
    } else {
        Some(failure_nudge(name, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_anchor_points_at_retry_not_reread() {
        let n = failure_nudge("Edit", "stale anchor 12:abcd");
        assert!(n.contains("fresh line:hash"));
        assert!(n.contains("Do not re-Read"));
    }

    #[test]
    fn json_in_success_stdout_is_not_schema() {
        let err = "Exit code: 1\nSTDOUT:\n快照已更新：C:\\tmp\\modelsdev.json\n  minimax=7\n\nSTDERR:\n\n";
        assert_eq!(ToolFailureClass::from_tool("Bash", err), ToolFailureClass::Other);
        assert!(skip_failure_nudge("Bash", err));
        assert!(failure_nudge_if_useful("Bash", err).is_none());
    }

    #[test]
    fn missing_parameter_is_schema() {
        assert_eq!(
            ToolFailureClass::from_tool("Read", "Missing required parameter: file_path"),
            ToolFailureClass::Schema
        );
    }

    #[test]
    fn cascade_skip_is_not_timeout() {
        let err = "Skipped because a previous tool call in this assistant turn failed. \
                   Retry with a larger timeout or exec_command polling.";
        assert_eq!(ToolFailureClass::from_tool("Edit", err), ToolFailureClass::Other);
    }

    #[test]
    fn exec_log_mentioning_not_found_is_not_path_lookup() {
        let err = "(process exited, exit_code=1)\nSTDERR:\nmodule not found: foo";
        assert_eq!(
            ToolFailureClass::from_tool("exec_command", err),
            ToolFailureClass::Other
        );
    }

    #[test]
    fn stale_write_stdin_session_skips_nudge() {
        assert!(skip_failure_nudge(
            "write_stdin",
            "write_stdin: unknown or finished session_id=1"
        ));
    }

    #[test]
    fn empty_bash_capture_still_nudges() {
        let err = "Exit code: 1\nSTDOUT:\n\nSTDERR:\n\n";
        assert!(!skip_failure_nudge("Bash", err));
    }
}
