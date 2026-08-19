//! Live evaluation harness: production `AgentEngine` on an isolated workspace.
//!
//! Never registers in `AgentRuntimeRegistry`, never writes conversation rows,
//! and never enables MCP / browser / computer-use / web search.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nomi_agent::bootstrap::AgentBootstrap;
use nomi_agent::output::OutputSink;
use nomi_agent::task_profile::{CodingEnvContext, TaskProfile};
use nomi_agent_eval::{
    collect_workspace_artifacts, materialize_files, Case, ConversationEvalHarness, EvalCaseTrace,
    HarnessError, TurnTranscript,
};
use nomi_config::config::Config;
use nomifun_common::AppError;
use nomifun_db::{IProviderModelRepository, IProviderRepository};

use crate::factory::provider_config::resolve_provider_config;

use super::capture::EvalCaptureSink;

const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Clone)]
pub struct LiveEvalTrace {
    pub case_id: String,
    pub sink: Arc<EvalCaptureSink>,
    pub workspace: PathBuf,
}

impl LiveEvalTrace {
    pub fn snapshot(&self) -> EvalCaseTrace {
        let mut trace = self.sink.snapshot_trace(&self.case_id, true);
        trace.artifacts = collect_workspace_artifacts(&self.workspace);
        trace
    }
}

pub struct LiveNomiHarness {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub work_root: PathBuf,
    pub traces_dir: PathBuf,
    pub provider_id: String,
    pub model: String,
    pub profile_override: Option<String>,
    pub live_trace: Arc<Mutex<Option<LiveEvalTrace>>>,
}

#[async_trait]
impl ConversationEvalHarness for LiveNomiHarness {
    async fn run_case(&self, case: &Case) -> Result<TurnTranscript, HarnessError> {
        self.run_case_inner(case).await.map_err(|error| HarnessError::Failed {
            case_id: case.id.clone(),
            message: error.to_string(),
        })
    }
}

impl LiveNomiHarness {
    async fn run_case_inner(&self, case: &Case) -> Result<TurnTranscript, AppError> {
        let workspace = self.work_root.join(sanitize_case_dir(&case.id));
        std::fs::create_dir_all(&workspace)
            .map_err(|e| AppError::Internal(format!("eval workspace: {e}")))?;
        materialize_files(&workspace, &case.workspace_files)
            .map_err(|e| AppError::Internal(format!("eval materialize: {e}")))?;

        let mut config = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            &self.provider_id,
            &self.model,
            &workspace,
        )
        .await?;
        let max_turns = case.budgets.max_turns.map(|n| n as usize);
        isolate_eval_config(&mut config, &workspace, max_turns);

        let sink = Arc::new(EvalCaptureSink::default());
        {
            let mut slot = self.live_trace.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(LiveEvalTrace {
                case_id: case.id.clone(),
                sink: sink.clone(),
                workspace: workspace.clone(),
            });
        }
        let output: Arc<dyn OutputSink> = sink.clone();
        let coding = is_coding_case(case, self.profile_override.as_deref());
        let bootstrap = AgentBootstrap::new(config, workspace.to_string_lossy().into_owned(), output)
            .install_embedded_agent_execution(false)
            .disable_web_search()
            .coding_boundary(coding);
        let mut built = bootstrap
            .build()
            .await
            .map_err(|e| AppError::Internal(format!("eval bootstrap: {e}")))?;

        if coding {
            built.engine.install_coding_harness(
                Some(CodingEnvContext {
                    cwd: workspace.clone(),
                    write_root: Some(workspace.clone()),
                }),
                nomi_agent::task_profile::CodingConfig::default(),
            );
        } else {
            built.engine.set_task_profile(TaskProfile::Office);
        }

        let timeout = Duration::from_secs(case.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, 600));
        let msg_id = format!("eval-{}", case.id);
        let exec = tokio::time::timeout(timeout, built.engine.execute_turn(&case.prompt, &msg_id)).await;
        let (captured_text, tool_names, tool_error_count) = sink.snapshot();

        let outcome = match exec {
            Ok(Ok(result)) => {
                let assistant_text = if result.text.trim().is_empty() {
                    captured_text.clone()
                } else {
                    result.text
                };
                Ok((
                    assistant_text,
                    result.turns as u32,
                    Some(stop_reason_label(result.stop_reason)),
                    result.usage.input_tokens,
                    result.usage.output_tokens,
                ))
            }
            Ok(Err(error)) => Err(AppError::Internal(format!("eval execute_turn: {error}"))),
            Err(_) => Err(AppError::Timeout(format!("eval case {} timed out", case.id))),
        };

        let artifacts = collect_workspace_artifacts(&workspace);
        let mut trace = sink.snapshot_trace(&case.id, false);
        if let Ok((ref assistant_text, _, _, _, _)) = outcome {
            trace.assistant_text = assistant_text.clone();
        } else if trace.assistant_text.is_empty() {
            trace.assistant_text = captured_text;
        }
        trace.artifacts = artifacts.clone();
        persist_case_trace(&self.traces_dir, &case.id, &trace)?;
        {
            let mut slot = self.live_trace.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        }

        let (assistant_text, turns, stop_reason, input_tokens, output_tokens) = outcome?;
        Ok(TurnTranscript {
            assistant_text,
            tool_names,
            turns,
            stop_reason,
            input_tokens,
            output_tokens,
            tool_error_count,
            workspace: Some(workspace),
            trajectory: trace.events,
            artifacts,
        })
    }
}

