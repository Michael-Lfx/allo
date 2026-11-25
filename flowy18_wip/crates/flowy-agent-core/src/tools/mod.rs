//! Host-owned tool registry and the built-in tool set.

pub mod adapter;
pub mod builtin;
pub mod matcher;
pub mod pi;
pub mod registry;
pub mod sandbox;
pub mod shell_policy;

pub use adapter::RegistryTool;
pub use builtin::register_builtin_tools;
pub use pi::pi_shell_definition;
pub use registry::{
    BoxFuture, ToolDefinition, ToolGuard, ToolPolicy, ToolPolicyDecision,
    ToolRegistry,
};
pub use sandbox::{
    SandboxAvailability, SandboxBackend, SandboxPolicy,
    detect_sandbox_availability, linux_bwrap_args, macos_seatbelt_profile,
};
pub use shell_policy::{
    SHELL_CREDENTIAL_ACCESS, SHELL_DESTRUCTIVE_HOST_OP, SHELL_PRIVACY_TAMPER,
    SHELL_SYSTEM_TAMPER, ShellPolicyDecision, evaluate_shell_command,
};
