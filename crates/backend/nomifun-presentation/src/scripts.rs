//! Run one ppt-master script with the capability's private interpreter.
//!
//! The agent names a script and its arguments; the backend decides the
//! interpreter, environment, timeout and output budget. Scripts run with the
//! owner's privileges (no OS sandbox), so the rules here are about keeping the
//! call predictable, not about containing a hostile agent that also has a
//! shell: the script must live under the snapshot's `scripts/`, browser
//! servers are refused (confirmations happen in chat), and the working
//! directory must be inside the conversation workspace.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

/// ppt-master's longest steps (full-deck export with images) take minutes.
pub const SCRIPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Newest output kept for the agent; quality reports end with the summary.
pub const MAX_SCRIPT_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_SCRIPT_ARGS: usize = 256;
pub const MAX_SCRIPT_ARG_BYTES: usize = 32 * 1024;

/// Local web servers that wait for a browser. The Flowy flow confirms in chat.
const SERVER_SCRIPT_DIRS: &[&str] = &["confirm_ui", "svg_editor", "spec_review"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScriptRun {
    /// Script path relative to `<SKILL_DIR>/scripts/`.
    pub script: String,
    pub cwd: String,
    /// `None` when the process was killed by a signal or timed out.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Combined stdout+stderr, newest part when truncated.
    pub output: String,
    /// Older output dropped to stay within the budget.
    pub truncated_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// The capability is not `Ready`; install it first.
    NotReady(String),
    /// The request itself is invalid (bad script path, cwd, args).
    Invalid(String),
    /// The process could not be started or was lost.
    Failed(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotReady(message) | Self::Invalid(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

/// `svg_to_pptx.py`, `scripts/svg_to_pptx.py`, `${SKILL_DIR}/scripts/x.py`
/// style prefixes are accepted; the result is a plain relative path ending in
/// `.py` with no `..`, and not one of the browser servers.
pub(crate) fn normalize_script(requested: &str) -> Result<PathBuf, ScriptError> {
    let invalid = |reason: &str| ScriptError::Invalid(format!("script {requested:?}: {reason}"));
    let mut text = requested.trim().replace('\\', "/");
    for prefix in ["${SKILL_DIR}/", "$SKILL_DIR/", "skills/ppt-master/"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.to_owned();
        }
    }
    let text = text.strip_prefix("scripts/").unwrap_or(&text);
    if text.is_empty() {
        return Err(invalid("empty script path"));
    }
    let path = Path::new(text);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| invalid("non-UTF-8 path"))?;
                if name.contains(':') {
                    return Err(invalid("must be a path under scripts/"));
                }
                clean.push(name);
            }
            Component::CurDir => {}
            _ => return Err(invalid("must be a relative path under scripts/ without `..`")),
        }
    }
    if clean.extension().and_then(|ext| ext.to_str()) != Some("py") {
        return Err(invalid("only .py scripts can be run"));
    }
    let first = clean.components().next().and_then(|component| component.as_os_str().to_str());
    if first.is_some_and(|first| SERVER_SCRIPT_DIRS.contains(&first)) {
        return Err(invalid(
            "browser servers are not used in Flowy; confirm with the user in the conversation instead",
        ));
    }
    Ok(clean)
}

/// Canonical `<scripts>/<script>`, refusing anything that resolves outside
/// `scripts_dir` (a symlink) or is not a file.
pub(crate) fn resolve_script(scripts_dir: &Path, script: &Path) -> Result<PathBuf, ScriptError> {
    let root = dunce::canonicalize(scripts_dir)
        .map_err(|error| ScriptError::NotReady(format!("Skill scripts directory is unavailable: {error}")))?;
    let candidate = root.join(script);
    let resolved = dunce::canonicalize(&candidate)
        .map_err(|_| ScriptError::Invalid(format!("script {} does not exist in ppt-master", script.display())))?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err(ScriptError::Invalid(format!("script {} is not a ppt-master script", script.display())));
    }
    Ok(resolved)
}

