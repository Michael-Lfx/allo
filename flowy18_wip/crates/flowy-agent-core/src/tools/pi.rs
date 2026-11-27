//! Bridge to pi-rust's native tool layer (`rpi-tools`).
//!
//! Host tools stay behind [`ToolDefinition`] so the registry policy, guards,
//! cancellation, and UI projection keep working. Only the tool body is pi's
//! implementation; bash in particular comes from `rpi-tools`, which owns shell
//! discovery, streaming output capture, truncation, and process cleanup.

use std::path::Path;
use std::sync::Arc;

use rpi_agent::{AgentError, AgentTool, AgentToolResult, TextContentOrImage};
#[cfg(not(target_os = "linux"))]
use rpi_tools::OsExecutionEnv;
#[cfg(windows)]
use rpi_tools::create_powershell_tool;
use rpi_tools::{
    BashToolOptions, ExecutionEnv, ExecutionToolContext, create_bash_tool,
};

use crate::tools::registry::ToolDefinition;
use crate::types::{ContentBlock, ToolExecutionResult};

/// pi-rust's bash tool running in `cwd`.
///
/// Non-Windows production uses this; Windows keeps it available for explicit
/// use/tests when a POSIX bash exists. pi applies its own default deadline
/// (120s), POSIX bash discovery, streaming capture, tail truncation, and
/// full-output spill to a temporary file.
#[must_use]
pub fn pi_bash_definition(cwd: &Path) -> Arc<ToolDefinition> {
    #[cfg(target_os = "linux")]
    let env = crate::tools::sandbox::linux_execution_env(
        cwd,
        crate::tools::sandbox::SandboxPolicy::for_workspace(cwd.to_path_buf()),
    );
    #[cfg(not(target_os = "linux"))]
    let env: Arc<dyn ExecutionEnv> =
        Arc::new(OsExecutionEnv::with_cwd(cwd.to_path_buf()));
    bash_definition(env)
}

fn bash_definition(env: Arc<dyn ExecutionEnv>) -> Arc<ToolDefinition> {
    let context = ExecutionToolContext::new(env, None);
    let tool = create_bash_tool(
        &context,
        Some(BashToolOptions {
            default_timeout: Some(DEFAULT_SHELL_TIMEOUT_SECS),
            ..BashToolOptions::default()
        }),
    );
    agent_tool_definition_with(tool, None, Some(DEFAULT_SHELL_TIMEOUT_SECS))
}

/// Production shell tool for the current platform.
///
/// Windows uses pi's PowerShell tool because a POSIX bash is not guaranteed and
/// Git Bash may be absent; every other platform keeps pi's bash tool.
#[must_use]
pub fn pi_shell_definition(cwd: &Path) -> Arc<ToolDefinition> {
    #[cfg(windows)]
    {
        pi_powershell_definition(cwd)
    }
    #[cfg(not(windows))]
    {
        pi_bash_definition(cwd)
    }
}

/// Production shell with the host's branded data directory in the Linux
/// credential protection policy.
#[must_use]
pub fn pi_shell_definition_for_host(
    cwd: &Path,
    data_root: &Path,
) -> Arc<ToolDefinition> {
    #[cfg(target_os = "linux")]
    {
        use crate::tools::sandbox::{SandboxPolicy, linux_execution_env};
        let policy = SandboxPolicy::for_workspace(cwd.to_path_buf())
            .with_protected_path(data_root.join("auth"));
        bash_definition(linux_execution_env(cwd, policy))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = data_root;
        pi_shell_definition(cwd)
    }
}

/// Default wall-clock timeout applied to shell commands when the model omits
/// `timeout`.
const DEFAULT_SHELL_TIMEOUT_SECS: f64 = 120.0;
/// Hard upper bound for any model-supplied shell timeout (10 minutes).
const MAX_SHELL_TIMEOUT_SECS: f64 = 600.0;

/// Windows-only fallback: pi-rust's PowerShell tool in `cwd`.
#[cfg(windows)]
#[must_use]
pub fn pi_powershell_definition(cwd: &Path) -> Arc<ToolDefinition> {
    let env: Arc<dyn ExecutionEnv> =
        Arc::new(OsExecutionEnv::with_cwd(cwd.to_path_buf()));
    let context = ExecutionToolContext::new(env, None);
    let tool = create_powershell_tool(&context);
    agent_tool_definition_with(
        tool,
        Some(POWERSHELL_DESCRIPTION.to_owned()),
        Some(DEFAULT_SHELL_TIMEOUT_SECS),
    )
}

