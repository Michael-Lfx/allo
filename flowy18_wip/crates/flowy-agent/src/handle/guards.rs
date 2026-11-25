//! Workspace tool registry, path confinement, and shell guard hooks.

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
    let guard: ToolGuard =
        Arc::new(move |args: &ToolCallArgs| path_guard(&root, args));
    registry.add_guard(guard);
    registry
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

pub(super) fn confined_to(root: &Path, raw: &str) -> bool {
    let path = Path::new(raw);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let root = resolve_deepest_ancestor(root);
    let candidate = resolve_deepest_ancestor(&normalized);
    within(&root, &candidate)
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
