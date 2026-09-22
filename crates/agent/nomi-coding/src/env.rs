//! Environment context injected into coding turn-tail (informational, not a sandbox).

use std::path::PathBuf;

/// Workspace bounds the model should respect during coding mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingEnvContext {
    pub cwd: PathBuf,
    pub write_root: Option<PathBuf>,
}

/// Format the workspace-bounds block for turn-tail injection.
///
/// Returns `None` when there is nothing useful to say (empty cwd).
///
/// **This block deliberately does not restate the working directory.** The
/// session cwd already rides the cache-stable system prompt
/// (`nomi_agent::context::build_system_prompt` emits `Working directory: "…"`),
/// so repeating it here would put the same path on the wire twice on *every*
/// provider pass — once nearly free in the cached prefix, once at full price in
/// the tail. Only the write-root constraint is unique to this block; the system
/// prompt knows nothing about it.
///
/// When no write root is configured there is no constraint to state, so the
/// block collapses to `None` rather than spending tail tokens on a reminder to
/// stay in a directory the model was already told about.
pub fn format_env_context(env: &CodingEnvContext) -> Option<String> {
    if env.cwd.as_os_str().is_empty() {
        return None;
    }
    let root = env.write_root.as_ref()?;
    Some(
        [
            "# Coding environment".to_string(),
            format!("- Write root (Edit/Write/ApplyPatch must stay inside): {}", root.display()),
            "- Prefer Grep/Glob with `path` under this root; avoid scanning outside it.".to_string(),
        ]
        .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_write_root() {
        let text = format_env_context(&CodingEnvContext {
            cwd: PathBuf::from("/proj"),
            write_root: Some(PathBuf::from("/proj")),
        })
        .expect("block");
        assert!(text.contains("Write root"));
        assert!(text.contains("/proj"));
    }

    /// The session cwd already rides the cache-stable system prompt, so the tail
    /// must not restate it. This is a per-pass duplication guard: the assertion
    /// is about the *wire*, not about whether the path is knowable.
    #[test]
    fn block_does_not_restate_the_working_directory() {
        let text = format_env_context(&CodingEnvContext {
            cwd: PathBuf::from("/proj"),
            write_root: Some(PathBuf::from("/proj")),
        })
        .expect("block");
        assert!(
            !text.contains("Working directory"),
            "the cwd line duplicates the system prompt; got: {text}"
        );
        assert!(
            !text.contains("cwd)"),
            "no cwd restatement in any form; got: {text}"
        );
    }

    #[test]
    fn no_write_root_yields_no_block() {
        // Nothing unique to say: the model already knows the cwd from the
        // system prompt, so emitting a block here would be pure overhead.
        assert_eq!(
            format_env_context(&CodingEnvContext {
                cwd: PathBuf::from("/proj"),
                write_root: None,
            }),
            None
        );
    }

    #[test]
    fn empty_cwd_yields_no_block() {
        assert_eq!(
            format_env_context(&CodingEnvContext {
                cwd: PathBuf::new(),
                write_root: Some(PathBuf::from("/proj")),
            }),
            None
        );
    }
}