#[cfg(windows)]
const POWERSHELL_DESCRIPTION: &str = "Execute a PowerShell command on Windows. \
Returns stdout and stderr. This is the only shell tool on this platform: use \
PowerShell syntax (Get-ChildItem, Select-String, ...), not POSIX bash syntax. \
Always pass `description`: a concise 5-10 word summary of the command's purpose; \
hosts show it in the collapsed tool row.";

/// Adapt one pi `AgentTool` into the host registry's [`ToolDefinition`].
///
/// Kept private: the host surface is the platform shell definition. A future
/// generic bridge must also map image content, usage, and termination metadata.
#[cfg(test)]
fn agent_tool_definition(tool: Arc<dyn AgentTool>) -> Arc<ToolDefinition> {
    agent_tool_definition_with(tool, None, None)
}

fn agent_tool_definition_with(
    tool: Arc<dyn AgentTool>,
    description: Option<String>,
    default_timeout: Option<f64>,
) -> Arc<ToolDefinition> {
    let schema = tool.schema().clone();
    let description = description.unwrap_or(schema.description);
    let parameters = schema.parameters.as_value().clone();
    Arc::new(ToolDefinition::new(
        schema.name,
        description,
        parameters,
        move |mut args, run_ctx| {
            let tool = Arc::clone(&tool);
            Box::pin(async move {
                if let Some(obj) = args.arguments.as_object_mut() {
                    let parsed_timeout = obj
                        .get("timeout")
                        .and_then(serde_json::Value::as_f64)
                        .filter(|v| v.is_finite() && *v > 0.0);
                    match parsed_timeout {
                        Some(v) if v > MAX_SHELL_TIMEOUT_SECS => {
                            obj.insert(
                                "timeout".to_owned(),
                                serde_json::json!(MAX_SHELL_TIMEOUT_SECS),
                            );
                        }
                        None => {
                            if let Some(timeout) = default_timeout {
                                obj.insert(
                                    "timeout".to_owned(),
                                    serde_json::json!(timeout),
                                );
                            }
                        }
                        Some(_) => {}
                    }
                }
                let signal = run_ctx.signal.inner();
                match tool
                    .execute(
                        &args.call_id,
                        args.arguments,
                        signal,
                        Arc::new(|_update| {}),
                    )
                    .await
                {
                    Ok(result) => {
                        let AgentToolResult {
                            content, details, ..
                        } = result;
                        ToolExecutionResult::Success {
                            content: to_host_content(content),
                            value: details,
                            meta: None,
                            delivery: None,
                        }
                    }
                    Err(AgentError::StructuredTool(result)) => {
                        let AgentToolResult {
                            content, details, ..
                        } = *result;
                        let message = text_of(&content);
                        let code = details
                            .get("failure_kind")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("TOOL_FAILED")
                            .to_owned();
                        ToolExecutionResult::Error {
                            message: if message.is_empty() {
                                "tool failed".to_owned()
                            } else {
                                message
                            },
                            code,
                            content: to_host_content(content),
                        }
                    }
                    Err(AgentError::Abort) => ToolExecutionResult::error(
                        "CANCELLED",
                        "tool call aborted",
                    ),
                    Err(error) => {
                        ToolExecutionResult::error("PI_TOOL", error.to_string())
                    }
                }
            })
        },
    ))
}

