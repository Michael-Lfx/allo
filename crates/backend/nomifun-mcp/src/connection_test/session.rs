// A persistent stdio MCP session: one handshake, then many calls.
//
// See doc 24 §5 (connector call proxy). The one-shot path spawns a child per
// call, which is correct but pays for an interpreter start-up (`npx`, `uvx`,
// …) and a handshake every time. This type keeps the child and its pipes alive
// so a *second* call costs one `tools/call` round trip.
//
// Only stdio is pooled, and the reason is ownership: we spawned this child, so
// its lifetime is entirely ours to control. A remote HTTP/SSE session id is
// expired by the *server* whenever it likes, so caching one would trade a
// reliable call for a saved round trip — see `pool.rs`.

use std::collections::HashMap;
use std::process::Stdio;

use nomi_process_runtime::{ChildProcessBuilder as CmdBuilder, kill_process_tree};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tracing::warn;

use super::protocol::{
    ToolCallReply, build_initialize_request, build_initialized_notification,
    build_tools_call_request, read_jsonrpc_response, tool_call_reply, write_jsonrpc_line,
};
use super::resolve_stdio_command;

/// Everything a spawned stdio session actually depends on.
///
/// This is what a pooled session is checked against, so it covers
/// *credentials* as well as configuration: the env is resolved **before** being
/// stored, so rotating the value behind an unchanged `secret:NAME` reference
/// invalidates the session instead of letting it keep serving calls with the
/// old credential. Comparison is in memory only — the identity carries resolved
/// secrets by nature, so it is never logged or persisted. `missing` (key names,
/// not values) is safe to log and is reported once, here.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct StdioIdentity {
    command: String,
    args: Vec<String>,
    /// Resolved env, sorted so equal maps compare equal regardless of order.
    env: Vec<(String, String)>,
}

impl StdioIdentity {
    pub(super) fn new(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        principal: Option<&str>,
    ) -> Self {
        // Same `secret:NAME` contract as the probe: a reference with no
        // credential is dropped and named, and the literal reference string is
        // never handed to the child (`17` §6 / `21` D5=C). Resolution is per
        // principal, so the resolved env below — which is what the pool compares —
        // differs whenever the caller's credentials do (34 §7).
        let resolved = nomifun_common::secret_ref::resolve_env_for(principal, env);
        if !resolved.missing.is_empty() {
            warn!(
                command = %command,
                missing = ?resolved.missing,
                "MCP stdio env has unresolved credential references; omitting them"
            );
        }

        let mut env: Vec<(String, String)> = resolved.env.into_iter().collect();
        env.sort();
        Self { command: command.to_owned(), args: args.to_vec(), env }
    }
}

/// Why a `tools/call` produced no reply, and whether the session survived it.
///
/// The distinction is the whole reason this is an enum rather than a `String`:
/// a well-framed rejection leaves request and response in step, so the session
/// is still usable. A framing or I/O failure does not, and a pipe whose state
/// is unknown must be thrown away — otherwise the *next* call can read this
/// one's reply and silently attribute it to the wrong request.
pub(super) enum StdioCallError {
    /// The server answered, and the answer was a JSON-RPC error.
    Rejected(String),
    /// Framing, I/O, or process failure: the session must not be reused.
    Broken(String),
}

impl StdioCallError {
    /// Consume the error into the message the connector call proxy reports.
    pub(super) fn into_message(self) -> String {
        match self {
            Self::Rejected(message) | Self::Broken(message) => message,
        }
    }
}

/// A live stdio MCP server: handshaken once, then called repeatedly.
///
/// Calls on one session are strictly serialized by the caller (`pool.rs` holds
/// it behind a mutex) because a stdio MCP server is a single request/response
/// pipe — interleaving two calls on it would corrupt both.
pub(super) struct StdioToolSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioToolSession {
    /// Spawn the server and complete `initialize` → `initialized`.
    pub(super) async fn connect(identity: StdioIdentity) -> Result<Self, String> {
        let program = resolve_stdio_command(&identity.command);

        let mut cmd = CmdBuilder::new(&program);
        cmd.args(&identity.args)
            .envs(identity.env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|error| format!("failed to start '{}': {error}", identity.command))?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 2,
        };
        match session.handshake().await {
            Ok(()) => Ok(session),
            Err(error) => {
                // A child that failed its handshake is still a child.
                session.close().await;
                Err(error)
            }
        }
    }

    async fn handshake(&mut self) -> Result<(), String> {
        write_jsonrpc_line(&mut self.stdin, &build_initialize_request(1))
            .await
            .map_err(|error| format!("failed to send initialize: {error}"))?;
        let response = read_jsonrpc_response(&mut self.stdout)
            .await
            .map_err(|error| format!("initialize response: {error}"))?;
        if let Some(error) = response.error {
            return Err(format!("initialize rejected: {} (code {})", error.message, error.code));
        }

        write_jsonrpc_line(&mut self.stdin, &build_initialized_notification())
            .await
            .map_err(|error| format!("failed to send initialized: {error}"))?;
        Ok(())
    }

    /// Call one tool on the already-handshaken session.
    pub(super) async fn call(
        &mut self,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> Result<ToolCallReply, StdioCallError> {
        let id = self.next_id;
        self.next_id += 1;

        write_jsonrpc_line(&mut self.stdin, &build_tools_call_request(id, tool, arguments))
            .await
            .map_err(|error| StdioCallError::Broken(format!("failed to send tools/call: {error}")))?;
        let response = read_jsonrpc_response(&mut self.stdout)
            .await
            .map_err(|error| StdioCallError::Broken(format!("tools/call response: {error}")))?;
        if let Some(error) = response.error {
            return Err(StdioCallError::Rejected(format!(
                "tools/call rejected: {} (code {})",
                error.message, error.code
            )));
        }

        Ok(tool_call_reply(response.result.unwrap_or(serde_json::Value::Null)))
    }

    /// Terminate the child's process tree.
    pub(super) async fn close(&mut self) {
        if let Err(error) = kill_process_tree(&mut self.child).await {
            warn!(%error, "failed to clean up MCP tool-call process tree");
        }
    }
}
