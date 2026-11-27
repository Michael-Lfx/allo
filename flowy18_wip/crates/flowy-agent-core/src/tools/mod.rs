//! Host-owned tool registry and the built-in tool set.

pub mod adapter;
pub mod builtin;
pub mod matcher;
pub mod pi;
pub mod provider;
pub mod registry;
pub mod sandbox;
pub mod search;
pub mod shell_policy;

pub use adapter::RegistryTool;
pub use builtin::{
    has_reserved_device_component, is_windows_reparse_point,
    is_windows_reserved_device_name, register_builtin_tools,
};
pub use pi::pi_shell_definition;
pub use provider::{ToolPolicyContext, ToolProvider, ToolProviderContext};
pub use registry::{
    BoxFuture, ToolDefinition, ToolExposure, ToolGuard, ToolPolicy,
    ToolPolicyDecision, ToolRegistry, ToolSource,
};
pub use sandbox::{
    SandboxAvailability, SandboxBackend, SandboxPolicy,
    detect_sandbox_availability, linux_bwrap_args, macos_seatbelt_profile,
};
pub use search::{
    ActivatedToolSet, MAX_ACTIVATED_TOOLS_PER_TURN, SearchableToolEntry,
    TOOL_SEARCH_NAME, format_deferred_tool_catalog, tool_search_definition,
};
pub use shell_policy::{
    SHELL_CONFIRM_REQUIRED, SHELL_CREDENTIAL_ACCESS, SHELL_DESTRUCTIVE_HOST_OP,
    SHELL_PRIVACY_TAMPER, SHELL_SYSTEM_TAMPER, ShellPolicyDecision,
    evaluate_shell_command,
};
