//! Workspace tool registry, path confinement, and shell guard hooks.

use flowy_agent_core::tools::evaluate_shell_command;

use super::{
    AgentWorkspace, Arc, Path, PathBuf, ToolCallArgs, ToolGuard, ToolRegistry,
    log_storage_io_error, pi_shell_definition, register_builtin_tools,
};

pub(super) fn ensure_tool_cwd(root: &Path) -> PathBuf {
    let dir = root.join("sessions").join("cwd");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log_storage_io_error("create tool working directory", &dir, &error);
    }
    dir
}

pub(super) fn session_working_directory(
    root: &Path,
    workspace: Option<&AgentWorkspace>,
) -> PathBuf {
    workspace.map_or_else(|| ensure_tool_cwd(root), |item| item.root.clone())
}

pub(super) fn tool_registry(cwd: &Path) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.register(crate::pptx::definition());
    registry.register(crate::verify::definition(None));
    registry.register(crate::verify::artifact_definition(None, None));
    registry.register(pi_shell_definition(cwd));
    // The app has no todo surface yet; expose only tools whose results the
    // host can satisfy instead of advertising one that always fails.
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let file_guard: ToolGuard =
        Arc::new(move |args: &ToolCallArgs| path_guard(&root, args));
    registry.add_guard(file_guard);
    let shell_root = cwd.to_path_buf();
    let shell_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_policy_guard(&shell_root, args)
    });
    registry.add_guard(shell_guard);
    registry
}

/// Static cross-platform safety guard for shell commands (`powershell` / `bash`).
///
/// Complements `path_guard` by blocking host-wide destructive operations,
/// credential store exfiltration, OS privilege escalation, and macOS privacy
/// dialog tampering before `pi_shell` spawns any subprocess.
pub(super) fn shell_policy_guard(
    root: &Path,
    args: &ToolCallArgs,
) -> Option<String> {
    if !matches!(args.name.as_str(), "powershell" | "bash") {
        return None;
    }
    let command = args
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)?;
    let decision = evaluate_shell_command(command, root);
    if let Some(reason) = decision.denial_message() {
        tracing::warn!(
            target: "flowy.sandbox",
            tool = %args.name,
            reason = %reason,
            "blocked shell command by static safety policy"
        );
        return Some(reason);
    }
    None
}

/// Host policy for file tools. The platform shell is intentionally exempt: it
/// executes commands, not host file paths, and the host has no OS sandbox for it.
pub(super) fn path_guard(root: &Path, args: &ToolCallArgs) -> Option<String> {
    let key = match args.name.as_str() {
        "read_file" | "write_file" | "edit_file" | "glob" | "grep"
        | "create_deck" | "verify_deck" | "verify_artifact" => "path",
        _ => return None,
    };
    let raw = args.arguments.get(key).and_then(|value| value.as_str())?;
    if raw.trim().is_empty() {
        return None;
    }
    (!confined_to(root, raw))
        .then(|| format!("path escapes the working directory: {raw}"))
}

pub fn is_windows_reserved_device_name(name: &str) -> bool {
    let trimmed = name.trim_end_matches([' ', '.']);
    let stem = trimmed.split('.').next().unwrap_or(trimmed).trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "CONIN$"
            | "CONOUT$"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    )
}

#[cfg(windows)]
fn is_windows_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_windows_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

pub fn has_reserved_device_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        std::path::Component::Normal(part) => {
            part.to_str().is_some_and(is_windows_reserved_device_name)
        }
        _ => false,
    })
}

pub fn confined_to(root: &Path, raw: &str) -> bool {
    if raw.trim().is_empty() || raw.contains('\0') {
        return false;
    }
    let path = Path::new(raw);
    if has_reserved_device_component(path) {
        return false;
    }
    // Reject Windows drive-relative (`C:foo`) or rooted-without-drive (`\foo`)
    // paths on Windows, and walk component-by-component canonicalizing existing
    // symlinks/junctions BEFORE folding `..` so `link/../outside` can never
    // lexically collapse inside `root` while the OS follows `link` outside.
    let resolved_root = resolve_deepest_ancestor(root);
    let mut current = if path.is_absolute() {
        PathBuf::new()
    } else {
        let has_prefix_or_root = path.components().any(|c| {
            matches!(
                c,
                std::path::Component::Prefix(_) | std::path::Component::RootDir
            )
        });
        if has_prefix_or_root {
            return false;
        }
        resolved_root.clone()
    };
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
            }
            std::path::Component::RootDir => {
                current.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !current.pop() {
                    return false;
                }
                if let Ok(canon) = std::fs::canonicalize(&current) {
                    current = canon;
                }
            }
            std::path::Component::Normal(part) => {
                current.push(part);
                if let Ok(meta) = std::fs::symlink_metadata(&current) {
                    if let Ok(canon) = std::fs::canonicalize(&current) {
                        current = canon;
                    } else if meta.file_type().is_symlink()
                        || is_windows_reparse_point(&meta)
                    {
                        // Dangling symlink or NTFS junction: fail closed
                        // rather than trusting a lexical target.
                        return false;
                    }
                }
            }
        }
    }
    let candidate = resolve_deepest_ancestor(&current);
    within(&resolved_root, &candidate)
}

pub(super) fn resolve_deepest_ancestor(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    // Walk up to the deepest existing ancestor and canonicalize it before
    // re-appending remaining lexical components, closing symlink/junction
    // escapes through multi-level non-existent subdirectories while staying
    // consistent when `root` itself is a synthetic (not-yet-created) path.
    let mut ancestor = path;
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) {
            let mut resolved = canonical_ancestor;
            for part in suffix.into_iter().rev() {
                resolved.push(part);
            }
            return resolved;
        }
        let Some(name) = ancestor.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            return path.to_path_buf();
        };
        ancestor = parent;
    }
}

pub(super) fn within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = root.to_string_lossy().replace('/', "\\").to_lowercase();
        let candidate = candidate
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        candidate == root || candidate.starts_with(&format!("{root}\\"))
    }
    #[cfg(not(windows))]
    {
        candidate.starts_with(root)
    }
}

pub(super) fn tool_target(args: &serde_json::Value) -> String {
    for key in [
        "description",
        "path",
        "file_path",
        "command",
        "query",
        "pattern",
        "target",
    ] {
        if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
            return value.to_owned();
        }
    }
    String::new()
}
