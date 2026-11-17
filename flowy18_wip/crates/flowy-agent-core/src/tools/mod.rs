//! Host-owned tool registry and the built-in tool set.

pub mod adapter;
pub mod builtin;
pub mod matcher;
pub mod pi;
pub mod registry;

pub use adapter::RegistryTool;
pub use builtin::register_builtin_tools;
pub use pi::pi_shell_definition;
pub use registry::{
    BoxFuture, ToolDefinition, ToolGuard, ToolPolicy, ToolPolicyDecision,
    ToolRegistry,
};