/// Canonical working directory: `requested` (absolute, or relative to the
/// workspace) must be an existing directory inside `workspace`.
pub fn resolve_cwd(workspace: &Path, requested: Option<&str>) -> Result<PathBuf, ScriptError> {
    // dunce: no `\?\` prefix on Windows; Python scripts print these paths.
    let workspace = dunce::canonicalize(workspace).map_err(|error| {
        ScriptError::Invalid(format!("conversation workspace {} is unavailable: {error}", workspace.display()))
    })?;
    if !workspace.is_dir() {
        return Err(ScriptError::Invalid(format!("conversation workspace {} is not a directory", workspace.display())));
    }
    let candidate = match requested.map(str::trim).filter(|value| !value.is_empty()) {
        None => return Ok(workspace),
        Some(value) if Path::new(value).is_absolute() => PathBuf::from(value),
        Some(value) => workspace.join(value),
    };
    let resolved = dunce::canonicalize(&candidate)
        .map_err(|error| ScriptError::Invalid(format!("working directory {}: {error}", candidate.display())))?;
    if !resolved.is_dir() || !resolved.starts_with(&workspace) {
        return Err(ScriptError::Invalid(format!(
            "working directory {} is outside the conversation workspace {}",
            resolved.display(),
            workspace.display()
        )));
    }
    Ok(resolved)
}

pub(crate) fn check_args(args: &[String]) -> Result<(), ScriptError> {
    if args.len() > MAX_SCRIPT_ARGS {
        return Err(ScriptError::Invalid(format!("at most {MAX_SCRIPT_ARGS} arguments")));
    }
    if args.iter().map(String::len).sum::<usize>() > MAX_SCRIPT_ARG_BYTES {
        return Err(ScriptError::Invalid(format!("arguments exceed {MAX_SCRIPT_ARG_BYTES} bytes")));
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return Err(ScriptError::Invalid("arguments must not contain NUL".into()));
    }
    Ok(())
}

/// Inserted after the SKILL.md frontmatter when the catalog loads the managed
/// Skill, so the model sees the Flowy rules before ppt-master's own
/// `python3 ...` instructions.
pub fn skill_host_note(skill_dir: &str) -> String {
    format!(
        "> **Flowy host note (managed install, read first).** This Skill is installed and run by Flowy.\n\
         > Before any step, call `nomi_presentation_status` and follow its `usage` exactly.\n\
         > - `SKILL_DIR` is `{skill_dir}`.\n\
         > - Every `python3 \"${{SKILL_DIR}}/scripts/<x>.py\" <args>` below means: call `nomi_presentation_run_script` with `script: \"<x>.py\"` and `args`. Never start python/python3 from a shell for this Skill.\n\
         > - Confirm ⛔ BLOCKING gates in the conversation; do not start confirm_ui, svg_editor or spec_review.\n"
    )
}

/// `content` with [`skill_host_note`] placed right after the YAML
/// frontmatter (or first, when there is none), keeping the frontmatter parseable.
pub fn with_host_note(content: &str, skill_dir: &str) -> String {
    let note = skill_host_note(skill_dir);
    let body_start = content.strip_prefix("---").and_then(|rest| {
        let rest_offset = 3;
        rest.find("\n---").map(|end| {
            let after = rest_offset + end + "\n---".len();
            content[after..].find('\n').map_or(content.len(), |newline| after + newline + 1)
        })
    });
    match body_start {
        Some(split) => format!("{}\n{note}\n{}", &content[..split], &content[split..]),
        None => format!("{note}\n{content}"),
    }
}