fn persist_case_trace(
    traces_dir: &Path,
    case_id: &str,
    trace: &EvalCaseTrace,
) -> Result<(), AppError> {
    std::fs::create_dir_all(traces_dir)
        .map_err(|e| AppError::Internal(format!("eval traces dir: {e}")))?;
    let path = traces_dir.join(format!("{}.json", sanitize_case_dir(case_id)));
    let json = serde_json::to_string_pretty(trace)
        .map_err(|e| AppError::Internal(format!("eval trace json: {e}")))?;
    std::fs::write(path, json).map_err(|e| AppError::Internal(format!("eval trace write: {e}")))
}

pub(crate) fn isolate_eval_config(config: &mut Config, workspace: &Path, max_turns: Option<usize>) {
    config.session.enabled = false;
    config.mcp.servers.clear();
    config.file_cache.enabled = false;
    config.tools.auto_approve = true;
    config.tools.browser.enabled = false;
    config.tools.computer.enabled = false;
    config.tools.web.enabled = false;
    config.tools.write_root = workspace.to_string_lossy().into_owned();
    config.tools.builtin_allowlist.clear();
    config.memory.distill_enabled = false;
    config.moa.enabled = false;
    config.max_turns = max_turns;
}

fn is_coding_case(case: &Case, override_profile: Option<&str>) -> bool {
    TaskProfile::parse(override_profile.or(case.task_profile.as_deref())).is_coding()
}

pub(crate) fn sanitize_case_dir(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "case".into()
    } else {
        cleaned
    }
}

fn stop_reason_label(reason: nomi_types::message::StopReason) -> String {
    match reason {
        nomi_types::message::StopReason::EndTurn => "end_turn".into(),
        nomi_types::message::StopReason::ToolUse => "tool_use".into(),
        nomi_types::message::StopReason::MaxTokens => "max_tokens".into(),
        nomi_types::message::StopReason::MaxTurns => "max_turns".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::config::CliArgs;
    use tempfile::tempdir;

    #[test]
    fn isolate_config_disables_host_side_effects() {
        let dir = tempdir().unwrap();
        let args = CliArgs {
            provider: Some("openai".into()),
            api_key: Some("sk-test".into()),
            base_url: None,
            model: Some("test-model".into()),
            max_tokens: Some(128),
            max_turns: Some(3),
            system_prompt: None,
            profile: None,
            auto_approve: false,
            project_dir: Some(dir.path().to_path_buf()),
        };
        let mut config = Config::resolve(&args).expect("config");
        config.mcp.servers.insert(
            "demo".into(),
            nomi_config::config::McpServerConfig {
                transport: nomi_config::config::TransportType::Stdio,
                command: Some("true".into()),
                args: None,
                env: None,
                url: None,
                headers: None,
                deferred: None,
            },
        );
        isolate_eval_config(&mut config, dir.path(), Some(6));
        assert!(config.mcp.servers.is_empty());
        assert!(!config.session.enabled);
        assert!(config.mcp.servers.is_empty());
        assert!(config.tools.auto_approve);
        assert!(!config.tools.browser.enabled);
        assert!(!config.tools.computer.enabled);
        assert!(!config.tools.web.enabled);
        assert!(!config.file_cache.enabled);
        assert!(!config.memory.distill_enabled);
        assert!(!config.moa.enabled);
        assert_eq!(config.max_turns, Some(6));
        assert!(!config.tools.write_root.is_empty());
    }

    #[test]
    fn coding_profile_follows_case_then_override() {
        let mut case = nomi_agent_eval::load_bundled_manifest("harness_control")
            .unwrap()
            .cases
            .remove(0);
        assert!(is_coding_case(&case, None));
        case.task_profile = Some("office".into());
        assert!(!is_coding_case(&case, None));
        assert!(is_coding_case(&case, Some("coding")));
    }
}
