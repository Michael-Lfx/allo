//! Workspace tool registry, path confinement, skill scoping, and shell guard hooks.

use std::collections::HashSet;
use std::sync::Mutex;

use flowy_agent_core::tools::{ShellPolicyDecision, evaluate_shell_command};
use flowy_skills::{InstalledSkill, SkillId};

use super::{
    AgentWorkspace, Arc, Path, PathBuf, ToolCallArgs, ToolGuard, ToolRegistry,
    lock, log_storage_io_error, pi_shell_definition, register_builtin_tools,
};

/// Compute a deterministic SHA-256 hex digest over `(tool_name, canonical(args))`
/// so `PendingCard::ToolApproval` approvals are pinned to exact invocation args.
#[must_use]
pub fn canonical_tool_args_hash(
    tool_name: &str,
    args: &serde_json::Value,
) -> String {
    let canonical = flowy_agent_core::canonical_tool_args_json(tool_name, args);
    crate::delivery::hash_bytes(canonical.as_bytes())
}

/// Read-only inspection and verification tools that remain permitted even
/// after a user denies a risky mutating action in the current turn.
const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "ls",
    "list_dir",
    "find_files",
    "verify_deck",
    "verify_artifact",
];

/// Build the host `PreToolHook` wired into `rpi_agent`'s `before_tool_call`
/// enforcing turn-scoped `UserDenied` anti-bypass, 3-way `shell_policy`
/// (`Allow | Ask | Deny`), and one-shot `args_hash` approval grants.
pub(super) fn build_pre_tool_hook(
    session_id: String,
    cwd: PathBuf,
    state: Arc<super::UiState>,
) -> flowy_agent_core::PreToolHook {
    Arc::new(move |tool_name: &str, args: &serde_json::Value| {
        if !READ_ONLY_TOOLS.contains(&tool_name) {
            let denied =
                lock(&state.denied_tool_turns).get(&session_id).cloned();
            if let Some(reason) = denied {
                return flowy_agent_core::PreToolDecision::Deny {
                    reason: format!(
                        "USER_DENIED: user denied a risky action in this turn ({reason}); bypassing with {tool_name} is forbidden"
                    ),
                };
            }
        }
        if matches!(tool_name, "bash" | "powershell")
            && let Some(command) =
                args.get("command").and_then(serde_json::Value::as_str)
        {
            match evaluate_shell_command(command, &cwd) {
                ShellPolicyDecision::Allow => {
                    return flowy_agent_core::PreToolDecision::Allow;
                }
                ShellPolicyDecision::Deny { code, reason } => {
                    return flowy_agent_core::PreToolDecision::Deny {
                        reason: format!("{code}: {reason}"),
                    };
                }
                ShellPolicyDecision::Ask { reason, .. } => {
                    let args_hash = canonical_tool_args_hash(tool_name, args);
                    let consumed_grant = {
                        let mut grants = lock(&state.approved_tool_calls);
                        if let Some(list) = grants.get_mut(&session_id)
                            && let Some(pos) = list.iter().position(|(t, h)| {
                                t == tool_name && h == &args_hash
                            })
                        {
                            list.remove(pos);
                            true
                        } else {
                            false
                        }
                    };
                    if consumed_grant {
                        return flowy_agent_core::PreToolDecision::Allow;
                    }
                    let preview =
                        flowy_agent_core::log_preview(command.trim(), 160);
                    return flowy_agent_core::PreToolDecision::Ask {
                        target_summary: format!(
                            "{tool_name} ({})",
                            cwd.display()
                        ),
                        args_hash,
                        risk_reason: reason,
                        command_preview: preview,
                    };
                }
            }
        }
        flowy_agent_core::PreToolDecision::Allow
    })
}

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

pub(super) fn tool_registry(
    cwd: &Path,
    skills: &[InstalledSkill],
    active_skills: Vec<SkillId>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.register(crate::pptx::definition());
    registry.register(pi_shell_definition(cwd));
    // The app has no todo surface yet; expose only tools whose results the
    // host can satisfy instead of advertising one that always fails.
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let active = Arc::new(Mutex::new(skill_scope(skills, active_skills)));
    let skills_for_paths = skills.to_vec();
    let path_active = Arc::clone(&active);
    let file_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        path_guard(&root, &skills_for_paths, &path_active, args)
    });
    registry.add_guard(file_guard);
    let skills_for_shell = skills.to_vec();
    let skill_shell_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_guard(&skills_for_shell, &active, args)
    });
    registry.add_guard(skill_shell_guard);
    let shell_root = cwd.to_path_buf();
    let policy_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_policy_guard(&shell_root, args)
    });
    registry.add_guard(policy_guard);
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

/// Host policy for file tools. The platform shell is handled by dedicated
/// guards because it executes commands rather than host file paths.
pub(super) fn path_guard(
    root: &Path,
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    let key = match args.name.as_str() {
        "read_file" | "write_file" | "edit_file" | "glob" | "grep"
        | "create_deck" | "verify_deck" | "verify_artifact" => "path",
        _ => return None,
    };
    let raw = args.arguments.get(key).and_then(|value| value.as_str())?;
    if raw.trim().is_empty() {
        return None;
    }
    if confined_to(root, raw) {
        return None;
    }
    let writable = matches!(
        args.name.as_str(),
        "write_file" | "edit_file" | "create_deck"
    );
    if writable {
        return Some(format!("path escapes the working directory: {raw}"));
    }
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        root.join(raw)
    };
    let Some(skill) = skills
        .iter()
        .find(|skill| confined_to(&skill.root, &path.to_string_lossy()))
    else {
        return Some(format!("path escapes the working directory: {raw}"));
    };
    if matches!(args.name.as_str(), "read_file" | "grep") {
        lock(active).insert(skill.id.clone());
    }
    None
}

pub(super) fn skill_scope(
    skills: &[InstalledSkill],
    explicit: Vec<SkillId>,
) -> HashSet<SkillId> {
    let mut active = explicit.into_iter().collect::<HashSet<_>>();
    active.extend(
        skills
            .iter()
            .filter(|skill| skill.auto_invoke)
            .map(|skill| skill.id.clone()),
    );
    active
}

pub(super) fn shell_guard(
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    if !matches!(args.name.as_str(), "bash" | "powershell") {
        return None;
    }
    let active = lock(active);
    let blocked = skills
        .iter()
        .find(|skill| active.contains(&skill.id) && !skill.scripts_enabled);
    blocked.map(|skill| {
        format!(
            "skill \"{}\" does not have script permission; enable it in My Skills before using a shell",
            skill.name
        )
    })
}

pub use flowy_agent_core::tools::{
    has_reserved_device_component, is_windows_reparse_point,
};

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