/// Agent-facing rules returned with the status once the capability is ready.
pub fn usage_contract(skill_dir: &str) -> String {
    format!(
        "SKILL_DIR={skill_dir}\n\
         1. Read {skill_dir}/SKILL.md first and follow its load order and routing; read every workflow and reference file by absolute path under SKILL_DIR.\n\
         2. Run ppt-master scripts ONLY with nomi_presentation_run_script (script relative to SKILL_DIR/scripts, e.g. \"project_manager.py\"). Never run python/python3 from a shell for this Skill: only the tool uses the private interpreter with the locked dependencies. Where SKILL.md says `python3 \"${{SKILL_DIR}}/scripts/attribution_guard.py\"`, call the tool with script \"attribution_guard.py\".\n\
         3. Scripts run in the conversation workspace (or `cwd` inside it). `project_manager.py init <name>` creates `projects/<name>/` there; pass project paths relative to the workspace, e.g. `projects/<name>`.\n\
         4. ⛔ BLOCKING gates and confirmations happen in this conversation. Do not start confirm_ui, svg_editor or spec_review; the tool refuses them.\n\
         5. The finished deck is `projects/<name>/exports/*.pptx`; tell the user its path."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_paths_are_normalized_and_confined() {
        for (input, expected) in [
            ("svg_to_pptx.py", "svg_to_pptx.py"),
            ("scripts/svg_to_pptx.py", "svg_to_pptx.py"),
            ("${SKILL_DIR}/scripts/attribution_guard.py", "attribution_guard.py"),
            ("skills/ppt-master/scripts/source_to_md/web_to_md.py", "source_to_md/web_to_md.py"),
            ("source_to_md\\pdf_to_md.py", "source_to_md/pdf_to_md.py"),
        ] {
            assert_eq!(normalize_script(input).unwrap(), PathBuf::from(expected), "{input}");
        }
        for bad in [
            "",
            "../evil.py",
            "scripts/../../etc/x.py",
            "/usr/bin/python3",
            "C:/Windows/x.py",
            "svg_to_pptx",
            "run.sh",
            "confirm_ui/server.py",
            "svg_editor/server.py",
            "scripts/spec_review/server.py",
        ] {
            assert!(normalize_script(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn resolve_script_requires_an_existing_file_inside_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir_all(scripts.join("pkg")).unwrap();
        std::fs::write(scripts.join("ok.py"), "").unwrap();
        assert!(resolve_script(&scripts, Path::new("ok.py")).is_ok());
        assert!(resolve_script(&scripts, Path::new("missing.py")).is_err());
        assert!(resolve_script(&scripts, Path::new("pkg")).is_err(), "directories are not scripts");
    }

    #[test]
    fn cwd_must_stay_inside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(workspace.join("projects/demo")).unwrap();
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();
        let root = dunce::canonicalize(&workspace).unwrap();
        assert_eq!(resolve_cwd(&workspace, None).unwrap(), root);
        assert_eq!(resolve_cwd(&workspace, Some("  ")).unwrap(), root);
        assert_eq!(resolve_cwd(&workspace, Some("projects/demo")).unwrap(), root.join("projects").join("demo"));
        assert!(resolve_cwd(&workspace, Some("../outside")).is_err());
        let outside = dir.path().join("outside").to_string_lossy().into_owned();
        assert!(resolve_cwd(&workspace, Some(&outside)).is_err());
        assert!(resolve_cwd(&workspace, Some("missing")).is_err());
    }

    #[test]
    fn argument_budget_is_enforced() {
        assert!(check_args(&["init".into(), "demo".into()]).is_ok());
        assert!(check_args(&vec!["x".into(); MAX_SCRIPT_ARGS + 1]).is_err());
        assert!(check_args(&["x".repeat(MAX_SCRIPT_ARG_BYTES + 1)]).is_err());
        assert!(check_args(&["a\0b".into()]).is_err());
    }

    #[test]
    fn host_note_goes_after_the_frontmatter() {
        let skill = "---\nname: ppt-master\ndescription: >\n  decks\n---\n\n# PPT Master Skill\n";
        let out = with_host_note(skill, "/s");
        assert!(out.starts_with("---\nname: ppt-master\ndescription: >\n  decks\n---\n"), "{out}");
        let note_at = out.find("Flowy host note").unwrap();
        assert!(note_at < out.find("# PPT Master Skill").unwrap());
        assert!(out.contains("`SKILL_DIR` is `/s`"));
        assert!(out.contains("nomi_presentation_run_script"));

        let bare = with_host_note("# no frontmatter\n", "/s");
        assert!(bare.starts_with("> **Flowy host note"));
        assert!(bare.ends_with("# no frontmatter\n"));
    }

    #[test]
    fn usage_contract_names_the_skill_dir_and_the_tool() {
        let text = usage_contract("/data/skill");
        assert!(text.starts_with("SKILL_DIR=/data/skill"));
        assert!(text.contains("nomi_presentation_run_script"));
        assert!(text.contains("projects/<name>/exports"));
    }
}
