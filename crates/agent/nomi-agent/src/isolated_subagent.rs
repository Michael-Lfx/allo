//! Depth-1 isolated coding/office recon loops. Not `nomi_delegate`.
//!
//! Parent context never receives the child transcript — only a structured
//! summary. Explore/research are concurrency-safe (shared permit cap 4);
//! verify_change is serial.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::local_agent_invocation::LocalAgentInvocationRunner;
use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::agent::{AgentInvocationInput, AgentToolPolicy};
use nomi_types::tool::{JsonSchema, ToolResult};

const EXPLORE_MAX_TURNS: usize = 8;
const VERIFY_MAX_TURNS: usize = 4;
const RESEARCH_MAX_TURNS: usize = 8;
const SUMMARY_CHAR_CAP: usize = 8_000;

const EXPLORE_SYSTEM: &str = "\
You are an isolated explore_code worker. Use only Read/Grep/Glob/DirTree. \
Do not edit files. Return a structured summary the parent can trust:
Files: paths you actually inspected
Symbols: definitions/usages worth editing
Suggested anchors: line:hash prefixes if you Read a file
Open questions: anything still unknown
Do not dump file bodies. The parent must not re-Grep/Read paths you already covered.";

const VERIFY_SYSTEM: &str = "\
You are an isolated verify_change worker. Run the requested build/test/lint command \
with Bash. Do not edit files. Return: command, exit meaning, and a short failure excerpt \
if it failed. Do not paste full logs.";

const RESEARCH_SYSTEM: &str = "\
You are an isolated research worker. Use Read/Grep/Glob/DirTree on local files. \
Return a structured summary (sources, facts, open questions). Do not dump raw extracts \
into a long transcript.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Explore,
    Verify,
    Research,
}

pub(crate) struct IsolatedSubagentTool {
    kind: Kind,
    runner: Arc<LocalAgentInvocationRunner>,
    permits: Option<Arc<Semaphore>>,
}

impl IsolatedSubagentTool {
    pub(crate) fn explore(runner: Arc<LocalAgentInvocationRunner>, permits: Arc<Semaphore>) -> Self {
        Self {
            kind: Kind::Explore,
            runner,
            permits: Some(permits),
        }
    }

    pub(crate) fn verify(runner: Arc<LocalAgentInvocationRunner>) -> Self {
        Self {
            kind: Kind::Verify,
            runner,
            permits: None,
        }
    }

    pub(crate) fn research(
        runner: Arc<LocalAgentInvocationRunner>,
        permits: Arc<Semaphore>,
    ) -> Self {
        Self {
            kind: Kind::Research,
            runner,
            permits: Some(permits),
        }
    }

    async fn run_direct_verify(&self, command: &str) -> ToolResult {
        let result = self.runner.run_shell_command(command).await;
        let body = format!("command: {command}\n{}", result.content);
        ToolResult {
            content: format_isolated_summary("verify_change", 1, result.is_error, body),
            is_error: result.is_error,
            images: Vec::new(),
        }
    }

    fn spec(&self) -> (&'static str, &'static str, usize, AgentToolPolicy) {
        match self.kind {
            Kind::Explore => (
                "explore_code",
                "Isolated read-only recon. Returns a summary (files, symbols, suggested anchors). \
                 Do not re-Read/Grep paths the summary already covered.",
                EXPLORE_MAX_TURNS,
                AgentToolPolicy::ReadOnly,
            ),
            Kind::Verify => (
                "verify_change",
                "Run a test/build/lint command. Prefer `command` (exact shell) — that runs \
                 directly, no nested model. Natural-language `prompt` only starts a bounded \
                 isolated agent. Returns exit meaning and a short failure excerpt, not the full log.",
                VERIFY_MAX_TURNS,
                AgentToolPolicy::ReadShell,
            ),
            Kind::Research => (
                "research",
                "Isolated read-only research over local files. Returns a summary, not raw dumps.",
                RESEARCH_MAX_TURNS,
                AgentToolPolicy::ReadOnly,
            ),
        }
    }
}

#[async_trait]
impl Tool for IsolatedSubagentTool {
    fn name(&self) -> &str {
        self.spec().0
    }