fn text_of(content: &[TextContentOrImage]) -> String {
    content
        .iter()
        .filter_map(|content| match content {
            TextContentOrImage::Text(text) => Some(text.text.as_str()),
            TextContentOrImage::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn to_host_content(content: Vec<TextContentOrImage>) -> Vec<ContentBlock> {
    content
        .into_iter()
        .filter_map(|content| match content {
            TextContentOrImage::Text(text) => {
                Some(ContentBlock::text(text.text))
            }
            // pi image content has no host block type yet; bash never emits it.
            TextContentOrImage::Image(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{agent_tool_definition, pi_bash_definition};
    use crate::tools::ToolRegistry;
    use crate::types::{CancelToken, ToolRunContext};
    use async_trait::async_trait;
    use rpi_agent::{
        AgentError, AgentTool, AgentToolResult, TextContentOrImage,
        ToolResultPartial,
    };
    use rpi_ai::types::{Schema, Tool};
    use serde_json::json;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    struct EchoTool {
        schema: Tool,
    }

    #[async_trait]
    impl AgentTool for EchoTool {
        fn schema(&self) -> &Tool {
            &self.schema
        }

        fn label(&self) -> &'static str {
            "echo"
        }

        async fn execute(
            &self,
            _tool_call_id: &str,
            params: serde_json::Value,
            _signal: CancellationToken,
            _on_update: Arc<dyn Fn(ToolResultPartial) + Send + Sync>,
        ) -> Result<AgentToolResult, AgentError> {
            Ok(AgentToolResult {
                content: vec![TextContentOrImage::text(
                    params
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                )],
                details: json!({ "echo": true }),
                ..AgentToolResult::default()
            })
        }
    }

    struct FailingTool {
        schema: Tool,
    }

    #[async_trait]
    impl AgentTool for FailingTool {
        fn schema(&self) -> &Tool {
            &self.schema
        }

        fn label(&self) -> &'static str {
            "failing"
        }

        async fn execute(
            &self,
            _tool_call_id: &str,
            _params: serde_json::Value,
            _signal: CancellationToken,
            _on_update: Arc<dyn Fn(ToolResultPartial) + Send + Sync>,
        ) -> Result<AgentToolResult, AgentError> {
            Err(AgentError::StructuredTool(Box::new(AgentToolResult {
                content: vec![TextContentOrImage::text("boom")],
                details: json!({
                    "exit_code": 1,
                    "failure_kind": "command_failed"
                }),
                ..AgentToolResult::default()
            })))
        }
    }

    fn run_ctx() -> ToolRunContext {
        ToolRunContext {
            signal: CancelToken::new(),
            agent_id: None,
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: crate::session::SessionOrigin::Unknown,
            output_root: None,
            artifact_delivery_available: false,
        }
    }

    #[tokio::test]
    async fn pi_tool_results_keep_host_content_and_details() {
        let registry = ToolRegistry::new();
        registry.register(agent_tool_definition(Arc::new(EchoTool {
            schema: Tool {
                name: "echo".into(),
                description: "echo text".into(),
                parameters: Schema::empty_object(),
                constrained_sampling: None,
            },
        })));
        let result = registry
            .execute(
                "call-1".into(),
                "echo".into(),
                json!({ "text": "pong" }),
                run_ctx(),
            )
            .await;
        assert!(!result.is_error());
        assert_eq!(
            result.content(),
            vec![crate::types::ContentBlock::text("pong")]
        );
        assert_eq!(
            result,
            crate::types::ToolExecutionResult::Success {
                content: vec![crate::types::ContentBlock::text("pong")],
                value: json!({ "echo": true }),
                meta: None,
                delivery: None,
            }
        );
    }

    #[tokio::test]
    async fn pi_structured_failure_keeps_output_and_code() {
        let registry = ToolRegistry::new();
        registry.register(agent_tool_definition(Arc::new(FailingTool {
            schema: Tool {
                name: "failing".into(),
                description: "fail".into(),
                parameters: Schema::empty_object(),
                constrained_sampling: None,
            },
        })));
        let result = registry
            .execute("call-1".into(), "failing".into(), json!({}), run_ctx())
            .await;
        match result {
            crate::types::ToolExecutionResult::Error {
                code,
                content,
                message,
            } => {
                assert_eq!(code, "command_failed");
                assert_eq!(message, "boom");
                assert_eq!(
                    content,
                    vec![crate::types::ContentBlock::text("boom")]
                );
            }
            other @ crate::types::ToolExecutionResult::Success { .. } => {
                panic!("expected structured error, got {other:?}")
            }
        }
    }

    #[test]
    fn production_shell_matches_the_platform() {
        let name = super::pi_shell_definition(std::path::Path::new("."))
            .name
            .clone();
        if cfg!(windows) {
            assert_eq!(name, "powershell");
        } else {
            assert_eq!(name, "bash");
        }
    }

    #[tokio::test]
    async fn default_timeout_is_injected_when_absent_and_preserved_when_set() {
        struct CaptureTimeoutTool {
            schema: Tool,
            seen: Arc<std::sync::Mutex<Vec<Option<f64>>>>,
        }
        #[async_trait]
        impl AgentTool for CaptureTimeoutTool {
            fn schema(&self) -> &Tool {
                &self.schema
            }
            fn label(&self) -> &'static str {
                "capture"
            }
            async fn execute(
                &self,
                _tool_call_id: &str,
                params: serde_json::Value,
                _signal: CancellationToken,
                _on_update: Arc<dyn Fn(ToolResultPartial) + Send + Sync>,
            ) -> Result<AgentToolResult, AgentError> {
                self.seen.lock().unwrap().push(
                    params.get("timeout").and_then(serde_json::Value::as_f64),
                );
                Ok(AgentToolResult::default())
            }
        }
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let registry = ToolRegistry::new();
        registry.register(super::agent_tool_definition_with(
            Arc::new(CaptureTimeoutTool {
                schema: Tool {
                    name: "capture".into(),
                    description: "capture".into(),
                    parameters: Schema::empty_object(),
                    constrained_sampling: None,
                },
                seen: Arc::clone(&seen),
            }),
            None,
            Some(120.0),
        ));
        let _ = registry
            .execute(
                "c1".into(),
                "capture".into(),
                json!({ "command": "pwd" }),
                run_ctx(),
            )
            .await;
        let _ = registry
            .execute(
                "c2".into(),
                "capture".into(),
                json!({ "command": "pwd", "timeout": 15.0 }),
                run_ctx(),
            )
            .await;
        assert_eq!(*seen.lock().unwrap(), vec![Some(120.0), Some(15.0)]);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requires nonprivileged Bubblewrap with mount and PID namespaces"]
    async fn linux_bwrap_tool_enforces_workspace_writes() {
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join(format!("flowy-bwrap-shell-{salt}"));
        let cwd = root.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let registry = ToolRegistry::new();
        std::fs::create_dir(root.join("auth")).unwrap();
        std::fs::write(root.join("auth/fixture"), "synthetic").unwrap();
        registry.register(super::pi_shell_definition_for_host(&cwd, &root));
        let result = registry
            .execute(
                "call-bwrap".into(),
                "bash".into(),
                json!({
                    "command": "set -e; printf inside > inside; test -d ../auth; test ! -e ../auth/fixture; if printf outside > ../outside; then exit 1; fi",
                    "timeout": 10,
                }),
                run_ctx(),
            )
            .await;
        let inside = std::fs::read(cwd.join("inside"));
        let outside_written = root.join("outside").exists();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(inside.unwrap(), b"inside");
        assert!(!outside_written, "shell wrote outside the workspace");
        assert!(!result.is_error(), "{result:?}");
    }

    #[tokio::test]
    #[ignore = "requires a bash executable recognized by pi-rust"]
    async fn pi_bash_runs_and_returns_output() {
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join("pi-bash");
        std::fs::create_dir_all(&cwd).unwrap();
        let definition = pi_bash_definition(&cwd);
        let registry = ToolRegistry::new();
        registry.register(definition);
        let result = registry
            .execute(
                "call-1".into(),
                "bash".into(),
                json!({ "command": "printf hi" }),
                run_ctx(),
            )
            .await;
        assert!(!result.is_error(), "{result:?}");
        assert!(result.content().iter().any(|block| {
            block.as_text().is_some_and(|text| text.contains("hi"))
        }));
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires Windows PowerShell"]
    async fn pi_powershell_runs_and_returns_output() {
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join("pi-powershell");
        std::fs::create_dir_all(&cwd).unwrap();
        let registry = ToolRegistry::new();
        registry.register(super::pi_powershell_definition(&cwd));
        let result = registry
            .execute(
                "call-1".into(),
                "powershell".into(),
                json!({ "command": "Write-Output 'ps-ok'" }),
                run_ctx(),
            )
            .await;
        assert!(!result.is_error(), "{result:?}");
        assert!(result.content().iter().any(|block| {
            block.as_text().is_some_and(|text| text.contains("ps-ok"))
        }));
    }
}
