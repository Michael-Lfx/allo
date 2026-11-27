//! Central resource classification for tools exposed to the engine loop.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceScope {
    None,
    Read,
    Write,
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolPolicy {
    pub workspace: WorkspaceScope,
    pub unproven_process: bool,
}

pub fn tool_policy(name: &str) -> ToolPolicy {
    match name {
        "todo_write" | "query_work_status" | "dispatch_work"
        | "control_work" | "ask_user_choice" => ToolPolicy {
            workspace: WorkspaceScope::None,
            unproven_process: false,
        },
        "read_file" | "grep" | "glob" => ToolPolicy {
            workspace: WorkspaceScope::Read,
            unproven_process: false,
        },
        "write_file" | "edit_file" => ToolPolicy {
            workspace: WorkspaceScope::Write,
            unproven_process: false,
        },
        "create_deck" | "verify_deck" | "verify_artifact" | "publish_file" => {
            ToolPolicy {
                workspace: WorkspaceScope::Device,
                unproven_process: false,
            }
        }
        _ if name.starts_with("automation_") => ToolPolicy {
            workspace: WorkspaceScope::None,
            unproven_process: false,
        },
        // Unknown tools are deliberately conservative. A new filesystem/process
        // capability must opt into a narrower class after its I/O and
        // quiescence behavior are proven.
        _ => ToolPolicy {
            workspace: WorkspaceScope::Device,
            unproven_process: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceScope, tool_policy};

    #[test]
    fn known_tools_have_stable_conservative_classes() {
        assert_eq!(tool_policy("read_file").workspace, WorkspaceScope::Read);
        assert_eq!(tool_policy("write_file").workspace, WorkspaceScope::Write);
        assert_eq!(
            tool_policy("publish_file").workspace,
            WorkspaceScope::Device
        );
        assert!(!tool_policy("publish_file").unproven_process);
        assert_eq!(
            tool_policy("automation_list").workspace,
            WorkspaceScope::None
        );
        assert_eq!(
            tool_policy("future_tool").workspace,
            WorkspaceScope::Device
        );
        assert!(tool_policy("future_tool").unproven_process);
    }
}