    fn description(&self) -> &str {
        self.spec().1
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to investigate or verify"
                },
                "command": {
                    "type": "string",
                    "description": "For verify_change: the exact shell command to run"
                }
            },
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        matches!(self.kind, Kind::Explore | Kind::Research)
    }

    fn is_deferred(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if self.kind == Kind::Verify
            && let Some(cmd) = direct_verify_command(command, prompt.unwrap_or(""))
        {
            return self.run_direct_verify(&cmd).await;
        }

        let Some(prompt) = prompt.or(command) else {
            return ToolResult::error("prompt or command is required");
        };

        let (name, _, max_turns, tool_policy) = self.spec();
        let system_prompt = match self.kind {
            Kind::Explore => EXPLORE_SYSTEM,
            Kind::Verify => VERIFY_SYSTEM,
            Kind::Research => RESEARCH_SYSTEM,
        };
        let mut child_prompt = prompt.to_string();
        if self.kind == Kind::Verify
            && let Some(command) = command
        {
            child_prompt = format!("Run this verification command and report the result:\n{command}");
        }

        let _permit = if let Some(sem) = &self.permits {
            match sem.clone().acquire_owned().await {
                Ok(p) => Some(p),
                Err(_) => {
                    return ToolResult::error(format!("{name} cancelled: permit closed"));
                }
            }
        } else {
            None
        };

        let billing_turn_id = nomi_providers::current_flowy_billing_turn_id();
        let output = nomi_providers::with_optional_flowy_billing_turn_id(
            billing_turn_id,
            self.runner.invoke_one(AgentInvocationInput {
                name: name.to_string(),
                prompt: child_prompt,
                max_turns,
                system_prompt: Some(system_prompt.to_string()),
                model: None,
                effort: Some("low".into()),
                tool_policy,
                exact_tools: Vec::new(),
            }),
        )
        .await;

        ToolResult {
            content: format_isolated_summary(name, output.turns, output.is_error, output.text),
            is_error: output.is_error,
            images: Vec::new(),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn describe(&self, input: &Value) -> String {
        let hint = input
            .get("prompt")
            .or_else(|| input.get("command"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let short = nomi_tools::truncate_utf8(hint, 72);
        format!("{}: {short}", self.spec().0)
    }
}

fn format_isolated_summary(name: &str, turns: usize, is_error: bool, text: String) -> String {
    let text = if text.chars().count() > SUMMARY_CHAR_CAP {
        let trimmed: String = text.chars().take(SUMMARY_CHAR_CAP).collect();
        format!("{trimmed}\n…[isolated summary truncated]")
    } else {
        text
    };
    format!("[{name} turns={turns} error={is_error}]\n{text}")
}

/// Direct Bash when the parent already named a command. Natural-language
/// prompts stay on the nested Agent so "did the tests pass?" still works.
fn direct_verify_command(command: Option<&str>, prompt: &str) -> Option<String> {
    if let Some(command) = command.filter(|s| !s.is_empty()) {
        return Some(command.to_string());
    }
    let line = prompt.trim();
    if line.is_empty() || line.contains('\n') {
        return None;
    }
    looks_like_shell_command(line).then(|| line.to_string())
}

fn looks_like_shell_command(line: &str) -> bool {
    const BINS: &[&str] = &[
        "cargo", "bun", "npm", "npx", "pnpm", "yarn", "go", "python", "python3", "pytest",
        "make", "just", "git", "rustc", "deno", "uv", "dotnet", "mvn", "gradle", "node",
        "tsc", "vitest",
    ];
    let first = line.split_whitespace().next().unwrap_or("");
    BINS.contains(&first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nomi_config::compat::ProviderCompat;
    use nomi_config::config::{Config, ProviderType};
    use nomi_process_runtime::CapabilityPolicy;
    use nomi_providers::{LlmProvider, ProviderError};
    use nomi_tools::Tool;
    use nomi_types::llm::{LlmEvent, LlmRequest};

    use crate::local_agent_invocation::LocalAgentInvocationRunner;

    struct NeverCalledProvider;

    #[async_trait]
    impl LlmProvider for NeverCalledProvider {
        async fn stream(
            &self,
            _request: &LlmRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<LlmEvent>, ProviderError> {
            panic!("verify_change with an exact command must not start a nested Agent")
        }
    }

    fn test_config() -> Config {
        Config {
            provider_label: "openai".into(),
            provider: ProviderType::OpenAI,
            api_key: "sk-test".into(),
            base_url: "http://localhost:0".into(),
            model: "gpt-test-model".into(),
            output_max_tokens: Some(1024),
            max_turns: Some(5),
            system_prompt: None,
            project_instructions: Default::default(),
            thinking: None,
            prompt_caching: false,
            compat: ProviderCompat::openai_defaults(),
            tools: Default::default(),
            session: Default::default(),
            compact: Default::default(),
            plan: Default::default(),
            file_cache: Default::default(),
            hooks: Default::default(),
            bedrock: None,
            vertex: None,
            mcp: Default::default(),
            logging: Default::default(),
            memory: Default::default(),
            moa: Default::default(),
        }
    }

    #[test]
    fn direct_verify_uses_command_or_shellish_prompt() {
        assert_eq!(
            direct_verify_command(Some("cargo test -p nomi-agent"), "ignored"),
            Some("cargo test -p nomi-agent".into())
        );
        assert_eq!(
            direct_verify_command(None, "bun run typecheck"),
            Some("bun run typecheck".into())
        );
        assert_eq!(direct_verify_command(None, "going to inspect src/"), None);
        assert_eq!(direct_verify_command(None, "did the tests pass?"), None);
        assert_eq!(
            direct_verify_command(None, "cargo test\nand also lint"),
            None
        );
    }

    #[tokio::test]
    async fn verify_change_with_command_runs_bash_without_nested_llm() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_path_buf();
        let runner = Arc::new(
            LocalAgentInvocationRunner::new(
                Arc::new(NeverCalledProvider),
                test_config(),
                cwd.clone(),
            )
            .with_process_capability(
                CapabilityPolicy::local_owner(cwd.clone()),
                Some(cwd),
                Vec::new(),
            ),
        );
        let tool = IsolatedSubagentTool::verify(runner);
        let result = tool
            .execute(json!({ "command": "echo verify-direct" }))
            .await;
        assert!(
            result.content.contains("[verify_change turns=1"),
            "{}",
            result.content
        );
        assert!(
            result.content.contains("verify-direct") || result.content.contains("Exit code: 0"),
            "{}",
            result.content
        );
        assert!(!result.is_error, "{}", result.content);
    }
}
