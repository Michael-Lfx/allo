pub mod config;
pub mod manager;
pub mod protocol;
pub mod remote_peer;
pub mod tool_proxy;
pub mod transport;

/// The MCP protocol version advertised by this client on `initialize`.
///
/// Re-exported so backend layers can reuse it without copying the literal;
/// drift between this and `nomifun-mcp::MCP_PROTOCOL_VERSION` is guarded by an
/// equality assertion in `nomifun-ai-agent`.
pub use remote_peer::CLIENT_PROTOCOL_VERSION_STR as MCP_PROTOCOL_VERSION;
