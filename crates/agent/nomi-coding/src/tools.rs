//! Coding-mode tool advertising (hide desktop distractions, keep coding essentials).

use crate::profile::TaskProfile;

/// Tools advertised in coding mode. Anything else (MCP, briefing, desktop) stays hidden.
///
/// Keep shell defaults simple (`Bash`) while still exposing long-running process
/// controls (`exec_command` + `write_stdin`) for bounded polling workflows.
const CODING_CORE_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "exec_command",
    "write_stdin",
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
    "Lsp",
    "explore_code",
    "verify_change",
    "research",
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
    CODING_CORE_TOOLS
        .iter()
        .any(|name| name.eq_ignore_ascii_case(tool_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_hides_browser_computer_and_apply_patch_only() {
        assert!(!advertise_tool(TaskProfile::Coding, "Browser"));
        assert!(!advertise_tool(TaskProfile::Coding, "Computer"));
        assert!(!advertise_tool(TaskProfile::Coding, "ApplyPatch"));
        assert!(!advertise_tool(TaskProfile::Coding, "LaunchApp"));
        assert!(!advertise_tool(TaskProfile::Coding, "briefing_create"));
        assert!(!advertise_tool(TaskProfile::Coding, "mcp__server__tool"));
        assert!(advertise_tool(TaskProfile::Coding, "exec_command"));
        assert!(advertise_tool(TaskProfile::Coding, "write_stdin"));
        assert!(advertise_tool(TaskProfile::Coding, "Edit"));
        assert!(advertise_tool(TaskProfile::Coding, "Bash"));
        assert!(advertise_tool(TaskProfile::Coding, "DirTree"));
        assert!(advertise_tool(TaskProfile::Coding, "Lsp"));
        assert!(advertise_tool(TaskProfile::Coding, "explore_code"));
        assert!(advertise_tool(TaskProfile::Coding, "verify_change"));
        assert!(advertise_tool(TaskProfile::Coding, "research"));
        assert!(advertise_tool(TaskProfile::Office, "Browser"));
        assert!(advertise_tool(TaskProfile::Office, "ApplyPatch"));
        assert!(advertise_tool(TaskProfile::Office, "exec_command"));
    }
}
