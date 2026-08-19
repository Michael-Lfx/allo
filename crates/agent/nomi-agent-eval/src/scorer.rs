//! Deterministic scorers over [`TurnTranscript`].

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::types::{ScorerResult, ScorerSpec, TurnTranscript};
use crate::workspace::safe_join;

/// Apply every scorer; overall success is the conjunction of all results.
pub fn score_all(specs: &[ScorerSpec], transcript: &TurnTranscript) -> (bool, Vec<ScorerResult>) {
    let results: Vec<ScorerResult> = specs.iter().map(|s| score_one(s, transcript)).collect();
    let success = results.iter().all(|r| r.passed);
    (success, results)
}

pub fn score_one(spec: &ScorerSpec, transcript: &TurnTranscript) -> ScorerResult {
    match spec {
        ScorerSpec::AssistantContains {
            marker,
            minimum_hits,
        } => {
            let hits = count_contains(&transcript.assistant_text, marker);
            ScorerResult {
                scorer_type: "assistant_contains".into(),
                passed: hits >= *minimum_hits,
                detail: Some(format!("hits={hits} minimum={minimum_hits} marker={marker}")),
            }
        }
        ScorerSpec::AssistantNotContains { marker } => {
            let hits = count_contains(&transcript.assistant_text, marker);
            ScorerResult {
                scorer_type: "assistant_not_contains".into(),
                passed: hits == 0,
                detail: Some(format!("hits={hits} marker={marker}")),
            }
        }
        ScorerSpec::ToolCalled { name } => {
            let called = transcript.tool_names.iter().any(|n| n == name);
            ScorerResult {
                scorer_type: "tool_called".into(),
                passed: called,
                detail: Some(format!("name={name} called={called}")),
            }
        }
        ScorerSpec::ToolNotCalled { name } => {
            let called = transcript.tool_names.iter().any(|n| n == name);
            ScorerResult {
                scorer_type: "tool_not_called".into(),
                passed: !called,
                detail: Some(format!("name={name} called={called}")),
            }
        }
        ScorerSpec::MaxToolCalls { max } => {
            let count = transcript.tool_names.len() as u32;
            ScorerResult {
                scorer_type: "max_tool_calls".into(),
                passed: count <= *max,
                detail: Some(format!("count={count} max={max}")),
            }
        }
        ScorerSpec::MaxTurns { max } => ScorerResult {
            scorer_type: "max_turns".into(),
            passed: transcript.turns <= *max,
            detail: Some(format!("turns={} max={max}", transcript.turns)),
        },
        ScorerSpec::RegexMatch {
            pattern,
            minimum_hits,
        } => match Regex::new(pattern) {
            Ok(re) => {
                let hits = re.find_iter(&transcript.assistant_text).count();
                ScorerResult {
                    scorer_type: "regex_match".into(),
                    passed: hits >= *minimum_hits,
                    detail: Some(format!("hits={hits} minimum={minimum_hits}")),
                }
            }
            Err(e) => ScorerResult {
                scorer_type: "regex_match".into(),
                passed: false,
                detail: Some(format!("invalid pattern: {e}")),
            },
        },
        ScorerSpec::FileContains { path, marker } => match read_workspace_file(transcript, path) {
            Ok(text) => {
                let hits = count_contains(&text, marker);
                ScorerResult {
                    scorer_type: "file_contains".into(),
                    passed: hits >= 1,
                    detail: Some(format!("path={path} hits={hits}")),
                }
            }
            Err(detail) => ScorerResult {
                scorer_type: "file_contains".into(),
                passed: false,
                detail: Some(detail),
            },
        },
        ScorerSpec::FileExists { path } => match workspace_file(transcript, path) {
            Ok(full) => ScorerResult {
                scorer_type: "file_exists".into(),
                passed: full.is_file(),
                detail: Some(format!("path={path} exists={}", full.is_file())),
            },
            Err(detail) => ScorerResult {
                scorer_type: "file_exists".into(),
                passed: false,
                detail: Some(detail),
            },
        },
        ScorerSpec::CommandExitZero { command } => {
            match run_in_workspace(transcript.workspace.as_deref(), command) {
                Ok((code, detail)) => ScorerResult {
                    scorer_type: "command_exit_zero".into(),
                    passed: code == 0,
                    detail: Some(detail),
                },
                Err(detail) => ScorerResult {
                    scorer_type: "command_exit_zero".into(),
                    passed: false,
                    detail: Some(detail),
                },
            }
        }
        ScorerSpec::PythonModule { args } => {
            match run_python_module(transcript.workspace.as_deref(), args) {
                Ok((code, detail)) => ScorerResult {
                    scorer_type: "python_module".into(),
                    passed: code == 0,
                    detail: Some(detail),
                },
                Err(detail) => ScorerResult {
                    scorer_type: "python_module".into(),
                    passed: false,
                    detail: Some(detail),
                },
            }
        }
        ScorerSpec::StopReasonIn { reasons } => {
            let actual = transcript.stop_reason.as_deref().unwrap_or("");
            let passed = reasons.iter().any(|r| r == actual);
            ScorerResult {
                scorer_type: "stop_reason_in".into(),
                passed,
                detail: Some(format!("stop_reason={actual}")),
            }
        }
        ScorerSpec::PythonHiddenCheck { entry_point, test } => {
            match run_python_hidden_check(transcript.workspace.as_deref(), entry_point, test) {
                Ok((code, detail)) => ScorerResult {
                    scorer_type: "python_hidden_check".into(),
                    passed: code == 0,
                    detail: Some(detail),
                },
                Err(detail) => ScorerResult {
                    scorer_type: "python_hidden_check".into(),
                    passed: false,
                    detail: Some(detail),
                },
            }
        }
    }
}

