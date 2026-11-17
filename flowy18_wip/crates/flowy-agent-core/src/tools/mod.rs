//! Host-owned tool registry and the built-in tool set.

pub mod adapter;
pub mod builtin;
pub mod matcher;
pub mod registry;

pub use adapter::RegistryTool;
pub use registry::{
    BoxFuture, ToolDefinition, ToolGuard, ToolPolicy, ToolPolicyDecision, ToolRegistry,
};
