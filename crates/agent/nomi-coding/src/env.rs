//! Environment context injected into coding turn-tail (informational, not a sandbox).

use std::path::PathBuf;

/// Workspace bounds the model should respect during coding mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingEnvContext {
    pub cwd: PathBuf,
    pub write_root: Option<PathBuf>,
}

/// Format a short environment block for turn-tail injection.
/// Returns `None` when there is nothing useful to say (empty cwd).
pub fn format_env_context(env: &CodingEnvContext) -> Option<String> {
    let cwd = env.cwd.display().to_string();
    if cwd.trim().is_empty() {
        return None;
    }
    let mut lines = vec![
        "# Coding environment".to_string(),
        format!("- Working directory (cwd): {cwd}"),
    ];
    match &env.write_root {
        Some(root) => {
            lines.push(format!(
                "- Write root (Edit/Write/ApplyPatch must stay inside): {}",
                root.display()
            ));
            lines.push(
                "- Prefer Grep/Glob with `path` under this root; avoid scanning outside it."
                    .to_string(),
            );
        }
        None => {
            lines.push(
                "- No write_root configured; still prefer staying inside the working directory."
                    .to_string(),
            );
        }
    }
    Some(lines.join("\n"))
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
}