fn count_contains(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

fn workspace_file(transcript: &TurnTranscript, relative: &str) -> Result<PathBuf, String> {
    let root = transcript
        .workspace
        .as_deref()
        .ok_or_else(|| "workspace missing for file scorer".to_owned())?;
    safe_join(root, relative).map_err(|e| e.to_string())
}

fn read_workspace_file(transcript: &TurnTranscript, relative: &str) -> Result<String, String> {
    let path = workspace_file(transcript, relative)?;
    fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn run_in_workspace(workspace: Option<&Path>, command: &str) -> Result<(i32, String), String> {
    let cwd = workspace.ok_or_else(|| "workspace missing for command scorer".to_owned())?;
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let output = cmd
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn failed: {e}"))?;
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut detail = format!("exit={code}");
    if !stdout.trim().is_empty() {
        detail.push_str(" stdout=");
        detail.push_str(&truncate(stdout.trim(), 240));
    }
    if !stderr.trim().is_empty() {
        detail.push_str(" stderr=");
        detail.push_str(&truncate(stderr.trim(), 240));
    }
    Ok((code, detail))
}

fn run_python_module(workspace: Option<&Path>, args: &[String]) -> Result<(i32, String), String> {
    let cwd = workspace.ok_or_else(|| "workspace missing for python_module".to_owned())?;
    let python = find_python().ok_or_else(|| "python_not_found".to_owned())?;
    let resolved = resolve_python_args(&python, args);
    let output = Command::new(&python)
        .args(&resolved)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn python failed: {e}"))?;
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut detail = format!("exit={code} argv={}", resolved.join(" "));
    if !stdout.trim().is_empty() {
        detail.push_str(" stdout=");
        detail.push_str(&truncate(stdout.trim(), 240));
    }
    if !stderr.trim().is_empty() {
        detail.push_str(" stderr=");
        detail.push_str(&truncate(stderr.trim(), 240));
    }
    Ok((code, detail))
}

fn resolve_python_args(python: &str, args: &[String]) -> Vec<String> {
    let pytest = args.len() >= 2 && args[0] == "-m" && args[1] == "pytest";
    if pytest && !python_module_exists(python, "pytest") {
        let mut fallback = vec!["-m".into(), "unittest".into()];
        for arg in args.iter().skip(2) {
            if arg == "-q" || arg.starts_with('-') {
                continue;
            }
            fallback.push(arg.clone());
        }
        return fallback;
    }
    args.to_vec()
}

fn python_module_exists(python: &str, module: &str) -> bool {
    if !module.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    Command::new(python)
        .args(["-c", &format!("import {module}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_python_hidden_check(
    workspace: Option<&Path>,
    entry_point: &str,
    test: &str,
) -> Result<(i32, String), String> {
    let cwd = workspace.ok_or_else(|| "workspace missing for python_hidden_check".to_owned())?;
    let python = find_python().ok_or_else(|| "python_not_found".to_owned())?;
    let check_path = cwd.join("_eval_check.py");
    let script = if entry_point == "_module" {
        format!(
            "import solution as candidate\n{test}\ncheck(candidate)\nprint('CHECK_OK')\n"
        )
    } else {
        if !entry_point
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("invalid python entry_point".into());
        }
        format!(
            "from solution import {entry_point} as candidate\n{test}\ncheck(candidate)\nprint('CHECK_OK')\n"
        )
    };
    fs::write(&check_path, script).map_err(|e| format!("write check script: {e}"))?;
    let output = Command::new(&python)
        .arg("_eval_check.py")
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn python failed: {e}"))?;
    let _ = fs::remove_file(&check_path);
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((
        code,
        format!("exit={code} stderr={}", truncate(stderr.trim(), 240)),
    ))
}

fn find_python() -> Option<String> {
    for bin in ["python3", "python"] {
        if Command::new(bin)
            .args(["-c", "import sys"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(bin.to_owned());
        }
    }
    None
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let clipped: String = text.chars().take(max).collect();
        format!("{clipped}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn assistant_contains_and_not_contains() {
        let t = TurnTranscript {
            assistant_text: "hello HELLO_OK world".into(),
            tool_names: vec![],
            turns: 1,
            ..TurnTranscript::default()
        };
        let pass = score_one(
            &ScorerSpec::AssistantContains {
                marker: "HELLO_OK".into(),
                minimum_hits: 1,
            },
            &t,
        );
        assert!(pass.passed);
        let fail = score_one(
            &ScorerSpec::AssistantNotContains {
                marker: "HELLO_OK".into(),
            },
            &t,
        );
        assert!(!fail.passed);
    }

    #[test]
    fn tool_and_turn_budgets() {
        let t = TurnTranscript {
            assistant_text: "DONE".into(),
            tool_names: vec!["echo".into(), "echo".into()],
            turns: 3,
            ..TurnTranscript::default()
        };
        assert!(
            score_one(
                &ScorerSpec::ToolCalled {
                    name: "echo".into()
                },
                &t
            )
            .passed
        );
        assert!(
            score_one(
                &ScorerSpec::ToolNotCalled {
                    name: "Bash".into()
                },
                &t
            )
            .passed
        );
        assert!(!score_one(&ScorerSpec::MaxToolCalls { max: 1 }, &t).passed);
        assert!(score_one(&ScorerSpec::MaxTurns { max: 3 }, &t).passed);
        assert!(!score_one(&ScorerSpec::MaxTurns { max: 2 }, &t).passed);
    }

    #[test]
    fn file_contains_reads_isolated_workspace() {
        let dir = tempdir().unwrap();
        let mut files = BTreeMap::new();
        files.insert("MARKER.txt".into(), "HARNESS_OK\n".into());
        crate::workspace::materialize_files(dir.path(), &files).unwrap();
        let t = TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            ..TurnTranscript::default()
        };
        assert!(
            score_one(
                &ScorerSpec::FileContains {
                    path: "MARKER.txt".into(),
                    marker: "HARNESS_OK".into(),
                },
                &t
            )
            .passed
        );
        assert!(
            !score_one(
                &ScorerSpec::FileContains {
                    path: "../escape".into(),
                    marker: "x".into(),
                },
                &t
            )
            .passed
        );
    }

    #[test]
    fn python_hidden_check_runs_when_interpreter_exists() {
        let Some(_) = find_python() else {
            return;
        };
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("solution.py"), "def add(a, b):\n    return a + b\n").unwrap();
        let t = TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            ..TurnTranscript::default()
        };
        let result = score_one(
            &ScorerSpec::PythonHiddenCheck {
                entry_point: "add".into(),
                test: "def check(candidate):\n    assert candidate(1, 2) == 3\n".into(),
            },
            &t,
        );
        assert!(result.passed, "{result:?}");
    }

    #[test]
    fn python_module_runs_workspace_tests() {
        let Some(_) = find_python() else {
            return;
        };
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("ok.py"), "VALUE = 1\n").unwrap();
        fs::write(
            dir.path().join("ok_test.py"),
            "import unittest\nimport ok\nclass T(unittest.TestCase):\n    def test_v(self):\n        self.assertEqual(ok.VALUE, 1)\n",
        )
        .unwrap();
        let t = TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            ..TurnTranscript::default()
        };
        let result = score_one(
            &ScorerSpec::PythonModule {
                args: vec![
                    "-m".into(),
                    "pytest".into(),
                    "ok_test.py".into(),
                    "-q".into(),
                ],
            },
            &t,
        );
        assert!(result.passed, "{result:?}");
    }
}
