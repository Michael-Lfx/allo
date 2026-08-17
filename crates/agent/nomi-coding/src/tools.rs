//! Coding-mode tool advertising (hide desktop distractions + complex shell adapters).

use crate::profile::TaskProfile;

/// Tools advertised in coding mode (plus MCP / companion that pass the allow path).
///
/// Intentionally **Bash-only** for shell — Open Vetta coding agents use a single
/// bash/shell tool. `exec_command`/`write_stdin` stay registered for office, but
/// their dual-mode schema (`cmd` vs `script`+`language`+`timeout`) routinely
/// causes invalid model calls in coding sessions.
const CODING_CORE_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "DirTree",
    "dir_tree",
    "ToolSearch",
    "Skill",
    "update_plan",
    "remember",
    "EnterPlanMode",
    "ExitPlanMode",
    "web_search",
    "web_extract",
    "update_goal",
];

/// Always hide these in coding mode even if registered.
const CODING_HIDDEN_TOOLS: &[&str] = &[
    "Browser",
    "Computer",
    "computer",
    "browser",
    // Coding UX is anchor Edit + Write; ApplyPatch remains for office / ACP Codex.
    "ApplyPatch",
    "apply_patch",
    // Complex dual-mode process tools — prefer Bash in coding.
    "exec_command",
    "write_stdin",
];

/// Whether `tool_name` should be advertised to the provider under `profile`.
pub fn advertise_tool(profile: TaskProfile, tool_name: &str) -> bool {
    if !profile.is_coding() {
        return true;
    }
    if CODING_HIDDEN_TOOLS
        .iter()
        .any(|hidden| hidden.eq_ignore_ascii_case(tool_name))
    {
        return false;
    }
    if CODING_CORE_TOOLS
        .iter()
        .any(|name| name.eq_ignore_ascii_case(tool_name))
    {
        return true;
    }
    // Allow MCP / companion / knowledge / requirement / summon / ssh tools.
    !matches!(
        tool_name,
        "Browser"
            | "Computer"
            | "LaunchApp"
            | "computer_use"
            | "ApplyPatch"
            | "exec_command"
            | "write_stdin"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_hides_browser_computer_apply_patch_and_exec_pair() {
        assert!(!advertise_tool(TaskProfile::Coding, "Browser"));
        assert!(!advertise_tool(TaskProfile::Coding, "Computer"));
        assert!(!advertise_tool(TaskProfile::Coding, "ApplyPatch"));
        assert!(!advertise_tool(TaskProfile::Coding, "exec_command"));
        assert!(!advertise_tool(TaskProfile::Coding, "write_stdin"));
        assert!(advertise_tool(TaskProfile::Coding, "Edit"));
        assert!(advertise_tool(TaskProfile::Coding, "Bash"));
        assert!(advertise_tool(TaskProfile::Coding, "DirTree"));
        assert!(advertise_tool(TaskProfile::Office, "Browser"));
        assert!(advertise_tool(TaskProfile::Office, "ApplyPatch"));
        assert!(advertise_tool(TaskProfile::Office, "exec_command"));
    }
}
